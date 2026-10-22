use super::{
    CommitDelta, CommitEventRow, CursorInvariantError, OperationKernel, PreparedRouteCommitRows,
    RelocatableResidentLaneStep, SelectedRouteCommitRow, SelectedRouteCommitRowsRef, Transport,
    state_index_to_usize,
};
use crate::global::const_dsl::{ReentryMark, ScopeId};
use crate::global::typestate::{EnabledEventCommit, StateIndex};

#[cfg(any(kani, all(test, hibana_repo_tests)))]
mod send_completion;

pub(crate) struct PreparedCommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: PreparedRouteCommitRows,
    pub(in crate::endpoint::kernel::core) fresh_route_start: u16,
    lane_relocation: Option<RelocatableResidentLaneStep>,
    cursor_after: StateIndex,
    body_reentry: crate::global::const_dsl::ScopeId,
}

impl PreparedCommitDelta {
    const NO_FRESH_ROUTE: u16 = u16::MAX;

    #[inline(always)]
    fn from_preflighted(
        delta: CommitDelta,
        selected_routes: PreparedRouteCommitRows,
        fresh_route_start: Option<usize>,
        body_reentry: Option<crate::global::const_dsl::ScopeId>,
    ) -> Self {
        let fresh_route_start = match fresh_route_start {
            Some(start) if start < selected_routes.len() && start < u16::MAX as usize => {
                start as u16
            }
            Some(_) => crate::invariant(),
            None => Self::NO_FRESH_ROUTE,
        };
        Self {
            event: delta.event(),
            selected_routes,
            fresh_route_start,
            lane_relocation: delta.lane_relocation(),
            cursor_after: delta.cursor_after(),
            body_reentry: body_reentry.unwrap_or(crate::global::const_dsl::ScopeId::none()),
        }
    }

    #[inline(always)]
    pub(crate) const fn event(&self) -> Option<CommitEventRow> {
        self.event
    }

    pub(in crate::endpoint::kernel::core) const fn body_reentry(
        &self,
    ) -> crate::global::const_dsl::ScopeId {
        self.body_reentry
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

    pub(in crate::endpoint::kernel::core) const fn route_is_fresh(&self, index: usize) -> bool {
        self.fresh_route_start != Self::NO_FRESH_ROUTE
            && index >= self.fresh_route_start as usize
            && index < self.selected_routes.len()
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel::core) fn take_selected_routes(
        &mut self,
    ) -> PreparedRouteCommitRows {
        self.selected_routes.take()
    }
}

pub(crate) struct CommittedCommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: PreparedRouteCommitRows,
    fresh_route_start: u16,
}

