//! Association table for session/lane claims.
//!
//! Tracks active `(SessionId, Lane)` claims, their local attachment counts,
//! waiters, and per-session fault state.
//!
//! # Unsafe Owner Contract
//!
//! This module owns the session/lane association storage. Unsafe blocks here may
//! access backing arrays only through the table's entry capacity and must keep
//! sid, lane, packed state, and waiter columns synchronized before waking waiters.

use core::{cell::UnsafeCell, marker::PhantomData, task::Waker};

use crate::session::types::{Lane, SessionId};

use super::waiter::WaiterSlot;

mod storage;

const ENTRY_COUNT_BITS: u32 = 5;
const ENTRY_COUNT_MASK: u8 = (1u8 << ENTRY_COUNT_BITS) - 1;
const ENTRY_FAULT_SHIFT: u32 = ENTRY_COUNT_BITS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionFaultKind {
    TransportClosed,
    PeerReset,
    DecodeFailed,
    ProtocolViolation,
    EndpointDropped,
    ProgressInvariantViolated,
}

impl SessionFaultKind {
    const ABSENT_CODE: u8 = 0;

    #[inline]
    const fn encode(self) -> u8 {
        match self {
            Self::TransportClosed => 1,
            Self::PeerReset => 2,
            Self::DecodeFailed => 3,
            Self::ProtocolViolation => 4,
            Self::EndpointDropped => 5,
            Self::ProgressInvariantViolated => 6,
        }
    }

    #[inline]
    const fn decode(raw: u8) -> Option<Self> {
        match raw {
            Self::ABSENT_CODE => None,
            1 => Some(Self::TransportClosed),
            2 => Some(Self::PeerReset),
            3 => Some(Self::DecodeFailed),
            4 => Some(Self::ProtocolViolation),
            5 => Some(Self::EndpointDropped),
            6 => Some(Self::ProgressInvariantViolated),
            _ => crate::invariant(),
        }
    }
}

/// Association table for active `(session, lane)` claims.
///
/// Lane validity is checked against the configured lane range. Storage entries
/// are independent from lane indices so multiple sessions can hold the same
/// physical lane without aliasing release authority.
pub(super) struct AssocTable {
    lane_base: u32,
    lane_slots: u16,
    assoc_slots: u16,
    entry_sids: UnsafeCell<*mut SessionId>,
    entry_lanes: UnsafeCell<*mut u8>,
    entry_states: UnsafeCell<*mut u8>,
    inline_waiter: UnsafeCell<WaiterSlot>,
    waiters: UnsafeCell<*mut WaiterSlot>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl AssocTable {
    #[inline]
    const fn entry_count(raw: u8) -> u8 {
        raw & ENTRY_COUNT_MASK
    }

    #[inline]
    const fn entry_fault_code(raw: u8) -> u8 {
        raw >> ENTRY_FAULT_SHIFT
    }

    #[inline]
    const fn entry_fault(raw: u8) -> Option<SessionFaultKind> {
        SessionFaultKind::decode(Self::entry_fault_code(raw))
    }

    #[inline]
    const fn entry_state(count: u8, fault: u8) -> u8 {
        if count > ENTRY_COUNT_MASK || fault > (u8::MAX >> ENTRY_FAULT_SHIFT) {
            crate::invariant();
        }
        (fault << ENTRY_FAULT_SHIFT) | count
    }

    const EMPTY_ENTRY_STATE: u8 = Self::entry_state(0, SessionFaultKind::ABSENT_CODE);
    const LIVE_ENTRY_STATE: u8 = Self::entry_state(1, SessionFaultKind::ABSENT_CODE);

    #[inline]
    fn lane_slots(&self) -> usize {
        self.lane_slots as usize
    }

    #[inline]
    pub(super) fn assoc_slots(&self) -> usize {
        self.assoc_slots as usize
    }

    #[inline]
    fn entry_sids_ptr(&self) -> *mut SessionId {
        /* SAFETY: `entry_sids` is written only by assoc storage binding, and
        callers must scan within `assoc_slots` before dereferencing it. */
        unsafe { *self.entry_sids.get() }
    }

    #[inline]
    fn entry_lanes_ptr(&self) -> *mut u8 {
        /* SAFETY: `entry_lanes` is written only by assoc storage binding, and
        callers must scan within `assoc_slots` before dereferencing it. */
        unsafe { *self.entry_lanes.get() }
    }

    #[inline]
    fn entry_states_ptr(&self) -> *mut u8 {
        /* SAFETY: `entry_states` is installed with the assoc entry range and is
        read or written only after a matching entry lookup or bounded entry scan. */
        unsafe { *self.entry_states.get() }
    }

    #[inline]
    fn waiters_ptr(&self) -> *mut WaiterSlot {
        /* SAFETY: `waiters` is initialized with one overflow `WaiterSlot` per
        assoc entry after the inline first entry. */
        unsafe { *self.waiters.get() }
    }

    #[inline]
    fn lane_offset(&self, lane: Lane) -> Option<u8> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base {
            return None;
        }
        let offset = lane_raw - self.lane_base;
        if (offset as usize) >= self.lane_slots() || offset > u8::MAX as u32 {
            return None;
        }
        Some(offset as u8)
    }

