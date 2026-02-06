//! Splice operations and state tracking.
//!
//! Implements SpliceTxn (two-step splice protocol), SpliceStateTable (local splices),
//! and DistributedSpliceTable (cross-Rendezvous splices).

use core::{cell::UnsafeCell, marker::PhantomData};

use super::{
    error::SpliceError,
    types::{Generation, Lane, RendezvousId, SessionId},
};
use crate::rendezvous::types::LocalSpliceInvariant;
use crate::{
    control::{
        automaton::distributed::{SpliceAck, SpliceIntent},
        automaton::txn::InAcked,
        types::One,
    },
    runtime::consts::LANES_MAX,
};

/// Pending splice state tracked per lane.
pub struct PendingSplice {
    sid: SessionId,
    target: Generation,
    state: InAcked<LocalSpliceInvariant, One>,
    fences: Option<(u32, u32)>,
}

impl PendingSplice {
    pub fn new(
        sid: SessionId,
        target: Generation,
        state: InAcked<LocalSpliceInvariant, One>,
        fences: Option<(u32, u32)>,
    ) -> Self {
        Self {
            sid,
            target,
            state,
            fences,
        }
    }

    #[inline]
    pub fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub fn target(&self) -> Generation {
        self.target
    }

    #[inline]
    pub fn state(&self) -> &InAcked<LocalSpliceInvariant, One> {
        &self.state
    }

    #[inline]
    pub fn into_state(self) -> InAcked<LocalSpliceInvariant, One> {
        self.state
    }

    #[inline]
    #[allow(clippy::type_complexity)]
    pub fn into_parts(
        self,
    ) -> (
        SessionId,
        Generation,
        InAcked<LocalSpliceInvariant, One>,
        Option<(u32, u32)>,
    ) {
        (self.sid, self.target, self.state, self.fences)
    }

    #[inline]
    pub fn fences(&self) -> Option<(u32, u32)> {
        self.fences
    }

    #[inline]
    pub fn take_fences(&mut self) -> Option<(u32, u32)> {
        self.fences.take()
    }
}

impl core::fmt::Debug for PendingSplice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PendingSplice")
            .field("sid", &self.sid)
            .field("target", &self.target)
            .field("lane", &self.state.lane())
            .finish()
    }
}

/// Snapshot representation used for read-only inspection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSpliceSnapshot {
    pub sid: SessionId,
    pub target: Generation,
    pub fences: Option<(u32, u32)>,
}

/// Local splice state table (per-lane).
///
/// Tracks pending splice operations within a single Rendezvous instance.
pub struct SpliceStateTable {
    lanes: UnsafeCell<[Option<PendingSplice>; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for SpliceStateTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SpliceStateTable {
    pub const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([const { None }; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Begin a splice operation.
    pub fn begin(&self, lane: Lane, pending: PendingSplice) -> Result<(), SpliceError> {
        unsafe {
            let slots = &mut *self.lanes.get();
            let idx = lane.raw() as usize;
            if slots[idx].is_some() {
                return Err(SpliceError::InProgress { lane });
            }
            slots[idx] = Some(pending);
            Ok(())
        }
    }

    /// Take (consume) pending splice.
    pub fn take(&self, lane: Lane) -> Option<PendingSplice> {
        unsafe {
            let slots = &mut *self.lanes.get();
            let idx = lane.raw() as usize;
            slots[idx].take()
        }
    }

    /// Peek at pending splice (non-consuming).
    pub fn peek(&self, lane: Lane) -> Option<PendingSpliceSnapshot> {
        unsafe {
            let slots = &*self.lanes.get();
            slots[lane.raw() as usize]
                .as_ref()
                .map(|pending| PendingSpliceSnapshot {
                    sid: pending.sid,
                    target: pending.target,
                    fences: pending.fences,
                })
        }
    }

    /// Reset lane (clear pending splice).
    pub fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = None;
        }
    }

    pub fn abort_sid(&self, sid: SessionId) {
        unsafe {
            let slots = &mut *self.lanes.get();
            for slot in slots.iter_mut() {
                if let Some(pending) = slot
                    && pending.sid == sid
                {
                    *slot = None;
                }
            }
        }
    }

    /// Commit a pending splice operation.
    ///
    /// This validates that the pending splice matches the given sid and clears it.
    pub fn commit(&self, lane: Lane, sid: SessionId) -> Result<(), SpliceError> {
        unsafe {
            let slots = &mut *self.lanes.get();
            let idx = lane.raw() as usize;
            match &slots[idx] {
                Some(pending) if pending.sid == sid => {
                    slots[idx] = None;
                    Ok(())
                }
                Some(pending) => Err(SpliceError::UnknownSession { sid: pending.sid }),
                None => Err(SpliceError::NoPending { lane }),
            }
        }
    }
}

/// Distributed splice entry (internal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DistributedSpliceEntry {
    pub(crate) intent: SpliceIntent,
    pub(crate) phase: DistributedSplicePhase,
}

impl DistributedSpliceEntry {
    fn sid(&self) -> SessionId {
        SessionId::new(self.intent.sid)
    }

