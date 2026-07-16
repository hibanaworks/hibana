use super::{CommitDeltaApplyPermit, RouteScopeSelectedArmSlot, RouteState};
use crate::global::const_dsl::ScopeId;

pub(in crate::endpoint::kernel) enum ReentryScopeLiveness {
    NotReentry,
    Incomplete,
    Complete,
}

impl RouteState {
    pub(in crate::endpoint::kernel) fn replace_route_selection_arm_for_scope(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        old_arm: u8,
        new_arm: u8,
    ) {
        let len = self.lane_route_arm_len(lane_idx);
        let mut idx = 0usize;
        while idx < len {
            let current = self.lane_route_arms.get(lane_idx, idx);
            if current.scope == scope
                && current.arm == old_arm
                && !self.lane_route_arms.set(lane_idx, idx, scope, new_arm)
            {
                crate::invariant();
            }
            idx += 1;
        }
    }

    pub(in crate::endpoint::kernel) fn replace_selected_arm_slot(
        &mut self,
        scope_slot: usize,
        old_arm: u8,
        new_arm: u8,
    ) {
        if scope_slot >= self.scope_selected_arm_count {
            crate::invariant();
        }
        let selected_slot = /* SAFETY: `RouteState` owns the selected-arm column; `scope_slot < scope_selected_arm_count` bounds an initialized slot and this `&mut self` call is the only mutation path for that slot. */ unsafe {
            &mut *self.scope_selected_arms.add(scope_slot)
        };
        if selected_slot.refs != 0 && selected_slot.arm == old_arm {
            selected_slot.arm = new_arm;
        }
    }

    pub(in crate::endpoint::kernel) fn active_reentry_scope_for_lane<F>(
        &self,
        lane_idx: usize,
        mut classify_reentry_scope: F,
    ) -> Option<ScopeId>
    where
        F: FnMut(ScopeId) -> ReentryScopeLiveness,
    {
        let mut completed_reentry = None;
        let len = self.lane_route_arm_len(lane_idx);
        let mut idx = len;
        while idx > 0 {
            idx -= 1;
            let slot = self.lane_route_arms.get(lane_idx, idx);
            let scope = slot.scope;
            if scope.is_none() {
                continue;
            }
            match classify_reentry_scope(scope) {
                ReentryScopeLiveness::NotReentry => {}
                ReentryScopeLiveness::Incomplete => {
                    return completed_reentry.or(Some(scope));
                }
                ReentryScopeLiveness::Complete => completed_reentry = Some(scope),
            }
        }
        completed_reentry
    }

    pub(in crate::endpoint::kernel) fn clear_lane_route_selections_in_scope<F, G, H>(
        &mut self,
        lane_idx: usize,
        mut contains_route_scope: F,
        mut route_scope_slot: G,
        mut route_scope_reentry: H,
        _permit: CommitDeltaApplyPermit,
    ) where
        F: FnMut(ScopeId) -> bool,
        G: FnMut(ScopeId) -> Option<usize>,
        H: FnMut(ScopeId) -> bool,
    {
        if self
            .lane_offer_states
            .lane_dense_ordinal(lane_idx)
            .is_none()
        {
            crate::invariant();
        }
        let mut len = self.lane_route_arms.lane_len(lane_idx);
        let mut idx = 0usize;
        while idx < len {
            let current = self.lane_route_arms.get(lane_idx, idx);
            if current.scope.is_none() || !contains_route_scope(current.scope) {
                idx += 1;
                continue;
            }
            let scope_slot = crate::invariant_some(route_scope_slot(current.scope));
            let next_slot = self.prepare_selected_arm_ref_release(scope_slot);
            len -= 1;
            if !self.lane_route_arms.remove(lane_idx, idx) {
                crate::invariant();
            }
            self.publish_selected_arm_slot(scope_slot, next_slot);
        }
        let mut has_reentry = false;
        let mut remaining = 0usize;
        while remaining < len {
            let scope = self.lane_route_arms.get(lane_idx, remaining).scope;
            if !scope.is_none() && route_scope_reentry(scope) {
                has_reentry = true;
                break;
            }
            remaining += 1;
        }
        if has_reentry {
            self.lane_reentry_lanes.insert(lane_idx);
        } else {
            self.lane_reentry_lanes.remove(lane_idx);
        }
    }

    fn prepare_selected_arm_ref_release(&self, scope_slot: usize) -> RouteScopeSelectedArmSlot {
        if scope_slot >= self.scope_selected_arm_count {
            crate::invariant();
        }
        let selected_slot = /* SAFETY: `scope_slot < scope_selected_arm_count`
        selects one initialized selected-arm slot. This copy remains unpublished
        until the matching sparse history row has been removed. */ unsafe {
            *self.scope_selected_arms.add(scope_slot)
        };
        selected_slot.prepared_release()
    }

    fn publish_selected_arm_slot(
        &mut self,
        scope_slot: usize,
        next_slot: RouteScopeSelectedArmSlot,
    ) {
        /* SAFETY: preparation bounded `scope_slot`; the sparse history removal
        has completed, so this write publishes its matching refcount exactly once. */
        unsafe {
            self.scope_selected_arms.add(scope_slot).write(next_slot);
        }
    }
}
