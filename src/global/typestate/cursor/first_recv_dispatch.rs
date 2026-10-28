#[cfg(all(test, hibana_repo_tests))]
use super::state_index_to_usize;
use super::{CursorInvariantError, EventCursorMachine, InboundFrameKey, ScopeId, StateIndex};
#[cfg(all(test, hibana_repo_tests))]
use crate::global::typestate::LocalConflict;
use crate::runtime_core::UniqueMatch;

impl EventCursorMachine {
    #[cfg(all(test, hibana_repo_tests))]
    pub(in crate::global::typestate::cursor) fn visit_first_recv_dispatch(
        &self,
        scope_id: ScopeId,
        mut visitor: impl FnMut(u8, StateIndex),
    ) -> Option<()> {
        self.route_scope_dense_ordinal(scope_id)?;
        self.visit_indexed_dispatch_subtree(scope_id, &mut visitor);
        Some(())
    }

    // Full subtree enumeration is retained as a test oracle. Production uses
    // the received event's source path below.
    #[cfg(all(test, hibana_repo_tests))]
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
                    let Some((parent, arm)) = self.passive_child_parent_route(current) else {
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
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        let program = self.event_program();
        Some(
            u8::from(program.route_arm_first_operation(slot, 0, true).is_some())
                | (u8::from(program.route_arm_first_operation(slot, 1, true).is_some()) << 1),
        )
    }

    pub(in crate::global::typestate::cursor) fn first_recv_descendant_target_for_key(
        &self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> Result<Option<(u8, StateIndex)>, CursorInvariantError> {
        self.route_scope_dense_ordinal(scope_id)
            .ok_or(CursorInvariantError::INVARIANT)?;
        let mut matched = UniqueMatch::NONE;
        let source_event = usize::from(key.frame_label);
        for index in self.event_program().source_effect_candidates(source_event) {
            let row = crate::invariant_some(self.event_program().event_row_at(index));
            let atom = row.atom();
            if row.direction() != crate::global::role_program::RoleLocalDirection::Recv
                || atom.from != key.source_role
                || atom.lane != key.lane
            {
                continue;
            }
            let Some(root_arm) = self
                .program_ref()
                .source_route_arm_for_event(scope_id, source_event)
            else {
                continue;
            };
            let target = StateIndex::from_usize(index);
            let mut current = scope_id;
            let mut remaining = self.event_program().footprint().route_scope_count;
            loop {
                if remaining == 0 {
                    crate::invariant();
                }
                remaining -= 1;
                let Some(arm) = self
                    .program_ref()
                    .source_route_arm_for_event(current, source_event)
                else {
                    break;
                };
                if self.route_scope_rows(current).is_some()
                    && self.route_recv_state(current, arm) == Some(target)
                {
                    matched = matched.add((root_arm, target));
                    // Further hits on this path name the same target and root
                    // arm. UniqueMatch treats those repeated observations as one.
                    break;
                }
                let Some(child) = self.indexed_dispatch_child(current, arm) else {
                    break;
                };
                current = child;
            }
        }
        matched
            .finish_optional()
            .map_err(|_| CursorInvariantError::INVARIANT)
    }

    #[inline(always)]
    #[cfg(all(test, hibana_repo_tests))]
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

    #[cfg(all(test, hibana_repo_tests))]
    fn passive_child_parent_route(&self, child_scope: ScopeId) -> Option<(ScopeId, u8)> {
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
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
