//! Internal endpoint kernel built on top of `PhaseCursor`.
//!
//! The kernel endpoint owns the rendezvous port outright and advances
//! according to the typestate cursor obtained from `RoleProgram` projection.

#![allow(private_interfaces, unused_imports)]

use core::{convert::TryFrom, ops::ControlFlow, task::Poll};

use super::authority::{
    Arm, DeferReason, DeferSource, LoopDecision, RouteDecisionSource, RouteDecisionToken,
    RouteResolveStep, route_policy_input_arg0, validate_route_decision_scope,
};
use super::evidence::{ScopeEvidence, ScopeFrameLabelMeta, ScopeLoopMeta};
use super::frontier::*;
use super::frontier_state::FrontierState;
use super::inbox::{BindingInbox, PackedIngressEvidence};
use super::lane_port;
use super::lane_slots::LaneSlotArray;
use super::layout::{EndpointArenaLayout, LeasedState};
use super::offer::RouteFrontierMachine;
use super::offer::*;
use super::route_state::{RouteArmCommitProof, RouteCommitProofWorkspace, RouteState};
use crate::binding::{BindingSlot, IngressEvidence, NoBinding};
use crate::eff::EffIndex;
use crate::global::ControlDesc;
#[cfg(test)]
use crate::global::LoopControlMeaning;
use crate::global::compiled::images::{ControlSemanticKind, ControlSemanticsTable};
use crate::global::const_dsl::{PolicyMode, ScopeId, ScopeKind};
use crate::global::role_program::LaneSetView;
use crate::global::typestate::{
    ARM_SHARED, JumpReason, LocalAction, LoopMetadata, LoopRole, PassiveArmNavigation, PhaseCursor,
    RecvMeta, SendMeta, StateIndex, state_index_to_usize,
};
#[cfg(test)]
use crate::global::{MessageSpec, SendableLabel};
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    control::{
        cap::atomic_codecs::TopologyHandle,
        cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, LoopDecisionHandle, RouteArmHandle,
        },
        cap::{
            mint::{
                CAP_HANDLE_LEN, CAP_TOKEN_LEN, CapHeader, CapShot, ControlOp, E0, EndpointEpoch,
                EpochTable, EpochTbl, GenericCapToken, MintConfigMarker, Owner, ResourceKind,
            },
            typed_tokens::RawRegisteredCapToken,
        },
        cluster::{
            core::{DynamicPolicyResolution, TopologyDescriptor, TopologyOperands},
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
        tables::LoopDisposition,
    },
    runtime::{config::OperationalDeadline, consts::LabelUniverse},
    transport::{
        FrameLabelMask, Transport, TransportMetrics,
        trace::TapFrameMeta,
        wire::{CodecError, FrameFlags, Payload},
    },
};

#[derive(Clone, Copy)]
enum BindingLanePreference {
    Any,
    Arm(u8),
    LabelMask(FrameLabelMask),
}

#[cfg(test)]
#[path = "test_support/core_offer_tests.rs"]
mod offer_regression_tests;

#[inline]
fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

#[inline]
pub(in crate::endpoint::kernel) fn scope_slot_for_route_from_cursor(
    cursor: &PhaseCursor,
    scope: ScopeId,
) -> Option<usize> {
    if scope.is_none() || scope.kind() != ScopeKind::Route {
        return None;
    }
    cursor.route_scope_slot(scope)
}

#[inline]
pub(in crate::endpoint::kernel) fn is_linger_route_from_cursor(
    cursor: &PhaseCursor,
    scope: ScopeId,
) -> bool {
    cursor
        .scope_region_by_id(scope)
        .map(|region| {
            if region.kind == ScopeKind::Loop {
                return true;
            }
            region.kind == ScopeKind::Route && region.linger
        })
        .unwrap_or(false)
}

