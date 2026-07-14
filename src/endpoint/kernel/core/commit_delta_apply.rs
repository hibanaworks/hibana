use super::{
    CommitEventRow, CursorEndpoint, PreparedRouteCommitRows, RelocatableResidentLaneStep,
    SelectedRouteCommitRow, Transport, commit_delta::CommitDeltaApplyPermit,
    commit_delta::CommittedCommitDelta, commit_delta::PreparedCommitDelta, state_index_to_usize,
};
use crate::global::const_dsl::{ReentryMark, ScopeId};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn commit_prepared_delta(
        &mut self,
        mut delta: PreparedCommitDelta,
    ) -> CommittedCommitDelta {
        let fresh_route_start = delta.fresh_route_start;
        let applies_route_completion =
            delta.event().is_some() || delta.selected_routes().len() != 0;
        let body_reentry_scope = self.prepared_body_reentry_scope(delta.event());
        if let Some(scope) = body_reentry_scope {
            self.clear_reentry_scope_state(scope);
        }
        let route_lane = delta.selected_route_lane();
        let mut idx = 0usize;
        while idx < delta.selected_routes().len() {
            let Some(row) = delta.selected_routes().get(&self.cursor, idx) else {
                crate::invariant();
            };
            let Some(lane) = route_lane else {
                crate::invariant();
            };
            self.apply_prepared_selected_route_commit_row(row, lane);
            idx += 1;
        }
        self.apply_prepared_cursor_index(state_index_to_usize(delta.cursor_after()));
        if let Some(event) = delta.event() {
            self.apply_prepared_lane_advance(event.progress_step());
        }
        if let Some(step) = delta.lane_relocation() {
            self.apply_prepared_lane_relocation(step);
        }
        if applies_route_completion {
            self.apply_prepared_route_completion_cursor(delta.selected_routes());
        }
        CommittedCommitDelta::from_applied(
            delta.event(),
            delta.take_selected_routes(),
            fresh_route_start,
        )
    }

    #[inline]
    fn prepared_body_reentry_scope(&self, event: Option<CommitEventRow>) -> Option<ScopeId> {
        let event = event?;
        let mut selected_arm_for_scope = |scope| self.selected_arm_for_scope(scope);
        self.cursor
            .roll_body_reentry_scope_for_step(event.progress_step(), &mut selected_arm_for_scope)
    }

    fn clear_reentry_scope_state(&mut self, scope: ScopeId) {
        self.cursor.clear_reentry_scope_events(scope);
        let lane_limit = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if self.decision_state.lane_route_arm_len(lane_idx) != 0 {
                {
                    let cursor = &self.cursor;
                    self.decision_state.clear_lane_route_selections_in_scope(
                        lane_idx,
                        |candidate| cursor.route_scope_contained_in_roll_scope(candidate, scope),
                        |candidate| cursor.route_scope_slot(candidate),
                        |candidate| cursor.route_scope_reentry(candidate),
                        CommitDeltaApplyPermit::new(),
                    );
                }
                self.refresh_lane_offer_state(lane_idx);
            }
            lane_idx += 1;
        }
    }

    #[inline(always)]
    fn apply_prepared_cursor_index(&mut self, idx: usize) {
        self.cursor.set_index(idx);
    }

    #[inline(always)]
    fn apply_prepared_lane_advance(&mut self, target: RelocatableResidentLaneStep) {
        let refresh = self.cursor.advance_lane_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn apply_prepared_lane_relocation(&mut self, target: RelocatableResidentLaneStep) {
        let refresh = self.cursor.set_lane_cursor_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn selected_arm_from_prepared_rows(
        rows: &PreparedRouteCommitRows,
        cursor: &super::EventCursor,
        scope: ScopeId,
    ) -> Option<u8> {
        let mut idx = 0usize;
        while idx < rows.len() {
            let row = rows.get(cursor, idx)?;
            if row.scope() == scope {
                return Some(row.selected_arm());
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    fn apply_prepared_route_completion_cursor(&mut self, rows: &PreparedRouteCommitRows) {
        let idx = self.cursor.index();
        if self.cursor.enclosing_route_scope_rows_at(idx).is_none() {
            return;
        }
        let cursor = &self.cursor;
        let decision_state = &self.decision_state;
        let Some(end) = cursor.selected_enclosing_route_scope_end_at(idx, |scope| {
            Self::selected_arm_from_prepared_rows(rows, cursor, scope).or_else(|| {
                let scope_slot = cursor.route_scope_slot(scope)?;
                decision_state.selected_arm_for_scope_slot(scope_slot)
            })
        }) else {
            return;
        };
        if end != idx && self.cursor.contains_node_index(end) {
            self.cursor.set_index(end);
        }
    }

    fn apply_prepared_selected_route_commit_row(&mut self, row: SelectedRouteCommitRow, lane: u8) {
        let lane_idx = lane as usize;
        let scope = row.scope();
        let Some(scope_slot) = self.cursor.route_scope_slot(scope) else {
            crate::invariant();
        };
        let reentry = if self.cursor.route_scope_reentry(scope) {
            ReentryMark::Reentrant
        } else {
            ReentryMark::SinglePass
        };
        let completed_iteration_arm = if reentry.is_reentrant() {
            self.selected_arm_for_scope(scope)
                .filter(|&arm| self.reentrant_selected_arm_complete(scope, arm))
        } else {
            None
        };
        if let Some(completed_arm) = completed_iteration_arm {
            let cursor = &self.cursor;
            let lane_limit = cursor.logical_lane_count();
            let mut clear_lane = 0usize;
            while clear_lane < lane_limit {
                if self.decision_state.lane_route_arm_len(clear_lane) != 0 {
                    self.decision_state.replace_route_selection_arm_for_scope(
                        clear_lane,
                        scope,
                        completed_arm,
                        row.selected_arm(),
                    );
                    self.decision_state.clear_lane_route_selections_in_scope(
                        clear_lane,
                        |candidate| {
                            cursor.route_scope_conflict_arm_for_scope(candidate, scope)
                                == Some(completed_arm)
                        },
                        |candidate| cursor.route_scope_slot(candidate),
                        |candidate| cursor.route_scope_reentry(candidate),
                        CommitDeltaApplyPermit::new(),
                    );
                }
                clear_lane += 1;
            }
            self.decision_state.replace_selected_arm_slot(
                scope_slot,
                completed_arm,
                row.selected_arm(),
            );
            self.cursor.clear_reentry_scope_events(scope);
        }
        self.decision_state.apply_prepared_route_selection(
            lane_idx,
            scope_slot,
            reentry,
            row,
            CommitDeltaApplyPermit::new(),
        );
        self.refresh_lane_offer_state(lane_idx);
    }
}
