//! Association table for mapping session IDs to lanes.
//!
//! Maintains bidirectional mapping between session IDs and lanes,
//! plus active/inactive status tracking.

use core::{cell::UnsafeCell, marker::PhantomData};

use super::types::{Lane, SessionId};
use crate::runtime::consts::LANES_MAX;

/// Association table (session ID ↔ lane mapping).
///
/// Tracks which lane is assigned to each session and whether it's active.
pub struct AssocTable {
    lane_to_sid: UnsafeCell<[Option<SessionId>; LANES_MAX as usize]>,
    ref_counts: UnsafeCell<[u8; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for AssocTable {
    fn default() -> Self {
        Self::new()
    }
}

impl AssocTable {
    pub const fn new() -> Self {
        Self {
            lane_to_sid: UnsafeCell::new([None; LANES_MAX as usize]),
            ref_counts: UnsafeCell::new([0; LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    #[inline]
    fn idx(lane: Lane) -> usize {
        lane.raw() as usize
    }

    /// Register a session on a lane that is currently unused.
    #[inline]
    pub fn register(&self, lane: Lane, sid: SessionId) {
        unsafe {
            let idx = Self::idx(lane);
            let sids = &mut *self.lane_to_sid.get();
            let counts = &mut *self.ref_counts.get();
            debug_assert!(
                counts[idx] == 0,
                "register called on lane with active attachments"
            );
            sids[idx] = Some(sid);
            counts[idx] = 1;
        }
    }

    /// Increment the attachment count for a lane already associated with `sid`.
    ///
    /// Returns the new attachment count on success.
    #[inline]
    pub fn increment(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        unsafe {
            let idx = Self::idx(lane);
            let sids = &mut *self.lane_to_sid.get();
            let counts = &mut *self.ref_counts.get();
            match sids[idx] {
                Some(existing) if existing == sid => {
                    let current = counts[idx];
                    if current == u8::MAX {
                        return None;
                    }
                    let next = current + 1;
                    counts[idx] = next;
                    Some(next)
                }
                _ => None,
            }
        }
    }

    /// Decrement the attachment count for `lane` associated with `sid`.
    ///
    /// Returns the remaining attachment count after the decrement, or `None`
    /// if the lane was not associated with `sid`.
    #[inline]
    pub fn decrement(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        unsafe {
            let idx = Self::idx(lane);
            let sids = &mut *self.lane_to_sid.get();
            let counts = &mut *self.ref_counts.get();
            match sids[idx] {
                Some(existing) if existing == sid => {
                    let current = counts[idx];
                    debug_assert!(current > 0, "decrement on zero-count lane");
                    let next = current - 1;
                    counts[idx] = next;
                    if next == 0 {
                        sids[idx] = None;
                    }
                    Some(next)
                }
                _ => None,
            }
        }
    }

    /// Reset the association for a lane, clearing the session and count.
    #[inline]
    pub fn unregister_lane(&self, lane: Lane) {
        unsafe {
            let idx = Self::idx(lane);
            (*self.lane_to_sid.get())[idx] = None;
            (*self.ref_counts.get())[idx] = 0;
        }
    }

    /// Find lane for a session ID.
    ///
    /// # Performance
    /// LANES_MAX is intentionally small (<=16) so a linear scan keeps the
    /// implementation no_alloc-friendly. If the maximum increases in the
    /// future consider introducing a tiny sid→lane index.
    #[inline]
    pub fn find_lane(&self, sid: SessionId) -> Option<Lane> {
        unsafe {
            let lanes = &*self.lane_to_sid.get();
            for (idx, entry) in lanes.iter().enumerate() {
                if entry.map(|stored| stored == sid).unwrap_or(false) {
                    return Some(Lane::new(idx as u32));
                }
            }
            None
        }
    }

    /// Check if a lane is active.
    #[inline]
    pub fn is_active(&self, lane: Lane) -> bool {
        unsafe { (*self.ref_counts.get())[Self::idx(lane)] > 0 }
    }

    /// Number of active attachments on a lane.
    #[inline]
    pub fn active_count(&self, lane: Lane) -> u8 {
        unsafe { (*self.ref_counts.get())[Self::idx(lane)] }
    }

    /// Get session ID for a lane (if registered).
    #[inline]
    pub fn get_sid(&self, lane: Lane) -> Option<SessionId> {
        unsafe { (*self.lane_to_sid.get())[Self::idx(lane)] }
    }
}