pub(in crate::endpoint::kernel) fn preflight_route_arm_commit_from_parts(
    route_state: &RouteState,
    cursor: &PhaseCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> Option<RouteArmCommitProof> {
    if scope.is_none() || scope.kind() != ScopeKind::Route {
        return None;
    }
    let lane_idx = lane as usize;
    if lane_idx >= cursor.logical_lane_count() {
        return None;
    }
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    route_state.preflight_route_arm_commit(
        lane_idx,
        scope,
        scope_slot,
        arm,
        is_linger_route_from_cursor(cursor, scope),
    )
}

pub(in crate::endpoint::kernel) fn preflight_route_arm_commit_after_clearing_other_lanes_from_parts(
    route_state: &RouteState,
    cursor: &PhaseCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> Option<RouteArmCommitProof> {
    if scope.is_none() || scope.kind() != ScopeKind::Route {
        return None;
    }
    let lane_idx = lane as usize;
    if lane_idx >= cursor.logical_lane_count() {
        return None;
    }
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    route_state.preflight_route_arm_commit_after_clearing_other_lanes(
        lane_idx,
        scope,
        scope_slot,
        arm,
        is_linger_route_from_cursor(cursor, scope),
    )
}

#[inline]
pub(in crate::endpoint::kernel) fn require_route_arm_commit_proof_from_parts(
    route_state: &RouteState,
    cursor: &PhaseCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> RecvResult<RouteArmCommitProof> {
    preflight_route_arm_commit_from_parts(route_state, cursor, lane, scope, arm)
        .ok_or(RecvError::PhaseInvariant)
}

#[inline]
fn selected_arm_for_scope_from_parts(
    route_state: &RouteState,
    cursor: &PhaseCursor,
    scope: ScopeId,
) -> Option<u8> {
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    route_state.selected_arm_for_scope_slot(scope_slot)
}

#[inline]
fn route_scope_materialization_index_from_cursor(
    cursor: &PhaseCursor,
    scope_id: ScopeId,
) -> Option<usize> {
    if let Some(offer_entry) = cursor.route_scope_offer_entry(scope_id)
        && !offer_entry.is_max()
    {
        return Some(state_index_to_usize(offer_entry));
    }
    cursor
        .scope_region_by_id(scope_id)
        .map(|region| region.start)
}

fn preview_scope_ack_token_non_consuming_from_parts<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    E: EpochTable + 'r,
>(
    ports: &LaneSlotArray<Port<'r, T, E>>,
    route_state: &RouteState,
    cursor: &PhaseCursor,
    scope_id: ScopeId,
    summary_lane_idx: usize,
    offer_lanes: LaneSetView,
) -> Option<RouteDecisionToken> {
    if let Some(slot) = scope_slot_for_route_from_cursor(cursor, scope_id)
        && let Some(token) = route_state.scope_evidence.peek_ack(slot)
    {
        return Some(token);
    }
    let lane_limit = cursor.logical_lane_count();
    if summary_lane_idx >= lane_limit {
        return None;
    }
    let mut next = offer_lanes.first_set(lane_limit);
    while let Some(lane_idx) = next {
        let pending = ports
            .get(summary_lane_idx)
            .and_then(|port| port.as_ref())
            .map(|port| {
                port.has_pending_route_decision_for_lane(scope_id, ROLE, Lane::new(lane_idx as u32))
            })
            .unwrap_or(false);
        if !pending {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        }
        let Some(port) = ports.get(lane_idx).and_then(|port| port.as_ref()) else {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        };
        let Some(arm) = port.peek_route_decision(scope_id, ROLE) else {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        };
        if let Some(arm) = Arm::new(arm) {
            return Some(RouteDecisionToken::from_ack(arm));
        }
        next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
    }
    None
}

fn preview_selected_arm_for_scope_from_parts<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    E: EpochTable + 'r,
>(
    ports: &LaneSlotArray<Port<'r, T, E>>,
    route_state: &RouteState,
    cursor: &PhaseCursor,
    scope_id: ScopeId,
) -> Option<u8> {
    if let Some(arm) = selected_arm_for_scope_from_parts(route_state, cursor, scope_id) {
        return Some(arm);
    }
    let offer_lanes = cursor
        .route_scope_offer_lane_set(scope_id)
        .unwrap_or(LaneSetView::EMPTY);
    let summary_lane_idx = offer_lanes.first_set(cursor.logical_lane_count())?;
    preview_scope_ack_token_non_consuming_from_parts::<ROLE, T, E>(
        ports,
        route_state,
        cursor,
        scope_id,
        summary_lane_idx,
        offer_lanes,
    )
    .map(|token| token.arm().as_u8())
    .or_else(|| {
        let slot = scope_slot_for_route_from_cursor(cursor, scope_id)?;
        let mask = route_state.scope_evidence.poll_ready_arm_mask(slot);
        (mask.count_ones() == 1)
            .then(|| Arm::new(mask.trailing_zeros() as u8))
            .flatten()
            .map(Arm::as_u8)
    })
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
    ) -> Poll<SendResult<SendTransportEmission>>;

    fn finish_send_after_transport_kernel(
        &mut self,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission,
    ) -> SendResult<SendControlOutcome<'r>>;
}

#[inline(never)]
pub(crate) fn kernel_recv<'r>(
    endpoint: &mut dyn RecvKernelEndpoint<'r>,
    logical_label: u8,
    expects_control: bool,
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
                        validate,
                    )
                    .map(|payload| unsafe {
                        // SAFETY: recv payloads returned by the kernel are backed by
                        // endpoint-resident transport, binding, or static empty storage.
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
            branch.staged_payload.is_none() && !branch.binding_evidence.is_present()
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
        endpoint.finish_decode_kernel(desc, prepared_meta, branch)
    };
    match result {
        Ok(payload) => {
            let _ = state.take_branch();
            state.restore_on_drop = false;
            Poll::Ready(Ok(unsafe {
                // SAFETY: committed decode payloads are staged in endpoint-resident
                // transport/binding storage or local synthetic scratch.
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
    cx: &mut core::task::Context<'_>,
) -> Poll<SendResult<SendControlOutcome<'r>>> {
    loop {
        match state {
            SendState::Init {
                descriptor,
                meta,
                preview_cursor_index,
                payload,
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
                SendInitOutcome::Pending {
                    meta,
                    preview_cursor_index,
                    pending,
                } => {
                    *state = SendState::Sending {
                        meta,
                        preview_cursor_index,
                        pending,
                        deadline: WaitDeadline::new(),
                    };
                }
                SendInitOutcome::Commit {
                    meta,
                    preview_cursor_index,
                    emission,
                } => {
                    *state = SendState::Committing {
                        meta,
                        preview_cursor_index,
                        emission,
                    };
                }
            },
            SendState::Sending {
                meta,
                preview_cursor_index,
                pending,
                ..
            } => match endpoint.poll_send_pending_kernel(pending, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(emission)) => {
                    *state = SendState::Committing {
                        meta: *meta,
                        preview_cursor_index: *preview_cursor_index,
                        emission,
                    };
                }
                Poll::Ready(Err(err)) => {
                    *state = SendState::Done;
                    return Poll::Ready(Err(err));
                }
            },
            SendState::Committing {
                meta,
                preview_cursor_index,
                emission,
            } => {
                let emission = core::mem::replace(emission, SendTransportEmission::empty());
                let result = endpoint.finish_send_after_transport_kernel(
                    *meta,
                    *preview_cursor_index,
                    emission,
                );
                *state = SendState::Done;
                return Poll::Ready(result);
            }
            SendState::Done => panic!("send future polled after completion"),
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> SendKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
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
    ) -> Poll<SendResult<SendTransportEmission>> {
        self.poll_send_pending(pending, cx)
    }

    #[inline]
    fn finish_send_after_transport_kernel(
        &mut self,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission,
    ) -> SendResult<SendControlOutcome<'r>> {
        self.finish_send_after_transport_runtime(meta, preview_cursor_index, emission)
    }
}

#[inline]
fn controller_arm_label(cursor: &PhaseCursor, scope_id: ScopeId, arm: u8) -> Option<u8> {
    cursor
        .shared_controller_arm_entry_by_arm(scope_id, arm)
        .map(|(_, label)| label)
}

#[inline]
fn controller_arm_semantic_kind(
    cursor: &PhaseCursor,
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
const fn is_loop_control_semantic(kind: ControlSemanticKind) -> bool {
    kind.is_loop()
}

#[inline]
const fn control_policy_is_validated_during_handle_preparation(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::CapDelegate | ControlOp::TopologyBegin | ControlOp::TopologyAck
    )
}

#[inline]
fn loop_control_kind_matches_disposition(
    semantic: ControlSemanticKind,
    disposition: LoopDisposition,
) -> bool {
    match disposition {
        LoopDisposition::Continue => semantic == ControlSemanticKind::LoopContinue,
        LoopDisposition::Break => semantic == ControlSemanticKind::LoopBreak,
    }
}

#[inline]
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

#[inline]
#[cfg(test)]
const fn loop_control_meaning_from_semantic(
    kind: ControlSemanticKind,
) -> Option<LoopControlMeaning> {
    match kind {
        ControlSemanticKind::LoopContinue => Some(LoopControlMeaning::Continue),
        ControlSemanticKind::LoopBreak => Some(LoopControlMeaning::Break),
        _ => None,
    }
}

#[cfg(test)]
#[inline]
fn stage_transport_payload(scratch: &mut [u8], payload: &[u8]) -> RecvResult<usize> {
    if payload.len() > scratch.len() {
        return Err(RecvError::PhaseInvariant);
    }
    scratch[..payload.len()].copy_from_slice(payload);
    Ok(payload.len())
}

#[cfg(test)]
fn endpoint_scope_frame_label_meta<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    scope_id: ScopeId,
    loop_meta: ScopeLoopMeta,
) -> ScopeFrameLabelMeta
where
    T: Transport,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_meta(
        &endpoint.cursor,
        &endpoint.control_semantics(),
        scope_id,
        loop_meta,
    )
}

#[cfg(test)]
#[path = "core/route_policy_tests.rs"]
mod route_policy_tests;

#[cfg(test)]
#[path = "core/send_rollback_tests.rs"]
mod send_rollback_tests;

#[path = "route_frontier/frontier_observation.rs"]
mod frontier_observation;
#[path = "route_frontier/frontier_select.rs"]
mod frontier_select;
#[path = "route_frontier/offer_refresh.rs"]
mod offer_refresh;
#[path = "route_frontier/scope_evidence_logic.rs"]
mod scope_evidence_logic;

mod binding_ingress;
mod frontier_helpers;
mod public_types;
mod route_policy;
mod route_preview;
mod route_preview_flow;
mod runtime_types;
mod scope_settlement;
mod send_control_ops;
mod send_ops;

pub(crate) use binding_ingress::*;
pub(crate) use frontier_helpers::*;
pub(crate) use public_types::*;
pub(crate) use route_policy::*;
pub(crate) use route_preview::*;
pub(crate) use route_preview_flow::*;
pub(crate) use runtime_types::*;
pub(crate) use scope_settlement::*;
pub(crate) use send_control_ops::*;
pub(crate) use send_ops::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub(crate) fn matches_session(&self, sid: SessionId) -> bool {
        self.sid == sid
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

    pub(crate) fn revoke_public_owner(&mut self) {
        for guard in self.guards.iter_mut() {
            if let Some(guard) = guard.as_mut() {
                guard.detach_rendezvous();
            }
        }
        self.invalidate_public_owner();
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> Drop
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    fn drop(&mut self) {
        if self.public_generation != 0 && !self.cursor.is_terminal() {
            let _ = self.poison_session(SessionFaultKind::EndpointDropped);
        }
        self.terminal_clear_public_offer_state();
        self.terminal_clear_public_decode_state();
        if let Some(branch) = self.public_route_branch.take() {
            branch.discard_terminal();
        }
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

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    fn topology_handle_from_operands(operands: TopologyOperands) -> TopologyHandle {
        TopologyHandle {
            src_rv: operands.src_rv.raw(),
            dst_rv: operands.dst_rv.raw(),
            src_lane: operands.src_lane.raw() as u16,
            dst_lane: operands.dst_lane.raw() as u16,
            old_gen: operands.old_gen.raw(),
            new_gen: operands.new_gen.raw(),
            seq_tx: operands.seq_tx,
            seq_rx: operands.seq_rx,
        }
    }
}
