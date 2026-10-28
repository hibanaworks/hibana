use super::super::super::facts::LocalDependencyState;
use super::super::{
    CursorInvariantError, EnabledEventCommit, EventCursor, LocalDependency, PackedEventConflict,
    RelocatableResidentLaneStep, ScopeId, StateIndex,
};
use crate::global::event_program::LocalEventRow;
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

/// Eligibility rejects immediately; only a caller that needs an error category
/// inspects which view must perform the blocked prefix.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventBlocker {
    Invariant,
    OwnershipPending,
    SourcePredecessor,
    Reentry,
}

impl From<CursorInvariantError> for EventBlocker {
    fn from(_: CursorInvariantError) -> Self {
        Self::Invariant
    }
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
        let mut pending = self.pending_event_steps(0..index);
        while let Some(prior) = pending.next() {
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
            if precedes && self.event_actor_is_local(prior, selected) != Some(false) {
                match self.event_conflict_row_admission_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    selected,
                ) {
                    super::ConflictAdmission::Allowed => {
                        if uses_caller {
                            return EventAdmissionError::PredecessorPending;
                        }
                        wait = EventAdmissionError::PayloadPredecessorPending;
                    }
                    super::ConflictAdmission::Excluded { scope, arm } => {
                        let next = self.excluded_arm_end(prior, scope, arm).min(index);
                        pending = self.pending_event_steps(next..index);
                    }
                    super::ConflictAdmission::Invalid => (),
                }
            }
        }
        wait
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
    fn validate_event_eligibility_reentry(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        body_reentry: ScopeId,
        lane: u8,
        arm_for_scope: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<(), EventBlocker> {
        if !body_reentry.is_none() {
            let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
            if !self.roll_scope_head_allows_index(body_reentry, idx, lane, &mut preview) {
                return Err(EventBlocker::Reentry);
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
            let completed_scope = self.complete_roll_body_scope_for_index(idx, &mut |scope| {
                arm_for_scope(scope, EventArmView::Committed)
            });
            if let Some(scope) = completed_scope {
                let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
                return (self.has_reentry_scopes()
                    && self.roll_scope_head_allows_index(scope, idx, lane, &mut preview))
                .then_some(())
                .ok_or(EventBlocker::Reentry);
            }
            let conflict = self.machine().event_conflict_for_index(idx);
            let mut committed = |scope| arm_for_scope(scope, EventArmView::Committed);
            return if self.event_conflict_row_allows_with_preview(
                conflict,
                conflict,
                &mut committed,
            ) {
                Ok(())
            } else {
                Err(EventBlocker::Invariant)
            };
        }
        if !self.has_reentry_scopes()
            || !self.roll_reentry_event_allows_index(idx, lane, arm_for_scope)
        {
            return Err(EventBlocker::Reentry);
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
        let row = self
            .machine()
            .event_program()
            .event_row_at(idx)
            .filter(|row| row.matches_commit(event))
            .ok_or(EventAdmissionError::Invariant)?;
        self.event_eligibility(row, event.body_reentry, arm_for_scope)
            .map_err(|blocker| match blocker {
                EventBlocker::Invariant => EventAdmissionError::Invariant,
                EventBlocker::OwnershipPending => EventAdmissionError::PredecessorPending,
                EventBlocker::SourcePredecessor => {
                    let moved_role = if row.direction() == RoleLocalDirection::Send
                        && let crate::eff::EventPayload::Endpoint(role) = row.atom().payload
                    {
                        Some(role)
                    } else {
                        None
                    };
                    self.source_predecessor_wait(
                        idx,
                        (row.logical_role(), moved_role),
                        row.conflict(),
                        &mut |scope| arm_for_scope(scope, EventArmView::Preview),
                    )
                }
                EventBlocker::Reentry => self.reentry_view_wait(idx, event, arm_for_scope),
            })
    }

    #[inline(never)]
    pub(crate) fn event_eligibility(
        &self,
        row: LocalEventRow,
        body_reentry: ScopeId,
        arm_for_scope: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<EnabledEventCommit, EventBlocker> {
        // The row constructor establishes operation identity. A row from
        // another image cannot authorize this cursor's progress.
        if !self.machine().event_program().contains_row(row) {
            return Err(EventBlocker::Invariant);
        }
        let idx = row.index();
        let lane = row.atom().lane;
        match self.event_actor_is_local(idx, &mut |scope| {
            arm_for_scope(scope, EventArmView::Preview)
        }) {
            Some(true) => {}
            Some(false) => return Err(EventBlocker::Invariant),
            None => return Err(EventBlocker::OwnershipPending),
        }
        let progress_step = self.relocatable_resident_lane_step_at_index(idx, lane as usize)?;
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
                return Err(EventBlocker::Invariant);
            }
            dependency_state
        };
        self.validate_event_eligibility_reentry(
            idx,
            progress_step,
            body_reentry,
            lane,
            arm_for_scope,
        )?;
        // A valid future event need not be a current lane head. Its projected
        // predecessor must commit before that head can become available.
        if dependency_state.is_some_and(|state| !state.allows_event()) {
            return Err(EventBlocker::SourcePredecessor);
        }
        let mut preview = |scope| arm_for_scope(scope, EventArmView::Preview);
        if !self.event_lane_head_allows(progress_step, preview_conflict, &mut preview) {
            return Err(EventBlocker::SourcePredecessor);
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
    fn canonical_row_admission_preserves_metadata_and_image_boundaries() {
        let global = g::seq(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::send::<1, 0, g::Msg<2, ()>>(),
        );
        let program = project::<0>(&global);
        let foreign_global = g::send::<0, 1, g::Msg<3, ()>>();
        let foreign = project::<0>(&foreign_global);
        let foreign_program =
            crate::global::event_program::LocalEventProgram::from_rows(foreign.role_image_ref());
        super::super::super::validated_metadata_tests::with_rows(
            program.role_image_ref(),
            |cursor| {
                let row = cursor.event_row_at(0).unwrap();
                let event = row.commit_meta();
                let canonical = cursor
                    .event_eligibility(row, ScopeId::none(), &mut |_, _| None)
                    .unwrap();
                assert_eq!(
                    cursor.event_enabled(0, event, &mut |_, _| None),
                    Ok(canonical),
                );
                let mut malformed = [event; 6];
                malformed[0].eff_index = cursor.event_row_at(1).unwrap().effect();
                malformed[1].label = 2;
                malformed[2].origin = crate::eff::EventOrigin::Session;
                malformed[3].scope = ScopeId::route(0);
                malformed[4].route_arm = Some(0);
                malformed[5].lane = 1;
                for (field, event) in malformed.into_iter().enumerate() {
                    assert_eq!(
                        cursor.event_enabled(0, event, &mut |_, _| None),
                        Err(EventAdmissionError::Invariant),
                        "changed metadata field {field}",
                    );
                }
                assert_eq!(
                    cursor.event_eligibility(
                        foreign_program.event_row_at(0).unwrap(),
                        ScopeId::none(),
                        &mut |_, _| None,
                    ),
                    Err(EventBlocker::Invariant),
                );
                assert!(!cursor.local_event_done(0));
                assert!(!cursor.local_event_done(1));
            },
        );
    }

    #[test]
    fn nested_owned_prefix_classification_matches_the_source_predecessor_set() {
        type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
        let global = g::seq(
            g::send::<0, 1, Loan<'_>>(),
            g::seq(
                g::par(
                    g::route(
                        g::seq(
                            g::send::<3, 2, g::Msg<2, ()>>(),
                            g::send::<2, 3, g::Msg<3, ()>>(),
                        ),
                        g::seq(
                            g::send::<3, 2, g::Msg<4, ()>>(),
                            g::send::<2, 3, g::Msg<5, ()>>(),
                        ),
                    ),
                    g::route(
                        g::send::<1, 2, g::Msg<6, ()>>(),
                        g::send::<1, 2, g::Msg<7, ()>>(),
                    ),
                ),
                g::send::<1, 0, Loan<'_>>(),
            ),
        )
        .roll();
        let program = project::<1>(&global);
        super::super::super::validated_metadata_tests::with_rows(
            program.role_image_ref(),
            |cursor| {
                let steps = cursor.local_steps_len();
                let routes = cursor.machine().route_scope_slot_count();
                assert!(steps <= 10 && routes <= 3);
                for completed in 0..1u32 << steps {
                    cursor.completed_event_words_mut()[0] = completed;
                    for choices in 0..3usize.pow(routes as u32) {
                        let selected = |scope| {
                            let slot = cursor.route_scope_slot(scope)?;
                            match choices / 3usize.pow(slot as u32) % 3 {
                                0 => None,
                                1 => Some(0),
                                _ => Some(1),
                            }
                        };
                        for index in 0..steps {
                            let (after, after_direction) = cursor
                                .machine()
                                .event_program()
                                .operation_at(index)
                                .unwrap();
                            let conflict = cursor.machine().event_conflict_for_index(index);
                            for caller in 0..4 {
                                for payload in [None, Some(0), Some(1), Some(2), Some(3)] {
                                    let live: std::vec::Vec<_> = (0..index)
                                        .filter(|&prior| {
                                            let (before, direction) = cursor
                                                .machine()
                                                .event_program()
                                                .operation_at(prior)
                                                .unwrap();
                                            completed & (1 << prior) == 0
                                                && (if before == after {
                                                    direction == RoleLocalDirection::Send
                                                        && after_direction
                                                            == RoleLocalDirection::Recv
                                                } else {
                                                    cursor.program_ref().source_precedes(
                                                        before.dense_ordinal(),
                                                        after.dense_ordinal(),
                                                    )
                                                })
                                                && cursor
                                                    .event_actor_is_local(prior, &mut |scope| {
                                                        selected(scope)
                                                    })
                                                    != Some(false)
                                                && cursor.event_conflict_row_allows_with_preview(
                                                    cursor
                                                        .machine()
                                                        .event_conflict_for_index(prior),
                                                    conflict,
                                                    &mut |scope| selected(scope),
                                                )
                                        })
                                        .collect();
                                    let expected = if live
                                        .iter()
                                        .any(|&prior| cursor.uses_view_at(prior, caller))
                                    {
                                        EventAdmissionError::PredecessorPending
                                    } else if payload.is_some_and(|role| {
                                        live.iter().any(|&prior| cursor.uses_view_at(prior, role))
                                    }) {
                                        EventAdmissionError::PayloadPredecessorPending
                                    } else {
                                        EventAdmissionError::OtherViewPending
                                    };
                                    assert_eq!(
                                        cursor.source_predecessor_wait(
                                            index,
                                            (caller, payload),
                                            conflict,
                                            &mut |scope| selected(scope)
                                        ),
                                        expected,
                                        "{completed:x}/{choices}/{index}/{caller}/{payload:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            },
        );
    }

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
                        let eligibility = cursor.event_eligibility(
                            cursor.event_row_at(transfer).unwrap(),
                            ScopeId::none(),
                            &mut |_, _| None,
                        );
                        let admission = cursor.event_enabled(transfer, event, &mut |_, _| None);
                        if received {
                            assert!(eligibility.is_ok(), "hint={hint}");
                            assert_eq!(eligibility.unwrap(), admission.unwrap(), "hint={hint}");
                        } else {
                            assert_eq!(
                                eligibility,
                                Err(EventBlocker::SourcePredecessor),
                                "hint={hint}"
                            );
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
