use super::super::{EventCursor, LocalDependency, ScopeId};
use crate::global::event_program::LocalEventRowSet;

impl EventCursor {
    pub(super) fn selected_route_arm_event_row_done(
        &self,
        scope_id: ScopeId,
        arm: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let Some(slot) = self.route_scope_slot_inner(scope_id) else {
            return false;
        };
        let Some(row_set) = self
            .machine()
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)
        else {
            return false;
        };
        self.event_row_set_live_events_done(row_set, selected_arm_for_scope)
    }

    pub(crate) fn reentrant_route_arm_event_row_done(
        &self,
        scope_id: ScopeId,
        arm: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.route_scope_reentry(scope_id)
            && self.selected_route_arm_event_row_done(scope_id, arm, selected_arm_for_scope)
    }

    pub(super) fn selected_route_arm_completes_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        if self.route_scope_reentry(scope_id) {
            return false;
        }
        self.selected_route_arm_event_row_done(scope_id, arm, selected_arm_for_scope)
    }

    pub(crate) fn selected_arm_for_reentry_preview_conflict(
        &self,
        scope_id: ScopeId,
        preview_conflict: crate::global::typestate::PackedEventConflict,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<u8> {
        let selected = selected_arm_for_scope(scope_id)?;
        if self
            .preview_conflict_arm(preview_conflict, scope_id)
            .is_some_and(|preview| preview != selected)
            && self.route_scope_reentry(scope_id)
            && self.selected_route_arm_event_row_done(scope_id, selected, selected_arm_for_scope)
        {
            return None;
        }
        Some(selected)
    }

    #[inline(never)]
    pub(super) fn dependency_row_live_events_done(
        &self,
        dependency: LocalDependency,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let row_set = self
            .machine()
            .event_program()
            .dependency_row_set(dependency);
        self.event_row_set_live_events_done(row_set, selected_arm_for_scope)
    }

    #[inline(never)]
    pub(super) fn event_row_set_live_events_done(
        &self,
        row_set: LocalEventRowSet,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let local_len = self.local_steps_len();
        if row_set.start() > local_len || row_set.end() > local_len {
            crate::invariant();
        }
        for idx in self.pending_event_steps(row_set.start()..row_set.end()) {
            if let Some(row) = self.machine().event_program().event_row_at(idx)
                && self.event_conflict_row_allows(
                    row.conflict(),
                    ScopeId::none(),
                    None,
                    selected_arm_for_scope,
                )
            {
                return false;
            }
        }
        true
    }
}
