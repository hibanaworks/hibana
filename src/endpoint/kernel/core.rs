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
use super::layout::{EndpointArenaLayout, LeasedState};
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
    rendezvous::{
        core::EndpointLeaseId,
        port::{Port, ScratchLease},
    },
    session::{
        brand::Owner,
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
        payload_schema: u32,
        state: &mut super::recv::RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<super::recv::MatchedRecvFrame<'r>>>;

    fn finish_recv_kernel_frame(
        &mut self,
        logical_label: u8,
        payload_schema: u32,
        frame: super::recv::MatchedRecvFrame<'r>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>>;
}

pub(crate) trait BranchRecvKernelEndpoint<'r> {
    fn has_branch_recv_kernel_branch(&self) -> bool;

    fn prepare_branch_recv_kernel_transport_wait(
        &mut self,
        logical_label: u8,
        payload_schema: u32,
    ) -> RecvResult<Option<RecvMeta>>;

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
        payload_schema: u32,
        prepared_meta: Option<RecvMeta>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>>;
}

pub(crate) trait SendKernelEndpoint<'r> {
    fn poll_send_init_kernel(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        route_authority: SendRouteAuthority,
        payload: Option<lane_port::RawSendPayload>,
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
    payload_schema: u32,
    validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    state: &mut super::recv::RecvState,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<Payload<'r>>> {
    match endpoint.poll_recv_kernel_frame_source(logical_label, payload_schema, state, cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(frame)) => {
            Poll::Ready(
                endpoint
                    .finish_recv_kernel_frame(logical_label, payload_schema, frame, validate)
                    .map(|payload| unsafe {
                        // SAFETY: recv payloads returned by the kernel are backed by
                        // endpoint-resident transport, ingress, or a canonical zero-length slice.
                        lane_port::endpoint_resident_payload(payload)
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
    payload_schema: u32,
    validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    state: &mut super::branch_recv::BranchRecvState,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<Payload<'r>>> {
    if !endpoint.has_branch_recv_kernel_branch() {
        return Poll::Ready(Err(RecvError::PhaseInvariant));
    }
    if state.prepared_meta().is_none() {
        let prepared = match endpoint
            .prepare_branch_recv_kernel_transport_wait(logical_label, payload_schema)
        {
            Ok(meta) => meta,
            Err(err) => return Poll::Ready(Err(err)),
        };
        state.set_prepared_meta(prepared);
    }
    if let Some(meta) = state.prepared_meta() {
        let frame = match endpoint.poll_branch_recv_kernel_transport_payload(
            meta,
            state.pending_recv_mut(),
            cx,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(payload)) => payload,
            Poll::Ready(Err(err)) => {
                state.set_prepared_meta(None);
                return Poll::Ready(Err(err));
            }
        };
        if let Err(err) = endpoint.stage_branch_recv_kernel_transport_payload(frame) {
            state.set_prepared_meta(None);
            return Poll::Ready(Err(err));
        }
    }
    let prepared_meta = state.prepared_meta();
    let result =
        endpoint.finish_branch_recv_kernel(logical_label, payload_schema, prepared_meta, validate);
    match result {
        Ok(payload) => Poll::Ready(Ok(unsafe {
            // SAFETY: committed decode payloads are staged in endpoint-resident
            // transport/ingress storage or the static empty local payload.
            lane_port::endpoint_resident_payload(payload)
        })),
        Err(err) => Poll::Ready(Err(err)),
    }
}

#[inline(never)]
pub(crate) fn kernel_send<'r>(
    endpoint: &mut dyn SendKernelEndpoint<'r>,
    state: &mut SendState<'r>,
    payload: &mut Option<lane_port::RawSendPayload>,
    cx: &mut core::task::Context<'_>,
) -> Poll<SendResult<SendCommitOutcome<'r>>> {
    loop {
        match state {
            SendState::Init {
                descriptor,
                meta,
                preview_cursor_index,
                route_authority,
            } => match endpoint.poll_send_init_kernel(
                *descriptor,
                *meta,
                *preview_cursor_index,
                *route_authority,
                payload.take(),
            ) {
                SendInitOutcome::Ready(result) => {
                    *state = SendState::Done;
                    return Poll::Ready(result);
                }
                SendInitOutcome::Pending { pending } => {
                    *state = SendState::Sending { pending };
                }
            },
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
    fn poll_send_init_kernel(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        route_authority: SendRouteAuthority,
        payload: Option<lane_port::RawSendPayload>,
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
mod public_types;
mod route_preview;
mod runtime_types;
mod send_ops;
mod send_preview;
mod send_preview_authority;
mod send_route_authority;

pub(crate) use super::decision_state::{
    PreparedRouteCommitRows, SelectedRouteCommitRow, SelectedRouteCommitRowsRef,
};
pub(in crate::endpoint::kernel) use commit_delta::CommitDeltaApplyPermit;
pub(crate) use commit_delta::{CommittedCommitDelta, PreparedCommitDelta};
pub(crate) use public_operation::*;
pub(crate) use public_types::*;
pub(in crate::endpoint::kernel) use route_preview::IngressEvidenceState;
pub(crate) use runtime_types::*;
pub(crate) use send_route_authority::*;

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    fn projection_complete(&self) -> bool {
        self.progress
            .cursor
            .is_complete(&mut |scope| self.selected_arm_for_scope(scope))
    }

    pub(crate) fn take_owned_slot_release(
        &mut self,
    ) -> Option<(
        &'r crate::session::cluster::core::SessionCluster<'r, T>,
        crate::session::types::RendezvousId,
        crate::rendezvous::core::EndpointLeaseId,
        u32,
    )> {
        if self.progress.public_slot_ownership == PublicSlotOwnership::Borrowed {
            return None;
        }
        if self.progress.public_generation == 0
            || self.public_header.generation() != self.progress.public_generation
            || self.public_header.role() != self.progress.cursor.role()
        {
            crate::invariant();
        }
        self.progress.public_slot_ownership = PublicSlotOwnership::Borrowed;
        Some((
            self.progress.session.cluster(),
            self.progress.public_rv,
            self.progress.public_slot,
            self.progress.public_generation,
        ))
    }

    /// Rendezvous id for the primary port.
    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.port().rv_id()
    }

    /// Get the descriptor-selected primary lane's port.
    fn port(&self) -> &Port<'r, T> {
        if self.io.ports[self.io.primary_lane].is_none() {
            crate::invariant();
        }
        crate::invariant_some(self.io.ports[self.io.primary_lane].as_ref())
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
        let port = self.port_for_lane(self.io.primary_lane);
        let scratch = lane_port::frontier_scratch(port, lease);
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
    pub(super) fn retire_actor(&mut self) {
        if self.progress.public_generation != 0
            && (self.io.public_active_op != PublicActiveOp::Idle || !self.projection_complete())
        {
            self.poison_session(SessionFaultKind::EndpointDropped);
        }
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
        if self.progress.public_generation != 0 {
            if self.progress.public_slot_ownership == PublicSlotOwnership::Owned {
                crate::invariant();
            }
            self.public_header.retire_generation();
            self.progress.public_generation = 0;
            self.progress.public_slot_ownership = PublicSlotOwnership::Borrowed;
        }
    }
}