    fn src_rv(&self) -> RendezvousId {
        self.intent.src_rv
    }

    fn dst_rv(&self) -> RendezvousId {
        self.intent.dst_rv
    }
}

/// Distributed splice phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DistributedSplicePhase {
    IntentSent,
    AckReceived(SpliceAck),
}

/// Table for tracking pending distributed splices.
///
/// Uses a small fixed-size array to track splice operations that
/// span multiple Rendezvous instances. Keyed by (sid, src_rv, dst_rv).
pub struct DistributedSpliceTable {
    entries: UnsafeCell<[Option<DistributedSpliceEntry>; 8]>, // Max 8 concurrent distributed splices
    _no_send_sync: PhantomData<*mut ()>,
}

/// Maximum number of staged distributed splice plans retained per rendezvous.
impl Default for DistributedSpliceTable {
    fn default() -> Self {
        Self::new()
    }
}

impl DistributedSpliceTable {
    pub const fn new() -> Self {
        Self {
            entries: UnsafeCell::new([None; 8]),
            _no_send_sync: PhantomData,
        }
    }

    /// Insert a new splice intent.
    pub fn insert(&self, intent: SpliceIntent) -> Result<(), SpliceError> {
        unsafe {
            let entries = &mut *self.entries.get();
            for slot in entries.iter_mut() {
                if slot.is_none() {
                    *slot = Some(DistributedSpliceEntry {
                        intent,
                        phase: DistributedSplicePhase::IntentSent,
                    });
                    return Ok(());
                }
            }
            Err(SpliceError::PendingTableFull)
        }
    }

    /// Update with received ack.
    pub fn update_with_ack(&self, ack: &SpliceAck) -> Result<(), SpliceError> {
        unsafe {
            let entries = &mut *self.entries.get();
            for slot in entries.iter_mut() {
                if let Some(entry) = slot
                    && entry.sid() == SessionId::new(ack.sid)
                    && entry.src_rv() == ack.src_rv
                    && entry.dst_rv() == ack.dst_rv
                {
                    entry.phase = DistributedSplicePhase::AckReceived(*ack);
                    return Ok(());
                }
            }
            Err(SpliceError::UnknownSession {
                sid: SessionId::new(ack.sid),
            })
        }
    }

    /// Take (consume) a distributed splice entry.
    pub(crate) fn take(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
        dst_rv: RendezvousId,
    ) -> Option<DistributedSpliceEntry> {
        unsafe {
            let entries = &mut *self.entries.get();
            for slot in entries.iter_mut() {
                if let Some(entry) = slot
                    && entry.sid() == sid
                    && entry.src_rv() == src_rv
                    && entry.dst_rv() == dst_rv
                {
                    return slot.take();
                }
            }
            None
        }
    }

    /// Clear all entries for a session.
    pub fn clear(&self, sid: SessionId) {
        unsafe {
            let entries = &mut *self.entries.get();
            for slot in entries.iter_mut() {
                if let Some(entry) = slot
                    && entry.sid() == sid
                {
                    *slot = None;
                }
            }
        }
    }
}
