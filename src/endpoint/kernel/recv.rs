//! Receive-path helpers for deterministic recv.

use core::task::Poll;

use super::{
    core::{
        CommitDelta, OperationKernel, PreparedCommitDelta,
        prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
    },
    lane_port,
    recv_commit_plan::RecvCommitPlan,
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::role_program::{LANE_SET_VIEW_WORDS, LaneWord, lane_word_index},
    global::typestate::{
        CursorInvariantError, DeterministicInboundKey, EnabledEventCommit, EventArmView,
        InboundFrameKey, PackedEventConflict, state_index_to_usize,
    },
    runtime_core::UniqueMatchFailure,
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

mod contract;
mod evidence;
mod matching;

pub(crate) use evidence::MatchedRecvFrame;
pub(in crate::endpoint::kernel) use evidence::RecvDescriptor;

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

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn selected_arm_for_recv_event(
        &self,
        preview_conflict: PackedEventConflict,
        scope: crate::global::const_dsl::ScopeId,
        view: EventArmView,
    ) -> Option<u8> {
        if matches!(view, EventArmView::Committed) {
            return self.selected_arm_for_scope(scope);
        }
        let mut selected_arm = |candidate| self.selected_arm_for_scope(candidate);
        self.progress
            .cursor
            .selected_arm_for_reentry_preview_conflict(scope, preview_conflict, &mut selected_arm)
    }

    fn accept_deterministic_recv_frame(
        &mut self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        frame: lane_port::PreambleFrame<'r>,
    ) -> RecvResult<MatchedRecvFrame<'r>> {
        let lane_wire = frame.lane_wire();
        let key = DeterministicInboundKey::new(lane_wire, target_label, target_payload);
        match self.unique_deterministic_recv_candidate(key)? {
            Ok(candidate) => {
                let desc = candidate.desc;
                let frame = self.accept_materialized_transport_frame(
                    desc.meta.lane as usize,
                    desc.meta.lane,
                    desc.meta.peer,
                    desc.meta.frame_label,
                    frame,
                )?;
                Ok(MatchedRecvFrame {
                    desc,
                    enabled: crate::invariant_ok(candidate.admission),
                    frame,
                })
            }
            Err(UniqueMatchFailure::None) => {
                self.emit_materialization_mismatch_observation(
                    frame.lane_idx(),
                    lane_wire,
                    lane_port::FrameMismatch::headerless_preamble(
                        self.context.sid.raw(),
                        lane_wire,
                        self.public_header.role(),
                    ),
                );
                frame.discard_uncommitted();
                Err(RecvError::PhaseInvariant)
            }
            Err(UniqueMatchFailure::Ambiguous) => {
                frame.discard_uncommitted();
                Err(RecvError::PhaseInvariant)
            }
        }
    }

    fn poll_recv_frame_source(
        &mut self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MatchedRecvFrame<'r>>> {
        let lane_limit = self.progress.cursor.logical_lane_count();
        let mut words = [0; LANE_SET_VIEW_WORDS];
        let (lanes, wait_candidate) =
            match self.live_recv_lanes(target_label, target_payload, &mut words) {
                Ok(live) => live,
                Err(RecvError::Unavailable | RecvError::OutOfOrder)
                    if self.operation.public_active_op == super::core::PublicActiveOp::Recv =>
                {
                    // A transport poll that returned Pending accepted no frame.
                    // Another view can now exclude this contract without making
                    // this call a protocol violation. Every frame observed by a
                    // plain receive is committed or requeued within that poll.
                    return Poll::Ready(Err(RecvError::Unavailable));
                }
                // A materialized branch preview retains its terminal contract.
                // Unavailability must not discard that already accepted input.
                Err(RecvError::Unavailable) => {
                    return Poll::Ready(Err(RecvError::PhaseInvariant));
                }
                Err(error) => return Poll::Ready(Err(error)),
            };
        let mut next = lanes.first_set(lane_limit);
        let mut pending_lane = false;
        let mut parked_lane = false;
        while let Some(lane_idx) = next {
            next = lanes.next_set_from(lane_idx + 1, lane_limit);
            let lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
            // A retained preview already owns this port's FIFO head and Rx
            // borrow. Its matching contract resumes through branch_recv.
            // Other contracts can still receive on independent lanes, but
            // must neither poll behind this frame nor consume it as another arm.
            if let Some(branch) = self
                .operation
                .public_route_branch
                .as_ref()
                .filter(|branch| {
                    branch
                        .offered_frame
                        .as_ref()
                        .is_some_and(|frame| frame.lane() == lane_wire)
                })
            {
                parked_lane = true;
                if self.operation.public_active_op == super::core::PublicActiveOp::Recv
                    && !self.recv_contract_allows_observation(
                        target_label,
                        target_payload,
                        state_index_to_usize(branch.branch_meta.cursor_index),
                    )
                {
                    return Poll::Ready(Err(RecvError::Unavailable));
                }
                continue;
            }
            let frame = match self.poll_received_transport_frame_for_lane(
                pending_recv,
                lane_idx,
                lane_wire,
                cx,
            ) {
                Poll::Pending => {
                    pending_lane = true;
                    continue;
                }
                Poll::Ready(Ok(frame)) => frame,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            };
            if frame.is_deterministic() {
                return Poll::Ready(self.accept_deterministic_recv_frame(
                    target_label,
                    target_payload,
                    frame,
                ));
            }
            let observed = InboundFrameKey::new(
                frame.observed_source_role(),
                frame.lane_wire(),
                frame.observed_frame_label_raw(),
            );
            match self.framed_recv_candidate(observed) {
                Ok(Ok(candidate)) => {
                    let desc = candidate.desc;
                    let same_contract =
                        desc.meta.label == target_label && desc.meta.payload_kind == target_payload;
                    if same_contract && let Ok(enabled) = candidate.admission {
                        return Poll::Ready(
                            self.accept_materialized_transport_frame(
                                lane_idx,
                                lane_wire,
                                desc.meta.peer,
                                desc.meta.frame_label,
                                frame,
                            )
                            .map(|frame| MatchedRecvFrame {
                                desc,
                                enabled,
                                frame,
                            }),
                        );
                    }
                    // A recognized source operation can belong to another
                    // contract or still await its receiver's local prefix.
                    // Retain its FIFO head even on the final lane.
                    // Looking at the header commits neither receive nor choice.
                    self.requeue_transport_frame(frame)?;
                    parked_lane = true;
                    if same_contract {
                        pending_lane |= matches!(
                            candidate.admission,
                            Err(crate::global::typestate::EventAdmissionError::OtherViewPending)
                        );
                        continue;
                    }
                    if self.operation.public_active_op == super::core::PublicActiveOp::Recv
                        && !self.recv_contract_allows_observation(
                            target_label,
                            target_payload,
                            state_index_to_usize(desc.cursor_index),
                        )
                    {
                        return Poll::Ready(Err(RecvError::Unavailable));
                    }
                    continue;
                }
                Ok(Err(UniqueMatchFailure::None)) => {}
                Ok(Err(UniqueMatchFailure::Ambiguous)) => {
                    frame.discard_uncommitted();
                    return Poll::Ready(Err(RecvError::PhaseInvariant));
                }
                Err(error) => {
                    frame.discard_uncommitted();
                    return Poll::Ready(Err(error));
                }
            }
            let mismatch = self.framed_recv_mismatch_for_unmatched_candidate(
                target_label,
                target_payload,
                &frame,
            );
            self.emit_materialization_mismatch_observation(lane_idx, lane_wire, mismatch);
            frame.discard_uncommitted();
            return Poll::Ready(Err(RecvError::PhaseInvariant));
        }
        if self.operation.public_active_op == super::core::PublicActiveOp::Recv
            && (self.recv_contract_excludes_wait(
                target_label,
                target_payload,
                None,
                Some(wait_candidate),
            ) || self.poll_excluding_receive_observation(
                target_label,
                target_payload,
                &words,
                pending_recv,
                cx,
            )?)
        {
            return Poll::Ready(Err(RecvError::Unavailable));
        }
        if parked_lane && !pending_lane {
            Poll::Ready(Err(
                if self.operation.public_active_op == super::core::PublicActiveOp::Recv {
                    RecvError::Unavailable
                } else {
                    RecvError::OutOfOrder
                },
            ))
        } else {
            Poll::Pending
        }
    }

    /// A choice can arrive on a different lane from the requested contract.
    /// Inspect only other currently enabled receive directions and retain each
    /// FIFO head. Independent input cannot exclude the requested receive.
    fn poll_excluding_receive_observation(
        &mut self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        inspected: &[LaneWord; LANE_SET_VIEW_WORDS],
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> RecvResult<bool> {
        let incoming = self.read_projected_incoming(inspected);
        for lane_idx in 0..self.progress.cursor.logical_lane_count() {
            let (word, bit) = lane_word_index(lane_idx);
            if incoming[word] & bit == 0 {
                continue;
            }
            if let Some(branch) = self.operation.public_route_branch.as_ref()
                && branch
                    .offered_frame
                    .as_ref()
                    .is_some_and(|frame| usize::from(frame.lane()) == lane_idx)
            {
                if !self.recv_contract_allows_observation(
                    target_label,
                    target_payload,
                    state_index_to_usize(branch.branch_meta.cursor_index),
                ) {
                    return Ok(true);
                }
                continue;
            }
            let frame = match self.poll_received_transport_frame_for_lane(
                pending_recv,
                lane_idx,
                lane_idx as u8,
                cx,
            ) {
                Poll::Pending => continue,
                Poll::Ready(result) => result?,
            };
            if frame.is_deterministic() {
                // A headerless frame carries no evidence about another
                // contract. Leave its interpretation to that contract's call.
                self.requeue_transport_frame(frame)?;
                continue;
            }
            let observed = InboundFrameKey::new(
                frame.observed_source_role(),
                frame.lane_wire(),
                frame.observed_frame_label_raw(),
            );
            let candidate = self.framed_recv_candidate(observed);
            // This port was inspected only for a choice excluding the
            // requested call. Do not consume even an unmatched FIFO head.
            self.requeue_transport_frame(frame)?;
            if let Ok(candidate) = candidate?
                && !self.recv_contract_allows_observation(
                    target_label,
                    target_payload,
                    state_index_to_usize(candidate.desc.cursor_index),
                )
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn build_recv_commit_plan(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        matched: MatchedRecvFrame<'r>,
    ) -> RecvResult<RecvCommitPlan<'r>> {
        let MatchedRecvFrame {
            desc,
            enabled,
            frame,
        } = matched;
        // A framed header is reported to the tap after matching. A callback
        // can retire a co-resident view or borrow while the kernel is leased.
        let retired = self.reconcile_retirements();
        if let Some(kind) = self.session_fault() {
            frame.discard_uncommitted();
            return Err(RecvError::SessionFault(kind));
        }
        let enabled = if retired {
            self.recv_event_enabled(state_index_to_usize(desc.cursor_index), desc.meta.into())
                .map_err(|_| RecvError::PhaseInvariant)
        } else {
            Ok(enabled)
        };
        let delta = match enabled.and_then(|enabled| {
            self.prepare_recv_commit_delta(logical_label, payload_kind, desc, &frame, enabled)
        }) {
            Ok(delta) => delta,
            Err(err) => {
                frame.discard_uncommitted();
                return Err(err);
            }
        };
        Ok(RecvCommitPlan::direct(delta, frame))
    }

    pub(in crate::endpoint::kernel) fn prepare_recv_commit_delta(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        desc: RecvDescriptor,
        frame: &lane_port::ReceivedFrame<'_>,
        enabled: EnabledEventCommit,
    ) -> RecvResult<PreparedCommitDelta> {
        let meta = desc.meta;
        if meta.label != logical_label {
            return Err(RecvError::LabelMismatch {
                expected: logical_label,
                actual: meta.label,
            });
        }
        if meta.payload_kind != payload_kind {
            return Err(RecvError::PayloadMismatch {
                expected: meta.payload_kind,
                actual: payload_kind,
            });
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        if frame.frame_label_raw() != meta.frame_label || frame.lane_wire() != meta.lane {
            return Err(RecvError::PhaseInvariant);
        }
        let cursor_index = state_index_to_usize(desc.cursor_index);
        let preview_conflict = self.progress.cursor.event_conflict_for_index(cursor_index);
        let route_rows = if meta.route_arm.is_some() {
            let route_rows = {
                let crate::endpoint::kernel::core::ActorProgress {
                    cursor,
                    decision_state,
                    route_commit_rows,
                    ..
                } = &mut *self.progress;
                let mut rows = route_commit_rows.begin();
                prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
                    decision_state,
                    cursor,
                    meta.lane,
                    cursor_index,
                    &mut rows,
                )?;
                rows.finish_for_lane(meta.lane)?
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
        .with_lane_relocation(self.progress.cursor.recv_reentry_cursor_step(
            meta,
            |scope| {
                if let Some(arm) = route_rows.arm_for_scope(&self.progress.cursor, scope) {
                    return Some(arm);
                }
                self.selected_arm_for_recv_event(preview_conflict, scope, EventArmView::Preview)
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
        payload_kind: crate::eff::EventPayload,
        frame: MatchedRecvFrame<'r>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<crate::endpoint::transfer::RecvOutput<'r>> {
        let plan = self.build_recv_commit_plan(logical_label, payload_kind, frame)?;
        self.publish_recv_commit_plan(plan, validate)
    }
}

impl<'r, T> super::core::RecvKernelEndpoint<'r> for OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn poll_recv_kernel_frame_source(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        state: &mut RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MatchedRecvFrame<'r>>> {
        let pending_recv = &mut state.pending_recv;
        self.poll_recv_frame_source(logical_label, payload_kind, pending_recv, cx)
    }

    #[inline]
    fn finish_recv_kernel_frame(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        frame: MatchedRecvFrame<'r>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<crate::endpoint::transfer::RecvOutput<'r>> {
        self.finish_recv_frame(logical_label, payload_kind, frame, validate)
    }
}
