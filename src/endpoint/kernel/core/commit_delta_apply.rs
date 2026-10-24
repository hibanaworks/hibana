use super::{
    CommitEventRow, OperationKernel, PreparedRouteCommitRows, PreparedViewCommit,
    RelocatableResidentLaneStep, SelectedRouteCommitRow, Transport,
    commit_delta::CommitDeltaApplyPermit, commit_delta::CommittedCommitDelta,
    commit_delta::PreparedCommitDelta, state_index_to_usize,
};
use crate::global::const_dsl::{ReentryMark, ScopeId};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn commit_prepared_delta(
        &mut self,
        mut delta: PreparedCommitDelta,
        view: Option<PreparedViewCommit>,
    ) -> (
        CommittedCommitDelta,
        Option<crate::endpoint::owner::EndpointOwner>,
    ) {
        let owner = self.commit_prepared_view(delta.event(), view);
        let fresh_route_start = delta.fresh_route_start;
        let applies_route_completion =
            delta.event().is_some() || delta.selected_routes().len() != 0;
        let body_reentry_scope = self.prepared_body_reentry_scope(delta.event());
        if let Some(scope) = body_reentry_scope {
            let event = crate::invariant_some(delta.event());
            let entry = crate::invariant_some(
                self.progress
                    .cursor
                    .node_index_for_relocatable_step(event.progress_step()),
            );
            self.clear_reentry_scope_state(scope, entry);
        }
        let route_lane = delta.selected_route_lane();
        let mut idx = 0usize;
        while idx < delta.selected_routes().len() {
            let Some(row) = delta.selected_routes().get(&self.progress.cursor, idx) else {
                crate::invariant();
            };
            let Some(lane) = route_lane else {
                crate::invariant();
            };
            let completed_iteration_arm = self.selected_arm_for_scope(row.scope()).filter(|&arm| {
                delta.route_is_fresh(idx) && self.reentrant_selected_arm_complete(row.scope(), arm)
            });
            self.apply_prepared_selected_route_commit_row(row, lane, completed_iteration_arm);
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
        // A parallel sibling may resume in an earlier compiled row. Refresh
        // lookup caches at the final position without changing event progress.
        if let Some(refresh) = self.progress.cursor.refresh_resident_row_at_index() {
            self.refresh_after_cursor_move(refresh);
        }
        let committed = CommittedCommitDelta::from_applied(
            delta.event(),
            delta.take_selected_routes(),
            fresh_route_start,
        );
        (committed, owner)
    }

    #[inline]
    pub(super) fn prepared_body_reentry_scope(
        &self,
        event: Option<CommitEventRow>,
    ) -> Option<ScopeId> {
        let event = event?;
        let mut selected_arm_for_scope = |scope| self.selected_arm_for_scope(scope);
        let index = self
            .progress
            .cursor
            .node_index_for_relocatable_step(event.progress_step())?;
        if let Some(scope) = self.borrowed_reentry_scope(index) {
            return Some(scope);
        }
        self.progress
            .cursor
            .roll_body_reentry_scope_for_step(event.progress_step(), &mut selected_arm_for_scope)
    }

    fn clear_reentry_scope_state(&mut self, scope: ScopeId, entry: usize) {
        let reset = self
            .progress
            .cursor
            .prepare_roll_reset(scope, Some(entry), &mut |scope| {
                self.selected_arm_for_scope(scope)
            });
        let lane_limit = self.progress.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if self.progress.decision_state.lane_route_arm_len(lane_idx) != 0 {
                {
                    let cursor = &self.progress.cursor;
                    self.progress
                        .decision_state
                        .clear_lane_route_selections_in_scope(
                            lane_idx,
                            |candidate| {
                                cursor.route_scope_contained_in_roll_scope(candidate, scope)
                                    && !cursor.roll_reentry_preserves_route(scope, candidate, entry)
                            },
                            |candidate| cursor.route_scope_slot(candidate),
                            |candidate| cursor.route_scope_reentry(candidate),
                            CommitDeltaApplyPermit::new(),
                        );
                }
            }
            lane_idx += 1;
        }
        self.progress.cursor.clear_reentry_scope_events(reset);
        self.sync_lane_offer_state();
    }

    #[inline(always)]
    fn apply_prepared_cursor_index(&mut self, idx: usize) {
        self.progress.cursor.set_index(idx);
    }

    #[inline(always)]
    fn apply_prepared_lane_advance(&mut self, target: RelocatableResidentLaneStep) {
        let refresh = self
            .progress
            .cursor
            .advance_lane_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn apply_prepared_lane_relocation(&mut self, target: RelocatableResidentLaneStep) {
        let refresh = self
            .progress
            .cursor
            .set_lane_cursor_to_relocatable_step(target);
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
        let idx = self.progress.cursor.index();
        if self
            .progress
            .cursor
            .enclosing_route_scope_rows_at(idx)
            .is_none()
        {
            return;
        }
        let cursor = &self.progress.cursor;
        let decision_state = &self.progress.decision_state;
        let Some(end) = cursor.selected_enclosing_route_scope_end_at(idx, |scope| {
            match Self::selected_arm_from_prepared_rows(rows, cursor, scope) {
                Some(prepared) => Some(prepared),
                None => {
                    let scope_slot = cursor.route_scope_slot(scope)?;
                    decision_state.selected_arm_for_scope_slot(scope_slot)
                }
            }
        }) else {
            return;
        };
        if end != idx && self.progress.cursor.contains_node_index(end) {
            self.progress.cursor.set_index(end);
        }
    }

    fn apply_prepared_selected_route_commit_row(
        &mut self,
        row: SelectedRouteCommitRow,
        lane: u8,
        completed_iteration_arm: Option<u8>,
    ) {
        let lane_idx = lane as usize;
        let scope = row.scope();
        let Some(scope_slot) = self.progress.cursor.route_scope_slot(scope) else {
            crate::invariant();
        };
        let reentry = if self.progress.cursor.route_scope_reentry(scope) {
            ReentryMark::Reentrant
        } else {
            ReentryMark::SinglePass
        };
        if let Some(completed_arm) = completed_iteration_arm {
            let cursor = &self.progress.cursor;
            let lane_limit = cursor.logical_lane_count();
            let mut clear_lane = 0usize;
            while clear_lane < lane_limit {
                if self.progress.decision_state.lane_route_arm_len(clear_lane) != 0 {
                    self.progress
                        .decision_state
                        .replace_route_selection_arm_for_scope(
                            clear_lane,
                            scope,
                            completed_arm,
                            row.selected_arm(),
                        );
                    self.progress
                        .decision_state
                        .clear_lane_route_selections_in_scope(
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
            let reset = self
                .progress
                .cursor
                .prepare_roll_reset(scope, None, &mut |scope| self.selected_arm_for_scope(scope));
            self.progress.decision_state.replace_selected_arm_slot(
                scope_slot,
                completed_arm,
                row.selected_arm(),
            );
            self.progress.cursor.clear_reentry_scope_events(reset);
        }
        self.progress.decision_state.apply_prepared_route_selection(
            lane_idx,
            scope_slot,
            reentry,
            row,
            CommitDeltaApplyPermit::new(),
        );
        if completed_iteration_arm.is_some() {
            self.sync_lane_offer_state();
        } else {
            self.refresh_lane_offer_state(lane_idx);
        }
    }
}
