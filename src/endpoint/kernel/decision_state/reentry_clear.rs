use super::{CommitDeltaApplyPermit, RouteArmState, RouteScopeSelectedArmSlot, RouteState};
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
            if current.scope == scope && current.arm == old_arm {
                self.lane_route_arms.set(
                    lane_idx,
                    idx,
                    RouteArmState {
                        scope,
                        arm: new_arm,
                    },
                );
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
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            crate::invariant();
        };
        let mut len = /* SAFETY: `dense` is produced by this route state's lane-offer view; it indexes an initialized lane route-arm length cell and this read only copies the current length. */ unsafe {
            *self.lane_route_arm_lens.add(dense) as usize
        };
        let mut idx = 0usize;
        while idx < len {
            let current = self.lane_route_arms.get(lane_idx, idx);
            if current.scope.is_none() || !contains_route_scope(current.scope) {
                idx += 1;
                continue;
            }
            self.release_selected_arm_ref(crate::invariant_some(route_scope_slot(current.scope)));
            len -= 1;
            self.remove_lane_route_arm(lane_idx, idx, len);
            /* SAFETY: `dense` still names the same initialized lane length cell; `remove_lane_route_arm` compacted this owned lane stack and `&mut self` writes the new bounded `len` without aliasing. */
            unsafe {
                self.lane_route_arm_lens.add(dense).write(len as u8);
            }
            if route_scope_reentry(current.scope) {
                self.decrement_lane_reentry_count(dense, lane_idx);
            }
        }
    }

    fn release_selected_arm_ref(&mut self, scope_slot: usize) {
        if scope_slot >= self.scope_selected_arm_count {
            crate::invariant();
        }
        let selected_slot = /* SAFETY: `scope_slot < scope_selected_arm_count` selects an initialized selected-arm slot owned by `RouteState`; this release path holds `&mut self` and mutates the refcount exactly once. */ unsafe {
            &mut *self.scope_selected_arms.add(scope_slot)
        };
        if selected_slot.refs == 0 {
            crate::invariant();
        }
        selected_slot.refs -= 1;
        if selected_slot.refs == 0 {
            *selected_slot = RouteScopeSelectedArmSlot::EMPTY;
        }
    }

    fn remove_lane_route_arm(&mut self, lane_idx: usize, idx: usize, len: usize) {
        let mut shift = idx + 1;
        while shift <= len {
            self.lane_route_arms.set(
                lane_idx,
                shift - 1,
                self.lane_route_arms.get(lane_idx, shift),
            );
            shift += 1;
        }
        self.lane_route_arms
            .set(lane_idx, len, RouteArmState::EMPTY);
    }

    fn decrement_lane_reentry_count(&mut self, dense: usize, lane_idx: usize) {
        let count = /* SAFETY: `dense` comes from the route state's initialized lane-offer map; the matching reentry-count cell belongs to this lane commit and `&mut self` owns the decrement mutation. */ unsafe {
            &mut *self.lane_reentry_counts.add(dense)
        };
        if *count == 0 {
            crate::invariant();
        }
        *count -= 1;
        if *count == 0 {
            self.lane_reentry_lanes.remove(lane_idx);
        }
    }
}
