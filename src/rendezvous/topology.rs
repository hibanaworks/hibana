//! Topology operations and state tracking.
//!
//! Implements topology transaction helpers and per-lane pending topology state.

use core::{cell::UnsafeCell, marker::PhantomData};

use super::error::TopologyError;
use crate::control::{
    automaton::{distributed::TopologyAck, txn::InAcked},
    types::{AtMostOnceCommit, Generation, Lane, NoCrossLaneAliasing, One, SessionId},
};

mod commit_reservation;
pub(crate) use commit_reservation::{
    PreparedDestinationTopologyCommit, PreparedSourceTopologyCommit,
};

/// Invariant marker for local topology transactions evaluated inside a rendezvous.
///
/// Guarantees that lane ownership is unique (no cross-lane aliasing) and that
/// commits happen at most once per transaction.
pub(super) struct LocalTopologyInvariant;

impl NoCrossLaneAliasing for LocalTopologyInvariant {}
impl AtMostOnceCommit for LocalTopologyInvariant {}

/// Pending topology state tracked per lane.
pub(super) struct PendingTopology {
    sid: SessionId,
    lane: Lane,
    previous_generation: Option<Generation>,
    target: Generation,
    lease_state: TopologyLeaseState,
    state: Option<InAcked<LocalTopologyInvariant, One>>,
    expected_ack: Option<TopologyAck>,
}

pub(super) struct PendingTopologyParts {
    pub lane: Lane,
    pub previous_generation: Option<Generation>,
    pub lease_state: TopologyLeaseState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TopologyLeaseState {
    SourcePrepared,
    SourceCommitReserved,
    DestinationPrepared,
    DestinationCommitReserved,
    DestinationCommitted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopologySessionState {
    SourcePending { lane: Lane },
    DestinationPending { lane: Lane },
    DestinationAttachReady { lane: Lane },
}

impl PendingTopology {
    pub(super) fn source_prepare(
        sid: SessionId,
        lane: Lane,
        previous_generation: Option<Generation>,
        target: Generation,
        state: InAcked<LocalTopologyInvariant, One>,
        expected_ack: TopologyAck,
    ) -> Self {
        Self {
            sid,
            lane,
            previous_generation,
            target,
            lease_state: TopologyLeaseState::SourcePrepared,
            state: Some(state),
            expected_ack: Some(expected_ack),
        }
    }

    pub(super) fn destination_prepare(
        sid: SessionId,
        lane: Lane,
        previous_generation: Option<Generation>,
        target: Generation,
        state: InAcked<LocalTopologyInvariant, One>,
    ) -> Self {
        Self {
            sid,
            lane,
            previous_generation,
            target,
            lease_state: TopologyLeaseState::DestinationPrepared,
            state: Some(state),
            expected_ack: None,
        }
    }

    #[inline]
    pub(super) fn lane(&self) -> Lane {
        self.lane
    }

    #[inline]
    pub(super) const fn expected_ack(&self) -> Option<TopologyAck> {
        self.expected_ack
    }

    #[inline]
    pub(super) const fn session_state(&self) -> TopologySessionState {
        match self.lease_state {
            TopologyLeaseState::SourcePrepared | TopologyLeaseState::SourceCommitReserved => {
                TopologySessionState::SourcePending { lane: self.lane }
            }
            TopologyLeaseState::DestinationPrepared
            | TopologyLeaseState::DestinationCommitReserved => {
                TopologySessionState::DestinationPending { lane: self.lane }
            }
            TopologyLeaseState::DestinationCommitted => {
                TopologySessionState::DestinationAttachReady { lane: self.lane }
            }
        }
    }

    #[inline]
    pub(super) const fn is_attach_ready(&self) -> bool {
        matches!(self.lease_state, TopologyLeaseState::DestinationCommitted)
    }

    #[inline]
    pub(super) fn into_parts(self) -> PendingTopologyParts {
        PendingTopologyParts {
            lane: self.lane,
            previous_generation: self.previous_generation,
            lease_state: self.lease_state,
        }
    }
}

impl core::fmt::Debug for PendingTopology {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PendingTopology")
            .field("sid", &self.sid)
            .field("lane", &self.lane)
            .field("previous_generation", &self.previous_generation)
            .field("target", &self.target)
            .field("lease_state", &self.lease_state)
            .finish()
    }
}

/// Local topology state table (per-lane).
///
/// Tracks pending topology operations within a single Rendezvous instance.
pub(super) struct TopologyStateTable {
    lane_base: u32,
    lane_slots: u16,
    lanes: UnsafeCell<*mut Option<PendingTopology>>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for TopologyStateTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl TopologyStateTable {
    pub(super) const fn empty() -> Self {
        Self {
            lane_base: 0,
            lane_slots: 0,
            lanes: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(super) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lanes).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(super) const fn storage_align() -> usize {
        core::mem::align_of::<Option<PendingTopology>>()
    }

    #[inline]
    pub(super) const fn storage_bytes(lane_slots: usize) -> usize {
        let size = core::mem::size_of::<Option<PendingTopology>>();
        if size != 0 && lane_slots > usize::MAX / size {
            crate::invariant();
        }
        lane_slots * size
    }

    pub(super) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let lanes = storage.cast::<Option<PendingTopology>>();
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                lanes.add(idx).write(None);
            }
            idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lanes.get_mut() = lanes;
    }

