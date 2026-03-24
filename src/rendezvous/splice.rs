//! Splice operations and state tracking.
//!
//! Implements SpliceTxn (two-step splice protocol), SpliceStateTable (local splices),
//! and DistributedSpliceTable (cross-Rendezvous splices).

use core::{cell::UnsafeCell, marker::PhantomData};

use super::error::SpliceError;
use crate::{
    control::{
        automaton::distributed::SpliceIntent,
        automaton::txn::InAcked,
        types::{
            AtMostOnceCommit, Generation, Lane, NoCrossLaneAliasing, One, RendezvousId, SessionId,
        },
    },
    runtime::consts::LANES_MAX,
};

/// Invariant marker for local splice transactions evaluated inside a rendezvous.
///
/// Guarantees that lane ownership is unique (no cross-lane aliasing) and that
/// commits happen at most once per transaction.
pub(super) struct LocalSpliceInvariant;

impl NoCrossLaneAliasing for LocalSpliceInvariant {}
impl AtMostOnceCommit for LocalSpliceInvariant {}

/// Pending splice state tracked per lane.
pub(super) struct PendingSplice {
    sid: SessionId,
    target: Generation,
    state: InAcked<LocalSpliceInvariant, One>,
    fences: Option<(u32, u32)>,
}

impl PendingSplice {
    pub(super) fn new(
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
    #[allow(clippy::type_complexity)]
    pub(super) fn into_parts(
        self,
    ) -> (
        SessionId,
        Generation,
        InAcked<LocalSpliceInvariant, One>,
        Option<(u32, u32)>,
    ) {
        (self.sid, self.target, self.state, self.fences)
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

/// Local splice state table (per-lane).
///
/// Tracks pending splice operations within a single Rendezvous instance.
pub(super) struct SpliceStateTable {
    lanes: UnsafeCell<[Option<PendingSplice>; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for SpliceStateTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SpliceStateTable {
    pub(super) const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([const { None }; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    /// Begin a splice operation.
    pub(super) fn begin(&self, lane: Lane, pending: PendingSplice) -> Result<(), SpliceError> {
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
    pub(super) fn take(&self, lane: Lane) -> Option<PendingSplice> {
        unsafe {
            let slots = &mut *self.lanes.get();
            let idx = lane.raw() as usize;
            slots[idx].take()
        }
    }

    /// Reset lane (clear pending splice).
    pub(super) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = None;
        }
    }

    /// Commit a pending splice operation.
    ///
    /// This validates that the pending splice matches the given sid and clears it.
    pub(super) fn commit(&self, lane: Lane, sid: SessionId) -> Result<(), SpliceError> {
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
pub(super) struct DistributedSpliceEntry {
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
pub(super) enum DistributedSplicePhase {
    IntentSent,
}

/// Table for tracking pending distributed splices.
///
/// Uses a small fixed-size array to track splice operations that
/// span multiple Rendezvous instances. Keyed by (sid, src_rv, dst_rv).
pub(super) struct DistributedSpliceTable {
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
    pub(super) const fn new() -> Self {
        Self {
            entries: UnsafeCell::new([None; 8]),
            _no_send_sync: PhantomData,
        }
    }

    /// Insert a new splice intent.
    pub(super) fn insert(&self, intent: SpliceIntent) -> Result<(), SpliceError> {
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

    /// Take (consume) a distributed splice entry.
    pub(super) fn take(
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
}
