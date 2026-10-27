use super::super::super::facts::LocalDependencyState;
use super::super::{
    CursorInvariantError, EnabledEventCommit, EventCursor, LocalDependency, PackedEventConflict,
    RelocatableResidentLaneStep, ScopeId, StateIndex,
};
use crate::global::{role_program::RoleLocalDirection, typestate::EventCommitMeta};

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
    /// A preceding action needs the endpoint payload. A send future would
    /// hold that value, while a parallel borrow leaves it available.
    PayloadPredecessorPending,
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
    fn source_predecessor_wait(
        &self,
        index: usize,
        blocked: (u8, Option<u8>),
        conflict: PackedEventConflict,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> EventAdmissionError {
        let mut wait = EventAdmissionError::OtherViewPending;
        let (after, after_direction) = self
            .machine()
            .event_program()
            .operation_at(index)
            .unwrap_or_else(|| crate::invariant());
        // Constructor-checked rows follow source order, with a send before
        // its co-resident receive. A predecessor must occur in this prefix.
        for prior in self.pending_event_steps(0..index) {
            let uses_caller = self.uses_view_at(prior, blocked.0);
            if !(uses_caller || blocked.1.is_some_and(|role| self.uses_view_at(prior, role))) {
                continue;
            }
            let (before, before_direction) = self
                .machine()
                .event_program()
                .operation_at(prior)
                .unwrap_or_else(|| crate::invariant());
            let precedes = if before == after {
                before_direction == RoleLocalDirection::Send
                    && after_direction == RoleLocalDirection::Recv
            } else {
                self.program_ref()
                    .source_precedes(before.dense_ordinal(), after.dense_ordinal())
            };
            if precedes
                && self.event_actor_is_local(prior, selected) != Some(false)
                && self.event_conflict_row_allows_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    selected,
                )
            {
                if uses_caller {
                    return EventAdmissionError::PredecessorPending;
                }
                wait = EventAdmissionError::PayloadPredecessorPending;
            }
        }
        wait
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

    #[inline]
    fn validate_event_enabled_reentry(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        event: EventCommitMeta,
        arm_for_scope: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<(), EventAdmissionError> {
        if !event.body_reentry.is_none() {
            let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
            if !self.roll_scope_head_allows_index(event.body_reentry, idx, event.lane, &mut preview)
            {
                return Err(self.reentry_view_wait(idx, event, arm_for_scope));
            }
            return Ok(());
        }
        // A lane head is a relocatable lookup position. Independent parallel
        // work can move it past an unfinished step. Completion and committed
        // choices determine whether this step belongs to the current visit.
        if !self.relocatable_step_done(progress_step) {
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
            return Err(self.source_predecessor_wait(
                idx,
                (role, moved_role),
                preview_conflict,
                &mut |scope| arm_for_scope(scope, EventArmView::Preview),
            ));
        }
        let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
        if !self.event_lane_head_allows(progress_step, preview_conflict, &mut preview) {
            let role = crate::invariant_some(self.logical_role_at(idx));
            return Err(self.source_predecessor_wait(
                idx,
                (role, moved_role),
                preview_conflict,
                &mut preview,
            ));
        }
        Ok(EnabledEventCommit::new(
            StateIndex::from_usize(idx),
            progress_step,
            cursor_after,
        ))
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::{Endpoint, g, runtime::program::project};

    #[test]
    fn payload_prefixes_are_distinct_from_the_borrowed_views_own_prefixes() {
        type Loan<'r> = g::Msg<2, Endpoint<'r, 3>>;
        let global = g::par(
            g::send::<0, 0, g::Msg<0, ()>>().roll(),
            g::seq(
                g::send::<0, 3, g::Msg<1, ()>>(),
                g::send::<1, 0, Loan<'_>>(),
            ),
        );
        let program = project::<1>(&global);
        super::super::super::validated_metadata_tests::with_rows(
            program.role_image_ref(),
            |cursor| {
                let receive = (0..cursor.local_steps_len())
                    .find(|&i| cursor.try_recv_meta_at(i).is_some_and(|m| m.label == 1))
                    .unwrap();
                let transfer = (0..cursor.local_steps_len())
                    .find(|&i| cursor.try_send_meta_at(i).is_some_and(|m| m.label == 2))
                    .unwrap();
                for received in [false, true] {
                    if received {
                        cursor.mark_local_event_done(receive);
                    }
                    for hint in 0..=u8::MAX {
                        cursor.set_index(usize::from(hint) % cursor.machine().node_len());
                        let event = cursor.try_send_meta_at(transfer).unwrap().into();
                        let admission = cursor.event_enabled(transfer, event, &mut |_, _| None);
                        if received {
                            assert!(admission.is_ok(), "hint={hint}");
                        } else {
                            assert!(
                                matches!(
                                    admission,
                                    Err(EventAdmissionError::PayloadPredecessorPending)
                                ),
                                "hint={hint}"
                            );
                        }
                    }
                }
            },
        );
    }
}
