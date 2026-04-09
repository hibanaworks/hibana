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
    lanes: UnsafeCell<*mut Option<PendingSplice>>,
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
            lanes: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(super) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).lanes).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(super) const fn storage_align() -> usize {
        core::mem::align_of::<Option<PendingSplice>>()
    }

    #[inline]
    pub(super) const fn storage_bytes() -> usize {
        LANES_MAX as usize * core::mem::size_of::<Option<PendingSplice>>()
    }

    pub(super) unsafe fn bind_from_storage(&mut self, storage: *mut u8) {
        let lanes = storage.cast::<Option<PendingSplice>>();
        let mut idx = 0usize;
        while idx < LANES_MAX as usize {
            unsafe {
                lanes.add(idx).write(None);
            }
            idx += 1;
        }
        *self.lanes.get_mut() = lanes;
    }

    #[inline]
    pub(super) fn is_bound(&self) -> bool {
        !self.lanes_ptr().is_null()
    }

    #[inline]
    fn lanes_ptr(&self) -> *mut Option<PendingSplice> {
        unsafe { *self.lanes.get() }
    }

    /// Begin a splice operation.
    pub(super) fn begin(&self, lane: Lane, pending: PendingSplice) -> Result<(), SpliceError> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return Err(SpliceError::PendingTableFull);
        }
        unsafe {
            let idx = lane.raw() as usize;
            let slot = &mut *slots.add(idx);
            if slot.is_some() {
                return Err(SpliceError::InProgress { lane });
            }
            *slot = Some(pending);
            Ok(())
        }
    }

    /// Take (consume) pending splice.
    pub(super) fn take(&self, lane: Lane) -> Option<PendingSplice> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return None;
        }
        unsafe {
            let idx = lane.raw() as usize;
            (*slots.add(idx)).take()
        }
    }

    /// Reset lane (clear pending splice).
    pub(super) fn reset_lane(&self, lane: Lane) {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return;
        }
        unsafe {
            *slots.add(lane.raw() as usize) = None;
        }
    }

    /// Commit a pending splice operation.
    ///
    /// This validates that the pending splice matches the given sid and clears it.
    pub(super) fn commit(&self, lane: Lane, sid: SessionId) -> Result<(), SpliceError> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return Err(SpliceError::NoPending { lane });
        }
        unsafe {
            let idx = lane.raw() as usize;
            let slot = &mut *slots.add(idx);
            match slot {
                Some(pending) if pending.sid == sid => {
                    *slot = None;
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
}

/// Table for tracking pending distributed splices.
///
/// Uses a small fixed-size array to track splice operations that
/// span multiple Rendezvous instances. Keyed by (sid, src_rv, dst_rv).
pub(super) struct DistributedSpliceTable {
    entries: UnsafeCell<*mut Option<DistributedSpliceEntry>>,
    _no_send_sync: PhantomData<*mut ()>,
}

/// Maximum number of staged distributed splice plans retained per rendezvous.
impl Default for DistributedSpliceTable {
    fn default() -> Self {
        Self::new()
    }
}

impl DistributedSpliceTable {
    const ENTRY_CAPACITY: usize = 8;

    pub(super) const fn new() -> Self {
        Self {
            entries: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(super) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).entries).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(super) const fn storage_align() -> usize {
        core::mem::align_of::<Option<DistributedSpliceEntry>>()
    }

    #[inline]
    pub(super) const fn storage_bytes() -> usize {
        Self::ENTRY_CAPACITY * core::mem::size_of::<Option<DistributedSpliceEntry>>()
    }

    pub(super) unsafe fn bind_from_storage(&mut self, storage: *mut u8) {
        let entries = storage.cast::<Option<DistributedSpliceEntry>>();
        let mut idx = 0usize;
        while idx < Self::ENTRY_CAPACITY {
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        *self.entries.get_mut() = entries;
    }

    #[inline]
    pub(super) fn is_bound(&self) -> bool {
        !self.entries_ptr().is_null()
    }

    #[inline]
    fn entries_ptr(&self) -> *mut Option<DistributedSpliceEntry> {
        unsafe { *self.entries.get() }
    }

    /// Insert a new splice intent.
    pub(super) fn insert(&self, intent: SpliceIntent) -> Result<(), SpliceError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(SpliceError::PendingTableFull);
        }
        unsafe {
            let mut idx = 0usize;
            while idx < Self::ENTRY_CAPACITY {
                let slot = &mut *entries.add(idx);
                if slot.is_none() {
                    *slot = Some(DistributedSpliceEntry { intent });
                    return Ok(());
                }
                idx += 1;
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
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        unsafe {
            let mut idx = 0usize;
            while idx < Self::ENTRY_CAPACITY {
                let slot = &mut *entries.add(idx);
                if let Some(entry) = slot
                    && SessionId::new(entry.intent.sid) == sid
                    && entry.intent.src_rv == src_rv
                    && entry.intent.dst_rv == dst_rv
                {
                    return slot.take();
                }
                idx += 1;
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DistributedSpliceTable, SpliceStateTable};
    use crate::{
        control::{
            automaton::distributed::SpliceIntent,
            types::{Generation, Lane, RendezvousId, SessionId},
        },
        rendezvous::error::SpliceError,
    };

    #[test]
    fn splice_state_table_unbound_reads_as_empty() {
        let table = SpliceStateTable::new();
        let lane = Lane::new(0);
        let sid = SessionId::new(7);

        assert!(!table.is_bound());
        assert!(table.take(lane).is_none());
        assert_eq!(
            table.commit(lane, sid),
            Err(SpliceError::NoPending { lane })
        );
        table.reset_lane(lane);
    }

    #[test]
    fn distributed_splice_table_unbound_reads_as_empty() {
        let table = DistributedSpliceTable::new();
        let sid = SessionId::new(7);
        let src = RendezvousId::new(1);
        let dst = RendezvousId::new(2);
        let intent = SpliceIntent::new(
            src,
            dst,
            sid.raw(),
            Generation(0),
            Generation(1),
            0,
            0,
            Lane::new(0),
            Lane::new(1),
        );

        assert!(!table.is_bound());
        assert_eq!(table.take(sid, src, dst), None);
        assert_eq!(table.insert(intent), Err(SpliceError::PendingTableFull));
    }
}
