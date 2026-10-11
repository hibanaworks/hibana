//! Descriptor-derived sparse route-arm history storage.
//!
//! # Unsafe Owner Contract
//! The view owns one packed route-state column partitioned by active lane.
//! Descriptor lane ordinals and per-lane lengths are initialized before the
//! endpoint is published. Every mutation preserves the packed partition and
//! updates lane and total lengths only after the row move completes.

use super::{DENSE_LANE_ABSENT, DenseLaneOrdinal, RouteArmState, ScopeId};

pub(super) struct RouteArmHistoryView {
    ptr: *mut RouteArmState,
    lane_lengths: *mut u16,
    lane_dense_by_lane: *mut DenseLaneOrdinal,
    lane_slot_count: usize,
    active_lane_count: usize,
    capacity: u16,
    len: u16,
}

impl RouteArmHistoryView {
    pub(super) unsafe fn init(
        dst: *mut Self,
        ptr: *mut RouteArmState,
        lane_lengths: *mut u16,
        lane_dense_by_lane: *mut DenseLaneOrdinal,
        lane_slot_count: usize,
        active_lane_count: usize,
        capacity: usize,
    ) {
        if capacity > u16::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: `RouteState::init_empty` passes an unpublished route history;
        the descriptor-derived sparse column is installed and initialized before
        exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_lengths).write(lane_lengths);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).active_lane_count).write(active_lane_count);
            core::ptr::addr_of_mut!((*dst).capacity).write(capacity as u16);
            core::ptr::addr_of_mut!((*dst).len).write(0);
        }
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: `idx < capacity` bounds one cell in the unpublished
            sparse-history owner; every cell is initialized before publication. */
            unsafe {
                ptr.add(idx).write(RouteArmState::EMPTY);
            }
            idx += 1;
        }
        let mut lane = 0usize;
        while lane < active_lane_count {
            /* SAFETY: `lane < active_lane_count` bounds one lane-length cell
            in the unpublished history owner, initialized before publication. */
            unsafe {
                lane_lengths.add(lane).write(0);
            }
            lane += 1;
        }
    }

    #[inline]
    pub(super) const fn capacity(&self) -> usize {
        self.capacity as usize
    }

    #[inline]
    pub(super) const fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    fn lane_dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.lane_slot_count {
            return None;
        }
        let dense = /* SAFETY: `lane_idx < lane_slot_count` bounds the immutable
        compiled lane map. */ unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == DENSE_LANE_ABSENT || dense.get() >= self.active_lane_count {
            None
        } else {
            Some(dense.get())
        }
    }

    #[inline]
    fn lane_range(&self, lane_idx: usize) -> Option<(usize, usize, usize)> {
        let dense = self.lane_dense_ordinal(lane_idx)?;
        let mut start = 0usize;
        let mut current = 0usize;
        while current < dense {
            start += /* SAFETY: `current < dense < active_lane_count` bounds an
            initialized lane length. */ unsafe { *self.lane_lengths.add(current) as usize };
            current += 1;
        }
        let len = /* SAFETY: `dense < active_lane_count` bounds the initialized
        lane length cell. */ unsafe { *self.lane_lengths.add(dense) as usize };
        if start + len > self.len() {
            crate::invariant();
        }
        Some((dense, start, len))
    }

    #[inline]
    pub(super) fn lane_len(&self, lane_idx: usize) -> usize {
        self.lane_range(lane_idx).map_or(0, |(_, _, len)| len)
    }

    #[inline]
    pub(super) fn get(&self, lane_idx: usize, lane_pos: usize) -> RouteArmState {
        let Some((_, start, len)) = self.lane_range(lane_idx) else {
            return RouteArmState::EMPTY;
        };
        if lane_pos >= len {
            return RouteArmState::EMPTY;
        }
        /* SAFETY: `start + lane_pos < start + len <= self.len` bounds one
        initialized packed history row. */
        unsafe { *self.ptr.add(start + lane_pos) }
    }

    #[inline]
    pub(super) fn set(
        &mut self,
        lane_idx: usize,
        lane_pos: usize,
        scope: ScopeId,
        arm: u8,
    ) -> bool {
        let Some((_, start, len)) = self.lane_range(lane_idx) else {
            return false;
        };
        if lane_pos >= len {
            return false;
        }
        /* SAFETY: `start + lane_pos < self.len`; `&mut self` owns the row. */
        unsafe {
            self.ptr
                .add(start + lane_pos)
                .write(RouteArmState::new(scope, arm));
        }
        true
    }

    #[inline]
    pub(super) fn has_capacity(&self) -> bool {
        self.len() < self.capacity()
    }

    pub(super) fn push(&mut self, lane_idx: usize, scope: ScopeId, arm: u8) -> bool {
        let Some((dense, start, lane_len)) = self.lane_range(lane_idx) else {
            return false;
        };
        if !self.has_capacity() || lane_len == u16::MAX as usize {
            return false;
        }
        let insert_idx = start + lane_len;
        let len = self.len();
        /* SAFETY: `insert_idx <= len < capacity`; shift the initialized suffix
        one cell right before publishing the new lane length and total length. */
        unsafe {
            core::ptr::copy(
                self.ptr.add(insert_idx),
                self.ptr.add(insert_idx + 1),
                len - insert_idx,
            );
            self.ptr
                .add(insert_idx)
                .write(RouteArmState::new(scope, arm));
            self.lane_lengths.add(dense).write((lane_len + 1) as u16);
        }
        self.len += 1;
        true
    }

    pub(super) fn remove(&mut self, lane_idx: usize, lane_pos: usize) -> bool {
        let Some((dense, start, lane_len)) = self.lane_range(lane_idx) else {
            return false;
        };
        if lane_pos >= lane_len {
            return false;
        }
        let remove_idx = start + lane_pos;
        let len = self.len();
        /* SAFETY: `remove_idx < len`; compact the initialized suffix, clear the
        stale tail, then publish both shorter lengths. */
        unsafe {
            core::ptr::copy(
                self.ptr.add(remove_idx + 1),
                self.ptr.add(remove_idx),
                len - remove_idx - 1,
            );
            self.ptr.add(len - 1).write(RouteArmState::EMPTY);
            self.lane_lengths.add(dense).write((lane_len - 1) as u16);
        }
        self.len -= 1;
        true
    }
}
