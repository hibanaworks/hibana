use super::{
    EventCursorMachine, InboundFrameKey, LocalAction, ScopeId, StateIndex, state_index_to_usize,
};
use crate::runtime_core::UniqueMatch;

#[inline(always)]
fn validated_dispatch_arm(arm: u8, target: StateIndex) -> u8 {
    if arm > 1 || target.is_absent() {
        crate::invariant();
    }
    arm
}

impl EventCursorMachine {
    pub(in crate::global::typestate::cursor) fn visit_first_recv_dispatch(
        &self,
        scope_id: ScopeId,
        mut visitor: impl FnMut(u8, StateIndex),
    ) -> Option<()> {
        self.route_scope_dense_ordinal(scope_id)?;
        let route_count = self.event_program().footprint().route_scope_count;
        let mut slot = 0usize;
        while slot < route_count {
            if let Some(region) = self.route_scope_rows_by_slot(slot) {
                let route_scope = region.scope();
                if route_scope.same(scope_id) {
                    self.visit_first_recv_dispatch_arm(route_scope, 0, 0, &mut visitor);
                    self.visit_first_recv_dispatch_arm(route_scope, 1, 1, &mut visitor);
                } else if let Some(root_arm) =
                    self.first_recv_dispatch_root_arm(scope_id, route_scope)
                {
                    self.visit_first_recv_dispatch_arm(route_scope, 0, root_arm, &mut visitor);
                    self.visit_first_recv_dispatch_arm(route_scope, 1, root_arm, &mut visitor);
                }
            }
            slot += 1;
        }
        Some(())
    }

    #[inline(always)]
    pub(in crate::global::typestate::cursor) fn first_recv_dispatch_arm_mask(
        &self,
        scope_id: ScopeId,
    ) -> Option<u8> {
        let mut mask = 0u8;
        self.visit_first_recv_dispatch(scope_id, |arm, target| {
            let arm = validated_dispatch_arm(arm, target);
            mask |= 1u8 << arm;
        })?;
        Some(mask)
    }

    pub(in crate::global::typestate::cursor) fn first_recv_descendant_target_for_key(
        &self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> Option<(u8, StateIndex)> {
        let mut matched = UniqueMatch::NONE;
        self.visit_first_recv_dispatch(scope_id, |arm, target| {
            let arm = validated_dispatch_arm(arm, target);
            let node = self.node(state_index_to_usize(target));
            let LocalAction::Recv {
                peer,
                lane: target_lane,
                frame_label: target_frame_label,
                ..
            } = node.action()
            else {
                return;
            };
            if peer == key.source_role
                && target_lane == key.lane
                && target_frame_label == key.frame_label
            {
                matched = matched.add((arm, target));
            }
        })?;
        matched.into_option()
    }

    #[inline(always)]
    fn visit_first_recv_dispatch_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        root_arm: u8,
        visitor: &mut impl FnMut(u8, StateIndex),
    ) {
        if let Some(target) = self.route_recv_state(scope_id, arm) {
            visitor(root_arm, target);
        }
    }

    #[inline(always)]
    fn first_recv_dispatch_root_arm(
        &self,
        root_scope: ScopeId,
        candidate_scope: ScopeId,
    ) -> Option<u8> {
        if candidate_scope.same(root_scope) {
            return None;
        }
        self.route_scope_dense_ordinal(candidate_scope)?;
        let route_count = self.event_program().footprint().route_scope_count;
        let mut current = candidate_scope;
        let mut hops = 0usize;
        while hops < route_count {
            let (parent, arm) = self.passive_child_parent_route(current)?;
            if parent.same(root_scope) {
                return Some(arm);
            }
            current = parent;
            hops += 1;
        }
        crate::invariant();
    }

    fn passive_child_parent_route(&self, child_scope: ScopeId) -> Option<(ScopeId, u8)> {
        let route_count = self.event_program().footprint().route_scope_count;
        let mut slot = 0usize;
        while slot < route_count {
            let mut arm = 0u8;
            while arm < 2 {
                if let Some(row) = self.passive_arm_child_fact_by_slot(slot, arm)
                    && row
                        .child_route_scope()
                        .is_some_and(|child| child.same(child_scope))
                {
                    return Some((row.route_scope(), row.arm()));
                }
                arm += 1;
            }
            slot += 1;
        }
        None
    }
}
