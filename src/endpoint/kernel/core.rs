//! Internal endpoint kernel built on top of `EventCursor`.
//!
//! The kernel endpoint owns the rendezvous port outright and advances
//! according to the typestate cursor obtained from `RoleProgram` projection.

use core::{convert::TryFrom, ops::ControlFlow, task::Poll};

use super::authority::{
    Arm, DeferReason, DeferSource, LoopDecision, RouteArmToken, RouteResolveStep,
    decision_policy_input_arg0,
};
use super::evidence::{ScopeEvidence, ScopeFrameLabelMeta, ScopeLoopMeta};
use super::frontier::*;
use super::frontier_state::FrontierState;
use super::lane_port;
use super::lane_slots::LaneSlotArray;
use super::layout::{EndpointArenaLayout, LeasedState};
use super::offer::*;
mod route_commit_helpers;
use super::decision_state::{RouteCommitRowSetBuilder, RouteState};
use crate::eff::EffIndex;
use crate::global::ControlDesc;
use crate::global::compiled::images::{ControlSemanticKind, ControlSemanticsTable};
use crate::global::const_dsl::{ResolverMode, ScopeId};
use crate::global::role_program::LaneSetView;
use crate::global::typestate::{
    CursorRefresh, EventCursor, FlowPreviewError, JumpReason, LoopRole, RecvMeta,
    RelocatableResidentLaneStep, ResidentLaneStepError, SendMeta, StateIndex, state_index_to_usize,
};
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    control::{
        cap::mint::{
            CAP_HANDLE_LEN, CAP_TOKEN_LEN, CapHeader, CapShot, ControlOp, E0, EndpointEpoch,
            EpochTable, EpochTbl, MintConfigMarker, Owner,
        },
        cap::resource_kinds::LoopDecisionHandle,
        cluster::{
            core::{
                DecisionSubject, DescriptorPublicationAuthority, DescriptorTerminal,
                DynamicPolicyResolution,
            },
            error::CpError,
        },
    },
    endpoint::{
        RecvError, RecvResult, SendError, SendResult, affine::LaneGuard, control::SessionControlCtx,
    },
    observe::core::{TapEvent, emit},
    observe::scope::ScopeTrace,
    observe::{events, ids},
    policy_runtime::{self, PolicySlot},
    rendezvous::SessionFaultKind,
    rendezvous::{
        capability::{CapEntry, CapReleaseCtx},
        core::EndpointLeaseId,
        port::Port,
    },
    runtime::consts::LabelUniverse,
    transport::{
        FrameLabelMask, Transport,
        trace::TapFrameMeta,
        wire::{CodecError, FrameFlags, Payload},
    },
};
pub(in crate::endpoint::kernel::core) use route_commit_helpers::prepare_route_site_materialization_rows_from_resident_route_commit_range;
pub(in crate::endpoint::kernel::core) use route_commit_helpers::preview_selected_arm_for_scope_from_parts;
pub(in crate::endpoint::kernel) use route_commit_helpers::{
    prepare_descriptor_checked_recv_linger_rows_from_resident_route_commit_range,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
    scope_slot_for_route_from_cursor,
};

#[inline]
fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

