//! Receive-path helpers for deterministic recv.

use core::task::Poll;

use super::{
    core::{
        CommitDelta, CursorEndpoint, PreparedCommitDelta,
        prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
    },
    lane_port,
    recv_commit_plan::{EndpointRxEventPlan, RecvCommitPlan},
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::{CursorInvariantError, EventCommitMeta, StateIndex, state_index_to_usize},
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

mod evidence;

pub(crate) use evidence::MatchedRecvFrame;
use evidence::{MatchAccumulator, MatchOutcome, ObservedInboundKey, RecvCandidate, RecvDescriptor};

pub(crate) struct RecvState {
    pending_recv: lane_port::PendingRecv,
}

impl RecvState {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            pending_recv: lane_port::PendingRecv::new(),
        }
    }
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn live_recv_label_on_lane(&self, lane_idx: usize, target_label: u8) -> bool {
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(meta) = self.cursor.try_recv_meta_at(idx)
                && meta.label == target_label
                && meta.lane as usize == lane_idx
                && !meta.origin.is_session()
                && self
                    .cursor
                    .event_enabled(idx, EventCommitMeta::from(meta), |scope| {
                        self.selected_arm_for_scope(scope)
                    })
                    .is_ok()
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn poll_recv_preamble_for_label(
        &mut self,
        target_label: u8,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::PreambleFrame<'r>>> {
        let lane_limit = self.cursor.logical_lane_count();
        let mut saw_candidate_lane = false;
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if self.live_recv_label_on_lane(lane_idx, target_label) {
                saw_candidate_lane = true;
                let lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
                match self.poll_received_transport_frame_for_lane(
                    pending_recv,
                    lane_idx,
                    lane_wire,
                    cx,
                ) {
                    Poll::Pending => {}
                    Poll::Ready(Ok(frame)) => return Poll::Ready(Ok(frame)),
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                }
            }
            lane_idx += 1;
        }
        if saw_candidate_lane {
            Poll::Pending
        } else {
            Poll::Ready(Err(RecvError::PhaseInvariant))
        }
    }

    fn recv_candidate_for_observed_key(
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
        let enabled = self
            .cursor
            .event_enabled(idx, EventCommitMeta::from(meta), |scope| {
                self.selected_arm_for_scope(scope)
            });
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

    fn unique_recv_candidate(
        &self,
        target_label: u8,
        observed: ObservedInboundKey,
    ) -> RecvResult<Result<RecvCandidate, MatchOutcome>> {
        let mut accumulator = MatchAccumulator::None;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(candidate) =
                self.recv_candidate_for_observed_key(idx, target_label, observed)?
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

    fn accept_observed_recv_frame(
        &mut self,
        target_label: u8,
        frame: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<MatchedRecvFrame<'r>> {
        let observed = ObservedInboundKey::from_frame::<ROLE>(self.sid.raw(), &frame);
        match self.unique_recv_candidate(target_label, observed)? {
            Ok(candidate) => {
                let desc = candidate.desc;
                let frame = self.accept_materialized_transport_frame(
                    desc.lane_idx,
                    desc.lane_wire,
                    desc.meta.peer,
                    desc.meta.frame_label,
                    frame,
                )?;
                Ok(MatchedRecvFrame { desc, frame })
            }
            Err(MatchOutcome::None) => {
                let mismatch = lane_port::FrameMismatch::label_mismatch(
                    frame.observed_transport_frame(self.sid.raw(), frame.lane_wire(), ROLE),
                );
                self.emit_materialization_mismatch_observation(
                    frame.lane_idx(),
                    frame.lane_wire(),
                    mismatch,
                );
                frame.discard_uncommitted();
                Err(RecvError::PhaseInvariant)
            }
            Err(MatchOutcome::Ambiguous) => {
                frame.discard_uncommitted();
                Err(RecvError::PhaseInvariant)
            }
        }
    }

    fn poll_recv_frame_source(
        &mut self,
        target_label: u8,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MatchedRecvFrame<'r>>> {
        let frame = match self.poll_recv_preamble_for_label(target_label, pending_recv, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        Poll::Ready(self.accept_observed_recv_frame(target_label, frame))
    }

    fn build_recv_commit_plan(
        &mut self,
        logical_label: u8,
        matched: MatchedRecvFrame<'r>,
    ) -> RecvResult<RecvCommitPlan<'r>> {
        let MatchedRecvFrame { desc, frame } = matched;
        let delta = match self.prepare_recv_commit_delta(logical_label, desc, &frame) {
            Ok(delta) => delta,
            Err(err) => {
                frame.discard_uncommitted();
                return Err(err);
            }
        };
        Ok(RecvCommitPlan::direct(
            EndpointRxEventPlan::direct(desc.lane_wire, desc.meta.label),
            delta,
            frame,
        ))
    }

    fn prepare_recv_commit_delta(
        &mut self,
        logical_label: u8,
        desc: RecvDescriptor,
        frame: &lane_port::ReceivedFrame<'_>,
    ) -> RecvResult<PreparedCommitDelta> {
        let meta = desc.meta;
        if meta.label != logical_label {
            return Err(RecvError::LabelMismatch {
                expected: logical_label,
                actual: meta.label,
            });
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        if frame.frame_label_raw() != meta.frame_label || frame.lane_wire() != desc.lane_wire {
            return Err(RecvError::PhaseInvariant);
        }
        let enabled = match self.cursor.event_enabled(
            state_index_to_usize(desc.cursor_index),
            EventCommitMeta::from(meta),
            |scope| self.selected_arm_for_scope(scope),
        ) {
            Ok(enabled) => enabled,
            Err(CursorInvariantError::INVARIANT) => {
                return Err(RecvError::PhaseInvariant);
            }
        };
        let route_rows = if let Some(arm) = meta.route_arm {
            let route_rows = {
                let Self {
                    cursor,
                    decision_state,
                    route_commit_rows,
                    ..
                } = &mut *self;
                let mut rows = route_commit_rows.begin();
                prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
                    decision_state,
                    cursor,
                    meta.lane,
                    state_index_to_usize(desc.cursor_index),
                    arm,
                    &mut rows,
                )?;
                rows.as_commit_rows(meta.lane)
            };
            if route_rows.is_empty() {
                return Err(RecvError::PhaseInvariant);
            }
            route_rows
        } else {
            super::SelectedRouteCommitRowsRef::EMPTY
        };
        let delta = CommitDelta::from_recv_meta(
            meta,
            route_rows,
            enabled.cursor_after(),
            enabled.progress_step(),
        );
        let delta = match self.prepare_commit_delta(delta) {
            Ok(delta) => delta,
            Err(CursorInvariantError::INVARIANT) => {
                return Err(RecvError::PhaseInvariant);
            }
        };

        Ok(delta)
    }

    fn finish_recv_frame(
        &mut self,
        logical_label: u8,
        frame: MatchedRecvFrame<'r>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        let plan = self.build_recv_commit_plan(logical_label, frame)?;
        self.publish_recv_commit_plan(plan, validate)
    }
}

impl<'r, const ROLE: u8, T> super::core::RecvKernelEndpoint<'r> for CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn poll_recv_kernel_frame_source(
        &mut self,
        logical_label: u8,
        state: &mut RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MatchedRecvFrame<'r>>> {
        let pending_recv = &mut state.pending_recv;
        self.poll_recv_frame_source(logical_label, pending_recv, cx)
    }

    #[inline]
    fn finish_recv_kernel_frame(
        &mut self,
        logical_label: u8,
        frame: MatchedRecvFrame<'r>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_recv_frame(logical_label, frame, validate)
    }
}
