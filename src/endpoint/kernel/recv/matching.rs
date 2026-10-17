use super::evidence::{RecvCandidate, RecvDescriptor};
use crate::{
    endpoint::kernel::{core::OperationKernel, lane_port},
    endpoint::{RecvError, RecvResult},
    global::typestate::{DeterministicInboundKey, InboundFrameKey, StateIndex},
    runtime_core::{UniqueMatch, UniqueMatchFailure},
    transport::Transport,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    fn recv_candidate_for_observed_evidence(
        &self,
        idx: usize,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        observed: InboundFrameKey,
    ) -> RecvResult<Option<RecvCandidate>> {
        let Some(meta) = self.progress.cursor.try_recv_meta_at(idx) else {
            return Ok(None);
        };
        if self.progress.cursor.logical_role_at(idx) != Some(self.public_header.role())
            || meta.label != target_label
            || meta.payload_kind != target_payload
        {
            return Ok(None);
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_idx = meta.lane as usize;
        if lane_idx >= self.progress.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        if lane_wire != meta.lane {
            return Err(RecvError::PhaseInvariant);
        }
        if !observed.matches_recv(meta) {
            return Ok(None);
        }
        let enabled = self.recv_event_enabled(idx, meta.into());
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
        let Some(meta) = self.progress.cursor.try_recv_meta_at(idx) else {
            return Ok(None);
        };
        if self.progress.cursor.logical_role_at(idx) != Some(self.public_header.role()) {
            return Ok(None);
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        let lane_idx = meta.lane as usize;
        if lane_idx >= self.progress.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let candidate_lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        if candidate_lane_wire != meta.lane {
            return Err(RecvError::PhaseInvariant);
        }
        if !key.matches_recv(meta) {
            return Ok(None);
        }
        let enabled = self.recv_event_enabled(idx, meta.into());
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
        target_payload: crate::eff::EventPayload,
        observed: InboundFrameKey,
    ) -> RecvResult<Result<RecvCandidate, UniqueMatchFailure>> {
        let mut accumulator = UniqueMatch::NONE;
        let mut idx = 0usize;
        while idx < self.progress.cursor.local_steps_len() {
            if let Some(candidate) = self.recv_candidate_for_observed_evidence(
                idx,
                target_label,
                target_payload,
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
        while idx < self.progress.cursor.local_steps_len() {
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
        target_payload: crate::eff::EventPayload,
        frame: &lane_port::PreambleFrame<'_>,
    ) -> lane_port::FrameMismatch {
        let observed = frame.observed_transport_frame(
            self.context.sid.raw(),
            frame.lane_wire(),
            self.public_header.role(),
        );
        let key = DeterministicInboundKey::new(frame.lane_wire(), target_label, target_payload);
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
