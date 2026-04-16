//! Association table for mapping session IDs to lanes.
//!
//! Maintains bidirectional mapping between session IDs and lanes,
//! plus active/inactive status tracking.

use core::{cell::UnsafeCell, marker::PhantomData};

use crate::control::types::{Lane, SessionId};

/// Association table (session ID ↔ lane mapping).
///
/// Tracks which lane is assigned to each lane slot inside the configured
/// rendezvous lane range and whether it is active.
pub(super) struct AssocTable {
    lane_base: u32,
    lane_slots: u16,
    lane_to_sid: UnsafeCell<*mut SessionId>,
    ref_counts: UnsafeCell<*mut u8>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for AssocTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl AssocTable {
    pub(super) const fn empty() -> Self {
        Self {
            lane_base: 0,
            lane_slots: 0,
            lane_to_sid: UnsafeCell::new(core::ptr::null_mut()),
            ref_counts: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(super) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_to_sid)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).ref_counts)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(super) const fn storage_align() -> usize {
        let sid_align = core::mem::align_of::<SessionId>();
        let count_align = core::mem::align_of::<u8>();
        if sid_align > count_align {
            sid_align
        } else {
            count_align
        }
    }

    #[inline]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline]
    pub(super) const fn storage_bytes(lane_slots: usize) -> usize {
        let sid_bytes = lane_slots.saturating_mul(core::mem::size_of::<SessionId>());
        let count_offset = Self::align_up(sid_bytes, core::mem::align_of::<u8>());
        count_offset.saturating_add(lane_slots.saturating_mul(core::mem::size_of::<u8>()))
    }

    unsafe fn bind_storage(
        &mut self,
        lane_base: u32,
        lane_slots: usize,
        lane_to_sid: *mut SessionId,
        ref_counts: *mut u8,
    ) {
        let mut idx = 0usize;
        while idx < lane_slots {
            unsafe {
                lane_to_sid.add(idx).write(SessionId::new(0));
                ref_counts.add(idx).write(0);
            }
            idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_to_sid.get_mut() = lane_to_sid;
        *self.ref_counts.get_mut() = ref_counts;
    }

    pub(super) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let lane_to_sid = storage.cast::<SessionId>();
        let count_offset = Self::align_up(
            storage as usize + lane_slots.saturating_mul(core::mem::size_of::<SessionId>()),
            core::mem::align_of::<u8>(),
        ) - storage as usize;
        let ref_counts = unsafe { storage.add(count_offset) }.cast::<u8>();
        unsafe {
            self.bind_storage(lane_base, lane_slots, lane_to_sid, ref_counts);
        }
    }

    #[inline]
    pub(super) fn is_bound(&self) -> bool {
        !self.lane_to_sid_ptr().is_null()
    }

    #[inline]
    pub(super) fn storage_ptr(&self) -> *mut u8 {
        self.lane_to_sid_ptr().cast::<u8>()
    }

    #[inline]
    pub(super) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.lane_slots as usize)
    }

    #[inline]
    fn lane_slots(&self) -> usize {
        self.lane_slots as usize
    }

    #[inline]
    fn lane_to_sid_ptr(&self) -> *mut SessionId {
        unsafe { *self.lane_to_sid.get() }
    }

    #[inline]
    fn ref_counts_ptr(&self) -> *mut u8 {
        unsafe { *self.ref_counts.get() }
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> Option<usize> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base {
            return None;
        }
        let slot = (lane_raw - self.lane_base) as usize;
        (slot < self.lane_slots()).then_some(slot)
    }

    /// Register a session on a lane that is currently unused.
    #[inline]
    pub(super) fn register(&self, lane: Lane, sid: SessionId) {
        let Some(idx) = self.lane_slot(lane) else {
            debug_assert!(false, "register called for lane outside rendezvous range");
            return;
        };
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            debug_assert!(
                *counts.add(idx) == 0,
                "register called on lane with active attachments"
            );
            sids.add(idx).write(sid);
            counts.add(idx).write(1);
        }
    }

    /// Increment the attachment count for a lane already associated with `sid`.
    ///
    /// Returns the new attachment count on success.
    #[inline]
    pub(super) fn increment(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let idx = self.lane_slot(lane)?;
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let current = *counts.add(idx);
            if current == 0 || *sids.add(idx) != sid {
                return None;
            }
            if current == u8::MAX {
                return None;
            }
            let next = current + 1;
            counts.add(idx).write(next);
            Some(next)
        }
    }

    /// Decrement the attachment count for `lane` associated with `sid`.
    ///
    /// Returns the remaining attachment count after the decrement, or `None`
    /// if the lane was not associated with `sid`.
    #[inline]
    pub(super) fn decrement(&self, lane: Lane, sid: SessionId) -> Option<u8> {
        let idx = self.lane_slot(lane)?;
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let current = *counts.add(idx);
            if current == 0 || *sids.add(idx) != sid {
                return None;
            }
            let next = current - 1;
            counts.add(idx).write(next);
            if next == 0 {
                sids.add(idx).write(SessionId::new(0));
            }
            Some(next)
        }
    }

    /// Find lane for a session ID.
    #[inline]
    pub(super) fn find_lane(&self, sid: SessionId) -> Option<Lane> {
        unsafe {
            let sids = self.lane_to_sid_ptr();
            let counts = self.ref_counts_ptr();
            let mut idx = 0usize;
            while idx < self.lane_slots() {
                if *counts.add(idx) != 0 && *sids.add(idx) == sid {
                    return Some(Lane::new(self.lane_base + idx as u32));
                }
                idx += 1;
            }
            None
        }
    }

    /// Check if a lane is active.
    #[inline]
    pub(super) fn is_active(&self, lane: Lane) -> bool {
        let Some(idx) = self.lane_slot(lane) else {
            return false;
        };
        unsafe { *self.ref_counts_ptr().add(idx) > 0 }
    }

    /// Get session ID for a lane (if registered).
    #[inline]
    pub(super) fn get_sid(&self, lane: Lane) -> Option<SessionId> {
        let idx = self.lane_slot(lane)?;
        unsafe {
            let counts = self.ref_counts_ptr();
            (*counts.add(idx) != 0).then_some(*self.lane_to_sid_ptr().add(idx))
        }
    }
}
