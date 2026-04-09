//! Association table for mapping session IDs to lanes.
//!
//! Maintains bidirectional mapping between session IDs and lanes,
//! plus active/inactive status tracking.

use core::{cell::UnsafeCell, marker::PhantomData};

use crate::control::types::{Lane, SessionId};
use crate::runtime::consts::LANES_MAX;

/// Association table (session ID ↔ lane mapping).
///
/// Tracks which lane is assigned to each session and whether it's active.
pub(super) struct AssocTable {
    lane_to_sid: UnsafeCell<[SessionId; LANES_MAX as usize]>,
    ref_counts: UnsafeCell<[u8; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for AssocTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AssocTable {
    pub(super) const fn new() -> Self {
        Self {
            lane_to_sid: UnsafeCell::new([SessionId::new(0); LANES_MAX as usize]),
            ref_counts: UnsafeCell::new([0; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    pub(super) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let sid_ptr = core::ptr::addr_of_mut!((*dst).lane_to_sid).cast::<SessionId>();
            let count_ptr = core::ptr::addr_of_mut!((*dst).ref_counts).cast::<u8>();
            let mut idx = 0usize;
            while idx < LANES_MAX as usize {
                sid_ptr.add(idx).write(SessionId::new(0));
                count_ptr.add(idx).write(0);
                idx += 1;
            }
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    fn idx(lane: Lane) -> usize {
        lane.raw() as usize
    }

    /// Register a session on a lane that is currently unused.
    #[inline]
    pub(super) fn register(&self, lane: Lane, sid: SessionId) {
        unsafe {
            let idx = Self::idx(lane);
            let sids = &mut *self.lane_to_sid.get();
            let counts = &mut *self.ref_counts.get();
            debug_assert!(
                counts[idx] == 0,
                "register called on lane with active attachments"
            );
            sids[idx] = sid;
            counts[idx] = 1;
        }
    }

    /// Increment the attachment count for a lane already associated with `sid`.
    ///
    /// Returns the new attachment count on success.
    #[inline]
    pub(super) fn increment(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        unsafe {
            let idx = Self::idx(lane);
            let sids = &mut *self.lane_to_sid.get();
            let counts = &mut *self.ref_counts.get();
            let current = counts[idx];
            if current == 0 || sids[idx] != sid {
                return None;
            }
            if current == u8::MAX {
                return None;
            }
            let next = current + 1;
            counts[idx] = next;
            Some(next)
        }
    }

    /// Decrement the attachment count for `lane` associated with `sid`.
    ///
    /// Returns the remaining attachment count after the decrement, or `None`
    /// if the lane was not associated with `sid`.
    #[inline]
    pub(super) fn decrement(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        unsafe {
            let idx = Self::idx(lane);
            let sids = &mut *self.lane_to_sid.get();
            let counts = &mut *self.ref_counts.get();
            let current = counts[idx];
            if current == 0 || sids[idx] != sid {
                return None;
            }
            let next = current - 1;
            counts[idx] = next;
            if next == 0 {
                sids[idx] = SessionId::new(0);
            }
            Some(next)
        }
    }

    /// Find lane for a session ID.
    ///
    /// # Performance
    /// LANES_MAX is intentionally small (<=8) so a linear scan keeps the
    /// implementation no_alloc-friendly. If the maximum increases in the
    /// future consider introducing a tiny sid→lane index.
    #[inline]
    pub(super) fn find_lane(&self, sid: SessionId) -> Option<Lane> {
        unsafe {
            let lanes = &*self.lane_to_sid.get();
            let counts = &*self.ref_counts.get();
            for (idx, stored_sid) in lanes.iter().enumerate() {
                if counts[idx] != 0 && *stored_sid == sid {
                    return Some(Lane::new(idx as u32));
                }
            }
            None
        }
    }

    /// Check if a lane is active.
    #[inline]
    pub(super) fn is_active(&self, lane: Lane) -> bool {
        unsafe { (*self.ref_counts.get())[Self::idx(lane)] > 0 }
    }

    /// Get session ID for a lane (if registered).
    #[inline]
    pub(super) fn get_sid(&self, lane: Lane) -> Option<SessionId> {
        unsafe {
            let idx = Self::idx(lane);
            let counts = &*self.ref_counts.get();
            (counts[idx] != 0).then_some((*self.lane_to_sid.get())[idx])
        }
    }
}
