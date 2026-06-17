//! Endpoint kernel built on top of `EventCursor`.
//!
//! The kernel endpoint owns the rendezvous port outright and advances
//! according to the typestate cursor obtained from `RoleProgram` projection.

use core::{ops::ControlFlow, task::Poll};

use super::authority::{Arm, RouteArmToken, RouteResolveStep};
use super::evidence::{
    ScopeEvidence, ScopeFrameLabelMeta, ScopeFrameLabelScratch, ScopeFrameLabelView,
    ScopeReentryMeta,
};
use super::frontier::*;
use super::frontier_state::{FrontierScratchState, FrontierState};
use super::lane_port;
use super::lane_slots::LaneSlotArray;
use super::layout::{EndpointArenaLayout, LeasedState};
use super::offer::*;
mod route_commit_helpers;
use super::decision_state::{RouteCommitRowSetBuilder, RouteState};
use crate::eff::EffIndex;
use crate::global::compiled::images::EventSemanticKind;
use crate::global::const_dsl::{RouteResolver, ScopeId};
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
    rendezvous::{core::EndpointLeaseId, port::Port},
    session::{
        brand::Owner,
        cluster::error::ClusterError,
        types::{Lane, RendezvousId, SessionId},
    },
    transport::{
        FrameLabelMask, Transport,
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
    fn prepare_recv_kernel_descriptor(
        &mut self,
        label: u8,
    ) -> RecvResult<super::recv::PreparedRecv>;

    fn poll_recv_kernel_payload_source(
        &mut self,
        desc: super::recv::RecvDescriptor,
        state: &mut super::recv::RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>>;

    fn finish_recv_kernel_payload(
        &mut self,
        desc: super::recv::RecvDescriptor,
        payload: Payload<'r>,
        erased: RecvRuntimeDesc,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>>;
}

pub(crate) trait BranchRecvKernelEndpoint<'r> {
    fn prepare_branch_recv_kernel_transport_wait(
        &mut self,
        desc: BranchRecvRuntimeDesc,
        branch: &MaterializedRouteBranch<'r>,
    ) -> RecvResult<Option<RecvMeta>>;

    fn poll_branch_recv_kernel_transport_payload(
        &mut self,
        meta: RecvMeta,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>>;

    fn finish_branch_recv_kernel(
        &mut self,
        desc: BranchRecvRuntimeDesc,
        prepared_meta: Option<RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>>;
}

pub(crate) trait SendKernelEndpoint<'r> {
    fn poll_send_init_kernel(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
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
    validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    state: &mut super::recv::RecvState,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<Payload<'r>>> {
    let prepared = match state.prepared() {
        Some(prepared) => prepared,
        None => {
            let prepared = match endpoint.prepare_recv_kernel_descriptor(logical_label) {
                Ok(prepared) => prepared,
                Err(err) => return Poll::Ready(Err(err)),
            };
            state.set_prepared(prepared);
            prepared
        }
    };
    match endpoint.poll_recv_kernel_payload_source(prepared.descriptor, state, cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(payload)) => {
            state.clear_prepared();
            Poll::Ready(
                endpoint
                    .finish_recv_kernel_payload(
                        prepared.descriptor,
                        payload,
                        prepared.runtime,
                        validate,
                    )
                    .map(|payload| unsafe {
                        // SAFETY: recv payloads returned by the kernel are backed by
                        // endpoint-resident transport, ingress, or a canonical zero-length slice.
                        lane_port::endpoint_resident_payload(payload)
                    }),
            )
        }
        Poll::Ready(Err(err)) => {
            state.clear_prepared();
            Poll::Ready(Err(err))
        }
    }
}

#[inline(never)]
pub(crate) fn kernel_branch_recv<'r>(
    endpoint: &mut dyn BranchRecvKernelEndpoint<'r>,
    desc: BranchRecvRuntimeDesc,
    state: &mut super::branch_recv::BranchRecvState<'r>,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<Payload<'r>>> {
    if state.branch().is_none() {
        return Poll::Ready(Err(RecvError::PhaseInvariant));
    }
    if state.prepared_meta().is_none() {
        let prepared = {
            let branch = crate::invariant_some(state.branch());
            match endpoint.prepare_branch_recv_kernel_transport_wait(desc, branch) {
                Ok(meta) => meta,
                Err(err) => return Poll::Ready(Err(err)),
            }
        };
        state.set_prepared_meta(prepared);
    }
    if let Some(meta) = state.prepared_meta() {
        let needs_transport = {
            let branch = crate::invariant_some(state.branch());
            branch.staged_payload.is_none()
        };
        if needs_transport {
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
            let branch = crate::invariant_some(state.branch_mut());
            branch.staged_payload = Some(StagedPayload::new(frame));
        }
    }
    let prepared_meta = state.prepared_meta();
    let result = {
        let branch = crate::invariant_some(state.branch_mut());
        endpoint.finish_branch_recv_kernel(desc, prepared_meta, branch)
    };
    match result {
        Ok(payload) => {
            drop(state.take_branch());
            state.disarm_restore();
            Poll::Ready(Ok(unsafe {
                // SAFETY: committed decode payloads are staged in endpoint-resident
                // transport/ingress storage or the static empty local payload.
                lane_port::endpoint_resident_payload(payload)
            }))
        }
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
            } => match endpoint.poll_send_init_kernel(
                *descriptor,
                *meta,
                *preview_cursor_index,
                payload.take(),
            ) {
                SendInitOutcome::Ready(result) => {
                    *state = SendState::Done;
                    return Poll::Ready(result);
                }
                SendInitOutcome::Pending { pending } => {
                    *state = SendState::Sending { pending };
                }
                SendInitOutcome::Commit { commit_plan } => {
                    let result = endpoint.finish_send_after_transport_kernel(commit_plan);
                    *state = SendState::Done;
                    return Poll::Ready(Ok(result));
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

impl<'r, const ROLE: u8, T> SendKernelEndpoint<'r> for CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn poll_send_init_kernel(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        payload: Option<lane_port::RawSendPayload>,
    ) -> SendInitOutcome<'r> {
        self.poll_send_init(descriptor, meta, preview_cursor_index, payload)
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

#[inline]
fn controller_arm_semantic_kind(
    cursor: &EventCursor,
    scope_id: ScopeId,
    arm: u8,
) -> Option<EventSemanticKind> {
    let (entry, _) = cursor.shared_controller_arm_entry_by_arm(scope_id, arm)?;
    Some(controller_arm_semantic_from_node(
        cursor.event_semantic_at(state_index_to_usize(entry)),
    ))
}

#[inline]
const fn controller_arm_semantic_from_node(kind: EventSemanticKind) -> EventSemanticKind {
    match kind {
        EventSemanticKind::DecisionArm | EventSemanticKind::ProtocolEvent => {
            EventSemanticKind::DecisionArm
        }
    }
}

mod commit_delta;
mod frontier_observation;
mod frontier_select;
mod offer_refresh;
mod scope_evidence_logic;

mod decision_resolver;
mod frontier_helpers;
mod public_types;
mod route_preview;
mod runtime_types;
mod send_ops;
mod send_preview;

pub(crate) use super::decision_state::{
    PreparedRouteCommitRows, SelectedRouteCommitRow, SelectedRouteCommitRowsRef,
};
pub(in crate::endpoint::kernel) use commit_delta::CommitDeltaApplyPermit;
pub(crate) use commit_delta::{CommittedCommitDelta, PreparedCommitDelta};
pub(crate) use public_types::*;
pub(in crate::endpoint::kernel) use route_preview::IngressEvidenceState;
pub(crate) use runtime_types::*;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    /// Rendezvous id for the primary port.
    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.port().rv_id()
    }

    /// Get the descriptor-selected primary lane's port.
    fn port(&self) -> &Port<'r, T> {
        if self.ports[self.primary_lane].is_none() {
            crate::invariant();
        }
        crate::invariant_some(self.ports[self.primary_lane].as_ref())
    }

    /// Get port for a specific lane.
    pub(crate) fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T> {
        if self.ports[lane_idx].is_none() {
            crate::invariant();
        }
        crate::invariant_some(self.ports[lane_idx].as_ref())
    }

    #[inline]
    pub(crate) fn frontier_scratch_view(&self) -> FrontierScratchView {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        frontier_scratch_view_from_storage(scratch_ptr, layout, self.cursor.max_frontier_entries())
    }

    #[inline]
    pub(crate) fn offer_lane_set_for_scope(&self, scope_id: ScopeId) -> LaneSetView<'static> {
        match self.cursor.route_scope_offer_lane_set(scope_id) {
            Some(lanes) => lanes,
            None => LaneSetView::EMPTY,
        }
    }

    #[inline]
    pub(crate) fn route_scope_arm_lane_set_for_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.cursor.route_scope_arm_lane_set(scope_id, arm)
    }

    #[inline]
    pub(crate) fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if let Some(lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) {
            lane_idx as u8
        } else {
            self.primary_lane as u8
        }
    }

    #[inline]
    pub(crate) fn controller_arm_at_cursor(&self, scope_id: ScopeId) -> Option<u8> {
        let idx = self.cursor.index();
        if let Some((entry, _)) = self.cursor.controller_arm_entry_by_arm(scope_id, 0)
            && idx == state_index_to_usize(entry)
        {
            return Some(0);
        }
        if let Some((entry, _)) = self.cursor.controller_arm_entry_by_arm(scope_id, 1)
            && idx == state_index_to_usize(entry)
        {
            return Some(1);
        }
        None
    }
}

