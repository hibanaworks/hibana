use super::{
    CommitDelta, CommitEventRow, CursorEndpoint, CursorInvariantError, PreparedRouteCommitRows,
    RelocatableResidentLaneStep, SelectedRouteCommitRow, Transport, state_index_to_usize,
};
use crate::global::const_dsl::ReentryMark;
use crate::global::typestate::StateIndex;

pub(crate) struct PreparedCommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: PreparedRouteCommitRows,
    lane_relocation: Option<RelocatableResidentLaneStep>,
    cursor_after: StateIndex,
}

impl PreparedCommitDelta {
    #[inline(always)]
    fn from_preflighted(delta: CommitDelta, selected_routes: PreparedRouteCommitRows) -> Self {
        Self {
            event: delta.event(),
            selected_routes,
            lane_relocation: delta.lane_relocation(),
            cursor_after: delta.cursor_after(),
        }
    }

    #[inline(always)]
    pub(crate) const fn event(&self) -> Option<CommitEventRow> {
        self.event
    }

    #[inline(always)]
    pub(crate) const fn selected_routes(&self) -> &PreparedRouteCommitRows {
        &self.selected_routes
    }

    #[inline(always)]
    pub(crate) const fn selected_route_lane(&self) -> Option<u8> {
        self.selected_routes.selected_lane()
    }

    #[inline(always)]
    pub(crate) const fn lane_relocation(&self) -> Option<RelocatableResidentLaneStep> {
        self.lane_relocation
    }

    #[inline(always)]
    pub(crate) const fn cursor_after(&self) -> StateIndex {
        self.cursor_after
    }

    #[inline(always)]
    fn take_selected_routes(&mut self) -> PreparedRouteCommitRows {
        self.selected_routes.take()
    }
}

pub(crate) struct CommittedCommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: PreparedRouteCommitRows,
}

impl CommittedCommitDelta {
    #[inline(always)]
    pub(crate) const fn event(&self) -> Option<CommitEventRow> {
        self.event
    }

    #[inline(always)]
    pub(crate) const fn selected_routes(&self) -> &PreparedRouteCommitRows {
        &self.selected_routes
    }

