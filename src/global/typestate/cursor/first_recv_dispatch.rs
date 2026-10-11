use super::{
    CursorInvariantError, EventCursorMachine, InboundFrameKey, LocalAction, ScopeId, StateIndex,
    state_index_to_usize,
};
use crate::global::typestate::LocalConflict;
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
        if self.event_program().has_passive_parent_index() {
            self.visit_indexed_dispatch_subtree(scope_id, &mut visitor);
            return Some(());
        }
        self.visit_scanned_first_recv_dispatch(scope_id, visitor)
    }

    fn visit_scanned_first_recv_dispatch(
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

    // The certificate proves forward child slots, unique inverse parents and
    // complete static row validation. Visit only the reachable subtree; the
    // two consumers accumulate an OR mask or an order-independent UniqueMatch.
    // Uncertified descriptors retain the original ordered scan and failures.
    fn visit_indexed_dispatch_subtree(
        &self,
        root: ScopeId,
        visitor: &mut impl FnMut(u8, StateIndex),
    ) {
        let limit = self.event_program().footprint().route_scope_count;
        let root_present = self.route_scope_rows(root).is_some();
        for root_arm in 0..2 {
            if root_present {
                self.visit_first_recv_dispatch_arm(root, root_arm, root_arm, visitor);
            }
            let Some(mut current) = self.indexed_dispatch_child(root, root_arm) else {
                continue;
            };
            let mut next_arm = 0u8;
            let mut steps = 0usize;
            loop {
                steps += 1;
                if steps > limit * 4 + 1 {
                    crate::invariant();
                }
                if next_arm < 2 {
                    let arm = next_arm;
                    next_arm += 1;
                    if self.route_scope_rows(current).is_some() {
                        self.visit_first_recv_dispatch_arm(current, arm, root_arm, visitor);
                    }
                    if let Some(child) = self.indexed_dispatch_child(current, arm) {
                        current = child;
                        next_arm = 0;
                    }
                } else {
                    let Some((parent, arm)) = self.indexed_passive_child_parent_route(current)
                    else {
                        crate::invariant();
                    };
                    if parent.same(root) {
                        break;
                    }
                    current = parent;
                    next_arm = arm + 1;
                }
            }
        }
    }

    #[inline]
    fn indexed_dispatch_child(&self, parent: ScopeId, arm: u8) -> Option<ScopeId> {
        let slot = self.route_scope_dense_ordinal(parent)?;
        let fact = self.passive_arm_child_fact_by_slot(slot, arm)?;
        fact.child_route_scope()
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
    ) -> Result<Option<(u8, StateIndex)>, CursorInvariantError> {
        let mut matched = UniqueMatch::NONE;
        let visited = self.visit_first_recv_dispatch(scope_id, |arm, target| {
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
        });
        let Some(()) = visited else {
            return Err(CursorInvariantError::INVARIANT);
        };
        matched
            .finish_optional()
            .map_err(|_| CursorInvariantError::INVARIANT)
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
        if self.event_program().has_passive_parent_index() {
            self.indexed_passive_child_parent_route(child_scope)
        } else {
            self.scanned_passive_child_parent_route(child_scope)
        }
    }

    fn indexed_passive_child_parent_route(&self, child_scope: ScopeId) -> Option<(ScopeId, u8)> {
        let child_slot = self.route_scope_dense_ordinal(child_scope)?;
        let LocalConflict::RouteArm { scope: parent, arm } = self
            .route_scope_conflict_by_slot(child_slot)
            .to_conflict()?
        else {
            return None;
        };
        let parent_slot = self.route_scope_dense_ordinal(parent)?;
        let row = self.passive_arm_child_fact_by_slot(parent_slot, arm)?;
        row.child_route_scope()
            .is_some_and(|child| child.same(child_scope))
            .then_some((row.route_scope(), row.arm()))
    }

    fn scanned_passive_child_parent_route(&self, child_scope: ScopeId) -> Option<(ScopeId, u8)> {
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

#[cfg(all(test, hibana_repo_tests))]
mod tests;
