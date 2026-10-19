use super::super::super::facts::LocalDependencyState;
use super::super::{
    CursorInvariantError, EnabledEventCommit, EventCursor, LocalAction, LocalDependency,
    PackedEventConflict, RelocatableResidentLaneStep, ResidentLaneStep, ScopeId, StateIndex,
    state_index_to_usize,
};
use crate::global::typestate::EventCommitMeta;

mod recovery;

/// Completion belongs to the committed visit; conflict preview belongs to the
/// candidate visit. A prospective arm must never rewrite completion history.
pub(crate) enum EventArmView {
    Committed,
    Preview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventAdmissionError {
    Invariant,
    PredecessorPending,
    /// The view's own preceding actions are complete. Another co-resident view
    /// must commit first; waiting grants no transport or progress authority.
    OtherViewPending,
}

impl From<CursorInvariantError> for EventAdmissionError {
    fn from(_: CursorInvariantError) -> Self {
        Self::Invariant
    }
}

impl From<EventAdmissionError> for CursorInvariantError {
    fn from(_: EventAdmissionError) -> Self {
        Self::INVARIANT
    }
}

impl EventCursor {
    /// Compact guards can cover an ancestor through another join. Classifying
    /// a blocked future must still see that ancestor if it needs a view held by
    /// the future itself. Independent parallel siblings are never ancestors.
    fn pending_source_predecessor_uses(
        &self,
        index: usize,
        blocked: (u8, Option<u8>),
        conflict: PackedEventConflict,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let after = match self.action_at(index) {
            LocalAction::Send { eff_index, .. }
            | LocalAction::Recv { eff_index, .. }
            | LocalAction::Local { eff_index, .. } => eff_index.dense_ordinal(),
            LocalAction::Terminate => crate::invariant(),
        };
        for step in self.pending_event_steps(0..self.local_steps_len()) {
            let Some(prior) = self
                .machine()
                .state_for_step_index(step)
                .map(state_index_to_usize)
            else {
                crate::invariant();
            };
            if !(self.uses_view_at(prior, blocked.0)
                || blocked.1.is_some_and(|role| self.uses_view_at(prior, role)))
            {
                continue;
            }
            let before = match self.action_at(prior) {
                LocalAction::Send { eff_index, .. }
                | LocalAction::Recv { eff_index, .. }
                | LocalAction::Local { eff_index, .. } => eff_index.dense_ordinal(),
                LocalAction::Terminate => crate::invariant(),
            };
            let precedes = if before == after {
                self.is_send_at(prior) && self.is_recv_at(index)
            } else {
                self.program_ref().source_precedes(before, after)
            };
            if precedes
                && self.event_actor_is_local(prior, selected) != Some(false)
                && self.event_conflict_row_allows_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    selected,
                )
            {
                return true;
            }
        }
        false
    }