    #[inline(always)]
    pub(crate) const fn selected_route_lane(&self) -> Option<u8> {
        self.selected_routes.selected_lane()
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CommitDeltaApplyPermit(());

impl CommitDeltaApplyPermit {
    #[inline(always)]
    const fn new() -> Self {
        Self(())
    }
}

impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn preflight_commit_delta(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        if let Some(event) = delta.event() {
            let idx = self
                .cursor
                .node_index_for_relocatable_step(event.progress_step())
                .ok_or(CursorInvariantError::INVARIANT)?;
            let enabled = self.cursor.event_enabled(
                idx,
                crate::global::typestate::EventCommitMeta::new(
                    event.eff_index(),
                    event.event_label(),
                    event.event_origin(),
                    event.scope(),
                    event.route_arm(),
                    event.lane(),
                ),
                |scope| {
                    if scope == event.scope() {
                        event.route_arm()
                    } else {
                        self.selected_arm_for_scope(scope)
                    }
                },
            )?;
            if enabled.progress_step() != event.progress_step()
                || enabled.cursor_after() != delta.cursor_after()
            {
                return Err(CursorInvariantError::INVARIANT);
            }
            self.preflight_event_selected_route_chain(idx, event.route_arm(), delta)?;
        } else if delta.selected_routes().is_empty() {
            self.preflight_cursor_realign_delta(delta)?;
        } else {
            self.preflight_route_only_cursor_after(delta)?;
        }
        self.preflight_selected_route_rows(delta)?;
        self.preflight_lane_relocation(delta.lane_relocation())?;
        Ok(())
    }

    #[inline]
    fn preflight_event_selected_route_chain(
        &self,
        event_idx: usize,
        event_arm: Option<u8>,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        let routes = delta.selected_routes();
        let conflict = self.cursor.event_conflict_for_index(event_idx);
        let Some(range) = self
            .cursor
            .route_commit_range_for_conflict(conflict, event_arm)
        else {
            return if event_arm.is_none() && routes.is_empty() {
                Ok(())
            } else {
                Err(CursorInvariantError::INVARIANT)
            };
        };
        if routes.len() != range.len() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let mut idx = 0usize;
        while idx < range.len() {
            let Some(expected) = self
                .cursor
                .route_commit_row_at(range, idx)
                .and_then(SelectedRouteCommitRow::from_resident_conflict)
            else {
                return Err(CursorInvariantError::INVARIANT);
            };
            let Some(row) = routes.get(&self.cursor, idx) else {
                return Err(CursorInvariantError::INVARIANT);
            };
            if row.scope() != expected.scope() || row.selected_arm() != expected.selected_arm() {
                return Err(CursorInvariantError::INVARIANT);
            }
            idx += 1;
        }
        Ok(())
    }

    #[inline]
    fn preflight_cursor_realign_delta(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        let cursor_after = state_index_to_usize(delta.cursor_after());
        if cursor_after >= self.cursor.local_steps_len() {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
    }

    #[inline]
    fn preflight_route_only_cursor_after(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        let routes = delta.selected_routes();
        if routes.is_empty() {
            return Err(CursorInvariantError::INVARIANT);
        }
        if delta.selected_route_lane().is_none() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let expected_after = StateIndex::from_usize(self.cursor.index());
        (expected_after == delta.cursor_after())
            .then_some(())
            .ok_or(CursorInvariantError::INVARIANT)
    }

    #[inline]
    fn preflight_selected_route_rows(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        let routes = delta.selected_routes();
        let route_lane = if routes.is_empty() {
            None
        } else {
            delta.selected_route_lane()
        };
        let mut idx = 0usize;
        while idx < routes.len() {
            let row = routes
                .get(&self.cursor, idx)
                .ok_or(CursorInvariantError::INVARIANT)?;
            let lane = route_lane.ok_or(CursorInvariantError::INVARIANT)?;
            self.preflight_selected_route_row(row, lane)?;
            let mut prev_idx = 0usize;
            while prev_idx < idx {
                let prev = routes
                    .get(&self.cursor, prev_idx)
                    .ok_or(CursorInvariantError::INVARIANT)?;
                if prev.scope() == row.scope() {
                    return Err(CursorInvariantError::INVARIANT);
                }
                prev_idx += 1;
            }
            idx += 1;
        }
        Ok(())
    }

    #[inline]
    fn preflight_selected_route_row(
        &self,
        row: SelectedRouteCommitRow,
        lane: u8,
    ) -> Result<(), CursorInvariantError> {
        if row.is_empty() || row.selected_arm() > 1 {
            return Err(CursorInvariantError::INVARIANT);
        }
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let scope = row.scope();
        if scope.is_none() || self.cursor.route_scope_slot(scope).is_none() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let scope_slot = self
            .cursor
            .route_scope_slot(scope)
            .ok_or(CursorInvariantError::INVARIANT)?;
        let reentry = if self.cursor.route_scope_reentry(scope) {
            ReentryMark::Reentrant
        } else {
            ReentryMark::SinglePass
        };
        if self
            .cursor
            .passive_observer_arm_entry_index(scope, row.selected_arm())
            .is_none()
            && self
                .cursor
                .controller_arm_entry_by_arm(scope, row.selected_arm())
                .is_none()
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        if let Some(selected) = self.selected_arm_for_scope(scope)
            && selected != row.selected_arm()
            && !reentry.is_reentrant()
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        self.decision_state
            .preflight_selected_route_commit(
                lane_idx,
                scope,
                scope_slot,
                row.selected_arm(),
                reentry,
            )
            .ok_or(CursorInvariantError::INVARIANT)?;
        Ok(())
    }

    #[inline]
    fn preflight_lane_relocation(
        &self,
        step: Option<super::RelocatableResidentLaneStep>,
    ) -> Result<(), CursorInvariantError> {
        let Some(step) = step else {
            return Ok(());
        };
        self.cursor
            .node_index_for_relocatable_step(step)
            .ok_or(CursorInvariantError::INVARIANT)?;
        Ok(())
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn prepare_commit_delta(
        &mut self,
        delta: CommitDelta,
    ) -> Result<PreparedCommitDelta, CursorInvariantError> {
        self.preflight_commit_delta(&delta)?;
        let selected_routes = self
            .route_commit_rows
            .seal(delta.selected_route_rows_ref())
            .map_err(|_| CursorInvariantError::INVARIANT)?;
        Ok(PreparedCommitDelta::from_preflighted(
            delta,
            selected_routes,
        ))
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn commit_cursor_realign_index(
        &mut self,
        idx: usize,
    ) -> Result<(), CursorInvariantError> {
        if idx > u16::MAX as usize {
            return Err(CursorInvariantError::INVARIANT);
        }
        let delta = CommitDelta::cursor_only(StateIndex::from_usize(idx));
        let delta = self.prepare_commit_delta(delta)?;
        self.commit_prepared_delta(delta);
        Ok(())
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn commit_prepared_delta(
        &mut self,
        mut delta: PreparedCommitDelta,
    ) -> CommittedCommitDelta {
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
        CommittedCommitDelta {
            event: delta.event(),
            selected_routes: delta.take_selected_routes(),
        }
    }

    #[inline(always)]
    fn apply_prepared_cursor_index(&mut self, idx: usize) {
        self.cursor.set_index(idx);
    }

    #[inline(always)]
    fn apply_prepared_lane_advance(&mut self, target: super::RelocatableResidentLaneStep) {
        let refresh = self.cursor.advance_lane_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn apply_prepared_lane_relocation(&mut self, target: super::RelocatableResidentLaneStep) {
        let refresh = self.cursor.set_lane_cursor_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn apply_prepared_selected_route_commit_row(&mut self, row: SelectedRouteCommitRow, lane: u8) {
        let lane_idx = lane as usize;
        let Some(scope_slot) = self.cursor.route_scope_slot(row.scope()) else {
            crate::invariant();
        };
        let reentry = if self.cursor.route_scope_reentry(row.scope()) {
            ReentryMark::Reentrant
        } else {
            ReentryMark::SinglePass
        };
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
