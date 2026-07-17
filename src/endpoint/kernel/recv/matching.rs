use super::evidence::{RecvCandidate, RecvDescriptor};
use crate::{
    endpoint::kernel::{core::CursorEndpoint, lane_port},
    endpoint::{RecvError, RecvResult},
    global::typestate::{DeterministicInboundKey, EventCommitMeta, InboundFrameKey, StateIndex},
    runtime_core::{UniqueMatch, UniqueMatchFailure},
    transport::Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn recv_candidate_for_observed_evidence(
        &self,
        idx: usize,
        target_label: u8,
        target_schema: u32,
        observed: InboundFrameKey,
    ) -> RecvResult<Option<RecvCandidate>> {
        let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
            return Ok(None);
        };
        if meta.label != target_label || meta.payload_schema != target_schema {
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
        if lane_wire != meta.lane {
            return Err(RecvError::PhaseInvariant);
        }
        if !observed.matches_recv(meta) {
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
            },
        }))
    }

    fn recv_candidate_for_deterministic_key(
        &self,
        idx: usize,
        key: DeterministicInboundKey,
    ) -> RecvResult<Option<RecvCandidate>> {
        let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
            return Ok(None);
        };
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_idx = meta.lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let candidate_lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        if candidate_lane_wire != meta.lane {
            return Err(RecvError::PhaseInvariant);
        }
        if !key.matches_recv(meta) {
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
            },
        }))
    }

    pub(in crate::endpoint::kernel::recv) fn unique_recv_candidate(
        &self,
        target_label: u8,
        target_schema: u32,
        observed: InboundFrameKey,
    ) -> RecvResult<Result<RecvCandidate, UniqueMatchFailure>> {
        let mut accumulator = UniqueMatch::NONE;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(candidate) = self.recv_candidate_for_observed_evidence(
                idx,
                target_label,
                target_schema,
                observed,
            )? {
                accumulator = accumulator.add(candidate);
                if accumulator.is_ambiguous() {
                    break;
                }
            }
            idx += 1;
        }
        Ok(accumulator.finish())
    }

    pub(in crate::endpoint::kernel::recv) fn unique_deterministic_recv_candidate(
        &self,
        key: DeterministicInboundKey,
    ) -> RecvResult<Result<RecvCandidate, UniqueMatchFailure>> {
        let mut accumulator = UniqueMatch::NONE;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(candidate) = self.recv_candidate_for_deterministic_key(idx, key)? {
                accumulator = accumulator.add(candidate);
                if accumulator.is_ambiguous() {
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
        target_schema: u32,
        frame: &lane_port::PreambleFrame<'_>,
    ) -> lane_port::FrameMismatch {
        let observed = frame.observed_transport_frame(self.sid.raw(), frame.lane_wire(), ROLE);
        let key = DeterministicInboundKey::new(frame.lane_wire(), target_label, target_schema);
        match self.unique_deterministic_recv_candidate(key) {
            Ok(Ok(candidate)) => lane_port::FrameMismatch::source_label_mismatch(
                observed,
                candidate.desc.meta.peer,
                candidate.desc.meta.frame_label,
            ),
            Ok(Err(_)) | Err(_) => lane_port::FrameMismatch::label_mismatch(observed),
        }
    }
}