    #[inline]
    fn find_entry_by_offset(&self, lane_offset: u8, sid: SessionId) -> Option<usize> {
        /* SAFETY: the scan is bounded by `assoc_slots` and only reads initialized columns. */
        unsafe {
            let lanes = self.entry_lanes_ptr();
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0
                    && *lanes.add(idx) == lane_offset
                    && *sids.add(idx) == sid
                {
                    return Some(idx);
                }
                idx += 1;
            }
        }
        None
    }

    #[inline]
    fn lane_has_entries_by_offset(&self, lane_offset: u8) -> bool {
        /* SAFETY: the scan is bounded by `assoc_slots` and only reads initialized columns. */
        unsafe {
            let lanes = self.entry_lanes_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0 && *lanes.add(idx) == lane_offset {
                    return true;
                }
                idx += 1;
            }
        }
        false
    }

    #[inline]
    pub(super) fn active_entry_count(&self) -> usize {
        /* SAFETY: the scan is bounded by `assoc_slots` and reads only count bytes. */
        unsafe {
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            let mut live = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0 {
                    live += 1;
                }
                idx += 1;
            }
            live
        }
    }

    #[inline]
    pub(super) fn has_entry(&self, lane: Lane, sid: SessionId) -> bool {
        let Some(lane_offset) = self.lane_offset(lane) else {
            return false;
        };
        self.find_entry_by_offset(lane_offset, sid).is_some()
    }

    #[inline]
    pub(super) fn lane_has_entries(&self, lane: Lane) -> bool {
        let Some(lane_offset) = self.lane_offset(lane) else {
            return false;
        };
        self.lane_has_entries_by_offset(lane_offset)
    }

    /// Register a session/lane claim in an empty association entry.
    #[inline]
    pub(super) fn register(&self, lane: Lane, sid: SessionId) -> bool {
        let Some(lane_offset) = self.lane_offset(lane) else {
            crate::invariant();
        };
        if self.find_entry_by_offset(lane_offset, sid).is_some() {
            crate::invariant();
        }
        /* SAFETY: the scan is bounded by `assoc_slots`. Register updates
        sid/lane/state/waiter columns as one empty-entry transition. */
        unsafe {
            let lanes = self.entry_lanes_ptr();
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) == 0 {
                    lanes.add(idx).write(lane_offset);
                    sids.add(idx).write(sid);
                    states.add(idx).write(Self::LIVE_ENTRY_STATE);
                    (*self.waiter_ptr(idx)).clear();
                    return true;
                }
                idx += 1;
            }
        }
        false
    }

    /// Increment the attachment count for an existing `(lane, sid)` claim.
    ///
    /// Returns the new attachment count on success.
    #[inline]
    pub(super) fn increment(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let lane_offset = self.lane_offset(lane)?;
        let idx = self.find_entry_by_offset(lane_offset, sid)?;
        /* SAFETY: `find_entry_by_offset` bounds `idx` inside the assoc columns.
        Increment writes only the count for the matching entry. */
        unsafe {
            let states = self.entry_states_ptr();
            let raw = *states.add(idx);
            let current = Self::entry_count(raw);
            if current == 0 {
                return None;
            }
            if current == ENTRY_COUNT_MASK {
                return None;
            }
            let next = current + 1;
            states
                .add(idx)
                .write(Self::entry_state(next, Self::entry_fault_code(raw)));
            Some(next)
        }
    }

    /// Decrement the attachment count for `lane` associated with `sid`.
    ///
    /// Returns the remaining attachment count after the decrement, or `None`
    /// if the lane was not associated with `sid`.
    #[inline]
    pub(super) fn decrement(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let lane_offset = self.lane_offset(lane)?;
        let idx = self.find_entry_by_offset(lane_offset, sid)?;
        /* SAFETY: `find_entry_by_offset` bounds `idx` inside the assoc columns.
        Decrement updates the count and clears the entry only when the last
        reference for `(lane, sid)` is released. */
        unsafe {
            let states = self.entry_states_ptr();
            let raw = *states.add(idx);
            let current = Self::entry_count(raw);
            if current == 0 {
                return None;
            }
            let next = current - 1;
            if next == 0 {
                self.remove_entry(idx);
            } else {
                states
                    .add(idx)
                    .write(Self::entry_state(next, Self::entry_fault_code(raw)));
            }
            Some(next)
        }
    }

    #[inline]
    pub(super) fn for_each_lane(&self, sid: SessionId, mut visit: impl FnMut(Lane)) {
        /* SAFETY: the scan is bounded by `0..assoc_slots` for the current assoc
        columns. It only reads sid/lane/state tuples and yields lanes derived
        from the same table's `lane_base`. */
        unsafe {
            let lanes = self.entry_lanes_ptr();
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0 && *sids.add(idx) == sid {
                    visit(Lane::new(self.lane_base + *lanes.add(idx) as u32));
                }
                idx += 1;
            }
        }
    }

    #[inline]
    pub(super) fn register_waiter(&self, sid: SessionId, lane: Lane, waker: &Waker) {
        let Some(lane_offset) = self.lane_offset(lane) else {
            return;
        };
        let Some(idx) = self.find_entry_by_offset(lane_offset, sid) else {
            return;
        };
        /* SAFETY: `find_entry_by_offset` bounds the waiter slot before replacing
        the waiter for that `(sid, lane)` claim. */
        unsafe {
            let states = self.entry_states_ptr();
            if Self::entry_count(*states.add(idx)) == 0 {
                return;
            }
            (*self.waiter_ptr(idx)).set(waker);
        }
    }

    #[inline]
    pub(super) fn clear_waiter(&self, sid: SessionId, lane: Lane) {
        let Some(lane_offset) = self.lane_offset(lane) else {
            return;
        };
        let Some(idx) = self.find_entry_by_offset(lane_offset, sid) else {
            return;
        };
        /* SAFETY: `find_entry_by_offset` bounds the waiter slot before clearing
        the waiter for that `(sid, lane)` claim. */
        unsafe {
            let states = self.entry_states_ptr();
            if Self::entry_count(*states.add(idx)) == 0 {
                return;
            }
            (*self.waiter_ptr(idx)).clear();
        }
    }

    #[inline]
    pub(super) fn wake_session_waiters(&self, sid: SessionId) {
        /* SAFETY: the scan is bounded by the current assoc entry count. Wakers
        are read from the waiter column only for slots whose paired sid/state
        columns belong to `sid`. */
        unsafe {
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0 && *sids.add(idx) == sid {
                    (*self.waiter_ptr(idx)).wake();
                }
                idx += 1;
            }
        }
    }

    #[inline]
    pub(super) fn session_fault(&self, sid: SessionId) -> Option<SessionFaultKind> {
        /* SAFETY: the scan is bounded by `assoc_slots` and reads the packed
        state only from slots whose sid/state pair belongs to the queried session. */
        unsafe {
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                let raw = *states.add(idx);
                if Self::entry_count(raw) != 0
                    && *sids.add(idx) == sid
                    && let Some(kind) = Self::entry_fault(raw)
                {
                    return Some(kind);
                }
                idx += 1;
            }
            None
        }
    }

    #[inline]
    pub(super) fn poison_session(
        &self,
        sid: SessionId,
        cause: SessionFaultKind,
    ) -> SessionFaultKind {
        if let Some(existing) = self.session_fault(sid) {
            return existing;
        }
        /* SAFETY: the scan is bounded by `assoc_slots` and writes the encoded
        fault only into slots whose sid/state pair belongs to the poisoned
        session. */
        unsafe {
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let encoded = cause.encode();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                let raw = *states.add(idx);
                let count = Self::entry_count(raw);
                if count != 0 && *sids.add(idx) == sid {
                    states.add(idx).write(Self::entry_state(count, encoded));
                }
                idx += 1;
            }
        }
        cause
    }
}
