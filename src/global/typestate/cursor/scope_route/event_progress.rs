use super::super::super::facts::LocalDependencyState;
use super::super::{
    CursorInvariantError, EnabledEventCommit, EventCursor, LocalDependency,
    RelocatableResidentLaneStep, ScopeId, StateIndex,
};
use crate::global::typestate::EventCommitMeta;

impl EventCursor {
    #[inline]
    pub(crate) fn parallel_scope_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().parallel_root(scope_id)
    }

    #[inline(always)]
    pub(crate) fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        self.machine().dependency_for_index(current_idx)
    }

    #[inline]
    fn dependency_state(
        &self,
        dependency: LocalDependency,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> LocalDependencyState {
        if !Self::dependency_applies(dependency, &mut *selected_arm_for_scope) {
            return LocalDependencyState::InactiveByConflict;
        }
        if self.dependency_row_live_events_done(dependency, selected_arm_for_scope) {
            LocalDependencyState::Satisfied
        } else {
            LocalDependencyState::Blocked
        }
    }

    pub(crate) fn event_row_matches_commit(&self, idx: usize, event: EventCommitMeta) -> bool {
        self.machine()
            .event_program()
            .event_row_at(idx)
            .is_some_and(|row| {
                row.matches_commit(
                    event.eff_index,
                    event.label,
                    event.origin,
                    event.scope,
                    event.route_arm,
                    event.lane,
                )
            })
    }

    #[inline]
    fn validate_event_enabled_dependency(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<(), CursorInvariantError> {
        if let Some(dependency) = self.dependency_for_index(idx)
            && !self
                .dependency_state(dependency, selected_arm_for_scope)
                .allows_event()
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
    }

    #[inline]
    fn validate_event_enabled_reentry_if_done(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        event: EventCommitMeta,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<(), CursorInvariantError> {
        if !self.relocatable_step_done(progress_step) {
            return Ok(());
        }
        if !self.has_reentry_scopes()
            || !self.roll_reentry_event_allows_index(idx, event.lane, &mut *selected_arm_for_scope)
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
    }

    #[inline]
    fn validate_event_enabled_commit(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        cursor_after: StateIndex,
        event: EventCommitMeta,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<(), CursorInvariantError> {
        if self.node_next_index_at(idx) != cursor_after {
            return Err(CursorInvariantError::INVARIANT);
        }
        self.validate_event_enabled_dependency(idx, selected_arm_for_scope)?;
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        if !self.event_conflict_row_allows_with_preview(
            preview_conflict,
            preview_conflict,
            &mut *selected_arm_for_scope,
        ) {
            return Err(CursorInvariantError::INVARIANT);
        }
        let resident_step =
            self.relocatable_resident_lane_step_at_index(idx, event.lane as usize)?;
        if resident_step != progress_step {
            return Err(CursorInvariantError::INVARIANT);
        }
        self.validate_event_enabled_reentry_if_done(
            idx,
            progress_step,
            event,
            selected_arm_for_scope,
        )?;
        if !self.event_lane_head_allows(
            progress_step,
            preview_conflict,
            &mut *selected_arm_for_scope,
        ) {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn event_enabled(
        &self,
        idx: usize,
        event: EventCommitMeta,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<EnabledEventCommit, CursorInvariantError> {
        if !self.event_row_matches_commit(idx, event) {
            return Err(CursorInvariantError::INVARIANT);
        }
        let progress_step =
            self.relocatable_resident_lane_step_at_index(idx, event.lane as usize)?;
        let cursor_after = self.node_next_index_at(idx);
        self.validate_event_enabled_commit(
            idx,
            progress_step,
            cursor_after,
            event,
            selected_arm_for_scope,
        )?;
        Ok(EnabledEventCommit::new(
            StateIndex::from_usize(idx),
            progress_step,
            cursor_after,
        ))
    }
}
