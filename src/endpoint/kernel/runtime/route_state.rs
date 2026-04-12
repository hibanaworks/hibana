//! Route-state owner for endpoint kernel runtime bookkeeping.

use super::evidence::RouteArmState;
use super::evidence_store::{ScopeEvidenceSlot, ScopeEvidenceTable};
use super::frontier::{LaneOfferState, MAX_ROUTE_ARM_STACK};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::MAX_LANES;

#[derive(Clone, Copy)]
struct RouteArmStackView {
    ptr: *mut RouteArmState,
    lane_dense_by_lane: [u8; MAX_LANES],
    depth: u8,
}

impl RouteArmStackView {
    unsafe fn init(
        dst: *mut Self,
        ptr: *mut RouteArmState,
        lane_dense_by_lane: &[u8; MAX_LANES],
        depth: usize,
    ) {
        if depth > u8::MAX as usize {
            panic!("route arm stack depth overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(*lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).depth).write(depth as u8);
        }
        let total = Self::allocated_slots(lane_dense_by_lane, depth);
        let mut idx = 0usize;
        while idx < total {
            unsafe {
                ptr.add(idx).write(RouteArmState::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn allocated_slots(lane_dense_by_lane: &[u8; MAX_LANES], depth: usize) -> usize {
        let mut lanes = 0usize;
        let mut idx = 0usize;
        while idx < MAX_LANES {
            if lane_dense_by_lane[idx] != u8::MAX {
                lanes += 1;
            }
            idx += 1;
        }
        lanes.saturating_mul(depth)
    }

    #[inline]
    fn lane_dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let dense = self.lane_dense_by_lane[lane_idx];
        if dense == u8::MAX {
            None
        } else {
            Some(dense as usize)
        }
    }

    #[inline]
    fn depth(&self) -> usize {
        self.depth as usize
    }

    #[inline]
    fn get(&self, lane_idx: usize, arm_idx: usize) -> RouteArmState {
        let Some(dense) = self.lane_dense_ordinal(lane_idx) else {
            return RouteArmState::EMPTY;
        };
        let depth = self.depth();
        if arm_idx >= depth {
            return RouteArmState::EMPTY;
        }
        unsafe { *self.ptr.add(dense * depth + arm_idx) }
    }

    #[inline]
    fn set(&mut self, lane_idx: usize, arm_idx: usize, state: RouteArmState) -> bool {
        let Some(dense) = self.lane_dense_ordinal(lane_idx) else {
            return false;
        };
        let depth = self.depth();
        if arm_idx >= depth {
            return false;
        }
        unsafe {
            self.ptr.add(dense * depth + arm_idx).write(state);
        }
        true
    }
}

#[derive(Clone, Copy)]
struct LaneOfferStateView {
    ptr: *mut LaneOfferState,
    lane_dense_by_lane: [u8; MAX_LANES],
    len: u8,
}

impl LaneOfferStateView {
    unsafe fn init(
        dst: *mut Self,
        ptr: *mut LaneOfferState,
        lane_dense_by_lane: &[u8; MAX_LANES],
        len: usize,
    ) {
        if len > u8::MAX as usize {
            panic!("lane offer state capacity overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(*lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).len).write(len as u8);
        }
        let mut idx = 0usize;
        while idx < len {
            unsafe {
                ptr.add(idx).write(LaneOfferState::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn lane_dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let dense = self.lane_dense_by_lane[lane_idx];
        if dense == u8::MAX {
            None
        } else {
            Some(dense as usize)
        }
    }

    #[inline]
    fn get(&self, lane_idx: usize) -> LaneOfferState {
        let Some(dense) = self.lane_dense_ordinal(lane_idx) else {
            return LaneOfferState::EMPTY;
        };
        if dense >= self.len as usize {
            return LaneOfferState::EMPTY;
        }
        unsafe { *self.ptr.add(dense) }
    }

    #[inline]
    fn get_mut(&mut self, lane_idx: usize) -> Option<&mut LaneOfferState> {
        let dense = self.lane_dense_ordinal(lane_idx)?;
        if dense >= self.len as usize {
            return None;
        }
        Some(unsafe { &mut *self.ptr.add(dense) })
    }
}

pub(super) struct RouteState {
    lane_route_arms: RouteArmStackView,
    lane_offer_states: LaneOfferStateView,
    pub(super) scope_evidence: ScopeEvidenceTable,
    pub(super) lane_route_arm_lens: [u8; MAX_LANES],
    pub(super) lane_linger_counts: [u8; MAX_LANES],
    pub(super) lane_linger_mask: u8,
    pub(super) lane_offer_linger_mask: u8,
    pub(super) active_offer_mask: u8,
}

impl RouteState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        route_arm_storage: *mut RouteArmState,
        lane_offer_state_storage: *mut LaneOfferState,
        scope_evidence_slots: *mut ScopeEvidenceSlot,
        lane_dense_by_lane: &[u8; MAX_LANES],
        lane_offer_state_count: usize,
        route_frame_depth: usize,
        scope_evidence_count: usize,
    ) {
        unsafe {
            RouteArmStackView::init(
                core::ptr::addr_of_mut!((*dst).lane_route_arms),
                route_arm_storage,
                lane_dense_by_lane,
                route_frame_depth,
            );
            LaneOfferStateView::init(
                core::ptr::addr_of_mut!((*dst).lane_offer_states),
                lane_offer_state_storage,
                lane_dense_by_lane,
                lane_offer_state_count,
            );
            ScopeEvidenceTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).scope_evidence),
                scope_evidence_slots,
                scope_evidence_count,
            );

            let lens_ptr = core::ptr::addr_of_mut!((*dst).lane_route_arm_lens).cast::<u8>();
            let mut lane_idx = 0usize;
            while lane_idx < MAX_LANES {
                lens_ptr.add(lane_idx).write(0);
                lane_idx += 1;
            }

            let linger_ptr = core::ptr::addr_of_mut!((*dst).lane_linger_counts).cast::<u8>();
            let mut linger_idx = 0usize;
            while linger_idx < MAX_LANES {
                linger_ptr.add(linger_idx).write(0);
                linger_idx += 1;
            }

            core::ptr::addr_of_mut!((*dst).lane_linger_mask).write(0);
            core::ptr::addr_of_mut!((*dst).lane_offer_linger_mask).write(0);
            core::ptr::addr_of_mut!((*dst).active_offer_mask).write(0);
        }
    }

    #[inline]
    pub(super) fn lane_route_arm_len(&self, lane_idx: usize) -> usize {
        self.lane_route_arm_lens.get(lane_idx).copied().unwrap_or(0) as usize
    }

    pub(super) fn set_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        arm: u8,
        is_linger: bool,
    ) -> Result<(), ()> {
        let len = self.lane_route_arm_len(lane_idx);
        for idx in 0..len {
            if self.lane_route_arms.get(lane_idx, idx).scope == scope {
                self.lane_route_arms
                    .set(lane_idx, idx, RouteArmState { scope, arm });
                return Ok(());
            }
        }

        if len >= self.lane_route_arms.depth() {
            return Err(());
        }
        if !self
            .lane_route_arms
            .set(lane_idx, len, RouteArmState { scope, arm })
        {
            return Err(());
        }
        self.lane_route_arm_lens[lane_idx] = self.lane_route_arm_lens[lane_idx].saturating_add(1);
        if is_linger {
            self.increment_linger_count(lane_idx);
        }
        Ok(())
    }

    pub(super) fn pop_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        is_linger: bool,
    ) -> bool {
        let len = self.lane_route_arm_len(lane_idx);
        if len == 0 {
            return false;
        }
        let Some(pos) = (0..len)
            .rev()
            .find(|&idx| self.lane_route_arms.get(lane_idx, idx).scope == scope)
        else {
            return false;
        };

        let last = len - 1;
        for idx in pos..last {
            let next = self.lane_route_arms.get(lane_idx, idx + 1);
            self.lane_route_arms.set(lane_idx, idx, next);
        }
        self.lane_route_arms
            .set(lane_idx, last, RouteArmState::EMPTY);
        self.lane_route_arm_lens[lane_idx] = self.lane_route_arm_lens[lane_idx].saturating_sub(1);
        if is_linger {
            self.decrement_linger_count(lane_idx);
        }
        true
    }

    pub(super) fn collect_lane_scopes<F>(
        &self,
        lane_idx: usize,
        out: &mut [ScopeId; MAX_ROUTE_ARM_STACK],
        mut include: F,
    ) -> usize
    where
        F: FnMut(ScopeId) -> bool,
    {
        let len = self.lane_route_arm_len(lane_idx);
        let mut out_len = 0usize;
        let mut idx = 0usize;
        while idx < len {
            let slot = self.lane_route_arms.get(lane_idx, idx);
            let scope = slot.scope;
            if include(scope) {
                out[out_len] = scope;
                out_len += 1;
            }
            idx += 1;
        }
        out_len
    }

    pub(super) fn route_arm_for(&self, lane_idx: usize, scope: ScopeId) -> Option<u8> {
        let len = self.lane_route_arm_len(lane_idx);
        (0..len).rev().find_map(|idx| {
            let slot = self.lane_route_arms.get(lane_idx, idx);
            (slot.scope == scope).then_some(slot.arm)
        })
    }

    pub(super) fn active_linger_scope_for_lane<F>(
        &self,
        lane_idx: usize,
        mut is_linger_route: F,
    ) -> Option<ScopeId>
    where
        F: FnMut(ScopeId) -> bool,
    {
        let len = self.lane_route_arm_len(lane_idx);
        let mut idx = len;
        while idx > 0 {
            idx -= 1;
            let slot = self.lane_route_arms.get(lane_idx, idx);
            let scope = slot.scope;
            if scope.is_none() || slot.arm != 0 {
                continue;
            }
            if is_linger_route(scope) {
                return Some(scope);
            }
        }
        None
    }

    #[inline]
    pub(super) fn increment_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count < u8::MAX);
        *count = count.saturating_add(1);
        if *count == 1 {
            self.lane_linger_mask |= 1u8 << lane_idx;
        }
    }

    #[inline]
    pub(super) fn decrement_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count > 0);
        if *count == 0 {
            return;
        }
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.lane_linger_mask &= !(1u8 << lane_idx);
        }
    }

    #[inline]
    pub(super) fn lane_offer_state(&self, lane_idx: usize) -> LaneOfferState {
        self.lane_offer_states.get(lane_idx)
    }

    #[inline]
    pub(super) fn lane_offer_state_mut(&mut self, lane_idx: usize) -> Option<&mut LaneOfferState> {
        self.lane_offer_states.get_mut(lane_idx)
    }

    #[inline]
    pub(super) fn clear_lane_offer_state(&mut self, lane_idx: usize) -> LaneOfferState {
        let bit = 1u8 << lane_idx;
        let old = self.lane_offer_state(lane_idx);
        if let Some(state) = self.lane_offer_state_mut(lane_idx) {
            *state = LaneOfferState::EMPTY;
        }
        self.active_offer_mask &= !bit;
        self.lane_offer_linger_mask &= !bit;
        old
    }

    #[inline]
    pub(super) fn set_lane_offer_state(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
        is_linger: bool,
    ) {
        let bit = 1u8 << lane_idx;
        let Some(state) = self.lane_offer_state_mut(lane_idx) else {
            debug_assert!(false, "lane offer state must exist for active lanes");
            return;
        };
        *state = info;
        self.active_offer_mask |= bit;
        if is_linger {
            self.lane_offer_linger_mask |= bit;
        } else {
            self.lane_offer_linger_mask &= !bit;
        }
    }
}
