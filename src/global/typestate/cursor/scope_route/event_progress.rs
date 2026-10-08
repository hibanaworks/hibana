use super::super::super::facts::LocalDependencyState;
use super::super::{
    CursorInvariantError, EnabledEventCommit, EventCursor, LocalDependency,
    RelocatableResidentLaneStep, ResidentLaneStep, ScopeId, StateIndex,
};
use crate::global::typestate::EventCommitMeta;

/// Completion belongs to the committed visit; conflict preview belongs to the
/// candidate visit. A prospective arm must never rewrite completion history.
pub(crate) enum EventArmView {
    Committed,
    Preview,
}

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

    #[inline(never)]
    fn event_progress_passed(&self, target: RelocatableResidentLaneStep) -> bool {
        if let Some(head) = self.step_index_at_lane(usize::from(target.0.lane)) {
            // A reset head bounds the fresh visit; completed events beyond the
            // enclosing roll must not leak into its new progress.
            return usize::from(target.0.step_idx) < head;
        }
        // A parked lane has no materialized head, including at a parallel join.
        // Only a later committed event proves that this visit passed a suffix.
        let event_program = self.machine().event_program();
        for step_idx in usize::from(target.0.step_idx) + 1..self.local_steps_len() {
            if event_program.local_step_lane(step_idx) == Some(target.0.lane)
                && self.relocatable_step_done(RelocatableResidentLaneStep(ResidentLaneStep {
                    step_idx: step_idx as u16,
                    lane: target.0.lane,
                }))
            {
                return true;
            }
        }
        false
    }

    #[inline]
    fn validate_event_enabled_reentry(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        event: EventCommitMeta,
        arm_for_scope: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<(), CursorInvariantError> {
        // Unchosen events keep a clear completion bit even after lane progress
        // has passed their region. They need a fresh roll visit just as consumed
        // events do; a previous visit's prefix cannot authorize a past suffix.
        if !self.relocatable_step_done(progress_step) && !self.event_progress_passed(progress_step)
        {
            // An unchosen suffix can lie ahead of a rewound lane head. Its
            // clear bit is not a fresh visit when the enclosing body is done.
            let mut committed = |scope| arm_for_scope(scope, EventArmView::Committed);
            if self
                .complete_roll_body_scope_for_index(idx, &mut committed)
                .is_none()
            {
                return Ok(());
            }
        }
        if !self.has_reentry_scopes()
            || !self.roll_reentry_event_allows_index(idx, event.lane, arm_for_scope)
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn event_enabled(
        &self,
        idx: usize,
        event: EventCommitMeta,
        arm_for_scope: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<EnabledEventCommit, CursorInvariantError> {
        // Descriptor facts are immutable within this admission. Keep one
        // checked row; completion, route choice and reentry remain live checks.
        let row = self
            .machine()
            .event_program()
            .event_row_at(idx)
            .ok_or(CursorInvariantError::INVARIANT)?;
        if !row.matches_commit(event) {
            return Err(CursorInvariantError::INVARIANT);
        }
        let progress_step =
            self.relocatable_resident_lane_step_at_index(idx, event.lane as usize)?;
        let cursor_after = row.next();
        let preview_conflict = row.conflict();
        {
            let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
            if let Some(dependency) = row.dependency()
                && !self
                    .dependency_state(dependency, &mut preview)
                    .allows_event()
            {
                return Err(CursorInvariantError::INVARIANT);
            }
            if !self.event_conflict_row_allows_with_preview(
                preview_conflict,
                preview_conflict,
                &mut preview,
            ) {
                return Err(CursorInvariantError::INVARIANT);
            }
        }
        self.validate_event_enabled_reentry(idx, progress_step, event, arm_for_scope)?;
        let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
        if !self.event_lane_head_allows(progress_step, preview_conflict, &mut preview) {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(EnabledEventCommit::new(
            StateIndex::from_usize(idx),
            progress_step,
            cursor_after,
        ))
    }
}
