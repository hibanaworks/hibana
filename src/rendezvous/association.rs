//! Association table for session/lane claims.
//!
//! Tracks active `(SessionId, Lane)` claims, local attachment counts, and
//! per-session fault state.
//!
//! # Unsafe Owner Contract
//!
//! This module owns the session/lane association storage. Unsafe blocks here may
//! access backing arrays only through the table's entry capacity and must keep
//! sid, lane, and packed state columns synchronized.

use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
};

use crate::session::types::{Lane, SessionId};

mod fault;
mod storage;
pub(crate) use fault::SessionFaultKind;

const ENTRY_COUNT_BITS: u32 = 5;
const ENTRY_COUNT_MASK: u8 = (1u8 << ENTRY_COUNT_BITS) - 1;
const ENTRY_FAULT_SHIFT: u32 = ENTRY_COUNT_BITS;
const ENTRY_COUNT_MAX: u8 = crate::g::ROLE_DOMAIN_SIZE;

/// Association table for active `(session, lane)` claims.
///
/// Lane validity is checked against the configured lane range. Storage entries
/// are independent from lane indices so multiple sessions can hold the same
/// physical lane without aliasing release authority.
pub(super) struct AssocTable {
    lane_base: Cell<u32>,
    lane_slots: Cell<u16>,
    assoc_slots: Cell<u16>,
    entry_sids: UnsafeCell<*mut SessionId>,
    entry_lanes: UnsafeCell<*mut u8>,
    entry_states: UnsafeCell<*mut u8>,
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
        if count > ENTRY_COUNT_MAX || fault > (u8::MAX >> ENTRY_FAULT_SHIFT) {
            crate::invariant();
        }
        (fault << ENTRY_FAULT_SHIFT) | count
    }

    const EMPTY_ENTRY_STATE: u8 = Self::entry_state(0, SessionFaultKind::ABSENT_CODE);
    const LIVE_ENTRY_STATE: u8 = Self::entry_state(1, SessionFaultKind::ABSENT_CODE);

    #[inline]
    fn lane_slots(&self) -> usize {
        self.lane_slots.get() as usize
    }

    #[inline]
    pub(super) fn assoc_slots(&self) -> usize {
        self.assoc_slots.get() as usize
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
    fn lane_offset(&self, lane: Lane) -> Option<u8> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base.get() {
            return None;
        }
        let offset = lane_raw - self.lane_base.get();
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
    pub(super) fn active_lane_slots(&self) -> usize {
        /* SAFETY: the scan is bounded by `assoc_slots` and reads synchronized
        lane/state columns from initialized entries. */
        unsafe {
            let lanes = self.entry_lanes_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            let mut required = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0 {
                    required = required.max(*lanes.add(idx) as usize + 1);
                }
                idx += 1;
            }
            required
        }
    }

    #[inline]
    pub(super) fn shrink_lane_slots(&self, required_lane_slots: usize) {
        if required_lane_slots > self.lane_slots()
            || required_lane_slots < self.active_lane_slots()
            || required_lane_slots > usize::from(u16::MAX)
        {
            crate::invariant();
        }
        self.lane_slots.set(required_lane_slots as u16);
    }

    #[inline]
    pub(super) fn has_entry(&self, lane: Lane, sid: SessionId) -> bool {
        let Some(lane_offset) = self.lane_offset(lane) else {
            return false;
        };
        self.find_entry_by_offset(lane_offset, sid).is_some()
    }

    #[inline]
    pub(super) fn has_session(&self, sid: SessionId) -> bool {
        /* SAFETY: the scan is bounded by `assoc_slots` and reads only initialized
        session/state columns. */
        unsafe {
            let sids = self.entry_sids_ptr();
            let states = self.entry_states_ptr();
            let mut idx = 0usize;
            while idx < self.assoc_slots() {
                if Self::entry_count(*states.add(idx)) != 0 && *sids.add(idx) == sid {
                    return true;
                }
                idx += 1;
            }
        }
        false
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
        /* SAFETY: the scan is bounded by `assoc_slots`. Register updates the
        sid/lane/state columns as one empty-entry transition. */
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
        let lane_offset = crate::invariant_some(self.lane_offset(lane));
        let idx = crate::invariant_some(self.find_entry_by_offset(lane_offset, sid));
        /* SAFETY: `find_entry_by_offset` bounds `idx` inside the assoc columns.
        Increment writes only the count for the matching entry. */
        unsafe {
            let states = self.entry_states_ptr();
            let raw = *states.add(idx);
            let current = Self::entry_count(raw);
            if current == 0 {
                crate::invariant();
            }
            if current == ENTRY_COUNT_MAX {
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
    /// Returns the remaining attachment count after the decrement.
    #[inline]
    pub(super) fn decrement(&self, lane: Lane, sid: SessionId) -> u8 {
        let lane_offset = crate::invariant_some(self.lane_offset(lane));
        let idx = crate::invariant_some(self.find_entry_by_offset(lane_offset, sid));
        /* SAFETY: `find_entry_by_offset` bounds `idx` inside the assoc columns.
        Decrement updates the count and clears the entry only when the last
        reference for `(lane, sid)` is released. */
        unsafe {
            let states = self.entry_states_ptr();
            let raw = *states.add(idx);
            let current = Self::entry_count(raw);
            if current == 0 {
                crate::invariant();
            }
            let next = current - 1;
            if next == 0 {
                self.remove_entry(idx);
            } else {
                states
                    .add(idx)
                    .write(Self::entry_state(next, Self::entry_fault_code(raw)));
            }
            next
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
