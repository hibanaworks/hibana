use crate::{
    endpoint::kernel::core::OperationKernel,
    endpoint::{RecvError, RecvResult},
    global::{
        role_program::{
            LANE_SET_VIEW_WORDS, LaneSetView, LaneWord, lane_word_count, lane_word_index,
        },
        typestate::{
            EnabledEventCommit, EventAdmissionError, EventCommitMeta, state_index_to_usize,
        },
    },
    transport::Transport,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn recv_event_enabled(
        &self,
        index: usize,
        meta: EventCommitMeta,
    ) -> Result<EnabledEventCommit, EventAdmissionError> {
        if !self.permits_event(index) {
            return Err(EventAdmissionError::Invariant);
        }
        let conflict = self.progress.cursor.event_conflict_for_index(index);
        self.event_enabled(index, meta, &mut |scope, view| {
            self.selected_arm_for_recv_event(conflict, scope, view)
        })
    }

    pub(in crate::endpoint::kernel::recv) fn live_recv_lanes<'lanes>(
        &self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        words: &'lanes mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> RecvResult<(LaneSetView<'lanes>, usize)> {
        words.fill(0);
        let mut expected_payload = None;
        let mut blocked = None;
        let mut wait_candidate = None;
        for idx in self
            .progress
            .cursor
            .receive_candidates(self.public_header.role(), target_label)
        {
            // The checked immutable index already binds role and logical label.
            let lane = usize::from(crate::invariant_some(
                self.progress.cursor.event_lane_at(idx),
            ));
            let (word, bit) = lane_word_index(lane);
            // One admitted matching receive is enough to poll this lane. Full
            // frame identity and uniqueness are checked after the frame arrives.
            if words[word] & bit != 0 {
                continue;
            }
            if let Some(meta) = self.progress.cursor.try_recv_meta_at(idx)
                && !meta.origin.is_session()
            {
                let admission = self.recv_event_enabled(idx, meta.into());
                if matches!(admission, Err(EventAdmissionError::OtherViewPending))
                    && meta.payload_kind == target_payload
                {
                    wait_candidate = Some(idx);
                }
                if matches!(admission, Err(EventAdmissionError::PredecessorPending))
                    && meta.payload_kind == target_payload
                {
                    blocked = Some(RecvError::OutOfOrder);
                }
                if admission.is_ok() {
                    if meta.payload_kind == target_payload {
                        let lane = meta.lane as usize;
                        if lane >= self.progress.cursor.logical_lane_count() {
                            return Err(RecvError::PhaseInvariant);
                        }
                        let (word, bit) = lane_word_index(lane);
                        words[word] |= bit;
                        wait_candidate.get_or_insert(idx);
                    } else {
                        expected_payload = Some(meta.payload_kind);
                    }
                }
            }
        }
        // The existing u8 lane domain bounds this temporary set to 32 bytes.
        // This set chooses ports to poll; full-key matching still checks all
        // candidate occurrences after receiving a frame.
        // SAFETY: The initialized words outlive the returned borrowed view;
        // the descriptor's lane count is bounded by the same wire domain.
        let lanes = unsafe {
            LaneSetView::from_parts(
                words.as_ptr(),
                lane_word_count(self.progress.cursor.logical_lane_count()),
            )
        };
        // No port is polled for a blocked row. The common actor's commit wakes
        // this view, which then repeats admission against the same progress.
        if let Some(index) = wait_candidate {
            // Only an immutable row locator crosses a transport callback.
            return Ok((lanes, index));
        }
        if let Some(error) = blocked {
            return Err(error);
        }
        // A live row with the same label can carry a different contract.
        // Check whether this exact contract is declared before reporting a
        // schema violation. This cold lookup never grants a lane.
        let declared_here = self
            .progress
            .cursor
            .declared_receive_candidates(self.public_header.role(), target_label)
            .any(|index| {
                self.permits_event(index)
                    && self
                        .progress
                        .cursor
                        .event_row_at(index)
                        .is_some_and(|row| row.atom().payload == target_payload)
            });
        // The current holder may have a whole view whose matching rows all
        // belong to a future holder's image.
        let declared_for_another_holder = !declared_here
            && self.region.is_whole()
            && self.context.program.has_actor_bindings()
            && (0..self.context.program.columns.atom_count()).any(|event| {
                let atom = self.context.program.event_atom_at(event);
                atom.from != atom.to
                    && atom.to == self.public_header.role()
                    && atom.label == target_label
                    && atom.payload == target_payload
                    && !atom.origin.is_session()
            });
        if declared_here || declared_for_another_holder {
            return Err(RecvError::Unavailable);
        }
        Err(match expected_payload {
            Some(expected) => RecvError::PayloadMismatch {
                expected,
                actual: target_payload,
            },
            None => RecvError::PhaseInvariant,
        })
    }

    fn recv_row_excludes_wait(
        &self,
        enabled: EnabledEventCommit,
        observed: Option<usize>,
        retained: bool,
        retired: bool,
    ) -> bool {
        let index = state_index_to_usize(enabled.event_index());
        if retained
            && self.progress.cursor.retained_receive_waits_for_own_send(
                index,
                |prior| self.permits_event(prior),
                &mut |scope| self.selected_arm_for_scope(scope),
            )
        {
            return true;
        }
        retired
            && self
                .borrowed_reentry_scope(index)
                .or_else(|| {
                    self.progress
                        .cursor
                        .roll_body_reentry_scope_for_step(enabled.progress_step(), &mut |scope| {
                            self.selected_arm_for_scope(scope)
                        })
                })
                .is_some_and(|scope| {
                    self.progress
                        .cursor
                        .roll_retirement_observed(scope, index, observed)
                })
    }

    /// Every admitted row must be excluded before a quiet or observed port can
    /// disprove this wait. Check the last promising row first against the
    /// progress and custody visible after the transport callback.
    pub(in crate::endpoint::kernel::recv) fn recv_contract_excludes_wait(
        &self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        observed: Option<usize>,
        preferred: Option<usize>,
    ) -> bool {
        let retired = self.progress.cursor.has_roll_retirement();
        let retained = observed.is_none() && self.progress.cursor.has_retained_roll_completion();
        if !retired && !retained {
            return false;
        }
        let mut found = false;
        let candidates = self
            .progress
            .cursor
            .receive_candidates(self.public_header.role(), target_label);
        for index in preferred
            .into_iter()
            .chain(candidates.filter(|index| Some(*index) != preferred))
        {
            let Some(meta) = self.progress.cursor.try_recv_meta_at(index) else {
                continue;
            };
            if meta.payload_kind != target_payload {
                continue;
            }
            let enabled = match self.recv_event_enabled(index, meta.into()) {
                Ok(enabled) => enabled,
                Err(EventAdmissionError::OtherViewPending) => return false,
                Err(_) => continue,
            };
            found = true;
            if !self.recv_row_excludes_wait(enabled, observed, retained, retired) {
                return false;
            }
        }
        found
    }

    /// An arrived source occurrence can exclude every current occurrence of
    /// another receive contract. This only reads its guards: no frame, route
    /// choice or progress is committed by the comparison.
    pub(in crate::endpoint::kernel::recv) fn recv_contract_allows_observation(
        &self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        observed_index: usize,
    ) -> bool {
        if self.recv_contract_excludes_wait(
            target_label,
            target_payload,
            Some(observed_index),
            None,
        ) {
            return false;
        }
        let observed = self
            .progress
            .cursor
            .event_conflict_for_index(observed_index);
        // A header identifies a source event, not a repeated visit. A queued
        // frame from a later roll visit must not exclude another visit's arm.
        let Some(observed) = self
            .progress
            .cursor
            .route_conflict_guard_rows(observed)
            .find(|row| {
                matches!(
                    row.to_conflict(),
                    Some(crate::global::typestate::LocalConflict::RouteArm { scope, .. })
                        if !self.progress.cursor.route_scope_reentry(scope)
                )
            })
        else {
            return true;
        };
        self.progress
            .cursor
            .receive_candidates(self.public_header.role(), target_label)
            .any(|index| {
                let Some(meta) = self.progress.cursor.try_recv_meta_at(index) else {
                    return false;
                };
                if meta.payload_kind != target_payload
                    || !matches!(
                        self.recv_event_enabled(index, meta.into()),
                        Ok(_) | Err(EventAdmissionError::OtherViewPending)
                    )
                {
                    return false;
                }
                let conflict = self.progress.cursor.event_conflict_for_index(index);
                self.progress.cursor.event_conflict_row_allows_with_preview(
                    conflict,
                    observed,
                    &mut |scope| {
                        self.selected_arm_for_recv_event(
                            conflict,
                            scope,
                            crate::global::typestate::EventArmView::Preview,
                        )
                    },
                )
            })
    }
}
