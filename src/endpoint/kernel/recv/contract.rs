use crate::{
    endpoint::kernel::core::{CommitEventRow, CommitRow, OperationKernel},
    endpoint::{RecvError, RecvResult},
    global::{
        role_program::{
            LANE_SET_VIEW_WORDS, LaneSetView, LaneWord, lane_word_count, lane_word_index,
        },
        typestate::{EnabledEventCommit, EventAdmissionError, EventCommitMeta},
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
        let enabled = self
            .progress
            .cursor
            .event_enabled(index, meta, &mut |scope, view| {
                self.selected_arm_for_recv_event(conflict, scope, view)
            })?;
        self.preflight_roll_entry_views(Some(CommitEventRow::new(
            meta.eff_index,
            meta.label,
            meta.origin,
            CommitRow::new(meta.scope, meta.route_arm, meta.lane),
            enabled.progress_step(),
        )))?;
        Ok(enabled)
    }

    pub(in crate::endpoint::kernel::recv) fn live_recv_lanes<'lanes>(
        &self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        words: &'lanes mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> RecvResult<LaneSetView<'lanes>> {
        words.fill(0);
        let mut expected_payload = None;
        let mut other_view_pending = false;
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
                    other_view_pending = true;
                }
                if admission.is_ok() {
                    if meta.payload_kind == target_payload {
                        let lane = meta.lane as usize;
                        if lane >= self.progress.cursor.logical_lane_count() {
                            return Err(RecvError::PhaseInvariant);
                        }
                        let (word, bit) = lane_word_index(lane);
                        words[word] |= bit;
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
        if other_view_pending
            || lanes
                .first_set(self.progress.cursor.logical_lane_count())
                .is_some()
        {
            return Ok(lanes);
        }
        match expected_payload {
            Some(expected) => Err(RecvError::PayloadMismatch {
                expected,
                actual: target_payload,
            }),
            None => Err(RecvError::PhaseInvariant),
        }
    }
}
