//! Route-state owner for endpoint kernel runtime bookkeeping.

use super::evidence::RouteArmState;
use super::evidence_store::{ScopeEvidenceSlot, ScopeEvidenceTable};
use super::frontier::LaneOfferState;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{LaneSet, LaneSetView, LaneWord};
const NO_SELECTED_ARM: u8 = u8::MAX;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct RouteScopeSelectedArmSlot {
    arm: u8,
    refs: u16,
}

impl RouteScopeSelectedArmSlot {
    const EMPTY: Self = Self {
        arm: NO_SELECTED_ARM,
        refs: 0,
    };
}

#[derive(Clone, Copy)]
struct RouteArmStackView {
    ptr: *mut RouteArmState,
    lane_dense_by_lane: *mut u8,
    lane_slot_count: usize,
    active_lane_count: usize,
    depth: u8,
}

impl RouteArmStackView {
    unsafe fn init(
        dst: *mut Self,
        ptr: *mut RouteArmState,
        lane_dense_by_lane: *mut u8,
        lane_slot_count: usize,
        active_lane_count: usize,
        depth: usize,
    ) {
        if depth > u8::MAX as usize {
            panic!("route arm stack depth overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).active_lane_count).write(active_lane_count);
            core::ptr::addr_of_mut!((*dst).depth).write(depth as u8);
        }
        let total = active_lane_count.saturating_mul(depth);
        let mut idx = 0usize;
        while idx < total {
            unsafe {
                ptr.add(idx).write(RouteArmState::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn lane_dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.lane_slot_count {
            return None;
        }
        let dense = unsafe { *self.lane_dense_by_lane.add(lane_idx) };
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
    lane_dense_by_lane: *mut u8,
    lane_slot_count: usize,
    len: usize,
}

impl LaneOfferStateView {
    unsafe fn init(
        dst: *mut Self,
        ptr: *mut LaneOfferState,
        lane_dense_by_lane: *mut u8,
        lane_slot_count: usize,
        len: usize,
    ) {
        if len > u8::MAX as usize {
            panic!("lane offer state capacity overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).len).write(len);
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
        if lane_idx >= self.lane_slot_count {
            return None;
        }
        let dense = unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == u8::MAX || dense as usize >= self.len {
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
    scope_selected_arms: *mut RouteScopeSelectedArmSlot,
    scope_selected_arm_count: usize,
    lane_route_arm_lens: *mut u8,
    lane_linger_counts: *mut u8,
    active_route_lanes: LaneSet,
    lane_linger_lanes: LaneSet,
    lane_offer_linger_lanes: LaneSet,
    active_offer_lanes: LaneSet,
}

impl RouteState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        route_arm_storage: *mut RouteArmState,
        lane_offer_state_storage: *mut LaneOfferState,
        scope_evidence_slots: *mut ScopeEvidenceSlot,
        scope_selected_arms: *mut RouteScopeSelectedArmSlot,
        lane_dense_by_lane: *mut u8,
        lane_slot_count: usize,
        lane_route_arm_lens: *mut u8,
        lane_linger_counts: *mut u8,
        active_route_lane_words: *mut LaneWord,
        lane_linger_words: *mut LaneWord,
        lane_offer_linger_words: *mut LaneWord,
        active_offer_lane_words: *mut LaneWord,
        active_lane_count: usize,
        lane_word_count: usize,
        lane_offer_state_count: usize,
        route_frame_depth: usize,
        scope_evidence_count: usize,
        scope_selected_arm_count: usize,
    ) {
        unsafe {
            RouteArmStackView::init(
                core::ptr::addr_of_mut!((*dst).lane_route_arms),
                route_arm_storage,
                lane_dense_by_lane,
                lane_slot_count,
                active_lane_count,
                route_frame_depth,
            );
            LaneOfferStateView::init(
                core::ptr::addr_of_mut!((*dst).lane_offer_states),
                lane_offer_state_storage,
                lane_dense_by_lane,
                lane_slot_count,
                lane_offer_state_count,
            );
            ScopeEvidenceTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).scope_evidence),
                scope_evidence_slots,
                scope_evidence_count,
            );
            core::ptr::addr_of_mut!((*dst).scope_selected_arms).write(scope_selected_arms);
            core::ptr::addr_of_mut!((*dst).scope_selected_arm_count)
                .write(scope_selected_arm_count);
            core::ptr::addr_of_mut!((*dst).lane_route_arm_lens).write(lane_route_arm_lens);
            core::ptr::addr_of_mut!((*dst).lane_linger_counts).write(lane_linger_counts);
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).active_route_lanes),
                active_route_lane_words,
                lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lane_linger_lanes),
                lane_linger_words,
                lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lane_offer_linger_lanes),
                lane_offer_linger_words,
                lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).active_offer_lanes),
                active_offer_lane_words,
                lane_word_count,
            );
            let mut lane_idx = 0usize;
            while lane_idx < active_lane_count {
                lane_route_arm_lens.add(lane_idx).write(0);
                lane_linger_counts.add(lane_idx).write(0);
                lane_idx += 1;
            }
            let mut scope_idx = 0usize;
            while scope_idx < scope_selected_arm_count {
                scope_selected_arms
                    .add(scope_idx)
                    .write(RouteScopeSelectedArmSlot::EMPTY);
                scope_idx += 1;
            }
        }
    }

    #[inline]
    pub(super) fn lane_route_arm_len(&self, lane_idx: usize) -> usize {
        self.lane_offer_states
            .lane_dense_ordinal(lane_idx)
            .map(|dense| unsafe { *self.lane_route_arm_lens.add(dense) as usize })
            .unwrap_or(0)
    }

    pub(super) fn set_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        arm: u8,
        is_linger: bool,
    ) -> Result<(), ()> {
        let len = self.lane_route_arm_len(lane_idx);
        for idx in 0..len {
            let current = self.lane_route_arms.get(lane_idx, idx);
            if current.scope == scope {
                if current.arm != arm {
                    self.replace_scope_selected_arm(scope_slot, current.arm, arm)?;
                }
                self.lane_route_arms
                    .set(lane_idx, idx, RouteArmState { scope, arm });
                self.active_route_lanes.insert(lane_idx);
                return Ok(());
            }
        }

        if len >= self.lane_route_arms.depth() {
            return Err(());
        }
        if self
            .lane_offer_states
            .lane_dense_ordinal(lane_idx)
            .is_none()
        {
            return Err(());
        }
        self.increment_scope_selected_arm(scope_slot, arm)?;
        if !self
            .lane_route_arms
            .set(lane_idx, len, RouteArmState { scope, arm })
        {
            return Err(());
        }
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            return Err(());
        };
        unsafe {
            let count = self.lane_route_arm_lens.add(dense);
            count.write((*count).saturating_add(1));
        }
        self.active_route_lanes.insert(lane_idx);
        if is_linger {
            self.increment_linger_count(lane_idx);
        }
        Ok(())
    }

    pub(super) fn pop_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
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

        let removed = self.lane_route_arms.get(lane_idx, pos);
        let last = len - 1;
        for idx in pos..last {
            let next = self.lane_route_arms.get(lane_idx, idx + 1);
            self.lane_route_arms.set(lane_idx, idx, next);
        }
        self.lane_route_arms
            .set(lane_idx, last, RouteArmState::EMPTY);
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            return false;
        };
        unsafe {
            let count = self.lane_route_arm_lens.add(dense);
            count.write((*count).saturating_sub(1));
        }
        if !self.decrement_scope_selected_arm(scope_slot, removed.arm) {
            return false;
        }
        if self.lane_route_arm_len(lane_idx) == 0 {
            self.active_route_lanes.remove(lane_idx);
        }
        if is_linger {
            self.decrement_linger_count(lane_idx);
        }
        true
    }

    #[inline]
    pub(super) fn last_lane_scope(&self, lane_idx: usize) -> Option<ScopeId> {
        let len = self.lane_route_arm_len(lane_idx);
        if len == 0 {
            None
        } else {
            Some(self.lane_route_arms.get(lane_idx, len - 1).scope)
        }
    }

    pub(super) fn route_arm_for(&self, lane_idx: usize, scope: ScopeId) -> Option<u8> {
        let len = self.lane_route_arm_len(lane_idx);
        (0..len).rev().find_map(|idx| {
            let slot = self.lane_route_arms.get(lane_idx, idx);
            (slot.scope == scope).then_some(slot.arm)
        })
    }

    #[inline]
    pub(super) fn selected_arm_for_scope_slot(&self, scope_slot: usize) -> Option<u8> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = unsafe { *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 || slot.arm == NO_SELECTED_ARM {
            None
        } else {
            Some(slot.arm)
        }
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
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            return;
        };
        unsafe {
            let count = &mut *self.lane_linger_counts.add(dense);
            debug_assert!(*count < u8::MAX);
            *count = count.saturating_add(1);
            if *count == 1 {
                self.lane_linger_lanes.insert(lane_idx);
            }
        }
    }

    #[inline]
    pub(super) fn decrement_linger_count(&mut self, lane_idx: usize) {
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            return;
        };
        unsafe {
            let count = &mut *self.lane_linger_counts.add(dense);
            debug_assert!(*count > 0);
            if *count == 0 {
                return;
            }
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.lane_linger_lanes.remove(lane_idx);
            }
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
    fn increment_scope_selected_arm(&mut self, scope_slot: usize, arm: u8) -> Result<(), ()> {
        if scope_slot >= self.scope_selected_arm_count {
            return Err(());
        }
        let slot = unsafe { &mut *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 {
            slot.arm = arm;
            slot.refs = 1;
            return Ok(());
        }
        if slot.arm != arm || slot.refs == u16::MAX {
            return Err(());
        }
        slot.refs += 1;
        Ok(())
    }

    #[inline]
    fn replace_scope_selected_arm(
        &mut self,
        scope_slot: usize,
        old_arm: u8,
        new_arm: u8,
    ) -> Result<(), ()> {
        if scope_slot >= self.scope_selected_arm_count {
            return Err(());
        }
        let slot = unsafe { &mut *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 || slot.arm != old_arm {
            return Err(());
        }
        if old_arm == new_arm {
            return Ok(());
        }
        if slot.refs != 1 {
            return Err(());
        }
        slot.arm = new_arm;
        Ok(())
    }

    #[inline]
    fn decrement_scope_selected_arm(&mut self, scope_slot: usize, arm: u8) -> bool {
        if scope_slot >= self.scope_selected_arm_count {
            return false;
        }
        let slot = unsafe { &mut *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 || slot.arm != arm {
            return false;
        }
        slot.refs -= 1;
        if slot.refs == 0 {
            *slot = RouteScopeSelectedArmSlot::EMPTY;
        }
        true
    }

    #[inline]
    pub(super) fn clear_lane_offer_state(&mut self, lane_idx: usize) -> LaneOfferState {
        let old = self.lane_offer_state(lane_idx);
        if let Some(state) = self.lane_offer_state_mut(lane_idx) {
            *state = LaneOfferState::EMPTY;
        }
        self.active_offer_lanes.remove(lane_idx);
        self.lane_offer_linger_lanes.remove(lane_idx);
        old
    }

    #[inline]
    pub(super) fn set_lane_offer_state(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
        is_linger: bool,
    ) {
        let Some(state) = self.lane_offer_state_mut(lane_idx) else {
            debug_assert!(false, "lane offer state must exist for active lanes");
            return;
        };
        *state = info;
        self.active_offer_lanes.insert(lane_idx);
        if is_linger {
            self.lane_offer_linger_lanes.insert(lane_idx);
        } else {
            self.lane_offer_linger_lanes.remove(lane_idx);
        }
    }

    #[inline]
    pub(super) fn active_offer_lanes(&self) -> LaneSetView {
        self.active_offer_lanes.view()
    }

    #[inline]
    pub(super) fn lane_linger_lanes(&self) -> LaneSetView {
        self.lane_linger_lanes.view()
    }

    #[inline]
    pub(super) fn lane_offer_linger_lanes(&self) -> LaneSetView {
        self.lane_offer_linger_lanes.view()
    }
}