    #[inline]
    pub(crate) fn parallel_scope_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().parallel_root(scope_id)
    }

    #[inline]
    fn dependency_state(
        &self,
        target: usize,
        dependency: LocalDependency,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> LocalDependencyState {
        if !Self::dependency_applies(dependency, &mut *selected_arm_for_scope) {
            return LocalDependencyState::InactiveByConflict;
        }
        if self.dependency_row_live_events_done(target, dependency, selected_arm_for_scope) {
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
    ) -> Result<(), EventAdmissionError> {
        // Unchosen events keep a clear completion bit even after lane progress
        // has passed their region. They need a fresh roll visit just as consumed
        // events do; a previous visit's prefix cannot authorize a past suffix.
        if !self.relocatable_step_done(progress_step) && !self.event_progress_passed(progress_step)
        {
            // A clear bit can belong to an unchosen arm at the lane head.
            // Without a completed explicit roll, committed choices still own
            // this visit. A candidate preview cannot replace that authority.
            let mut committed = |scope| arm_for_scope(scope, EventArmView::Committed);
            if self
                .complete_roll_body_scope_for_index(idx, &mut committed)
                .is_none()
            {
                let conflict = self.machine().event_conflict_for_index(idx);
                return if self.event_conflict_row_allows_with_preview(
                    conflict,
                    conflict,
                    &mut committed,
                ) {
                    Ok(())
                } else {
                    Err(EventAdmissionError::Invariant)
                };
            }
        }
        if !self.has_reentry_scopes()
            || !self.roll_reentry_event_allows_index(idx, event.lane, arm_for_scope)
        {
            return Err(self.reentry_view_wait(idx, event, arm_for_scope));
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn event_enabled(
        &self,
        idx: usize,
        event: EventCommitMeta,
        arm_for_scope: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<EnabledEventCommit, EventAdmissionError> {
        // Descriptor facts are immutable within this admission. Keep one
        // checked row; completion, route choice and reentry remain live checks.
        let row = self
            .machine()
            .event_program()
            .event_row_at(idx)
            .ok_or(CursorInvariantError::INVARIANT)?;
        if !row.matches_commit(event) {
            return Err(EventAdmissionError::Invariant);
        }
        match self.event_actor_is_local(idx, &mut |scope| {
            arm_for_scope(scope, EventArmView::Preview)
        }) {
            Some(true) => {}
            Some(false) => return Err(EventAdmissionError::Invariant),
            None => return Err(EventAdmissionError::PredecessorPending),
        }
        let progress_step =
            self.relocatable_resident_lane_step_at_index(idx, event.lane as usize)?;
        // An owned send holds the resource view in its future. Work that still
        // requires that view cannot complete while the send waits.
        // The row already matched this effect. Ownership needs only its
        // payload kind, not another complete send descriptor.
        let moved_role = if self.is_send_at(idx)
            && let crate::eff::EventPayload::Endpoint(role) = self
                .program_ref()
                .event_atom_at(event.eff_index.dense_ordinal())
                .payload
        {
            Some(role)
        } else {
            None
        };
        let cursor_after = row.next();
        let preview_conflict = row.conflict();
        let dependency_state = {
            let mut preview = |scope| {
                // The candidate path guards its predecessors even when a
                // completed parent visit has been masked for fresh admission.
                // The separate reentry check still reads committed history.
                arm_for_scope(scope, EventArmView::Preview)
                    .or_else(|| self.preview_conflict_arm(preview_conflict, scope))
            };
            let dependency_state = row
                .dependency()
                .map(|dependency| self.dependency_state(idx, dependency, &mut preview));
            if !self.event_conflict_row_allows_with_preview(
                preview_conflict,
                preview_conflict,
                &mut preview,
            ) {
                return Err(EventAdmissionError::Invariant);
            }
            dependency_state
        };
        self.validate_event_enabled_reentry(idx, progress_step, event, arm_for_scope)?;
        // A valid future event need not be a current lane head. Its projected
        // predecessor must commit before that head can become available.
        if dependency_state.is_some_and(|state| !state.allows_event()) {
            let role = crate::invariant_some(self.logical_role_at(idx));
            let own_pending = self.pending_source_predecessor_uses(
                idx,
                (role, moved_role),
                preview_conflict,
                &mut |scope| arm_for_scope(scope, EventArmView::Preview),
            );
            return Err(if own_pending {
                EventAdmissionError::PredecessorPending
            } else {
                EventAdmissionError::OtherViewPending
            });
        }
        let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
        if !self.event_lane_head_allows(progress_step, preview_conflict, &mut preview) {
            let role = crate::invariant_some(self.logical_role_at(idx));
            return Err(
                if self.pending_source_predecessor_uses(
                    idx,
                    (role, moved_role),
                    preview_conflict,
                    &mut preview,
                ) {
                    EventAdmissionError::PredecessorPending
                } else {
                    EventAdmissionError::OtherViewPending
                },
            );
        }
        Ok(EnabledEventCommit::new(
            StateIndex::from_usize(idx),
            progress_step,
            cursor_after,
        ))
    }
}