    pub(super) unsafe fn rebind_from_storage_preserving(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let old_base = self.lane_base;
        let old_slots = self.lane_slots as usize;
        let old_lanes = self.lanes_ptr();
        let lanes = storage.cast::<Option<PendingTopology>>();
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                lanes.add(idx).write(None);
            }
            idx += 1;
        }
        let mut old_idx = 0usize;
        while old_idx < old_slots {
            let lane = old_base + old_idx as u32;
            if lane >= lane_base {
                let new_idx = (lane - lane_base) as usize;
                if new_idx < lane_slots {
                    /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                    unsafe {
                        lanes.add(new_idx).write((*old_lanes.add(old_idx)).take());
                    }
                }
            }
            old_idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lanes.get_mut() = lanes;
    }

    #[inline]
    pub(super) fn is_bound(&self) -> bool {
        !self.lanes_ptr().is_null()
    }

    #[inline]
    pub(super) const fn lane_slots(&self) -> usize {
        self.lane_slots as usize
    }

    #[inline]
    pub(super) fn storage_ptr(&self) -> *mut u8 {
        self.lanes_ptr().cast::<u8>()
    }

    #[inline]
    pub(super) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.lane_slots as usize)
    }

    #[inline]
    fn lanes_ptr(&self) -> *mut Option<PendingTopology> {
        /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */
        unsafe { *self.lanes.get() }
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> Option<usize> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base {
            return None;
        }
        let slot = (lane_raw - self.lane_base) as usize;
        (slot < self.lane_slots as usize).then_some(slot)
    }

    pub(super) fn pending_lane_for_sid(&self, sid: SessionId) -> Option<Lane> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return None;
        }

        let mut idx = 0usize;
        while idx < self.lane_slots as usize {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if let Some(pending) = (&*slots.add(idx)).as_ref()
                    && pending.sid == sid
                {
                    return Some(pending.lane());
                }
            }
            idx += 1;
        }

        None
    }

    pub(super) fn preflight_begin(&self, lane: Lane, sid: SessionId) -> Result<(), TopologyError> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return Ok(());
        }
        if let Some(existing_lane) = self.pending_lane_for_sid(sid) {
            return Err(TopologyError::InProgress {
                lane: existing_lane,
            });
        }
        let Some(idx) = self.lane_slot(lane) else {
            return Ok(());
        };
        /* SAFETY: the offset was checked against the backing allocation before raw access. */
        unsafe {
            if (&*slots.add(idx)).is_some() {
                Err(TopologyError::InProgress { lane })
            } else {
                Ok(())
            }
        }
    }

    pub(super) fn take_pending_for_sid(&self, sid: SessionId) -> Option<PendingTopology> {
        let lane = self.pending_lane_for_sid(sid)?;
        self.take(lane)
    }

    pub(super) fn session_state(&self, sid: SessionId) -> Option<TopologySessionState> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return None;
        }

        let mut idx = 0usize;
        while idx < self.lane_slots as usize {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let Some(pending) = (&*slots.add(idx)).as_ref() else {
                    idx += 1;
                    continue;
                };
                if pending.sid == sid {
                    return Some(pending.session_state());
                }
            }
            idx += 1;
        }

        None
    }

    /// Begin a topology operation.
    pub(super) fn begin(&self, lane: Lane, pending: PendingTopology) -> Result<(), TopologyError> {
        let slots = self.lanes_ptr();
        let Some(idx) = self.lane_slot(lane) else {
            return Err(TopologyError::PendingTableFull);
        };
        if let Some(existing_lane) = self.pending_lane_for_sid(pending.sid) {
            return Err(TopologyError::InProgress {
                lane: existing_lane,
            });
        }
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let slot = &mut *slots.add(idx);
            if slot.is_some() {
                return Err(TopologyError::InProgress { lane });
            }
            *slot = Some(pending);
            Ok(())
        }
    }

    /// Take (consume) pending topology state.
    pub(super) fn take(&self, lane: Lane) -> Option<PendingTopology> {
        let slots = self.lanes_ptr();
        let idx = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { (*slots.add(idx)).take() }
    }

    /// Validate that the given lane still owns a pending topology transition for `sid`.
    pub(super) fn preflight_commit(&self, lane: Lane, sid: SessionId) -> Result<(), TopologyError> {
        let slots = self.lanes_ptr();
        let Some(idx) = self.lane_slot(lane) else {
            return Err(TopologyError::NoPending { lane });
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            match (&*slots.add(idx)).as_ref() {
                Some(pending)
                    if pending.sid == sid
                        && matches!(
                            pending.lease_state,
                            TopologyLeaseState::DestinationPrepared
                        ) =>
                {
                    Ok(())
                }
                Some(pending) if pending.sid == sid => Err(TopologyError::InProgress { lane }),
                Some(pending) => Err(TopologyError::UnknownSession { sid: pending.sid }),
                None => Err(TopologyError::NoPending { lane }),
            }
        }
    }

    /// Return the expected distributed-topology ACK for a pending session.
    pub(super) fn expected_ack_for_session(
        &self,
        sid: SessionId,
    ) -> Result<TopologyAck, TopologyError> {
        let slots = self.lanes_ptr();
        if slots.is_null() {
            return Err(TopologyError::UnknownSession { sid });
        }

        let mut idx = 0usize;
        while idx < self.lane_slots as usize {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let Some(pending) = (&*slots.add(idx)).as_ref() else {
                    idx += 1;
                    continue;
                };
                if pending.sid != sid {
                    idx += 1;
                    continue;
                }
                return pending.expected_ack().ok_or(TopologyError::NoPending {
                    lane: pending.lane(),
                });
            }
        }

        Err(TopologyError::UnknownSession { sid })
    }

    /// Reset lane (clear pending topology state).
    pub(super) fn reset_lane(&self, lane: Lane) {
        let slots = self.lanes_ptr();
        let Some(idx) = self.lane_slot(lane) else {
            return;
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *slots.add(idx) = None;
        }
    }

    pub(super) fn attach_ready_sid(&self, lane: Lane) -> Option<SessionId> {
        let slots = self.lanes_ptr();
        let idx = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            match (&*slots.add(idx)).as_ref() {
                Some(pending) if pending.is_attach_ready() => Some(pending.sid),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TopologyStateTable;
    use crate::{
        control::types::{Lane, SessionId},
        rendezvous::error::TopologyError,
    };

    #[test]
    fn topology_state_table_unbound_reads_as_empty() {
        let table = TopologyStateTable::empty();
        let lane = Lane::new(0);
        let sid = SessionId::new(7);

        assert!(!table.is_bound());
        assert!(table.take(lane).is_none());
        assert_eq!(
            table.preflight_commit(lane, sid),
            Err(TopologyError::NoPending { lane })
        );
        table.reset_lane(lane);
    }
}
