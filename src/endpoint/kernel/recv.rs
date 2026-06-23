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
    global::typestate::{
        CursorInvariantError, EventCommitMeta, PackedEventConflict, state_index_to_usize,
    },
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

mod evidence;
mod matching;

pub(crate) use evidence::MatchedRecvFrame;
use evidence::{MatchOutcome, ObservedInboundKey, RecvDescriptor};

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
    fn selected_arm_for_recv_event(
        &self,
        preview_conflict: PackedEventConflict,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<u8> {
        let mut selected_arm = |candidate| self.selected_arm_for_scope(candidate);
        self.cursor.selected_arm_for_reentry_preview_conflict(
            scope,
            preview_conflict,
            &mut selected_arm,
        )
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

    fn accept_framed_recv_frame(
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
                let mismatch =
                    self.framed_recv_mismatch_for_unmatched_candidate(target_label, &frame);
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

    fn accept_deterministic_recv_frame(
        &mut self,
        target_label: u8,
        frame: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<MatchedRecvFrame<'r>> {
        let lane_wire = frame.lane_wire();
        match self.unique_deterministic_recv_candidate(target_label, lane_wire)? {
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
                self.emit_materialization_mismatch_observation(
                    frame.lane_idx(),
                    lane_wire,
                    lane_port::FrameMismatch::headerless_preamble(self.sid.raw(), lane_wire, ROLE),
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

    fn accept_recv_frame(
        &mut self,
        target_label: u8,
        frame: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<MatchedRecvFrame<'r>> {
        if frame.is_deterministic() {
            self.accept_deterministic_recv_frame(target_label, frame)
        } else {
            self.accept_framed_recv_frame(target_label, frame)
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
        Poll::Ready(self.accept_recv_frame(target_label, frame))
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
        let cursor_index = state_index_to_usize(desc.cursor_index);
        let preview_conflict = self.cursor.event_conflict_for_index(cursor_index);
        let mut selected_arm = |scope| self.selected_arm_for_recv_event(preview_conflict, scope);
        let enabled = match self.cursor.event_enabled(
            cursor_index,
            EventCommitMeta::from(meta),
            &mut selected_arm,
        ) {
            Ok(enabled) => enabled,
            Err(CursorInvariantError::INVARIANT) => {
                return Err(RecvError::PhaseInvariant);
            }
        };
        let route_rows = if meta.route_arm.is_some() {
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
                    cursor_index,
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
        )
        .with_lane_relocation(self.cursor.recv_reentry_cursor_step(
            meta,
            enabled.cursor_after(),
            |scope| {
                let mut row_idx = 0usize;
                while row_idx < route_rows.len() {
                    if let Some(row) = route_rows.get(&self.cursor, row_idx)
                        && row.scope() == scope
                    {
                        return Some(row.selected_arm());
                    }
                    row_idx += 1;
                }
                self.selected_arm_for_recv_event(preview_conflict, scope)
            },
        ));
        let delta = match self.prepare_enabled_event_commit_delta(delta, enabled) {
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