pub(crate) trait RecvKernelEndpoint<'r> {
    fn prepare_recv_kernel_descriptor(
        &mut self,
        label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
    ) -> RecvResult<super::recv::PreparedRecv>;

    fn poll_recv_kernel_payload_source(
        &mut self,
        desc: super::recv::RecvDescriptor,
        accepts_empty_payload: bool,
        state: &mut super::recv::RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<super::recv::RecvPayloadSource<'r>>>;

    fn finish_recv_kernel_payload(
        &mut self,
        desc: super::recv::RecvDescriptor,
        payload_source: super::recv::RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
        control: Option<ControlDesc>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>>;
}

pub(crate) trait DecodeKernelEndpoint<'r> {
    fn prepare_decode_kernel_transport_wait(
        &mut self,
        desc: DecodeRuntimeDesc,
        branch: &MaterializedRouteBranch<'r>,
    ) -> RecvResult<Option<RecvMeta>>;

    fn poll_decode_kernel_transport_payload(
        &mut self,
        meta: RecvMeta,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>>;

    fn finish_decode_kernel(
        &mut self,
        desc: DecodeRuntimeDesc,
        control: Option<ControlDesc>,
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
    expects_control: bool,
    control: Option<ControlDesc>,
    accepts_empty_payload: bool,
    validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    state: &mut super::recv::RecvState,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<Payload<'r>>> {
    let prepared = match state.prepared() {
        Some(prepared) => prepared,
        None => {
            let prepared = match endpoint.prepare_recv_kernel_descriptor(
                logical_label,
                expects_control,
                accepts_empty_payload,
            ) {
                Ok(prepared) => prepared,
                Err(err) => return Poll::Ready(Err(err)),
            };
            state.set_prepared(prepared);
            prepared
        }
    };
    match endpoint.poll_recv_kernel_payload_source(
        prepared.descriptor,
        prepared.runtime.accepts_empty_payload(),
        state,
        cx,
    ) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(payload_source)) => {
            state.clear_prepared();
            Poll::Ready(
                endpoint
                    .finish_recv_kernel_payload(
                        prepared.descriptor,
                        payload_source,
                        prepared.runtime,
                        control,
                        validate,
                    )
                    .map(|payload| unsafe {
                        // SAFETY: recv payloads returned by the kernel are backed by
                        // endpoint-resident transport, ingress, or static empty storage.
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
pub(crate) fn kernel_decode<'r>(
    endpoint: &mut dyn DecodeKernelEndpoint<'r>,
    desc: DecodeRuntimeDesc,
    control: Option<ControlDesc>,
    state: &mut super::decode::DecodeState<'r>,
    cx: &mut core::task::Context<'_>,
) -> Poll<RecvResult<Payload<'r>>> {
    if state.branch().is_none() {
        return Poll::Ready(Err(RecvError::PhaseInvariant));
    }
    if state.prepared_meta().is_none() {
        let prepared = {
            let branch = state.branch().expect("decode branch checked above");
            match endpoint.prepare_decode_kernel_transport_wait(desc, branch) {
                Ok(meta) => meta,
                Err(err) => return Poll::Ready(Err(err)),
            }
        };
        state.set_prepared_meta(prepared);
    }
    if let Some(meta) = state.prepared_meta() {
        let needs_transport = {
            let branch = state.branch().expect("decode branch checked above");
            branch.staged_payload.is_none()
        };
        if needs_transport {
            let frame = match endpoint.poll_decode_kernel_transport_payload(
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
            let branch = state.branch_mut().expect("decode branch checked above");
            branch.staged_payload = Some(StagedPayload::Transport { frame });
        }
    }
    let prepared_meta = state.prepared_meta();
    let result = {
        let branch = state.branch_mut().expect("decode branch checked above");
        endpoint.finish_decode_kernel(desc, control, prepared_meta, branch)
    };
    match result {
        Ok(payload) => {
            let _ = state.take_branch();
            state.restore_on_drop = false;
            Poll::Ready(Ok(unsafe {
                // SAFETY: committed decode payloads are staged in endpoint-resident
                // transport/ingress storage or local synthetic scratch.
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
            SendState::Done => panic!("send future polled after completion"),
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint> SendKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
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
    _semantics: &ControlSemanticsTable,
    scope_id: ScopeId,
    arm: u8,
) -> Option<ControlSemanticKind> {
    let (entry, _) = cursor.shared_controller_arm_entry_by_arm(scope_id, arm)?;
    loop_control_semantic_kind(cursor.control_semantic_at(state_index_to_usize(entry)))
}

#[inline]
const fn loop_control_semantic_kind(kind: ControlSemanticKind) -> Option<ControlSemanticKind> {
    if kind.is_loop() { Some(kind) } else { None }
}

#[inline]
const fn control_policy_is_validated_during_handle_preparation(op: ControlOp) -> bool {
    matches!(op, ControlOp::TopologyBegin | ControlOp::TopologyAck)
}

#[inline]
#[cfg(test)]
fn next_preferred_lane_in_lane_set(
    preferred_lane_idx: usize,
    offer_lanes: LaneSetView,
    lane_limit: usize,
    scan_idx: &mut usize,
) -> Option<usize> {
    if *scan_idx == 0 {
        *scan_idx = 1;
        if preferred_lane_idx < lane_limit && offer_lanes.contains(preferred_lane_idx) {
            return Some(preferred_lane_idx);
        }
    }

    let mut start = scan_idx.saturating_sub(1);
    while let Some(lane_idx) = offer_lanes.next_set_from(start, lane_limit) {
        *scan_idx = lane_idx.saturating_add(2);
        start = lane_idx.saturating_add(1);
        if lane_idx != preferred_lane_idx {
            return Some(lane_idx);
        }
    }

    None
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "core/decision_policy_tests.rs"]
mod decision_policy_tests;

#[cfg(all(test, hibana_repo_tests))]
#[path = "core/send_rollback_tests.rs"]
mod send_rollback_tests;

mod commit_delta;
mod frontier_observation;
mod frontier_select;
mod offer_refresh;
mod scope_evidence_logic;

mod decision_policy;
mod frontier_helpers;
mod public_types;
mod route_preview;
mod route_preview_flow;
mod runtime_types;
mod send_control_commit;
mod send_control_ops;
mod send_descriptor_publication;
mod send_descriptor_terminal;
mod send_ops;

pub(crate) use super::decision_state::{
    PreparedRouteCommitRows, SelectedRouteCommitRow, SelectedRouteCommitRowsRef,
};
pub(in crate::endpoint::kernel) use commit_delta::CommitDeltaApplyPermit;
#[cfg(all(test, hibana_repo_tests))]
pub(in crate::endpoint::kernel) use commit_delta::test_commit_delta_apply_permit;
pub(crate) use commit_delta::{CommittedCommitDelta, PreparedCommitDelta};
pub(crate) use public_types::*;
pub(crate) use runtime_types::*;
pub(crate) use send_descriptor_publication::*;
pub(crate) use send_descriptor_terminal::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(crate) fn matches_session(&self, sid: SessionId) -> bool {
        self.sid == sid
    }

    /// Rendezvous id for the primary port.
    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.port().rv_id()
    }

    /// Get the descriptor-selected primary lane's port.
    fn port(&self) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[self.primary_lane].is_some(),
            "port: primary lane {} has no port (invariant violation)",
            self.primary_lane
        );
        self.ports[self.primary_lane]
            .as_ref()
            .expect("cursor endpoint retains primary port")
    }

    /// Get port for a specific lane.
    pub(crate) fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[lane_idx].is_some(),
            "port_for_lane: lane {} has no port",
            lane_idx
        );
        self.ports[lane_idx]
            .as_ref()
            .expect("port not acquired for lane")
    }

    #[inline]
    pub(crate) fn frontier_scratch_view(&self) -> FrontierScratchView {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        frontier_scratch_view_from_storage(scratch_ptr, layout, self.cursor.max_frontier_entries())
    }

    pub(crate) fn loop_index(scope: ScopeId) -> Option<u8> {
        u8::try_from(scope.ordinal()).ok()
    }

    #[inline]
    pub(crate) fn offer_lane_set_for_scope(&self, scope_id: ScopeId) -> LaneSetView<'static> {
        self.cursor
            .route_scope_offer_lane_set(scope_id)
            .unwrap_or(LaneSetView::EMPTY)
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

    pub(crate) fn for_each_physical_lane(&self, mut f: impl FnMut(Lane)) {
        let logical_lane_count = self.cursor.logical_lane_count();
        for slot in self.ports.iter().take(logical_lane_count) {
            if let Some(port) = slot.as_ref() {
                f(port.lane);
            }
        }
    }

    pub(crate) fn invalidate_public_owner(&mut self) {
        self.public_header.invalidate();
        self.public_generation = 0;
        self.public_slot_owned = false;
    }

    pub(crate) fn prepare_public_owner_revocation(
        &mut self,
        terminal: &mut EndpointRevocationTerminal<'r>,
    ) {
        terminal.set_waiter_lane(self.primary_physical_lane());
        self.revoke_drain_public_send_terminal(terminal);
        self.revoke_clear_public_recv_state();
        self.revoke_clear_public_offer_state();
        self.revoke_clear_public_decode_state();
        if let Some(branch) = self.public_route_branch.take() {
            branch.discard_terminal();
        }
        self.clear_public_op_terminal();
    }

    pub(crate) fn finish_public_owner_revocation(&mut self) {
        self.invalidate_public_owner();
        self.revoke_finish_public_send_state();
        for port in self.ports.iter_mut() {
            if let Some(port) = port.take() {
                drop(port);
            }
        }
        for guard in self.guards.iter_mut() {
            if let Some(guard) = guard.as_mut() {
                guard.detach_rendezvous();
            }
            if let Some(guard) = guard.take() {
                drop(guard);
            }
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint> Drop
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    fn drop(&mut self) {
        if self.public_generation != 0 && !self.cursor.is_terminal() {
            let _ = self.poison_session(SessionFaultKind::EndpointDropped);
        }
        self.terminal_clear_public_send_state();
        self.terminal_clear_public_recv_state();
        self.terminal_clear_public_offer_state();
        self.terminal_clear_public_decode_state();
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
        if self.public_generation != 0
            && let Some(cluster) = self.control.cluster()
        {
            if self.public_slot_owned {
                cluster.release_public_endpoint_slot_owned(
                    self.public_rv,
                    self.public_slot,
                    self.public_generation,
                );
            }
            self.public_header.invalidate();
            self.public_generation = 0;
            self.public_slot_owned = false;
        }
    }
}
