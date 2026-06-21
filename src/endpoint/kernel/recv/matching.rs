use super::evidence::{
    MatchAccumulator, MatchOutcome, ObservedInboundKey, RecvCandidate, RecvDescriptor,
};
use crate::{
    endpoint::kernel::{core::CursorEndpoint, lane_port},
    endpoint::{RecvError, RecvResult},
    global::typestate::{EventCommitMeta, StateIndex},
    transport::Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::recv) fn live_recv_label_on_lane(
        &self,
        lane_idx: usize,
        target_label: u8,
    ) -> bool {
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(meta) = self.cursor.try_recv_meta_at(idx)
                && meta.label == target_label
                && meta.lane as usize == lane_idx
                && !meta.origin.is_session()
                && {
                    let preview_conflict = self.cursor.event_conflict_for_index(idx);
                    let mut selected_arm =
                        |scope| self.selected_arm_for_recv_event(preview_conflict, scope);
                    self.cursor
                        .event_enabled(idx, EventCommitMeta::from(meta), &mut selected_arm)
                        .is_ok()
                }
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn recv_candidate_for_observed_evidence(
        &self,
        idx: usize,
        target_label: u8,
        observed: ObservedInboundKey,
    ) -> RecvResult<Option<RecvCandidate>> {
        let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
            return Ok(None);
        };
        if meta.label != target_label {
            return Ok(None);
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_idx = meta.lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        if !observed.matches_recv_meta(self.sid.raw(), lane_wire, ROLE, meta) {
            return Ok(None);
        }
        let preview_conflict = self.cursor.event_conflict_for_index(idx);
        let mut selected_arm = |scope| self.selected_arm_for_recv_event(preview_conflict, scope);
        let enabled =
            self.cursor
                .event_enabled(idx, EventCommitMeta::from(meta), &mut selected_arm);
        if enabled.is_err() {
            return Ok(None);
        }

        Ok(Some(RecvCandidate {
            desc: RecvDescriptor {
                meta,
                cursor_index: StateIndex::from_usize(idx),
                lane_idx,
                lane_wire,
            },
        }))
    }

    fn recv_candidate_for_deterministic_lane(
        &self,
        idx: usize,
        target_label: u8,
        lane_wire: u8,
    ) -> RecvResult<Option<RecvCandidate>> {
        let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
            return Ok(None);
        };
        if meta.label != target_label {
            return Ok(None);
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_idx = meta.lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let candidate_lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        if candidate_lane_wire != lane_wire {
            return Ok(None);
        }
        let preview_conflict = self.cursor.event_conflict_for_index(idx);
        let mut selected_arm = |scope| self.selected_arm_for_recv_event(preview_conflict, scope);
        let enabled =
            self.cursor
                .event_enabled(idx, EventCommitMeta::from(meta), &mut selected_arm);
        if enabled.is_err() {
            return Ok(None);
        }

        Ok(Some(RecvCandidate {
            desc: RecvDescriptor {
                meta,
                cursor_index: StateIndex::from_usize(idx),
                lane_idx,
                lane_wire: candidate_lane_wire,
            },
        }))
    }

    pub(in crate::endpoint::kernel::recv) fn unique_recv_candidate(
        &self,
        target_label: u8,
        observed: ObservedInboundKey,
    ) -> RecvResult<Result<RecvCandidate, MatchOutcome>> {
        let mut accumulator = MatchAccumulator::None;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(candidate) =
                self.recv_candidate_for_observed_evidence(idx, target_label, observed)?
            {
                accumulator = accumulator.add(candidate);
                if matches!(accumulator, MatchAccumulator::Ambiguous) {
                    break;
                }
            }
            idx += 1;
        }
        Ok(accumulator.finish())
    }

    pub(in crate::endpoint::kernel::recv) fn unique_deterministic_recv_candidate(
        &self,
        target_label: u8,
        lane_wire: u8,
    ) -> RecvResult<Result<RecvCandidate, MatchOutcome>> {
        let mut accumulator = MatchAccumulator::None;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(candidate) =
                self.recv_candidate_for_deterministic_lane(idx, target_label, lane_wire)?
            {
                accumulator = accumulator.add(candidate);
                if matches!(accumulator, MatchAccumulator::Ambiguous) {
                    break;
                }
            }
            idx += 1;
        }
        Ok(accumulator.finish())
    }

    pub(in crate::endpoint::kernel::recv) fn framed_recv_mismatch_for_unmatched_candidate(
        &self,
        target_label: u8,
        frame: &lane_port::PreambleFrame<'_>,
    ) -> lane_port::FrameMismatch {
        let observed = frame.observed_transport_frame(self.sid.raw(), frame.lane_wire(), ROLE);
        match self.unique_deterministic_recv_candidate(target_label, frame.lane_wire()) {
            Ok(Ok(candidate)) => lane_port::FrameMismatch::source_label_mismatch(
                observed,
                candidate.desc.meta.peer,
                candidate.desc.meta.frame_label,
            ),
            Ok(Err(_)) | Err(_) => lane_port::FrameMismatch::label_mismatch(observed),
        }
    }
}
