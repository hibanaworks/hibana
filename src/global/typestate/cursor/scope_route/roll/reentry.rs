use super::super::super::{EventCursor, RelocatableResidentLaneStep, ScopeId};
use super::RollLaneAdmission;

impl EventCursor {
    /// A completed ancestor is retained when this event enters only a child
    /// visit. Completion alone never permits erasing a required prefix.
    pub(crate) fn route_reentry_head_allows_index(
        &self,
        scope: ScopeId,
        idx: usize,
        lane: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.route_scope_reentry(scope)
            && self.roll_scope_lane_allows_index(
                scope,
                idx,
                lane,
                RollLaneAdmission::Head,
                selected_arm_for_scope,
            )
    }

    #[inline(never)]
    pub(crate) fn roll_body_reentry_scope_for_step(
        &self,
        target: RelocatableResidentLaneStep,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        if !self.has_reentry_scopes() {
            return None;
        }
        let idx = self.node_index_for_relocatable_step(target)?;
        let lane = self
            .machine()
            .event_program()
            .local_step_lane(target.0.step_idx as usize)?;
        let scope = self.complete_roll_scope_for_index(idx, &mut *selected_arm_for_scope)?;
        if !matches!(
            scope.kind(),
            Some(crate::global::const_dsl::ScopeKind::Roll)
        ) {
            return None;
        }
        self.roll_scope_lane_allows_index(
            scope,
            idx,
            lane,
            RollLaneAdmission::Head,
            selected_arm_for_scope,
        )
        .then_some(scope)
    }

    #[inline(never)]
    pub(crate) fn clear_reentry_scope_events(&mut self, scope: ScopeId) {
        let Some((mut idx, end)) = self.reentry_scope_event_bounds(scope) else {
            crate::invariant();
        };
        let mut rewind_row = None;
        while idx < end && self.contains_node_index(idx) {
            if self.roll_scope_contains_index(scope, idx)
                && let Some(lane) = self.event_lane_at(idx)
            {
                self.clear_node_event_done_for_lane(idx, lane);
                let Ok(step) = self.relocatable_resident_lane_step_at_index(idx, usize::from(lane))
                else {
                    crate::invariant();
                };
                let Ok(row) = self
                    .resident_lane_step_locator(usize::from(lane), usize::from(step.0.step_idx))
                else {
                    crate::invariant();
                };
                // Materialize the first row once. Selecting a later row while
                // resetting another lane would overwrite the fresh prefix.
                if rewind_row.is_none() {
                    rewind_row = Some(row);
                    self.set_lane_cursor_to_relocatable_step(step);
                } else if rewind_row == Some(row) && self.event_is_before_lane_cursor(idx, lane) {
                    self.set_lane_cursor_to_relocatable_step(step);
                }
            }
            idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) fn route_scope_contained_in_roll_scope(
        &self,
        route_scope: ScopeId,
        roll_scope: ScopeId,
    ) -> bool {
        let Some(roll_row) = self.machine().roll_scope_row(roll_scope) else {
            return false;
        };
        let Some(route_row) = self.machine().route_scope_rows(route_scope) else {
            return false;
        };
        roll_row.start() <= route_row.start() && route_row.end() <= roll_row.end()
    }
}