impl CommittedCommitDelta {
    #[inline(always)]
    pub(in crate::endpoint::kernel::core) const fn from_applied(
        event: Option<CommitEventRow>,
        selected_routes: PreparedRouteCommitRows,
        fresh_route_start: u16,
    ) -> Self {
        Self {
            event,
            selected_routes,
            fresh_route_start,
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
    pub(crate) const fn route_is_fresh(&self, index: usize) -> bool {
        self.fresh_route_start != PreparedCommitDelta::NO_FRESH_ROUTE
            && index >= self.fresh_route_start as usize
            && index < self.selected_routes.len()
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CommitDeltaApplyPermit(());

impl CommitDeltaApplyPermit {
    #[inline(always)]
    pub(in crate::endpoint::kernel::core) const fn new() -> Self {
        Self(())
    }
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn preflight_commit_delta(
        &self,
        delta: &CommitDelta,
    ) -> Result<Option<ScopeId>, CursorInvariantError> {
        let body_reentry = if let Some(event) = delta.event() {
            if !self.region.contains(event.eff_index().dense_ordinal()) {
                return Err(CursorInvariantError::INVARIANT);
            }
            let body_reentry = self.preflight_roll_entry_views(Some(event))?;
            let idx = self
                .progress
                .cursor
                .node_index_for_relocatable_step(event.progress_step())
                .ok_or(CursorInvariantError::INVARIANT)?;
            let mut selected_arm = |scope, view| {
                if matches!(view, crate::global::typestate::EventArmView::Preview)
                    && scope == event.scope()
                {
                    event.route_arm()
                } else {
                    self.selected_arm_for_scope(scope)
                }
            };
            let enabled = self.event_enabled(
                idx,
                crate::global::typestate::EventCommitMeta::new(
                    event.eff_index(),
                    event.event_label(),
                    event.event_origin(),
                    event.scope(),
                    event.route_arm(),
                    event.lane(),
                ),
                &mut selected_arm,
            )?;
            if enabled.progress_step() != event.progress_step()
                || enabled.cursor_after() != delta.cursor_after()
            {
                return Err(CursorInvariantError::INVARIANT);
            }
            self.preflight_event_selected_route_chain(idx, delta)?;
            body_reentry
        } else {
            if delta.selected_routes().is_empty() {
                self.preflight_cursor_realign_delta(delta)?;
            } else {
                self.preflight_route_only_cursor_after(delta)?;
            }
            None
        };
        self.preflight_selected_route_rows(delta)?;
        self.preflight_lane_relocation(delta.lane_relocation())?;
        Ok(body_reentry)
    }

    #[inline]
    fn preflight_event_selected_route_chain(
        &self,
        event_idx: usize,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        let routes = delta.selected_routes();
        let conflict = self.progress.cursor.event_conflict_for_index(event_idx);
        let Some(range) = self
            .progress
            .cursor
            .route_commit_range_for_conflict(conflict)
        else {
            return if conflict.to_conflict().is_none() && routes.is_empty() {
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
                .progress
                .cursor
                .route_commit_row_at(range, idx)
                .and_then(SelectedRouteCommitRow::from_resident_conflict)
            else {
                return Err(CursorInvariantError::INVARIANT);
            };
            let Some(row) = routes.get(&self.progress.cursor, idx) else {
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
        if cursor_after >= self.progress.cursor.local_steps_len() {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
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
                .get(&self.progress.cursor, idx)
                .ok_or(CursorInvariantError::INVARIANT)?;
            let lane = route_lane.ok_or(CursorInvariantError::INVARIANT)?;
            self.preflight_selected_route_row(row, lane)?;
            let mut prev_idx = 0usize;
            while prev_idx < idx {
                let prev = routes
                    .get(&self.progress.cursor, prev_idx)
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

    fn fresh_route_suffix_start(
        &self,
        delta: &CommitDelta,
    ) -> Result<Option<usize>, CursorInvariantError> {
        let routes = delta.selected_route_rows_ref();
        let entry_idx = match delta.event() {
            Some(event) => self
                .progress
                .cursor
                .node_index_for_relocatable_step(event.progress_step())
                .ok_or(CursorInvariantError::INVARIANT)?,
            None => state_index_to_usize(delta.cursor_after()),
        };
        self.fresh_route_start_for_rows(routes, entry_idx, delta.selected_route_lane())
    }

    pub(in crate::endpoint::kernel::core) fn fresh_route_start_for_rows(
        &self,
        routes: SelectedRouteCommitRowsRef,
        entry_idx: usize,
        lane: Option<u8>,
    ) -> Result<Option<usize>, CursorInvariantError> {
        let mut start = None;
        let mut idx = 0usize;
        while idx < routes.len() {
            let row = routes
                .get(&self.progress.cursor, idx)
                .ok_or(CursorInvariantError::INVARIANT)?;
            let scope = row.scope();
            let fresh = match self.selected_arm_for_scope(scope) {
                None => true,
                Some(selected) => {
                    self.progress.cursor.route_scope_reentry(scope)
                        && self.reentrant_selected_arm_complete(scope, selected)
                        && {
                            let mut preview = |candidate, view| {
                                if matches!(view, crate::global::typestate::EventArmView::Committed)
                                {
                                    return self.selected_arm_for_scope(candidate);
                                }
                                let mut row_idx = 0;
                                while row_idx < routes.len() {
                                    let row = crate::invariant_some(
                                        routes.get(&self.progress.cursor, row_idx),
                                    );
                                    if row.scope() == candidate {
                                        return Some(row.selected_arm());
                                    }
                                    row_idx += 1;
                                }
                                self.selected_arm_for_scope(candidate)
                            };
                            self.progress.cursor.route_reentry_head_allows_index(
                                scope,
                                entry_idx,
                                lane.ok_or(CursorInvariantError::INVARIANT)?,
                                &mut preview,
                            )
                        }
                }
            };
            if let Some(root_idx) = start {
                // A fresh containing visit also owns its descriptor-proven
                // descendants, even when their previous selection was live.
                let root = routes
                    .get(&self.progress.cursor, root_idx)
                    .ok_or(CursorInvariantError::INVARIANT)?;
                if self
                    .progress
                    .cursor
                    .route_scope_conflict_arm_for_scope(scope, root.scope())
                    != Some(root.selected_arm())
                {
                    return Err(CursorInvariantError::INVARIANT);
                }
            } else if fresh {
                start = Some(idx);
            } else if self.selected_arm_for_scope(scope) != Some(row.selected_arm()) {
                // A retained ancestor cannot change its arm or lose its prefix.
                return Err(CursorInvariantError::INVARIANT);
            }
            idx += 1;
        }
        Ok(start)
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
        if lane_idx >= self.progress.cursor.logical_lane_count() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let scope = row.scope();
        if scope.is_none() || self.progress.cursor.route_scope_slot(scope).is_none() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let scope_slot = self
            .progress
            .cursor
            .route_scope_slot(scope)
            .ok_or(CursorInvariantError::INVARIANT)?;
        let reentry = if self.progress.cursor.route_scope_reentry(scope) {
            ReentryMark::Reentrant
        } else {
            ReentryMark::SinglePass
        };
        if self
            .progress
            .cursor
            .passive_observer_arm_entry_index(scope, row.selected_arm())
            .is_none()
            && self
                .progress
                .cursor
                .controller_arm_entry_by_arm(scope, row.selected_arm())
                .is_none()
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        if let Some(selected) = self.selected_arm_for_scope(scope) {
            if reentry.is_reentrant() && self.reentrant_selected_arm_complete(scope, selected) {
                return Ok(());
            }
            if selected != row.selected_arm() {
                return Err(CursorInvariantError::INVARIANT);
            }
        }
        self.progress
            .decision_state
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
        self.progress
            .cursor
            .node_index_for_relocatable_step(step)
            .ok_or(CursorInvariantError::INVARIANT)?;
        Ok(())
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn prepare_commit_delta(
        &mut self,
        delta: CommitDelta,
    ) -> Result<PreparedCommitDelta, CursorInvariantError> {
        let body_reentry = self.preflight_commit_delta(&delta)?;
        let fresh_route_start = self.fresh_route_suffix_start(&delta)?;
        let selected_routes = self
            .progress
            .route_commit_rows
            .seal(delta.selected_route_rows_ref())
            .map_err(|_| CursorInvariantError::INVARIANT)?;
        Ok(PreparedCommitDelta::from_preflighted(
            delta,
            selected_routes,
            fresh_route_start,
            body_reentry,
        ))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn prepare_enabled_event_commit_delta(
        &mut self,
        delta: CommitDelta,
        enabled: EnabledEventCommit,
    ) -> Result<PreparedCommitDelta, CursorInvariantError> {
        let event = delta.event().ok_or(CursorInvariantError::INVARIANT)?;
        let body_reentry = self.preflight_roll_entry_views(Some(event))?;
        if event.progress_step() != enabled.progress_step()
            || delta.cursor_after() != enabled.cursor_after()
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        let idx = enabled.event_index().as_usize();
        self.preflight_event_selected_route_chain(idx, &delta)?;
        self.preflight_selected_route_rows(&delta)?;
        self.preflight_lane_relocation(delta.lane_relocation())?;
        let fresh_route_start = self.fresh_route_suffix_start(&delta)?;
        let selected_routes = self
            .progress
            .route_commit_rows
            .seal(delta.selected_route_rows_ref())
            .map_err(|_| CursorInvariantError::INVARIANT)?;
        Ok(PreparedCommitDelta::from_preflighted(
            delta,
            selected_routes,
            fresh_route_start,
            body_reentry,
        ))
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn commit_cursor_realign_index(
        &mut self,
        idx: usize,
    ) -> Result<(), CursorInvariantError> {
        let cursor_after =
            StateIndex::checked_from_usize(idx).ok_or(CursorInvariantError::INVARIANT)?;
        let delta = CommitDelta::cursor_only(cursor_after);
        let delta = self.prepare_commit_delta(delta)?;
        let _ = self.commit_prepared_delta(delta, None);
        Ok(())
    }
}
