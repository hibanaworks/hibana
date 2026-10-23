use super::super::super::{EventCursor, RelocatableResidentLaneStep, ScopeId};

impl EventCursor {
    pub(crate) fn roll_entry_views(
        &self,
        scope: ScopeId,
    ) -> Option<impl Iterator<Item = (u8, u16, crate::global::role_program::PackedLaneRange)> + '_>
    {
        let range = self.machine().event_program().roll_entry_range(scope)?;
        Some((range.start()..range.end()).map(|index| {
            let program = self.machine().event_program();
            let (role, actor) = program.roll_entry_view(index);
            (role, actor, program.roll_entry_receipts(index))
        }))
    }

    /// A completed ancestor is retained when this event enters only a child
    /// visit. Completion alone never permits erasing a required prefix.
    pub(crate) fn route_reentry_head_allows_index(
        &self,
        scope: ScopeId,
        idx: usize,
        lane: u8,
        arm_for_scope: &mut dyn FnMut(ScopeId, super::super::EventArmView) -> Option<u8>,
    ) -> bool {
        let mut committed = |scope| arm_for_scope(scope, super::super::EventArmView::Committed);
        let Some(body) = self.complete_roll_body_scope_for_index(idx, &mut committed) else {
            return false;
        };
        let mut preview = |scope| arm_for_scope(scope, super::super::EventArmView::Preview);
        self.route_scope_reentry(scope)
            && self.route_scope_contained_in_roll_scope(scope, body)
            && self.roll_scope_head_allows_index(body, idx, lane, &mut preview)
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
        let scope = self.complete_roll_body_scope_for_index(idx, &mut *selected_arm_for_scope)?;
        // Completion uses the committed visit above. Head admission uses the
        // candidate's conflict so an old alternative cannot block the new head.
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        let mut preview = |candidate| {
            self.selected_arm_for_reentry_preview_conflict(
                candidate,
                preview_conflict,
                selected_arm_for_scope,
            )
        };
        self.roll_scope_head_allows_index(scope, idx, lane, &mut preview)
            .then_some(scope)
    }

    #[inline(never)]
    pub(crate) fn clear_reentry_scope_events(&mut self, scope: ScopeId, entry: Option<usize>) {
        let Some((mut idx, end)) = self.reentry_scope_event_bounds(scope) else {
            crate::invariant();
        };
        let preserves_children = self
            .machine()
            .event_program()
            .footprint()
            .borrow_epoch_count
            != 0;
        let mut rewind_row = None;
        while idx < end && self.contains_node_index(idx) {
            if self.roll_scope_contains_index(scope, idx)
                && (!preserves_children
                    || self
                        .roll_reentry_preserved_child(scope, idx, entry)
                        .is_none())
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
        self.retain_reentry_completion(scope, entry);
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
