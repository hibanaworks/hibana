//! Borrowed operation kernel over the actor's `EventCursor`.
//!
//! Pinned actor storage owns semantic progress; logical-view storage owns ports
//! and pending I/O. Each carrier entry borrows both for one synchronous poll.

use core::{ops::ControlFlow, task::Poll};

use super::authority::{Arm, RouteArmToken, RouteResolveStep};
use super::evidence::{ScopeEvidence, ScopeReentryMeta};
use super::frontier::*;
use super::frontier_state::FrontierState;
use super::lane_port;
use super::lane_slots::LaneSlotArray;
use super::layout::LeasedState;
use super::offer::*;
mod route_commit_helpers;
use super::decision_state::{ReentryScopeLiveness, RouteCommitRowSetBuilder, RouteState};
use crate::eff::EffIndex;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::LaneSetView;
use crate::global::typestate::{
    CursorInvariantError, CursorRefresh, EventCursor, RecvMeta, RelocatableResidentLaneStep,
    SendMeta, SendPreviewError, StateIndex, state_index_to_usize,
};
use crate::{
    endpoint::{
        RecvError, RecvResult, SendError, SendResult, affine::LaneGuard, session::SessionCtx,
    },
    observe::core::{TapEvent, emit},
    observe::{events, ids},
    rendezvous::SessionFaultKind,
    rendezvous::{core::EndpointLeaseId, core::ScratchLease, port::Port},
    session::{
        cluster::error::ClusterError,
        types::{Lane, RendezvousId, SessionId},
    },
    transport::{
        Transport,
        trace::TapFrameMeta,
        wire::{CodecError, Payload},
    },
};
pub(in crate::endpoint::kernel::core) use route_commit_helpers::prepare_route_site_materialization_rows_from_resident_route_commit_range;
pub(in crate::endpoint::kernel::core) use route_commit_helpers::preview_selected_arm_for_scope_from_parts;
pub(in crate::endpoint::kernel) use route_commit_helpers::{
    prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
    scope_slot_for_route_from_cursor,
};

pub(crate) trait RecvKernelEndpoint<'r> {
    fn poll_recv_kernel_frame_source(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        state: &mut super::recv::RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<super::recv::MatchedRecvFrame<'r>>>;

    fn finish_recv_kernel_frame(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        frame: super::recv::MatchedRecvFrame<'r>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<crate::endpoint::transfer::RecvOutput<'r>>;
}

pub(crate) trait BranchRecvKernelEndpoint<'r> {
    fn has_branch_recv_kernel_branch(&self) -> bool;

    fn prepare_branch_recv_kernel_transport_wait(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
    ) -> Poll<RecvResult<Option<RecvMeta>>>;

    fn poll_branch_recv_kernel_transport_payload(
        &mut self,
        meta: RecvMeta,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>>;

    fn stage_branch_recv_kernel_transport_payload(
        &mut self,
        frame: lane_port::ReceivedFrame<'r>,
    ) -> RecvResult<()>;

    fn finish_branch_recv_kernel(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        prepared_meta: Option<RecvMeta>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<crate::endpoint::transfer::RecvOutput<'r>>;
}

pub(crate) trait SendKernelEndpoint<'r> {
    fn poll_send_preview_kernel(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
    ) -> Poll<SendResult<SendPreview>>;

    fn poll_send_init_kernel(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        route_authority: SendRouteAuthority,
        payload: Option<lane_port::SendInput>,
    ) -> SendInitOutcome<'r>;

    fn poll_send_pending_kernel(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendCommitPlan<'r>>>;

    fn finish_send_after_transport_kernel(
        &mut self,
        commit_plan: SendCommitPlan<'r>,
    ) -> SendCommitOutcome<'r>;
}

#[inline(never)]
pub(crate) fn kernel_recv<'r>(
    endpoint: &mut dyn RecvKernelEndpoint<'r>,
    logical_label: u8,
    payload_kind: crate::eff::EventPayload,
    validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    state: &mut super::recv::RecvState,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<crate::endpoint::transfer::RecvOutput<'r>>> {
    match endpoint.poll_recv_kernel_frame_source(logical_label, payload_kind, state, cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(frame)) => {
            Poll::Ready(
                endpoint
                    .finish_recv_kernel_frame(logical_label, payload_kind, frame, validate)
                    .map(|payload| unsafe {
                        // SAFETY: recv payloads returned by the kernel are backed by
                        // endpoint-resident transport, ingress, or a canonical zero-length slice.
                        payload.into_resident()
                    }),
            )
        }
        Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
    }
}