impl<'r, const ROLE: u8, T> Drop for CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn drop(&mut self) {
        if self.public_generation != 0 && !self.cursor.is_terminal() {
            self.poison_session(SessionFaultKind::EndpointDropped);
        }
        self.terminal_clear_public_send_state();
        self.terminal_clear_public_recv_state();
        self.terminal_clear_public_offer_state();
        self.terminal_clear_public_branch_recv_state();
        if let Some(branch) = self.public_route_branch.take() {
            branch.discard_terminal();
        }
        self.clear_public_op_terminal();
        // Drop all active ports and guards
        for port in self.ports.iter_mut() {
            if let Some(p) = port.take() {
                drop(p);
            }
        }
        for guard in self.guards.iter_mut() {
            if let Some(g) = guard.take() {
                drop(g);
            }
        }
        if self.public_generation != 0 {
            let cluster = self.session.cluster();
            cluster.unbind_session_role(self.sid, ROLE, self.public_rv);
            if self.public_slot_ownership == PublicSlotOwnership::Owned {
                cluster.release_public_endpoint_slot_owned(
                    self.public_rv,
                    self.public_slot,
                    self.public_generation,
                );
            }
            self.public_header.retire_generation();
            self.public_generation = 0;
            self.public_slot_ownership = PublicSlotOwnership::Borrowed;
        }
    }
}