#[inline(never)]
pub(crate) fn kernel_branch_recv<'r>(
    endpoint: &mut dyn BranchRecvKernelEndpoint<'r>,
    logical_label: u8,
    payload_kind: crate::eff::EventPayload,
    validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    state: &mut super::branch_recv::BranchRecvState,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<crate::endpoint::transfer::RecvOutput<'r>>> {
    if !endpoint.has_branch_recv_kernel_branch() {
        return Poll::Ready(Err(RecvError::PhaseInvariant));
    }
    let prepared_meta =
        match endpoint.prepare_branch_recv_kernel_transport_wait(logical_label, payload_kind) {
            Poll::Ready(Ok(meta)) => meta,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        };
    if let Some(meta) = prepared_meta {
        let frame = match endpoint.poll_branch_recv_kernel_transport_payload(
            meta,
            state.pending_recv_mut(),
            cx,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(payload)) => payload,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        if let Err(err) = endpoint.stage_branch_recv_kernel_transport_payload(frame) {
            return Poll::Ready(Err(err));
        }
    }
    let result =
        endpoint.finish_branch_recv_kernel(logical_label, payload_kind, prepared_meta, validate);
    match result {
        Ok(payload) => Poll::Ready(Ok(unsafe {
            // SAFETY: committed decode payloads are staged in endpoint-resident
            // transport/ingress storage or the static empty local payload.
            payload.into_resident()
        })),
        Err(err) => Poll::Ready(Err(err)),
    }
}

#[inline(never)]
pub(crate) fn kernel_send<'r>(
    endpoint: &mut dyn SendKernelEndpoint<'r>,
    state: &mut SendState<'r>,
    payload: &mut Option<lane_port::SendInput>,
    cx: &mut core::task::Context<'_>,
) -> Poll<SendResult<SendCommitOutcome<'r>>> {
    loop {
        match state {
            SendState::Preparing {
                logical_label,
                payload_kind,
                input,
            } => {
                if let Some(payload) = payload.take()
                    && input.replace(payload).is_some()
                {
                    crate::invariant();
                }
                let preview = match endpoint.poll_send_preview_kernel(*logical_label, *payload_kind)
                {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => {
                        *state = SendState::Done;
                        return Poll::Ready(Err(error));
                    }
                    Poll::Ready(Ok(preview)) => preview,
                };
                let descriptor = SendRuntimeDesc::new(
                    *logical_label,
                    crate::transport::FrameLabel::new(preview.frame_label()),
                );
                let (meta, cursor_index, route_authority) = preview.into_parts();
                match endpoint.poll_send_init_kernel(
                    descriptor,
                    meta,
                    Some(cursor_index),
                    route_authority,
                    input.take(),
                ) {
                    SendInitOutcome::Ready(result) => {
                        *state = SendState::Done;
                        return Poll::Ready(result);
                    }
                    SendInitOutcome::Pending { pending } => {
                        *state = SendState::Sending { pending };
                    }
                }
            }
            SendState::Sending { pending } => {
                match endpoint.poll_send_pending_kernel(pending, cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(commit_plan)) => {
                        let result = endpoint.finish_send_after_transport_kernel(commit_plan);
                        *state = SendState::Done;
                        return Poll::Ready(Ok(result));
                    }
                    Poll::Ready(Err(err)) => {
                        *state = SendState::Done;
                        return Poll::Ready(Err(err));
                    }
                }
            }
            SendState::Done => crate::invariant(),
        }
    }
}

impl<'r, T> SendKernelEndpoint<'r> for OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn poll_send_preview_kernel(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
    ) -> Poll<SendResult<SendPreview>> {
        self.poll_send_preview(logical_label, payload_kind)
    }

    #[inline]
    fn poll_send_init_kernel(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        route_authority: SendRouteAuthority,
        payload: Option<lane_port::SendInput>,
    ) -> SendInitOutcome<'r> {
        self.poll_send_init(
            descriptor,
            meta,
            preview_cursor_index,
            route_authority,
            payload,
        )
    }

    #[inline]
    fn poll_send_pending_kernel(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendCommitPlan<'r>>> {
        self.poll_send_pending(pending, cx)
    }

    #[inline]
    fn finish_send_after_transport_kernel(
        &mut self,
        commit_plan: SendCommitPlan<'r>,
    ) -> SendCommitOutcome<'r> {
        self.finish_send_after_transport_runtime(commit_plan)
    }
}

#[inline]
fn controller_arm_label(cursor: &EventCursor, scope_id: ScopeId, arm: u8) -> Option<u8> {
    cursor
        .shared_controller_arm_entry_by_arm(scope_id, arm)
        .map(|(_, label)| label)
}

mod commit_delta;
mod commit_delta_apply;
mod commit_delta_route_only;
mod frontier_observation;
mod frontier_select;
mod offer_refresh;
mod public_operation;
mod scope_evidence_logic;

mod decision_resolver;
mod frontier_helpers;
mod owned;
mod public_types;
mod route_preview;
mod runtime_types;
mod send_ops;
mod send_preview;
mod send_preview_authority;
mod send_route_authority;
#[cfg(kani)]
mod send_wait_kani;

pub(crate) use super::decision_state::{
    PreparedRouteCommitRows, SelectedRouteCommitRow, SelectedRouteCommitRowsRef,
};
pub(in crate::endpoint::kernel) use commit_delta::CommitDeltaApplyPermit;
pub(crate) use commit_delta::{CommittedCommitDelta, PreparedCommitDelta};
pub(in crate::endpoint::kernel) use owned::PreparedViewCommit;
pub(crate) use public_operation::*;
pub(crate) use public_types::*;
pub(in crate::endpoint::kernel) use route_preview::IngressEvidenceState;
pub(crate) use runtime_types::*;
pub(crate) use send_route_authority::*;

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    /// Resolver authority belongs to the issued logical view, even when its
    /// cursor is shared with other views at the same physical actor.
    pub(in crate::endpoint::kernel) fn is_route_controller(&self, scope: ScopeId) -> bool {
        self.progress.cursor.contains_route_controller(scope)
            && self.context.program.route_controller_role(scope) == self.public_header.role()
    }

    fn projection_complete(&self) -> bool {
        if self.context.program.actor_binding_at(0).is_none() {
            return self
                .progress
                .cursor
                .is_complete(&mut |scope| self.selected_arm_for_scope(scope));
        }
        self.progress
            .cursor
            .is_view_complete(self.public_header.role(), &mut |scope| {
                self.selected_arm_for_scope(scope)
            })
    }

    /// Get port for a specific lane.
    pub(crate) fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T> {
        if self.io.ports[lane_idx].is_none() {
            crate::invariant();
        }
        crate::invariant_some(self.io.ports[lane_idx].as_ref())
    }

    #[inline]
    pub(crate) fn frontier_scratch_workspace<'lease>(
        &self,
        lease: &'lease mut ScratchLease<'r>,
    ) -> FrontierScratchWorkspace<'lease> {
        let scratch = self.context.session.rendezvous().frontier_scratch(lease);
        let layout = self.progress.cursor.frontier_scratch_layout();
        FrontierScratchWorkspace::from_storage(scratch, layout)
    }

    #[inline]
    pub(crate) fn offer_lane_set_for_scope(&self, scope_id: ScopeId) -> LaneSetView<'static> {
        match self.progress.cursor.route_scope_offer_lane_set(scope_id) {
            Some(lanes) => lanes,
            None => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) fn route_scope_arm_lane_set_for_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.progress.cursor.route_scope_arm_lane_set(scope_id, arm)
    }

    #[inline]
    pub(crate) fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let lane_idx =
            crate::invariant_some(offer_lanes.first_set(self.progress.cursor.logical_lane_count()));
        crate::invariant_ok(u8::try_from(lane_idx))
    }

    #[inline]
    pub(crate) fn controller_arm_at_cursor(&self, scope_id: ScopeId) -> Option<u8> {
        let idx = self.progress.cursor.index();
        if let Some((entry, _)) = self
            .progress
            .cursor
            .controller_arm_entry_by_arm(scope_id, 0)
            && idx == state_index_to_usize(entry)
        {
            return Some(0);
        }
        if let Some((entry, _)) = self
            .progress
            .cursor
            .controller_arm_entry_by_arm(scope_id, 1)
            && idx == state_index_to_usize(entry)
        {
            return Some(1);
        }
        None
    }
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint) fn retire_actor(&mut self) {
        self.retire_view();
        if self.context.public_generation.get() != 0 {
            if self.context.public_slot_ownership.get() == PublicSlotOwnership::Owned {
                crate::invariant();
            }
            self.public_header.retire_generation();
            self.context.public_generation.set(0);
            self.context
                .public_slot_ownership
                .set(PublicSlotOwnership::Borrowed);
        }
    }

    pub(in crate::endpoint) fn retire_view(&mut self) {
        if self.context.public_generation.get() != 0
            && (self.io.public_active_op != PublicActiveOp::Idle || !self.projection_complete())
        {
            self.poison_session(SessionFaultKind::EndpointDropped);
        }
        self.finish_view_io();
    }

    pub(in crate::endpoint) fn finish_view_io(&mut self) {
        self.terminal_clear_public_send_state();
        self.terminal_clear_public_recv_state();
        self.terminal_clear_public_offer_state();
        self.terminal_clear_public_branch_recv_state();
        if let Some(branch) = self.io.public_route_branch.take() {
            branch.discard_terminal();
        }
        self.clear_public_op_terminal();
        self.retire_transport_handles();
        for guard in self.io.guards.iter_mut() {
            if let Some(g) = guard.take() {
                drop(g);
            }
        }
    }
}
