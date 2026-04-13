//! Internal endpoint kernel built on top of `PhaseCursor`.
//!
//! The kernel endpoint owns the rendezvous port outright and advances
//! according to the typestate cursor obtained from `RoleProgram` projection.

use core::{convert::TryFrom, future::poll_fn, ops::ControlFlow, task::Poll};

use super::authority::{
    Arm, DeferReason, DeferSource, LoopDecision, RouteDecisionSource, RouteDecisionToken,
    RoutePolicyDecision, RouteResolveStep, ScopeHint, route_policy_decision_from_action,
    route_policy_input_arg0, validate_route_decision_scope,
};
use super::evidence::{ScopeEvidence, ScopeLabelMeta, ScopeLoopMeta};
use super::frontier::*;
use super::frontier_state::FrontierState;
use super::inbox::BindingInbox;
use super::lane_port;
use super::lane_slots::LaneSlotArray;
use super::layout::{EndpointArenaLayout, LeasedState};
use super::offer::*;
use super::route_state::RouteState;
use crate::binding::{BindingSlot, NoBinding};
use crate::eff::EffIndex;
#[cfg(test)]
use crate::global::LoopControlMeaning;
use crate::global::const_dsl::{PolicyMode, ScopeId, ScopeKind};
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{
    ARM_SHARED, JumpReason, LoopMetadata, LoopRole, PassiveArmNavigation, PhaseCursor, RecvMeta,
    SendMeta, StateIndex, state_index_to_usize,
};
use crate::global::{
    CanonicalControl, ControlHandling, ControlPayloadKind, ExternalControl, MessageSpec, NoControl,
    SendableLabel,
    compiled::{ControlSemanticKind, ControlSemanticsTable},
};
use crate::runtime::config::Clock;
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    control::{
        cap::resource_kinds::{
            CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoopBreakKind, LoopContinueKind,
            LoopDecisionHandle, RerouteKind, RollbackKind, RouteDecisionHandle, RouteDecisionKind,
            SpliceAckKind, SpliceHandle, SpliceIntentKind, splice_flags,
        },
        cap::{
            mint::{
                CAP_TOKEN_LEN, CapShot, ControlMint, E0, EndpointEpoch, EpochTable, EpochTbl,
                GenericCapToken, MintConfigMarker, Owner, ResourceKind,
            },
            typed_tokens::{CapFlowToken, ErasedRegisteredCapToken, RegisteredTokenParts},
        },
        cluster::{
            core::{DynamicResolution, SpliceOperands},
            effects::CpEffect,
            error::CpError,
        },
    },
    endpoint::{
        RecvError, RecvResult, SendError, SendResult,
        affine::LaneGuard,
        control::{ControlOutcome, SessionControlCtx},
    },
    epf::{self, AbortInfo, Action, vm::Slot},
    observe::core::{TapEvent, emit},
    observe::scope::ScopeTrace,
    observe::{events, ids, policy_abort, policy_trap},
    rendezvous::core::EndpointLeaseId,
    rendezvous::{port::Port, tables::LoopDisposition},
    runtime::consts::LabelUniverse,
    transport::{
        Transport, TransportMetrics,
        trace::TapFrameMeta,
        wire::{FrameFlags, WireEncode},
    },
};

type PortStorage<'r, T, E> = LaneSlotArray<Port<'r, T, E>>;
type GuardStorage<'r, T, U, C> = LaneSlotArray<LaneGuard<'r, T, U, C>>;

type StoredMint<Mint> = crate::control::cap::mint::MintConfig<
    <Mint as MintConfigMarker>::Spec,
    <Mint as MintConfigMarker>::Policy,
>;

#[cfg(test)]
use super::authority::resolve_route_decision_handle_with_policy;
#[cfg(test)]
use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

#[inline]
fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

#[inline]
fn controller_arm_label(cursor: &PhaseCursor, scope_id: ScopeId, arm: u8) -> Option<u8> {
    cursor
        .controller_arm_entry_by_arm(scope_id, arm)
        .map(|(_, label)| label)
}

#[inline]
fn controller_arm_semantic_kind(
    cursor: &PhaseCursor,
    semantics: &ControlSemanticsTable,
    scope_id: ScopeId,
    arm: u8,
) -> Option<ControlSemanticKind> {
    let (entry, _label) = cursor.controller_arm_entry_by_arm(scope_id, arm)?;
    let entry_idx = state_index_to_usize(entry);
    cursor
        .try_local_meta_at(entry_idx)
        .and_then(|meta| loop_control_semantic_kind_from_resource(semantics, meta.resource))
        .or_else(|| {
            cursor
                .try_send_meta_at(entry_idx)
                .and_then(|meta| loop_control_semantic_kind_from_resource(semantics, meta.resource))
        })
        .or_else(|| {
            cursor
                .try_recv_meta_at(entry_idx)
                .and_then(|meta| loop_control_semantic_kind_from_resource(semantics, meta.resource))
        })
}

#[inline]
const fn loop_control_semantic_kind_from_resource(
    semantics: &ControlSemanticsTable,
    resource: Option<u8>,
) -> Option<ControlSemanticKind> {
    let kind = semantics.semantic_for_resource_tag(resource);
    if kind.is_loop() { Some(kind) } else { None }
}

#[inline]
const fn is_loop_control_label_or_resource(
    semantics: &ControlSemanticsTable,
    label: u8,
    resource: Option<u8>,
) -> bool {
    let kind = semantics.semantic_for(label, resource);
    kind.is_loop()
}

#[inline]
fn loop_control_kind_matches_disposition(
    semantics: &ControlSemanticsTable,
    label: u8,
    resource: Option<u8>,
    disposition: LoopDisposition,
) -> bool {
    match disposition {
        LoopDisposition::Continue => {
            semantics.semantic_for(label, resource) == ControlSemanticKind::LoopContinue
        }
        LoopDisposition::Break => {
            semantics.semantic_for(label, resource) == ControlSemanticKind::LoopBreak
        }
    }
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

#[inline]
const fn is_splice_or_reroute_semantic(kind: ControlSemanticKind) -> bool {
    matches!(
        kind,
        ControlSemanticKind::SpliceIntent
            | ControlSemanticKind::SpliceAck
            | ControlSemanticKind::Reroute
    )
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
fn endpoint_scope_label_meta<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    scope_id: ScopeId,
    loop_meta: ScopeLoopMeta,
) -> ScopeLabelMeta
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_label_meta(
        &endpoint.cursor,
        &endpoint.control_semantics(),
        scope_id,
        loop_meta,
    )
}

#[cfg(test)]
mod route_policy_tests {
    use super::*;

    #[test]
    fn route_policy_input_arg0_defaults_to_zero() {
        assert_eq!(route_policy_input_arg0(&[0; 4]), 0);
    }

    #[test]
    fn route_policy_input_arg0_reads_arg0() {
        assert_eq!(
            route_policy_input_arg0(&[0xABCD_1234, 0, 0, 0]),
            0xABCD_1234
        );
    }

    #[test]
    fn route_policy_action_mapping_is_explicit() {
        assert_eq!(
            route_policy_decision_from_action(Action::Route { arm: 1 }, 44),
            RoutePolicyDecision::RouteArm(1)
        );
        assert_eq!(
            route_policy_decision_from_action(Action::Route { arm: 2 }, 44),
            RoutePolicyDecision::Abort(44)
        );
        assert_eq!(
            route_policy_decision_from_action(
                Action::Abort(AbortInfo {
                    reason: 77,
                    trap: None,
                }),
                44
            ),
            RoutePolicyDecision::Abort(77)
        );
        assert_eq!(
            route_policy_decision_from_action(Action::Proceed, 44),
            RoutePolicyDecision::DelegateResolver
        );
        assert_eq!(
            route_policy_decision_from_action(
                Action::Tap {
                    id: 1,
                    arg0: 2,
                    arg1: 3
                },
                44
            ),
            RoutePolicyDecision::DelegateResolver
        );
        assert_eq!(
            route_policy_decision_from_action(Action::Defer { retry_hint: 9 }, 44),
            RoutePolicyDecision::Defer {
                retry_hint: 9,
                source: DeferSource::Epf
            }
        );
    }

    #[test]
    fn route_policy_delegates_to_resolver_result() {
        let scope = ScopeId::generic(10);
        let handle = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::DelegateResolver,
            || Ok(RouteDecisionHandle { scope, arm: 1 }),
        )
        .expect("delegation should use resolver");
        assert_eq!(handle.arm, 1);
    }

    #[test]
    fn route_policy_route_arm_skips_resolver_delegation() {
        let scope = ScopeId::generic(14);
        let mut delegate_called = false;
        let handle = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::RouteArm(1),
            || {
                delegate_called = true;
                Ok(RouteDecisionHandle { scope, arm: 0 })
            },
        )
        .expect("route arm should be accepted directly");
        assert_eq!(handle.arm, 1);
        assert!(!delegate_called);
    }

    #[test]
    fn route_policy_abort_skips_resolver_delegation() {
        let scope = ScopeId::generic(15);
        let mut delegate_called = false;
        let err = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::Abort(77),
            || {
                delegate_called = true;
                Ok(RouteDecisionHandle { scope, arm: 0 })
            },
        )
        .expect_err("abort should short-circuit");
        assert!(matches!(err, SendError::PolicyAbort { reason: 77 }));
        assert!(!delegate_called);
    }

    #[test]
    fn route_policy_delegation_propagates_resolver_abort() {
        let scope = ScopeId::generic(11);
        let err = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::DelegateResolver,
            || Err(SendError::PolicyAbort { reason: 99 }),
        )
        .expect_err("resolver abort should propagate");
        assert!(matches!(err, SendError::PolicyAbort { reason: 99 }));
    }

    #[test]
    fn route_policy_enforces_scope_match_before_route_handle() {
        let scope = ScopeId::generic(12);
        let err = resolve_route_decision_handle_with_policy(
            scope,
            ScopeId::generic(13),
            RoutePolicyDecision::RouteArm(0),
            || Ok(RouteDecisionHandle { scope, arm: 0 }),
        )
        .expect_err("scope mismatch must fail");
        assert!(matches!(err, SendError::PhaseInvariant));
    }

    #[test]
    fn route_policy_scope_mismatch_blocks_resolver_delegation() {
        let scope = ScopeId::generic(16);
        let mut delegate_called = false;
        let err = resolve_route_decision_handle_with_policy(
            scope,
            ScopeId::generic(17),
            RoutePolicyDecision::DelegateResolver,
            || {
                delegate_called = true;
                Ok(RouteDecisionHandle { scope, arm: 1 })
            },
        )
        .expect_err("scope mismatch must fail before resolver delegation");
        assert!(matches!(err, SendError::PhaseInvariant));
        assert!(!delegate_called);
    }

    #[test]
    fn route_policy_allows_static_route_scope_without_policy_scope() {
        let scope = ScopeId::generic(18);
        let handle = resolve_route_decision_handle_with_policy(
            scope,
            ScopeId::none(),
            RoutePolicyDecision::RouteArm(1),
            || Ok(RouteDecisionHandle { scope, arm: 0 }),
        )
        .expect("static route scope should remain valid without policy scope");
        assert_eq!(handle, RouteDecisionHandle { scope, arm: 1 });
    }
}

#[path = "route_frontier/frontier_observation.rs"]
mod frontier_observation;
#[path = "route_frontier/frontier_select.rs"]
mod frontier_select;
#[path = "route_frontier/offer_refresh.rs"]
mod offer_refresh;
#[cfg(test)]
#[path = "core_offer_tests.rs"]
mod offer_regression_tests;
#[path = "route_frontier/scope_evidence_logic.rs"]
mod scope_evidence_logic;

/// Internal endpoint kernel. Owns the rendezvous port as well as the lane
/// release handle. Dropping the endpoint releases the lane back to the
/// `SessionCluster` via the handle.
pub struct CursorEndpoint<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U = crate::runtime::consts::DefaultLabelUniverse,
    C = crate::runtime::config::CounterClock,
    E: EpochTable = EpochTbl,
    const MAX_RV: usize = 8,
    Mint = crate::control::cap::mint::MintConfig,
    B: BindingSlot = NoBinding,
> where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    pub(super) ports: PortStorage<'r, T, E>,
    /// Multi-lane guard array. Each active lane has its own guard.
    pub(super) guards: GuardStorage<'r, T, U, C>,
    /// Primary lane index (first active lane, typically 0).
    pub(super) primary_lane: usize,
    pub(super) sid: SessionId,
    pub(super) _owner: Owner<'r, E0>,
    pub(super) _epoch: EndpointEpoch<'r, E>,
    /// Phase-aware cursor for multi-lane parallel execution.
    pub(super) cursor: PhaseCursor,
    pub(super) public_rv: RendezvousId,
    pub(super) public_slot: EndpointLeaseId,
    pub(super) public_generation: u32,
    pub(super) public_slot_owned: bool,
    pub(super) control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    pub(super) route_state: LeasedState<RouteState>,
    pub(super) frontier_state: LeasedState<FrontierState>,
    pub(super) binding_inbox: LeasedState<BindingInbox>,
    pub(super) pending_branch_preview: Option<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>,
    pub(super) liveness_policy: crate::runtime::config::LivenessPolicy,
    pub(super) mint: StoredMint<Mint>,
    pub(super) binding: B,
}

pub struct RouteBranch<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U,
    C,
    E: EpochTable,
    const MAX_RV: usize,
    Mint,
    B: BindingSlot,
> where
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
{
    pub(super) label: u8,
    pub(super) cursor_index: StateIndex,
    pub(super) transport_payload_len: usize,
    pub(super) transport_payload_lane: u8,
    pub(super) binding_channel: Option<crate::binding::Channel>,
    pub(super) branch_meta: BranchMeta,
    _cfg: core::marker::PhantomData<fn() -> (&'r T, U, C, E, Mint, B)>,
}

#[derive(Clone, Copy)]
pub(crate) struct SendPreview {
    meta: SendMeta,
    cursor_index: StateIndex,
}

impl SendPreview {
    #[inline]
    pub(crate) const fn new(meta: SendMeta, cursor_index: StateIndex) -> Self {
        Self { meta, cursor_index }
    }

    #[inline]
    pub(crate) const fn into_parts(self) -> (SendMeta, StateIndex) {
        (self.meta, self.cursor_index)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> Clone
    for RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            label: self.label,
            cursor_index: self.cursor_index,
            transport_payload_len: self.transport_payload_len,
            transport_payload_lane: self.transport_payload_lane,
            binding_channel: self.binding_channel,
            branch_meta: self.branch_meta,
            _cfg: core::marker::PhantomData,
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub(super) fn matches_send_meta(&self, meta: SendMeta) -> bool {
        self.branch_meta.kind == BranchKind::ArmSendHint
            && self.label == meta.label
            && self.branch_meta.scope_id == meta.scope
            && meta.route_arm == Some(self.branch_meta.selected_arm)
    }
}

#[derive(Clone, Copy)]
struct ErasedCapFlowToken {
    bytes: [u8; CAP_TOKEN_LEN],
}

impl ErasedCapFlowToken {
    #[inline(always)]
    fn from_typed<K: ResourceKind>(token: CapFlowToken<K>) -> Self {
        Self {
            bytes: token.into_bytes(),
        }
    }

    #[inline(always)]
    fn bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.bytes
    }

    #[inline(always)]
    fn into_generic<K: ResourceKind>(self) -> GenericCapToken<K> {
        GenericCapToken::from_bytes(self.bytes)
    }

    #[inline(always)]
    fn into_flow_token<K: ResourceKind>(self) -> CapFlowToken<K> {
        CapFlowToken::new(self.into_generic::<K>())
    }
}

#[derive(Clone, Copy)]
struct SendDescriptor<E> {
    label: u8,
    expects_control: bool,
    mint_token: MintSendTokenFn<E>,
    stage_payload: StageSendPayloadFn,
    dispatch_control: DispatchSendTokenFn<E>,
}

struct PreparedSendControl<E> {
    minted_token: Option<ErasedCapFlowToken>,
    stage_payload: StageSendPayloadFn,
    dispatch_control: DispatchSendTokenFn<E>,
}

#[derive(Clone, Copy)]
enum StagedSendControl {
    None,
    Canonical(ErasedCapFlowToken),
    External {
        dispatch_token: Option<ErasedCapFlowToken>,
        external_token: Option<ErasedCapFlowToken>,
    },
}

type MintSendTokenFn<E> = fn(&E, SendMeta) -> SendResult<Option<ErasedCapFlowToken>>;
type DispatchSendTokenFn<E> = fn(&E, ErasedCapFlowToken) -> SendResult<DispatchSendTokenResult>;

enum DispatchSendTokenResult {
    None,
    Registered(RegisteredTokenParts),
    CanonicalFallback,
}

type StageSendPayloadFn = for<'payload, 'scratch> fn(
    Option<ErasedCapFlowToken>,
    Option<lane_port::ErasedSendPayload<'payload>>,
    &'scratch mut [u8],
) -> SendResult<StagedSendPayload>;

struct StagedSendPayload {
    encoded_len: usize,
    control: StagedSendControl,
}

struct SendTransportEmission<E> {
    control: StagedSendControl,
    dispatch_control: DispatchSendTokenFn<E>,
}

enum ErasedControlOutcome<'rv> {
    None,
    Canonical(ErasedRegisteredCapToken<'rv>),
    External(ErasedCapFlowToken),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CursorEndpointStorageLayout {
    header_bytes: usize,
    header_align: usize,
    port_slots_offset: usize,
    port_slots_bytes: usize,
    port_slots_align: usize,
    guard_slots_offset: usize,
    guard_slots_bytes: usize,
    guard_slots_align: usize,
    arena_offset: usize,
    arena_bytes: usize,
    arena_align: usize,
    total_bytes: usize,
    total_align: usize,
}

impl CursorEndpointStorageLayout {
    #[inline(always)]
    pub(crate) const fn header_bytes(self) -> usize {
        self.header_bytes
    }

    #[inline(always)]
    pub(crate) const fn port_slots_offset(self) -> usize {
        self.port_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn port_slots_bytes(self) -> usize {
        self.port_slots_bytes
    }

    #[inline(always)]
    pub(crate) const fn guard_slots_offset(self) -> usize {
        self.guard_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn guard_slots_bytes(self) -> usize {
        self.guard_slots_bytes
    }

    #[inline(always)]
    pub(crate) const fn arena_offset(self) -> usize {
        self.arena_offset
    }

    #[inline(always)]
    pub(crate) const fn arena_bytes(self) -> usize {
        self.arena_bytes
    }

    #[inline(always)]
    pub(crate) const fn arena_align(self) -> usize {
        self.arena_align
    }

    #[inline(always)]
    pub(crate) const fn total_bytes(self) -> usize {
        self.total_bytes
    }

    #[inline(always)]
    pub(crate) const fn total_align(self) -> usize {
        self.total_align
    }
}

#[inline(always)]
const fn storage_align_up(value: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[inline(always)]
const fn storage_max(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}

#[inline]
pub(crate) const fn cursor_endpoint_storage_layout<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
>(
    arena_layout: &EndpointArenaLayout,
    lane_slot_count: usize,
) -> CursorEndpointStorageLayout
where
    T: Transport + 'r,
    U: LabelUniverse + 'r,
    C: crate::runtime::config::Clock + 'r,
    E: EpochTable + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    let header_bytes =
        core::mem::size_of::<CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>();
    let header_align =
        core::mem::align_of::<CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>();
    let port_slots_align = core::mem::align_of::<Option<Port<'r, T, E>>>();
    let port_slots_bytes =
        core::mem::size_of::<Option<Port<'r, T, E>>>().saturating_mul(lane_slot_count);
    let port_slots_offset = storage_align_up(header_bytes, port_slots_align);
    let guard_slots_align = core::mem::align_of::<Option<LaneGuard<'r, T, U, C>>>();
    let guard_slots_bytes =
        core::mem::size_of::<Option<LaneGuard<'r, T, U, C>>>().saturating_mul(lane_slot_count);
    let guard_slots_offset =
        storage_align_up(port_slots_offset + port_slots_bytes, guard_slots_align);
    let arena_offset = storage_align_up(
        guard_slots_offset + guard_slots_bytes,
        arena_layout.header_align(),
    );
    let total_align = storage_max(
        storage_max(
            storage_max(header_align, port_slots_align),
            guard_slots_align,
        ),
        arena_layout.header_align(),
    );
    CursorEndpointStorageLayout {
        header_bytes,
        header_align,
        port_slots_offset,
        port_slots_bytes,
        port_slots_align,
        guard_slots_offset,
        guard_slots_bytes,
        guard_slots_align,
        arena_offset,
        arena_bytes: arena_layout.total_bytes(),
        arena_align: arena_layout.total_align(),
        total_bytes: arena_offset + arena_layout.total_bytes(),
        total_align,
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
    #[inline(always)]
    pub(super) fn set_cursor_index(&mut self, idx: usize) {
        self.cursor.set_index(idx);
    }

    #[inline]
    pub(crate) fn stash_pending_branch_preview(
        &mut self,
        branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) {
        self.pending_branch_preview = Some(branch);
    }

    #[inline]
    pub(super) fn take_pending_branch_preview(
        &mut self,
    ) -> Option<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        self.pending_branch_preview.take()
    }

    #[inline]
    fn take_matching_pending_send_branch_preview(
        &mut self,
        meta: SendMeta,
    ) -> Option<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let preview = self.pending_branch_preview.as_ref().cloned()?;
        if preview.matches_send_meta(meta) {
            self.pending_branch_preview = None;
            Some(preview)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn control_semantics(&self) -> ControlSemanticsTable {
        self.cursor.control_semantics()
    }

    #[inline]
    pub(super) fn scope_trace(&self, scope: ScopeId) -> Option<ScopeTrace> {
        if scope.is_none() {
            return None;
        }
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| ScopeTrace::new(region.range, region.nest))
    }

    #[inline]
    pub(super) fn control_semantic_kind(
        &self,
        label: u8,
        resource: Option<u8>,
    ) -> ControlSemanticKind {
        self.control_semantics().semantic_for(label, resource)
    }

    #[inline]
    fn is_loop_semantic_label(&self, label: u8) -> bool {
        self.control_semantics().is_loop_label(label)
    }

    #[inline]
    fn loop_control_drop_label_mask(&self) -> u128 {
        let mut mask = 0u128;
        let mut label = 0u8;
        while label < u128::BITS as u8 {
            if self.is_loop_semantic_label(label) {
                mask |= ScopeLabelMeta::label_bit(label);
            }
            label += 1;
        }
        mask
    }

    /// Set route arm for (lane, scope) — update-in-place if exists, insert if not.
    ///
    /// Returns `Err(PhaseInvariant)` on capacity overflow or invalid lane.
    /// This prevents silent drops that could hide correctness bugs.
    pub(super) fn set_route_arm(
        &mut self,
        lane: u8,
        scope: ScopeId,
        arm: u8,
    ) -> Result<(), RecvError> {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return Ok(());
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return Err(RecvError::PhaseInvariant);
        }
        let is_linger = self.is_linger_route(scope);
        self.route_state
            .set_route_arm(lane_idx, scope, arm, is_linger)
            .map_err(|()| RecvError::PhaseInvariant)?;
        self.refresh_lane_offer_state(lane_idx);
        Ok(())
    }

    fn pop_route_arm(&mut self, lane: u8, scope: ScopeId) {
        if scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        debug_assert!(
            lane_idx < MAX_LANES,
            "pop_route_arm: lane {} exceeds MAX_LANES {}",
            lane_idx,
            MAX_LANES
        );
        if lane_idx >= MAX_LANES {
            return;
        }
        let is_linger = self.is_linger_route(scope);
        if self.route_state.pop_route_arm(lane_idx, scope, is_linger) {
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    fn scope_is_descendant_of(&self, scope: ScopeId, ancestor: ScopeId) -> bool {
        if scope.is_none() || ancestor.is_none() || scope == ancestor {
            return false;
        }
        let mut current = scope;
        while let Some(parent) = self.cursor.scope_parent(current) {
            if parent == ancestor {
                return true;
            }
            current = parent;
        }
        false
    }

    fn clear_descendant_route_state_for_lane(&mut self, lane: u8, ancestor_scope: ScopeId) {
        if ancestor_scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return;
        }
        if self.route_state.lane_route_arm_len(lane_idx) == 0 {
            return;
        }
        let mut stale_scopes = [ScopeId::none(); MAX_ROUTE_ARM_STACK];
        let mut stale_len =
            self.route_state
                .collect_lane_scopes(lane_idx, &mut stale_scopes, |scope| {
                    !scope.is_none()
                        && scope.kind() == ScopeKind::Route
                        && self.scope_is_descendant_of(scope, ancestor_scope)
                });
        while stale_len > 0 {
            stale_len -= 1;
            let scope = stale_scopes[stale_len];
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    fn prune_route_state_to_cursor_path_for_lane(&mut self, lane: u8) {
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return;
        }
        if self.route_state.lane_route_arm_len(lane_idx) == 0 {
            return;
        }
        let cursor_scope = self.cursor.node_scope_id();
        let mut stale_scopes = [ScopeId::none(); MAX_ROUTE_ARM_STACK];
        let mut stale_len =
            self.route_state
                .collect_lane_scopes(lane_idx, &mut stale_scopes, |scope| {
                    let keep = !scope.is_none()
                        && (scope == cursor_scope
                            || self.scope_is_descendant_of(cursor_scope, scope));
                    !keep && !scope.is_none()
                });
        while stale_len > 0 {
            stale_len -= 1;
            let scope = stale_scopes[stale_len];
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    pub(super) fn is_linger_route(&self, scope: ScopeId) -> bool {
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| {
                if region.kind == ScopeKind::Loop {
                    return true;
                }
                region.kind == ScopeKind::Route && region.linger
            })
            .unwrap_or(false)
    }

    pub(super) fn route_arm_for(&self, lane: u8, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        self.route_state.route_arm_for(lane_idx, scope)
    }

    pub(super) fn selected_arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(arm) = self.route_arm_for(lane_idx as u8, scope) {
                return Some(arm);
            }
            lane_idx += 1;
        }
        None
    }

    fn route_scope_offer_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
        let offer_entry = self.cursor.route_scope_offer_entry(scope_id)?;
        Some(if offer_entry.is_max() {
            self.cursor.index()
        } else {
            state_index_to_usize(offer_entry)
        })
    }

    fn route_scope_materialization_index(&self, scope_id: ScopeId) -> Option<usize> {
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
        {
            return Some(state_index_to_usize(offer_entry));
        }
        self.cursor
            .scope_region_by_id(scope_id)
            .map(|region| region.start)
    }

    fn preview_passive_materialization_index_for_selected_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        let mut scope = scope_id;
        let mut selected_arm = arm;
        let mut depth = 0usize;
        while depth < crate::eff::meta::MAX_EFF_NODES {
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope, selected_arm) {
                return Some(entry);
            }
            let PassiveArmNavigation::WithinArm { entry } = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope, selected_arm)?;
            let entry_idx = state_index_to_usize(entry);
            if self.cursor.is_recv_at(entry_idx)
                || self.cursor.is_send_at(entry_idx)
                || self.cursor.is_local_action_at(entry_idx)
                || self.cursor.is_jump_at(entry_idx)
            {
                return Some(entry_idx);
            }
            let child_scope = self
                .cursor
                .passive_arm_scope_by_arm(scope, selected_arm)
                .or_else(|| {
                    let node_scope = self.cursor.node_scope_id_at(entry_idx);
                    (node_scope != scope && node_scope.kind() == ScopeKind::Route)
                        .then_some(node_scope)
                })?;
            selected_arm = self.preview_selected_arm_for_scope(child_scope)?;
            scope = child_scope;
            depth += 1;
        }
        None
    }

    fn preview_selected_arm_for_scope(&self, scope_id: ScopeId) -> Option<u8> {
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
        if offer_lanes_len == 0 {
            return None;
        }
        let mut offer_lane_mask = 0u8;
        let mut lane_idx = 0usize;
        while lane_idx < offer_lanes_len {
            let lane = offer_lanes[lane_idx] as usize;
            if lane < MAX_LANES {
                offer_lane_mask |= 1u8 << lane;
            }
            lane_idx += 1;
        }
        self.preview_scope_ack_token_non_consuming(
            scope_id,
            offer_lanes[0] as usize,
            offer_lane_mask,
        )
        .map(|token| token.arm().as_u8())
        .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    fn scope_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_offer_entry_index(scope_id).or_else(|| {
            self.cursor
                .scope_region_by_id(scope_id)
                .map(|region| region.start)
        })
    }

    fn scope_within_parent_arm(
        &self,
        child_scope: ScopeId,
        parent_scope: ScopeId,
        parent_arm: u8,
    ) -> bool {
        let Some(child_entry_idx) = self.scope_entry_index(child_scope) else {
            return false;
        };
        let arm_start = if let Some((entry, _)) = self
            .cursor
            .controller_arm_entry_by_arm(parent_scope, parent_arm)
        {
            state_index_to_usize(entry)
        } else if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(parent_scope, parent_arm)
        {
            state_index_to_usize(entry)
        } else if let Some(entry) = self
            .cursor
            .route_scope_arm_recv_index(parent_scope, parent_arm)
        {
            entry
        } else {
            return false;
        };
        if child_entry_idx < arm_start {
            return false;
        }
        let sibling_arm = if parent_arm == 0 { 1 } else { 0 };
        if let Some((entry, _)) = self
            .cursor
            .controller_arm_entry_by_arm(parent_scope, sibling_arm)
        {
            let sibling_start = state_index_to_usize(entry);
            if parent_arm == 0 {
                return child_entry_idx < sibling_start;
            }
            return child_entry_idx >= arm_start;
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(parent_scope, sibling_arm)
        {
            let sibling_start = state_index_to_usize(entry);
            if parent_arm == 0 {
                return child_entry_idx < sibling_start;
            }
        }
        true
    }

    fn structural_arm_for_child_scope(
        &self,
        parent_scope: ScopeId,
        child_scope: ScopeId,
    ) -> Option<u8> {
        let child_in_arm0 = self.scope_within_parent_arm(child_scope, parent_scope, 0);
        let child_in_arm1 = self.scope_within_parent_arm(child_scope, parent_scope, 1);
        match (child_in_arm0, child_in_arm1) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            _ => None,
        }
    }

    #[inline]
    fn current_offer_scope_id(&self) -> ScopeId {
        let node_scope = self.cursor.node_scope_id();
        if node_scope.is_none() {
            return node_scope;
        }
        let mut child_scope = node_scope;
        while let Some(parent_scope) = self.cursor.scope_parent(child_scope) {
            if parent_scope.kind() != ScopeKind::Route {
                child_scope = parent_scope;
                continue;
            }
            let child_selected_arm = self.selected_arm_for_scope(child_scope);
            let Some(parent_arm) = self
                .selected_arm_for_scope(parent_scope)
                .or_else(|| {
                    // Once we have descended into a selected child route, the
                    // ancestor arm is derivable from the structural placement
                    // of that child. Do not invent ancestor authority before
                    // the child itself has become selected.
                    if child_selected_arm.is_some() {
                        self.structural_arm_for_child_scope(parent_scope, child_scope)
                    } else {
                        None
                    }
                })
                .or_else(|| self.preview_selected_arm_for_scope(parent_scope))
            else {
                return parent_scope;
            };
            if !self.scope_within_parent_arm(child_scope, parent_scope, parent_arm) {
                return parent_scope;
            }
            child_scope = parent_scope;
        }
        node_scope
    }

    fn rebase_passive_descendant_scope(
        &self,
        stop_scope: ScopeId,
        initial_scope: ScopeId,
    ) -> ScopeId {
        let mut target_scope = initial_scope;
        let mut attempts = 0usize;
        'rebase: while attempts < crate::eff::meta::MAX_EFF_NODES {
            let mut child_scope = target_scope;
            let mut depth = 0usize;
            while depth < crate::eff::meta::MAX_EFF_NODES {
                let Some(parent_scope) = self.cursor.scope_parent(child_scope) else {
                    break 'rebase;
                };
                if parent_scope == stop_scope {
                    break 'rebase;
                }
                if parent_scope.kind() == ScopeKind::Route
                    && let Some(parent_arm) = self
                        .selected_arm_for_scope(parent_scope)
                        .or_else(|| self.preview_selected_arm_for_scope(parent_scope))
                    && !self.scope_within_parent_arm(child_scope, parent_scope, parent_arm)
                {
                    if let Some(scope) = self
                        .cursor
                        .passive_arm_scope_by_arm(parent_scope, parent_arm)
                        && scope != child_scope
                    {
                        target_scope = scope;
                        attempts += 1;
                        continue 'rebase;
                    }
                    if let Some(entry_idx) = self
                        .preview_passive_materialization_index_for_selected_arm(
                            parent_scope,
                            parent_arm,
                        )
                    {
                        let scope = self.cursor.node_scope_id_at(entry_idx);
                        if scope.kind() == ScopeKind::Route
                            && scope != parent_scope
                            && scope != child_scope
                        {
                            target_scope = scope;
                            attempts += 1;
                            continue 'rebase;
                        }
                    }
                    break 'rebase;
                }
                child_scope = parent_scope;
                depth += 1;
            }
            break;
        }
        target_scope
    }

    fn ensure_current_route_arm_state(&mut self) -> RecvResult<Option<bool>> {
        let Some(region) = self.cursor.scope_region() else {
            return Ok(None);
        };
        if region.kind != ScopeKind::Route {
            return Ok(None);
        }
        let Some(current_arm) = self.cursor.typestate_node(self.cursor.index()).route_arm() else {
            return Ok(None);
        };
        if let Some(selected_arm) = self.selected_arm_for_scope(region.scope_id) {
            return Ok((selected_arm == current_arm).then_some(false));
        }
        let lane = self.offer_lane_for_scope(region.scope_id);
        self.set_route_arm(lane, region.scope_id, current_arm)?;
        Ok(Some(true))
    }

    #[inline]
    pub(super) fn endpoint_policy_args(lane: Lane, label: u8, flags: FrameFlags) -> u32 {
        ((ROLE as u32) << 24)
            | ((lane.as_wire() as u32) << 16)
            | ((label as u32) << 8)
            | flags.bits() as u32
    }

    /// Emit a policy-layer tap event associated with this endpoint.
    ///
    /// The event is tagged with the current lane and session so downstream
    /// tap inspection can attribute `POLICY_*` events to the correct
    /// rendezvous lane. Use this for recording resolver / EPF decisions such
    /// as `policy_effect`, `policy_trap`, or `policy_abort`.
    #[inline]
    fn emit_policy_event(&self, id: u16, arg0: u32, arg1: u32, scope: ScopeId, lane: Lane) {
        let port = self.port_for_lane(lane.raw() as usize);
        let causal = {
            let raw = lane.raw();
            debug_assert!(
                raw <= u32::from(u8::MAX),
                "lane id must fit within causal key encoding"
            );
            TapEvent::make_causal_key(raw as u8 + 1, 0)
        };
        let mut event = events::RawEvent::new(port.now32(), id)
            .with_causal_key(causal)
            .with_arg0(arg0)
            .with_arg1(arg1);
        if let Some(trace) = self.scope_trace(scope) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    #[inline]
    fn emit_policy_audit_event(&self, id: u16, arg0: u32, arg1: u32, arg2: u32, lane: Lane) {
        let port = self.port_for_lane(lane.raw() as usize);
        let causal = {
            let raw = lane.raw();
            debug_assert!(
                raw <= u32::from(u8::MAX),
                "lane id must fit within causal key encoding"
            );
            TapEvent::make_causal_key(raw as u8 + 1, 0)
        };
        let event = events::RawEvent::new(port.now32(), id)
            .with_causal_key(causal)
            .with_arg0(arg0)
            .with_arg1(arg1)
            .with_arg2(arg2);
        emit(port.tap(), event);
    }

    #[inline]
    fn emit_policy_defer_event(
        &self,
        source: DeferSource,
        reason: DeferReason,
        scope_id: ScopeId,
        frontier: FrontierKind,
        selected_arm: Option<u8>,
        hint: Option<u8>,
        retry_hint: u8,
        liveness: OfferLivenessState,
        ready_arm_mask: u8,
        binding_ready: bool,
        exhausted: bool,
        lane: u8,
    ) {
        let source_tag = match source {
            DeferSource::Epf => 1u32,
            DeferSource::Resolver => 2u32,
        };
        let scope_slot = self
            .scope_slot_for_route(scope_id)
            .and_then(|slot| u16::try_from(slot).ok())
            .unwrap_or(u16::MAX) as u32;
        let arm = selected_arm.unwrap_or(u8::MAX) as u32;
        let hint = hint.unwrap_or(0) as u32;
        let arg0 =
            (source_tag << 24) | ((retry_hint as u32) << 16) | (liveness.remaining_defer as u32);
        let arg1 = (scope_slot << 16) | (arm << 8) | (ready_arm_mask as u32);
        let arg2 = ((reason as u32) << 16)
            | (hint << 8)
            | ((frontier.as_audit_tag() as u32) << 4)
            | ((u32::from(binding_ready)) << 1)
            | u32::from(exhausted);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_DEFER,
            arg0,
            arg1,
            arg2,
            Lane::new(lane as u32),
        );
    }

    pub(super) fn emit_endpoint_event(
        &self,
        id: u16,
        meta: TapFrameMeta,
        scope_trace: Option<ScopeTrace>,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let packed = ((ROLE as u32) << 24)
            | ((meta.lane as u32) << 16)
            | ((meta.label as u32) << 8)
            | meta.flags.bits() as u32;
        let mut event = events::RawEvent::new(port.now32(), id)
            .with_arg0(meta.sid)
            .with_arg1(packed);
        if let Some(scope) = scope_trace {
            event = event.with_arg2(scope.pack());
        }
        emit(port.tap(), event);
    }

    pub(super) fn eval_endpoint_policy(
        &self,
        slot: Slot,
        event_id: u16,
        arg0: u32,
        arg1: u32,
        lane: Lane,
    ) -> Action {
        let port = self.port_for_lane(lane.raw() as usize);
        let event = events::RawEvent::new(port.now32(), event_id)
            .with_arg0(arg0)
            .with_arg1(arg1);
        let _ = port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let signals = self.policy_signals_for_slot(slot);
        let policy_input = signals.input;
        let policy_digest = port.policy_digest(slot);
        let event_hash = epf::hash_tap_event(&event);
        let signals_input_hash = epf::hash_policy_input(policy_input);
        let signals_attrs_hash = signals.attrs().hash32();
        let transport_snapshot_hash = epf::hash_transport_snapshot(transport_metrics);
        let replay_transport = epf::replay_transport_inputs(transport_metrics);
        let replay_transport_presence = epf::replay_transport_presence(transport_metrics);
        let slot_id = epf::slot_tag(slot);
        let mode_id = epf::policy_mode_tag(port.policy_mode(slot));
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            signals_attrs_hash,
            transport_snapshot_hash,
            ((slot_id as u32) << 24) | ((mode_id as u32) << 16),
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_input[0],
            policy_input[1],
            policy_input[2],
            lane,
        );
        self.emit_policy_audit_event(ids::POLICY_REPLAY_INPUT1, policy_input[3], 0, 0, lane);
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT0,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT1,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
            lane,
        );
        let action = port.run_policy(
            slot,
            &event,
            port.caps_mask(),
            Some(self.sid),
            Some(lane),
            move |ctx| {
                ctx.set_transport_snapshot(transport_metrics);
                ctx.set_policy_input(policy_input);
            },
        );
        let verdict = action.verdict();
        let verdict_meta =
            ((epf::verdict_tag(verdict) as u32) << 24) | ((epf::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            epf::verdict_reason(verdict) as u32,
            port.last_policy_fuel_used(slot) as u32,
            lane,
        );
        action
    }

    fn apply_send_policy(&self, action: Action, scope: ScopeId, lane: Lane) -> SendResult<()> {
        if let Some((id, arg0, arg1)) = action.tap_payload() {
            self.emit_policy_event(id, arg0, arg1, scope, lane);
        }

        match action.verdict() {
            epf::PolicyVerdict::Proceed | epf::PolicyVerdict::RouteArm(_) => Ok(()),
            epf::PolicyVerdict::Reject(reason) => {
                if let Some(info) = action.abort_info() {
                    return Err(self.policy_abort_send(info, scope, lane));
                }
                self.emit_policy_event(policy_trap(), reason as u32, self.sid.raw(), scope, lane);
                self.emit_policy_event(policy_abort(), reason as u32, self.sid.raw(), scope, lane);
                Err(SendError::PolicyAbort { reason })
            }
        }
    }

    fn policy_abort_send(&self, info: AbortInfo, scope: ScopeId, lane: Lane) -> SendError {
        if info.trap.is_some() {
            self.emit_policy_event(
                policy_trap(),
                info.reason as u32,
                self.sid.raw(),
                scope,
                lane,
            );
        }
        self.emit_policy_event(
            policy_abort(),
            info.reason as u32,
            self.sid.raw(),
            scope,
            lane,
        );
        SendError::PolicyAbort {
            reason: info.reason,
        }
    }

    pub(super) fn apply_recv_policy(
        &self,
        action: Action,
        scope: ScopeId,
        lane: Lane,
    ) -> RecvResult<()> {
        if let Some((id, arg0, arg1)) = action.tap_payload() {
            self.emit_policy_event(id, arg0, arg1, scope, lane);
        }

        match action.verdict() {
            epf::PolicyVerdict::Proceed | epf::PolicyVerdict::RouteArm(_) => Ok(()),
            epf::PolicyVerdict::Reject(reason) => {
                if let Some(info) = action.abort_info() {
                    return Err(self.policy_abort_recv(info, scope, lane));
                }
                self.emit_policy_event(policy_trap(), reason as u32, self.sid.raw(), scope, lane);
                self.emit_policy_event(policy_abort(), reason as u32, self.sid.raw(), scope, lane);
                Err(RecvError::PolicyAbort { reason })
            }
        }
    }

    fn policy_abort_recv(&self, info: AbortInfo, scope: ScopeId, lane: Lane) -> RecvError {
        if info.trap.is_some() {
            self.emit_policy_event(
                policy_trap(),
                info.reason as u32,
                self.sid.raw(),
                scope,
                lane,
            );
        }
        self.emit_policy_event(
            policy_abort(),
            info.reason as u32,
            self.sid.raw(),
            scope,
            lane,
        );
        RecvError::PolicyAbort {
            reason: info.reason,
        }
    }

    #[inline]
    fn preview_scope_region_at(&self, idx: usize) -> Option<crate::global::typestate::ScopeRegion> {
        let scope_id = self.cursor.node_scope_id_at(idx);
        if scope_id.is_none() {
            None
        } else {
            self.cursor.scope_region_by_id(scope_id)
        }
    }

    #[inline]
    fn preview_is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: usize) -> bool {
        let mut arm = 0u8;
        while arm <= 1 {
            if self
                .cursor
                .controller_arm_entry_by_arm(scope_id, arm)
                .map(|(entry, _)| state_index_to_usize(entry) == idx)
                .unwrap_or(false)
            {
                return true;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        false
    }

    fn preview_follow_jumps_from(&self, mut idx: usize) -> SendResult<usize> {
        let mut flow_iter = 0u32;
        while self.cursor.is_jump_at(idx) {
            if self.cursor.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch) {
                break;
            }
            idx = state_index_to_usize(self.cursor.typestate_node(idx).next());
            flow_iter += 1;
            if flow_iter > crate::eff::meta::MAX_EFF_NODES as u32 {
                return Err(SendError::PhaseInvariant);
            }
        }
        Ok(idx)
    }

    fn preview_find_arm_for_send_label_in_scope(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> Option<u8> {
        let mut arm = 0u8;
        while arm <= 1 {
            let Some(PassiveArmNavigation::WithinArm { entry }) = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope_id, arm)
            else {
                if arm == 1 {
                    break;
                }
                arm += 1;
                continue;
            };
            let entry_idx = state_index_to_usize(entry);
            let matches = self
                .cursor
                .try_send_meta_at(entry_idx)
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
                || self
                    .cursor
                    .try_local_meta_at(entry_idx)
                    .map(|meta| meta.label == target_label)
                    .unwrap_or(false);
            if matches {
                return Some(arm);
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    fn preview_follow_passive_observer_for_label(
        &self,
        idx: usize,
        target_label: u8,
    ) -> Option<usize> {
        let scope_id = self.cursor.node_scope_id_at(idx);
        let target_arm = self.preview_find_arm_for_send_label_in_scope(scope_id, target_label)?;
        match self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, target_arm)?
        {
            PassiveArmNavigation::WithinArm { entry } => Some(state_index_to_usize(entry)),
        }
    }

    #[inline]
    fn preview_route_arm_for(
        &self,
        lane: u8,
        scope: ScopeId,
        preview_route_arm: Option<(u8, ScopeId, u8)>,
    ) -> Option<u8> {
        if let Some((preview_lane, preview_scope, preview_arm)) = preview_route_arm
            && preview_lane == lane
            && preview_scope == scope
        {
            return Some(preview_arm);
        }
        self.route_arm_for(lane, scope)
    }

    fn preview_selected_arm_for_scope_with_route(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<(u8, ScopeId, u8)>,
    ) -> Option<u8> {
        if scope_id.is_none() {
            return None;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(arm) =
                self.preview_route_arm_for(lane_idx as u8, scope_id, preview_route_arm)
            {
                return Some(arm);
            }
            lane_idx += 1;
        }
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
        if offer_lanes_len == 0 {
            return None;
        }
        let mut offer_lane_mask = 0u8;
        let mut offer_lane_idx = 0usize;
        while offer_lane_idx < offer_lanes_len {
            let lane = offer_lanes[offer_lane_idx] as usize;
            if lane < MAX_LANES {
                offer_lane_mask |= 1u8 << lane;
            }
            offer_lane_idx += 1;
        }
        self.preview_scope_ack_token_non_consuming(
            scope_id,
            offer_lanes[0] as usize,
            offer_lane_mask,
        )
        .map(|token| token.arm().as_u8())
        .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    #[inline]
    fn preview_can_advance_route_scope(
        &self,
        scope_id: ScopeId,
        target_label: u8,
        preview_route_arm: Option<(u8, ScopeId, u8)>,
    ) -> bool {
        let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
        self.preview_route_arm_for(lane_wire, scope_id, preview_route_arm)
            .is_some()
    }

    #[inline]
    fn preview_flow_start_index(&self, target_label: u8) -> usize {
        if self
            .cursor
            .try_recv_meta()
            .map(|meta| meta.label == target_label)
            .unwrap_or(false)
            || self
                .cursor
                .try_send_meta()
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
            || self
                .cursor
                .try_local_meta()
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
        {
            return self.cursor.index();
        }
        if let Some(region) = self.cursor.scope_region()
            && region.kind == ScopeKind::Route
            && self.cursor.is_route_controller(region.scope_id)
            && self
                .cursor
                .controller_arm_entry_for_label(region.scope_id, target_label)
                .is_some()
        {
            return self.cursor.index();
        }
        if let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label)
            && let Some(idx) = self.cursor.index_for_lane_step(lane_idx)
        {
            return idx;
        }
        self.cursor.index()
    }

    /// Preview the current send transition without mutating endpoint state.
    pub(super) fn preview_flow_meta<M>(
        &mut self,
    ) -> SendResult<crate::endpoint::kernel::SendPreview>
    where
        M: MessageSpec + SendableLabel,
        T: Transport + 'r,
        U: LabelUniverse,
        C: Clock,
        E: EpochTable,
        Mint: MintConfigMarker,
    {
        let target_label = <M as MessageSpec>::LABEL;
        let mut idx = self.preview_flow_start_index(target_label);
        let mut preview_route_arm: Option<(u8, ScopeId, u8)> = None;

        if let Some(region) = self.preview_scope_region_at(idx) {
            if region.kind == ScopeKind::Route {
                let scope_id = region.scope_id;
                let at_route_start = idx == region.start;
                let unlabeled = !self.cursor.is_send_at(idx)
                    && !self.cursor.is_recv_at(idx)
                    && !self.cursor.is_local_action_at(idx);
                let at_decision = at_route_start || unlabeled || self.cursor.is_jump_at(idx);

                if region.linger && self.cursor.is_jump_at(idx) {
                    idx = self.preview_follow_jumps_from(idx)?;
                }

                if self.cursor.is_route_controller(scope_id) {
                    let at_arm_entry = self.preview_is_at_controller_arm_entry(scope_id, idx);
                    let at_decision = at_arm_entry || at_decision;
                    if at_decision {
                        if let Some(entry_idx) = self
                            .cursor
                            .controller_arm_entry_for_label(scope_id, target_label)
                        {
                            idx = state_index_to_usize(entry_idx);
                        }
                    }
                } else if at_decision {
                    let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
                    let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
                    let preview_arm = if offer_lanes_len == 0 {
                        None
                    } else {
                        let mut offer_lane_mask = 0u8;
                        let mut lane_idx = 0usize;
                        while lane_idx < offer_lanes_len {
                            let lane = offer_lanes[lane_idx] as usize;
                            if lane < MAX_LANES {
                                offer_lane_mask |= 1u8 << lane;
                            }
                            lane_idx += 1;
                        }
                        self.preview_scope_ack_token_non_consuming(
                            scope_id,
                            offer_lanes[0] as usize,
                            offer_lane_mask,
                        )
                        .map(|token| token.arm().as_u8())
                    };
                    let selected_arm = preview_arm
                        .or_else(|| {
                            self.preview_selected_arm_for_scope_with_route(
                                scope_id,
                                preview_route_arm,
                            )
                        })
                        .or_else(|| {
                            self.preview_route_arm_for(lane_wire, scope_id, preview_route_arm)
                        });
                    if let Some(selected_arm) = selected_arm {
                        preview_route_arm = Some((lane_wire, scope_id, selected_arm));
                        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
                            .cursor
                            .follow_passive_observer_arm_for_scope(scope_id, selected_arm)
                        {
                            idx = state_index_to_usize(entry);
                        }
                    }
                }
            }
        }

        let mut flow_iter = 0u32;
        loop {
            flow_iter += 1;
            debug_assert!(
                flow_iter <= crate::eff::meta::MAX_EFF_NODES as u32,
                "flow(): exceeded MAX_EFF_NODES iterations - CFG cycle bug"
            );
            if flow_iter > crate::eff::meta::MAX_EFF_NODES as u32 {
                return Err(SendError::PhaseInvariant);
            }

            idx = self.preview_follow_jumps_from(idx)?;

            if self.cursor.is_jump_at(idx)
                && self.cursor.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch)
                && let Some(next_idx) =
                    self.preview_follow_passive_observer_for_label(idx, target_label)
            {
                idx = next_idx;
                continue;
            }

            if !self.cursor.is_send_at(idx) && !self.cursor.is_local_action_at(idx) {
                if let Some(region) = self.preview_scope_region_at(idx)
                    && region.kind == ScopeKind::Route
                    && self.preview_can_advance_route_scope(
                        region.scope_id,
                        target_label,
                        preview_route_arm,
                    )
                {
                    idx = region.end;
                    continue;
                }
                return Err(SendError::PhaseInvariant);
            }

            let current_meta = if self.cursor.is_local_action_at(idx) {
                let local = self
                    .cursor
                    .try_local_meta_at(idx)
                    .ok_or(SendError::PhaseInvariant)?;
                SendMeta::new(
                    local.eff_index,
                    ROLE,
                    local.label,
                    local.resource,
                    local.is_control,
                    local.next,
                    local.scope,
                    local.route_arm,
                    local.shot,
                    local.policy,
                    local.lane,
                )
            } else {
                self.cursor
                    .try_send_meta_at(idx)
                    .ok_or(SendError::PhaseInvariant)?
            };

            if current_meta.label == target_label {
                return Ok(crate::endpoint::kernel::SendPreview::new(
                    current_meta,
                    checked_state_index(idx).ok_or(SendError::PhaseInvariant)?,
                ));
            }

            if let Some(region) = self.preview_scope_region_at(idx)
                && region.kind == ScopeKind::Route
                && self.preview_can_advance_route_scope(
                    region.scope_id,
                    target_label,
                    preview_route_arm,
                )
            {
                idx = region.end;
                continue;
            }

            return Err(SendError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            });
        }
    }

    fn evaluate_dynamic_policy(&mut self, meta: &SendMeta, target_label: u8) -> SendResult<()> {
        if !meta.policy().is_dynamic() {
            return Ok(());
        }
        let dynamic_kind = self.control_semantic_kind(target_label, meta.resource);
        if is_splice_or_reroute_semantic(dynamic_kind) {
            return Ok(());
        }
        let route_signals = self.policy_signals_for_slot(Slot::Route).into_owned();
        match dynamic_kind {
            ControlSemanticKind::LoopContinue | ControlSemanticKind::LoopBreak => {
                self.evaluate_loop_policy(meta, &route_signals)
            }
            ControlSemanticKind::RouteArm | ControlSemanticKind::Other => {
                self.evaluate_route_policy(meta, target_label, &route_signals)
            }
            ControlSemanticKind::SpliceIntent
            | ControlSemanticKind::SpliceAck
            | ControlSemanticKind::Reroute => Ok(()),
        }
    }

    fn evaluate_route_arm_from_epf(
        &self,
        scope_id: ScopeId,
        lane: u8,
        policy_id: u16,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> RoutePolicyDecision {
        if scope_id.is_none() {
            return RoutePolicyDecision::Abort(policy_id);
        }
        let port = self.port_for_lane(lane as usize);
        let _ = port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let policy_input = signals.input;
        let arg0 = route_policy_input_arg0(&policy_input);
        let mut event = events::RawEvent::new(port.now32(), ids::ROUTE_DECISION)
            .with_arg0(arg0)
            .with_arg1(policy_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let policy_digest = port.policy_digest(Slot::Route);
        let event_hash = epf::hash_tap_event(&event);
        let signals_input_hash = epf::hash_policy_input(policy_input);
        let signals_attrs_hash = signals.attrs().hash32();
        let transport_snapshot_hash = epf::hash_transport_snapshot(transport_metrics);
        let replay_transport = epf::replay_transport_inputs(transport_metrics);
        let replay_transport_presence = epf::replay_transport_presence(transport_metrics);
        let mode_id = epf::policy_mode_tag(port.policy_mode(Slot::Route));
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            signals_attrs_hash,
            transport_snapshot_hash,
            ((epf::slot_tag(Slot::Route) as u32) << 24) | ((mode_id as u32) << 16),
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_input[0],
            policy_input[1],
            policy_input[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT1,
            policy_input[3],
            0,
            0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT0,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT1,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
            port.lane(),
        );
        let action = port.run_policy(
            Slot::Route,
            &event,
            port.caps_mask(),
            Some(self.sid),
            Some(port.lane()),
            move |ctx| {
                ctx.set_transport_snapshot(transport_metrics);
                ctx.set_policy_input(policy_input);
            },
        );
        let verdict = action.verdict();
        let verdict_meta =
            ((epf::verdict_tag(verdict) as u32) << 24) | ((epf::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            epf::verdict_reason(verdict) as u32,
            port.last_policy_fuel_used(Slot::Route) as u32,
            port.lane(),
        );
        route_policy_decision_from_action(action, policy_id)
    }

    fn evaluate_route_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        // Route decisions are fixed at the offer/decode decision point.
        // Re-evaluating dynamic route policy for local self-send can diverge from
        // the selected arm and introduce non-deterministic PolicyAbort.
        if meta.peer == ROLE {
            return Ok(());
        }

        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match self.evaluate_route_arm_from_epf(scope_id, meta.lane, policy_id, signals) {
            RoutePolicyDecision::RouteArm(arm) => {
                return if arm == arm_index {
                    Ok(())
                } else {
                    Err(SendError::PolicyAbort { reason: policy_id })
                };
            }
            RoutePolicyDecision::Abort(reason) => {
                return Err(SendError::PolicyAbort { reason });
            }
            RoutePolicyDecision::DelegateResolver | RoutePolicyDecision::Defer { .. } => {}
        }

        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs();
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                metrics,
                signals.input,
                attrs,
            )
            .map_err(Self::map_cp_error)?;

        match resolution {
            DynamicResolution::RouteArm { arm } if arm == arm_index => Ok(()),
            DynamicResolution::RouteArm { .. } => Err(SendError::PolicyAbort { reason: policy_id }),
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    fn evaluate_loop_policy(
        &mut self,
        meta: &SendMeta,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        // For CanonicalControl (self-send), the caller explicitly chooses continue/break.
        // No resolver validation is needed - the caller's choice is authoritative.
        if meta.peer == ROLE {
            return Ok(());
        }

        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;

        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs();
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                metrics,
                signals.input,
                attrs,
            )
            .map_err(Self::map_cp_error)?;

        if meta.scope.is_none() || meta.scope != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match resolution {
            DynamicResolution::Loop { decision } => {
                let disposition = if decision {
                    LoopDisposition::Continue
                } else {
                    LoopDisposition::Break
                };
                if !loop_control_kind_matches_disposition(
                    &self.control_semantics(),
                    meta.label,
                    meta.resource,
                    disposition,
                ) {
                    return Err(SendError::PolicyAbort { reason: policy_id });
                }
                Ok(())
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    /// Preview recv metadata from a precomputed route-arm entry table.
    fn select_cached_route_arm_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        target_arm: u8,
    ) -> CachedRecvMeta {
        let Some(recv_entry) = materialization_meta.recv_entry(target_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        let idx = state_index_to_usize(recv_entry);
        let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
            return CachedRecvMeta::EMPTY;
        };
        Self::cached_recv_meta_from_recv(idx, meta, Some(target_arm))
    }

    #[inline]
    fn cached_recv_meta_from_recv(
        cursor_index: usize,
        mut meta: RecvMeta,
        route_arm: Option<u8>,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(meta.next) else {
            return CachedRecvMeta::EMPTY;
        };
        if let Some(route_arm) = route_arm {
            meta.route_arm = Some(route_arm);
        }
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            resource: meta.resource,
            is_control: meta.is_control,
            next,
            scope: meta.scope,
            route_arm: meta.route_arm.unwrap_or(u8::MAX),
            is_choice_determinant: meta.is_choice_determinant,
            shot: meta.shot,
            policy: meta.policy,
            lane: meta.lane,
            flags: CachedRecvMeta::FLAG_RECV_STEP,
        }
    }

    #[inline]
    fn cached_recv_meta_from_send(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        meta: SendMeta,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(meta.next) else {
            return CachedRecvMeta::EMPTY;
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            resource: meta.resource,
            is_control: meta.is_control,
            next,
            scope: scope_id,
            route_arm,
            is_choice_determinant: false,
            shot: meta.shot,
            policy: meta.policy(),
            lane: meta.lane,
            flags: 0,
        }
    }

    #[inline]
    fn cached_recv_meta_from_local(
        cursor_index: usize,
        route_arm: u8,
        meta: crate::global::typestate::LocalMeta,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(meta.next) else {
            return CachedRecvMeta::EMPTY;
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: ROLE,
            label: meta.label,
            resource: meta.resource,
            is_control: meta.is_control,
            next,
            scope: meta.scope,
            route_arm,
            is_choice_determinant: false,
            shot: meta.shot,
            policy: meta.policy,
            lane: meta.lane,
            flags: 0,
        }
    }

    #[inline]
    fn synthetic_cached_recv_meta(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        label: u8,
        next: usize,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(next) else {
            return CachedRecvMeta::EMPTY;
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: EffIndex::ZERO,
            peer: ROLE,
            label,
            resource: None,
            is_control: true,
            next,
            scope: scope_id,
            route_arm,
            is_choice_determinant: false,
            shot: None,
            policy: PolicyMode::static_mode(),
            lane,
            flags: 0,
        }
    }

    #[inline]
    fn synthetic_cached_recv_meta_for_arm(
        &self,
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        next: usize,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(label) = controller_arm_label(&self.cursor, scope_id, route_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        Self::synthetic_cached_recv_meta(cursor_index, scope_id, route_arm, label, next, lane)
    }

    fn compute_passive_arm_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        scope_id: ScopeId,
        target_arm: u8,
        offer_lane: u8,
    ) -> CachedRecvMeta {
        let Some(entry) = materialization_meta.passive_arm_entry(target_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        let entry_idx = state_index_to_usize(entry);
        if let Some(recv_meta) = self.cursor.try_recv_meta_at(entry_idx) {
            return Self::cached_recv_meta_from_recv(entry_idx, recv_meta, None);
        }
        if let Some(send_meta) = self.cursor.try_send_meta_at(entry_idx) {
            return Self::cached_recv_meta_from_send(entry_idx, scope_id, target_arm, send_meta);
        }
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return CachedRecvMeta::EMPTY;
        };
        if self.cursor.is_jump_at(entry_idx) {
            let Some(scope_end) = self.cursor.jump_target_at(entry_idx) else {
                return CachedRecvMeta::EMPTY;
            };
            if region.linger {
                return self.synthetic_cached_recv_meta_for_arm(
                    scope_end, scope_id, target_arm, scope_end, offer_lane,
                );
            }
            if let Some(recv_meta) = self.cursor.try_recv_meta_at(scope_end) {
                return Self::cached_recv_meta_from_recv(scope_end, recv_meta, None);
            }
            if let Some(send_meta) = self.cursor.try_send_meta_at(scope_end) {
                return Self::cached_recv_meta_from_send(
                    scope_end, scope_id, target_arm, send_meta,
                );
            }
            return CachedRecvMeta::EMPTY;
        }
        if region.linger {
            return self.synthetic_cached_recv_meta_for_arm(
                entry_idx, scope_id, target_arm, entry_idx, offer_lane,
            );
        }
        if let Some(target_idx) =
            self.preview_passive_materialization_index_for_selected_arm(scope_id, target_arm)
        {
            if let Some(recv_meta) = self.cursor.try_recv_meta_at(target_idx) {
                return Self::cached_recv_meta_from_recv(target_idx, recv_meta, Some(target_arm));
            }
            if let Some(send_meta) = self.cursor.try_send_meta_at(target_idx) {
                return Self::cached_recv_meta_from_send(
                    target_idx, scope_id, target_arm, send_meta,
                );
            }
        }
        CachedRecvMeta::EMPTY
    }

    #[inline]
    fn compute_scope_passive_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        scope_id: ScopeId,
        offer_lane: u8,
    ) -> [CachedRecvMeta; 2] {
        [
            self.compute_passive_arm_recv_meta(materialization_meta, scope_id, 0, offer_lane),
            self.compute_passive_arm_recv_meta(materialization_meta, scope_id, 1, offer_lane),
        ]
    }

    #[inline]
    fn selection_arm_has_recv(&self, selection: OfferScopeSelection, arm: u8) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        materialization_meta.recv_entry(arm).is_some()
            || materialization_meta.controller_arm_is_recv(arm)
            || materialization_meta.arm_has_first_recv_dispatch(arm)
            || passive_recv_meta
                .get(arm as usize)
                .copied()
                .map(|meta| meta.is_recv_step())
                .unwrap_or(false)
    }

    #[inline]
    fn selection_arm_requires_materialization_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
    ) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        if is_route_controller && selection.at_route_offer_entry {
            if materialization_meta.controller_arm_entry(arm).is_some() {
                return materialization_meta.controller_arm_requires_ready_evidence(arm);
            }
        }
        if selection.at_route_offer_entry && materialization_meta.passive_arm_entry(arm).is_some() {
            if materialization_meta.arm_has_first_recv_dispatch(arm) {
                return !self
                    .selection_arm_dispatch_materializes_without_ready_evidence(selection, arm);
            }
            return false;
        }
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let Some(passive_meta) = passive_recv_meta.get(arm as usize).copied() else {
            return materialization_meta.recv_entry(arm).is_some();
        };
        if passive_meta.is_recv_step() {
            if passive_meta.peer == ROLE {
                return false;
            }
            if passive_meta.is_control {
                if materialization_meta
                    .controller_arm_entry(arm)
                    .map(|(_, label)| label)
                    == Some(passive_meta.label)
                {
                    return false;
                }
                if !is_route_controller
                    && self
                        .control_semantic_kind(passive_meta.label, passive_meta.resource)
                        .is_loop()
                {
                    return false;
                }
            }
            return true;
        }
        materialization_meta.recv_entry(arm).is_some()
    }

    #[inline]
    fn selection_arm_dispatch_materializes_without_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        arm: u8,
    ) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(entry) = materialization_meta.passive_arm_entry(arm) else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        if self.cursor.is_recv_at(entry_idx)
            || self.cursor.is_send_at(entry_idx)
            || self.cursor.is_local_action_at(entry_idx)
            || self.cursor.is_jump_at(entry_idx)
        {
            return true;
        }
        materialization_meta
            .passive_arm_scope(arm)
            .or_else(|| {
                let scope = self.cursor.node_scope_id_at(entry_idx);
                (scope != selection.scope_id && scope.kind() == ScopeKind::Route).then_some(scope)
            })
            .filter(|scope| scope.kind() == ScopeKind::Route)
            .and_then(|scope| self.preview_selected_arm_for_scope(scope))
            .is_some()
    }

    #[inline]
    fn selection_non_wire_loop_control_recv(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
        label: u8,
    ) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let Some(passive_meta) = passive_recv_meta.get(arm as usize).copied() else {
            return false;
        };
        passive_meta.is_recv_step()
            && passive_meta.is_control
            && passive_meta.label == label
            && (passive_meta.peer == ROLE
                || (!is_route_controller
                    && self
                        .control_semantic_kind(passive_meta.label, passive_meta.resource)
                        .is_loop()))
    }

    /// Preview recv metadata from a precomputed first-recv dispatch table.
    fn select_cached_dispatch_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        target_arm: u8,
        resolved_label_hint: Option<u8>,
    ) -> CachedRecvMeta {
        let Some(label) = resolved_label_hint else {
            return CachedRecvMeta::EMPTY;
        };
        let Some((dispatch_arm, target_idx)) = materialization_meta.first_recv_target(label) else {
            return CachedRecvMeta::EMPTY;
        };
        if dispatch_arm != ARM_SHARED && dispatch_arm != target_arm {
            return CachedRecvMeta::EMPTY;
        }
        let target_idx = state_index_to_usize(target_idx);
        let route_arm = if dispatch_arm == ARM_SHARED {
            target_arm
        } else {
            dispatch_arm
        };
        let Some(meta) = self.cursor.try_recv_meta_at(target_idx) else {
            return CachedRecvMeta::EMPTY;
        };
        Self::cached_recv_meta_from_recv(target_idx, meta, Some(route_arm))
    }

    fn preview_selected_arm_meta(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
        resolved_label_hint: Option<u8>,
    ) -> RecvResult<CachedRecvMeta> {
        let scope_id = selection.scope_id;
        let selected_label_meta = self.selection_label_meta(selection);
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let controller_arm_entry = if selection.at_route_offer_entry {
            materialization_meta.controller_arm_entry(selected_arm)
        } else {
            None
        };
        let dispatch_meta = if controller_arm_entry.is_none() {
            self.select_cached_dispatch_recv_meta(
                materialization_meta,
                selected_arm,
                resolved_label_hint,
            )
        } else {
            CachedRecvMeta::EMPTY
        };

        let direct_meta = if let Some((arm_entry_idx, arm_entry_label)) = controller_arm_entry {
            let arm_entry_idx = state_index_to_usize(arm_entry_idx);
            if let Some(local_meta) = self.cursor.try_local_meta_at(arm_entry_idx) {
                Self::cached_recv_meta_from_local(arm_entry_idx, selected_arm, local_meta)
            } else {
                Self::synthetic_cached_recv_meta(
                    arm_entry_idx,
                    scope_id,
                    selected_arm,
                    arm_entry_label,
                    arm_entry_idx,
                    selection.offer_lane,
                )
            }
        } else if !dispatch_meta.is_empty() {
            dispatch_meta
        } else if selected_arm < materialization_meta.arm_count {
            self.select_cached_route_arm_recv_meta(materialization_meta, selected_arm)
        } else {
            CachedRecvMeta::EMPTY
        };

        let mut meta = if !direct_meta.is_empty() {
            direct_meta
        } else {
            passive_recv_meta
                .get(selected_arm as usize)
                .copied()
                .ok_or(RecvError::PhaseInvariant)?
        };

        if self.selection_arm_has_recv(selection, selected_arm)
            && let Some(resolved_label) = resolved_label_hint
        {
            if Self::scope_label_to_arm(selected_label_meta, resolved_label) == Some(selected_arm) {
                meta.label = resolved_label;
            }
        }

        Ok(meta)
    }

    pub(super) fn descend_selected_passive_route(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
    ) -> RecvResult<bool> {
        if resolved.resolved_label_hint.is_some() {
            return Ok(false);
        }
        let scope_id = selection.scope_id;
        let selected_arm = resolved.selected_arm;
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(nested_scope) = materialization_meta.passive_arm_scope(selected_arm) else {
            return Ok(false);
        };
        let nested_scope = self.rebase_passive_descendant_scope(scope_id, nested_scope);
        if nested_scope == scope_id || nested_scope.kind() != ScopeKind::Route {
            return Ok(false);
        }
        self.propagate_recvless_parent_route_decision(scope_id, selected_arm);
        if matches!(resolved.route_token.source(), RouteDecisionSource::Poll) {
            self.emit_route_decision(
                scope_id,
                selected_arm,
                RouteDecisionSource::Poll,
                selection.offer_lane,
            );
        }
        self.set_route_arm(selection.offer_lane, scope_id, selected_arm)?;
        let mut target_scope = nested_scope;
        loop {
            let target_preview_arm = self.preview_selected_arm_for_scope(target_scope);
            if let Some(arm) = target_preview_arm {
                self.set_route_arm(selection.offer_lane, target_scope, arm)?;
                if let Some(child_scope) = self.cursor.passive_arm_scope_by_arm(target_scope, arm)
                    && child_scope.kind() == ScopeKind::Route
                {
                    target_scope = child_scope;
                    continue;
                }
            }
            let target_index = self
                .route_scope_materialization_index(target_scope)
                .ok_or(RecvError::PhaseInvariant)?;
            self.set_cursor_index(target_index);
            break;
        }
        self.align_cursor_to_selected_scope()?;
        Ok(true)
    }

    pub(super) fn emit_route_decision(
        &self,
        scope_id: ScopeId,
        arm: u8,
        source: RouteDecisionSource,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let causal = TapEvent::make_causal_key(port.lane().as_wire(), source.as_tap_seq());
        let arg0 = self.sid.raw();
        let arg1 = ((scope_id.raw() as u32) << 16) | (arm as u32);
        let mut event = events::RouteDecision::with_causal(port.now32(), causal, arg0, arg1);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    pub(super) fn prepare_route_decision_from_resolver(
        &mut self,
        scope_id: ScopeId,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> RecvResult<RouteResolveStep> {
        let (policy, eff_index, tag) = self
            .cursor
            .route_scope_controller_policy(scope_id)
            .ok_or(RecvError::PhaseInvariant)?;
        if !policy.is_dynamic() {
            return Err(RecvError::PhaseInvariant);
        }
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(RecvError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane = self.offer_lane_for_scope(scope_id);
        match self.evaluate_route_arm_from_epf(scope_id, offer_lane, policy_id, signals) {
            RoutePolicyDecision::RouteArm(arm) => {
                let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
                self.record_route_decision_for_lane(offer_lane as usize, scope_id, arm.as_u8());
                self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
                self.emit_route_decision(
                    scope_id,
                    arm.as_u8(),
                    RouteDecisionSource::Resolver,
                    offer_lane,
                );
                return Ok(RouteResolveStep::Resolved(arm));
            }
            RoutePolicyDecision::Abort(reason) => return Ok(RouteResolveStep::Abort(reason)),
            RoutePolicyDecision::Defer { retry_hint, source } => {
                return Ok(RouteResolveStep::Deferred { retry_hint, source });
            }
            RoutePolicyDecision::DelegateResolver => {}
        }
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(offer_lane as usize);
        let lane = Lane::new(port.lane().raw());
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs();
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            eff_index,
            tag,
            metrics,
            signals.input,
            attrs,
        ) {
            Ok(resolution) => resolution,
            Err(CpError::PolicyAbort { reason }) => return Ok(RouteResolveStep::Abort(reason)),
            Err(_) => return Err(RecvError::PhaseInvariant),
        };
        let arm = match resolution {
            DynamicResolution::RouteArm { arm } => arm,
            DynamicResolution::Loop { decision } => {
                if decision {
                    0
                } else {
                    1
                }
            }
            DynamicResolution::Defer { retry_hint } => {
                return Ok(RouteResolveStep::Deferred {
                    retry_hint,
                    source: DeferSource::Resolver,
                });
            }
            _ => return Err(RecvError::PhaseInvariant),
        };
        let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
        self.record_route_decision_for_lane(offer_lane as usize, scope_id, arm.as_u8());
        self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
        self.emit_route_decision(
            scope_id,
            arm.as_u8(),
            RouteDecisionSource::Resolver,
            offer_lane,
        );
        Ok(RouteResolveStep::Resolved(arm))
    }

    /// Route decision via controller_arm_entry labels.
    fn prepare_route_decision_from_resolver_via_arm_entry(
        &mut self,
        scope_id: ScopeId,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> RecvResult<RouteResolveStep> {
        // Get arm 0's entry to find the label used for resolver lookup
        let (arm0_entry, _arm0_label) = self
            .cursor
            .controller_arm_entry_by_arm(scope_id, 0)
            .ok_or(RecvError::PhaseInvariant)?;

        // Navigate to arm0_entry to get the node's metadata
        // The arm entry node should be a Local (self-send) node with a policy annotation.
        let local_meta = self
            .cursor
            .try_local_meta_at(state_index_to_usize(arm0_entry))
            .ok_or(RecvError::PhaseInvariant)?;

        let policy = local_meta.policy;
        if !policy.is_dynamic() {
            return Ok(RouteResolveStep::Abort(0));
        }
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(RecvError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(RecvError::PhaseInvariant);
        }
        match self.evaluate_route_arm_from_epf(scope_id, local_meta.lane, policy_id, signals) {
            RoutePolicyDecision::RouteArm(arm) => {
                let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
                self.record_route_decision_for_lane(
                    local_meta.lane as usize,
                    scope_id,
                    arm.as_u8(),
                );
                self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
                self.emit_route_decision(
                    scope_id,
                    arm.as_u8(),
                    RouteDecisionSource::Resolver,
                    local_meta.lane,
                );
                return Ok(RouteResolveStep::Resolved(arm));
            }
            RoutePolicyDecision::Abort(reason) => return Ok(RouteResolveStep::Abort(reason)),
            RoutePolicyDecision::Defer { retry_hint, source } => {
                return Ok(RouteResolveStep::Deferred { retry_hint, source });
            }
            RoutePolicyDecision::DelegateResolver => {}
        }

        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(local_meta.lane as usize);
        let lane = Lane::new(port.lane().raw());
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs();
        let tag = local_meta.resource.unwrap_or(0);
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            local_meta.eff_index,
            tag,
            metrics,
            signals.input,
            attrs,
        ) {
            Ok(resolution) => resolution,
            Err(CpError::PolicyAbort { reason }) => return Ok(RouteResolveStep::Abort(reason)),
            Err(_) => return Err(RecvError::PhaseInvariant),
        };

        let arm = match resolution {
            DynamicResolution::RouteArm { arm } => arm,
            DynamicResolution::Loop { decision } => {
                if decision {
                    0
                } else {
                    1
                }
            }
            DynamicResolution::Defer { retry_hint } => {
                return Ok(RouteResolveStep::Deferred {
                    retry_hint,
                    source: DeferSource::Resolver,
                });
            }
            _ => return Err(RecvError::PhaseInvariant),
        };
        let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
        self.record_route_decision_for_lane(local_meta.lane as usize, scope_id, arm.as_u8());
        self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
        self.emit_route_decision(
            scope_id,
            arm.as_u8(),
            RouteDecisionSource::Resolver,
            local_meta.lane,
        );
        Ok(RouteResolveStep::Resolved(arm))
    }

    fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }

    #[inline]
    fn commit_send_after_emit(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<()> {
        self.commit_send_preview(preview_cursor_index, meta)?;
        self.commit_send_progress(meta);
        Ok(())
    }

    #[inline(never)]
    fn commit_send_preview(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<()> {
        if let Some(preview) = self.take_matching_pending_send_branch_preview(meta) {
            self.commit_pending_branch_preview(preview)
                .map_err(|_| SendError::PhaseInvariant)?;
        }
        if let Some(preview_cursor_index) = preview_cursor_index {
            self.set_cursor_index(state_index_to_usize(preview_cursor_index));
        }
        self.cursor
            .try_advance_past_jumps_in_place()
            .map_err(|_| SendError::PhaseInvariant)
    }

    #[inline(never)]
    fn commit_send_progress(&mut self, meta: SendMeta) {
        let lane_idx = meta.lane as usize;
        self.advance_lane_cursor(lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
    }

    pub(crate) fn send_with_preview_in_place<'a, M>(
        &'a mut self,
        preview: crate::endpoint::kernel::SendPreview,
        payload: Option<&'a <M as MessageSpec>::Payload>,
    ) -> impl core::future::Future<
        Output = SendResult<
            ControlOutcome<
                'r,
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >,
        >,
    > + 'a
    where
        M: MessageSpec + SendableLabel + 'a,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        let (meta, cursor_index) = preview.into_parts();
        self.send_with_meta_and_cursor_in_place::<M>(meta, Some(cursor_index), payload)
    }

    #[cfg(test)]
    pub(crate) async fn send_with_meta_in_place<M>(
        &mut self,
        meta: SendMeta,
        payload: Option<&<M as MessageSpec>::Payload>,
    ) -> SendResult<
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    >
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        self.send_with_meta_and_cursor_in_place::<M>(meta, None, payload)
            .await
    }

    async fn send_with_meta_and_cursor_in_place<M>(
        &mut self,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        payload: Option<&<M as MessageSpec>::Payload>,
    ) -> SendResult<
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    >
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        let prepared = self.prepare_send_control(meta, Self::send_descriptor::<M>())?;
        let emission = self
            .emit_send_transport(
                meta,
                payload.map(lane_port::ErasedSendPayload::from_typed::<M::Payload>),
                prepared,
            )
            .await?;
        let control =
            self.finish_send_after_transport_erased(meta, preview_cursor_index, emission)?;
        Ok(Self::typed_control_outcome::<M>(control))
    }

    #[inline(always)]
    fn send_descriptor<M>() -> SendDescriptor<Self>
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        let (expects_control, mint_token, stage_payload, dispatch_control): (
            bool,
            MintSendTokenFn<Self>,
            StageSendPayloadFn,
            DispatchSendTokenFn<Self>,
        ) = match <M::ControlKind as ControlPayloadKind>::HANDLING {
            ControlHandling::None => (
                false,
                Self::mint_no_send_token,
                Self::stage_data_send_payload,
                Self::dispatch_no_send_token,
            ),
            ControlHandling::Canonical => (
                true,
                Self::mint_send_token::<M>,
                Self::stage_canonical_send_payload,
                Self::dispatch_send_token::<M>,
            ),
            ControlHandling::External => (
                true,
                Self::mint_send_token::<M>,
                Self::stage_external_send_payload,
                Self::dispatch_send_token::<M>,
            ),
        };
        SendDescriptor {
            label: <M as MessageSpec>::LABEL,
            expects_control,
            mint_token,
            stage_payload,
            dispatch_control,
        }
    }

    #[inline(always)]
    fn mint_no_send_token(
        _endpoint: &Self,
        _meta: SendMeta,
    ) -> SendResult<Option<ErasedCapFlowToken>> {
        Ok(None)
    }

    #[inline(always)]
    fn dispatch_no_send_token(
        _endpoint: &Self,
        _token: ErasedCapFlowToken,
    ) -> SendResult<DispatchSendTokenResult> {
        Ok(DispatchSendTokenResult::None)
    }

    #[inline(always)]
    fn mint_send_token<M>(endpoint: &Self, meta: SendMeta) -> SendResult<Option<ErasedCapFlowToken>>
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        <M::ControlKind as CanonicalTokenProvider<
            'r,
            ROLE,
            T,
            U,
            C,
            E,
            Mint,
            MAX_RV,
            M,
            B,
        >>::into_token(endpoint, &meta)
        .map(|token| token.map(ErasedCapFlowToken::from_typed))
    }

    #[inline(always)]
    fn dispatch_send_token<M>(
        endpoint: &Self,
        token: ErasedCapFlowToken,
    ) -> SendResult<DispatchSendTokenResult>
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        let cluster = endpoint
            .control
            .cluster()
            .ok_or(SendError::PhaseInvariant)?;
        let flow_token: CapFlowToken<
            <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
        > = token.into_flow_token();
        let frame = flow_token.into_frame();
        match cluster.dispatch_typed_control_frame(endpoint.rendezvous_id(), frame, None) {
            Ok(Some(registered)) => Ok(DispatchSendTokenResult::Registered(
                RegisteredTokenParts::from_typed(registered),
            )),
            Ok(None) => Ok(DispatchSendTokenResult::None),
            Err(CpError::Authorisation {
                effect: CpEffect::SpliceAck,
            }) if matches!(
                <M::ControlKind as ControlPayloadKind>::HANDLING,
                ControlHandling::Canonical
            ) =>
            {
                Ok(DispatchSendTokenResult::CanonicalFallback)
            }
            Err(_) => Err(SendError::PhaseInvariant),
        }
    }

    #[inline(always)]
    fn stage_data_send_payload(
        minted_token: Option<ErasedCapFlowToken>,
        payload: Option<lane_port::ErasedSendPayload<'_>>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if minted_token.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let data = payload.ok_or(SendError::PhaseInvariant)?;
        Ok(StagedSendPayload {
            encoded_len: data.encode_into(scratch)?,
            control: StagedSendControl::None,
        })
    }

    #[inline(always)]
    fn stage_canonical_send_payload(
        minted_token: Option<ErasedCapFlowToken>,
        payload: Option<lane_port::ErasedSendPayload<'_>>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if payload.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let token = minted_token.ok_or(SendError::PhaseInvariant)?;
        let bytes = token.bytes();
        scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
        Ok(StagedSendPayload {
            encoded_len: CAP_TOKEN_LEN,
            control: StagedSendControl::Canonical(token),
        })
    }

    #[inline(always)]
    fn stage_external_send_payload(
        minted_token: Option<ErasedCapFlowToken>,
        payload: Option<lane_port::ErasedSendPayload<'_>>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if let Some(token) = minted_token {
            let bytes = token.bytes();
            scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
            return Ok(StagedSendPayload {
                encoded_len: CAP_TOKEN_LEN,
                control: StagedSendControl::External {
                    dispatch_token: Some(token),
                    external_token: Some(token),
                },
            });
        }

        let data = payload.ok_or(SendError::PhaseInvariant)?;
        Ok(StagedSendPayload {
            encoded_len: data.encode_into(scratch)?,
            control: StagedSendControl::External {
                dispatch_token: None,
                external_token: None,
            },
        })
    }

    #[inline(never)]
    fn prepare_send_control(
        &mut self,
        meta: SendMeta,
        descriptor: SendDescriptor<Self>,
    ) -> SendResult<PreparedSendControl<Self>> {
        if meta.is_control != descriptor.expects_control {
            return Err(SendError::PhaseInvariant);
        }

        self.evaluate_dynamic_policy(&meta, descriptor.label)?;

        let lane = Lane::new(meta.lane as u32);
        let policy_action = self.eval_endpoint_policy(
            Slot::EndpointTx,
            ids::ENDPOINT_SEND,
            self.sid.raw(),
            Self::endpoint_policy_args(lane, meta.label, FrameFlags::empty()),
            lane,
        );
        self.apply_send_policy(policy_action, meta.scope, lane)?;

        let minted_token = (descriptor.mint_token)(self, meta)?;

        Ok(PreparedSendControl {
            minted_token,
            stage_payload: descriptor.stage_payload,
            dispatch_control: descriptor.dispatch_control,
        })
    }

    async fn emit_send_transport(
        &mut self,
        meta: SendMeta,
        payload: Option<lane_port::ErasedSendPayload<'_>>,
        prepared: PreparedSendControl<Self>,
    ) -> SendResult<SendTransportEmission<Self>> {
        let mut staged_send = None;
        {
            let port = self.port_for_lane(meta.lane as usize);
            let payload_view = lane_port::staged_payload(port, |scratch| {
                let staged = (prepared.stage_payload)(prepared.minted_token, payload, scratch)?;
                let encoded_len = staged.encoded_len;
                staged_send = Some(staged);
                Ok::<usize, SendError>(encoded_len)
            })?;

            let outgoing = crate::transport::Outgoing {
                meta: crate::transport::SendMeta {
                    eff_index: meta.eff_index,
                    label: meta.label,
                    peer: meta.peer,
                    lane: meta.lane,
                    direction: if meta.peer == ROLE {
                        crate::transport::LocalDirection::Local
                    } else {
                        crate::transport::LocalDirection::Send
                    },
                    is_control: meta.is_control,
                },
                payload: payload_view,
            };

            if !outgoing.meta.is_local() {
                lane_port::send_outgoing(port, outgoing)
                    .await
                    .map_err(SendError::Transport)?;
            }
        }

        let staged_send = staged_send.ok_or(SendError::PhaseInvariant)?;
        Ok(SendTransportEmission {
            control: staged_send.control,
            dispatch_control: prepared.dispatch_control,
        })
    }

    #[inline(never)]
    fn finish_send_after_transport_erased(
        &mut self,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission<Self>,
    ) -> SendResult<ErasedControlOutcome<'r>> {
        let mut control_outcome = ErasedControlOutcome::None;
        self.commit_send_after_emit(preview_cursor_index, meta)?;

        let lane_wire = self.port_for_lane(meta.lane as usize).lane().as_wire();
        let logical_meta = TapFrameMeta::new(
            self.sid.raw(),
            lane_wire,
            ROLE,
            meta.label,
            FrameFlags::empty(),
        );
        let scope_trace = self.scope_trace(meta.scope);
        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_SEND
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);

        match emission.control {
            StagedSendControl::None => {}
            StagedSendControl::Canonical(token) => {
                match (emission.dispatch_control)(self, token)? {
                    DispatchSendTokenResult::Registered(parts) => {
                        control_outcome = ErasedControlOutcome::Canonical(
                            ErasedRegisteredCapToken::from_parts(parts),
                        );
                    }
                    DispatchSendTokenResult::CanonicalFallback => {
                        control_outcome =
                            ErasedControlOutcome::Canonical(ErasedRegisteredCapToken::from_parts(
                                RegisteredTokenParts::from_bytes(token.bytes()),
                            ));
                    }
                    DispatchSendTokenResult::None => return Err(SendError::PhaseInvariant),
                }
            }
            StagedSendControl::External {
                dispatch_token,
                external_token,
            } => {
                if let Some(token) = dispatch_token {
                    match (emission.dispatch_control)(self, token)? {
                        DispatchSendTokenResult::None | DispatchSendTokenResult::Registered(_) => {}
                        DispatchSendTokenResult::CanonicalFallback => {
                            return Err(SendError::PhaseInvariant);
                        }
                    }
                }
                if let Some(token) = external_token {
                    control_outcome = ErasedControlOutcome::External(token);
                }
            }
        }

        Ok(control_outcome)
    }

    #[inline(always)]
    fn typed_control_outcome<M>(
        outcome: ErasedControlOutcome<'r>,
    ) -> ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        match outcome {
            ErasedControlOutcome::None => ControlOutcome::None,
            ErasedControlOutcome::Canonical(token) => ControlOutcome::Canonical(token.into_typed()),
            ErasedControlOutcome::External(token) => ControlOutcome::External(token.into_generic()),
        }
    }

    fn record_loop_decision(
        &self,
        metadata: &LoopMetadata,
        decision: LoopDecision,
        lane: u8,
    ) -> SendResult<()> {
        let idx = Self::loop_index(metadata.scope).ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(lane as usize);
        let disposition = match decision {
            LoopDecision::Continue => LoopDisposition::Continue,
            LoopDecision::Break => LoopDisposition::Break,
        };
        let arm = match decision {
            LoopDecision::Continue => 0,
            LoopDecision::Break => 1,
        };
        let epoch = port.record_loop_decision(idx, disposition);
        let ts = port.now32();
        let causal = TapEvent::make_causal_key(ROLE, idx);
        let arg1 = match decision {
            LoopDecision::Continue => ((idx as u32) << 16) | epoch as u32,
            LoopDecision::Break => ((idx as u32) << 16) | (epoch as u32) | 0x1,
        };
        let event = events::LoopDecision::with_causal_and_scope(
            ts,
            causal,
            self.sid.raw(),
            arg1,
            self.scope_trace(metadata.scope)
                .map(|t| t.pack())
                .unwrap_or(0),
        );
        emit(port.tap(), event);
        if metadata.scope.kind() == ScopeKind::Route {
            self.port_for_lane(lane as usize)
                .record_route_decision(metadata.scope, arm);
            self.emit_route_decision(metadata.scope, arm, RouteDecisionSource::Ack, lane);
        }
        Ok(())
    }

    pub(super) fn select_scope(&mut self) -> RecvResult<OfferScopeSelection> {
        self.align_cursor_to_selected_scope()?;
        // O(1) entry: offer() must be called at a Route decision point.
        // Use the node's scope directly (no parent traversal).
        let node_scope = self.current_offer_scope_id();
        let Some(region) = self.cursor.scope_region_by_id(node_scope) else {
            return Err(RecvError::PhaseInvariant);
        };
        if region.kind != ScopeKind::Route {
            return Err(RecvError::PhaseInvariant);
        }
        let scope_id = region.scope_id;
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
            && self.cursor.index() != state_index_to_usize(offer_entry)
        {
            let selected_arm = self.selected_arm_for_scope(scope_id);
            let current_arm = self.cursor.typestate_node(self.cursor.index()).route_arm();
            if selected_arm.is_none() || current_arm != selected_arm {
                return Err(RecvError::PhaseInvariant);
            }
        }
        let current_idx = self.cursor.index();
        let cached_entry_state = self
            .offer_entry_state_snapshot(current_idx)
            .filter(|state| {
                state.active_mask != 0 && self.offer_entry_scope_id(current_idx, *state) == scope_id
            });
        // Route hints are offer-scoped; preview only inspects them here.
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
        if offer_lanes_len == 0 {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane_mask = if let Some(entry_state) = cached_entry_state {
            self.offer_entry_offer_lane_mask(current_idx, entry_state)
        } else {
            let mut offer_lane_mask = 0u8;
            let mut offer_lane_idx = 0usize;
            while offer_lane_idx < offer_lanes_len {
                let lane = offer_lanes[offer_lane_idx] as usize;
                if lane < MAX_LANES {
                    offer_lane_mask |= 1u8 << lane;
                }
                offer_lane_idx += 1;
            }
            offer_lane_mask
        };
        let offer_lanes_len = offer_lanes_len as u8;
        if offer_lanes_len == 0 {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane = offer_lanes[0];
        let offer_lane_idx = offer_lane;
        let at_route_offer_entry = self
            .cursor
            .route_scope_offer_entry(scope_id)
            .map(|entry| entry.is_max() || current_idx == state_index_to_usize(entry))
            .unwrap_or(true);
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root: Self::parallel_scope_root(&self.cursor, scope_id),
            offer_lanes,
            offer_lane_mask,
            offer_lanes_len,
            offer_lane,
            offer_lane_idx,
            at_route_offer_entry,
        })
    }

    // Stage 3 of the offer kernel: materialize the selected branch from
    // precomputed route metadata and late binding demux state. This stage must
    // not perform arm arbitration.
    pub(super) fn materialize_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
        is_route_controller: bool,
        mut binding_classification: Option<crate::binding::IncomingClassification>,
        mut transport_payload_len: usize,
        transport_payload_lane: u8,
    ) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let scope_id = selection.scope_id;
        let route_token = resolved.route_token;
        let selected_arm = resolved.selected_arm;
        let resolved_label_hint = resolved.resolved_label_hint;
        let binding_channel: Option<crate::binding::Channel> = None;
        let preview_meta =
            self.preview_selected_arm_meta(selection, selected_arm, resolved_label_hint)?;
        let (_cursor_index, meta) = preview_meta.recv_meta().ok_or(RecvError::PhaseInvariant)?;

        let lane_wire = meta.lane;

        // Determine BranchKind before late binding resolution so wire-bound
        // branches can decide whether to wait for one additional ingress turn.
        let passive_linger_loop_label = !is_route_controller
            && self.is_linger_route(scope_id)
            && self
                .control_semantic_kind(meta.label, meta.resource)
                .is_loop();
        let branch_kind = if self.cursor.is_recv() {
            if passive_linger_loop_label
                || (!is_route_controller
                    && self
                        .control_semantic_kind(meta.label, meta.resource)
                        .is_loop()
                    && self.selection_non_wire_loop_control_recv(
                        selection,
                        is_route_controller,
                        selected_arm,
                        meta.label,
                    ))
            {
                BranchKind::LocalControl
            } else {
                BranchKind::WireRecv
            }
        } else if self.cursor.is_send() {
            BranchKind::ArmSendHint
        } else if self.cursor.is_local_action() || self.cursor.is_jump() {
            BranchKind::LocalControl
        } else {
            BranchKind::EmptyArmTerminal
        };

        // Late binding channel resolution: for wire recv branches, prefer
        // binding ingress even when transport payload bytes were staged earlier.
        let label_meta = self.selection_label_meta(selection);
        let binding_channel = if transport_payload_len == 0
            || matches!(branch_kind, BranchKind::WireRecv)
        {
            let mut channel = binding_channel;
            let lane_idx = meta.lane as usize;
            if let Some(expected_label) = label_meta.preferred_binding_label(Some(selected_arm)) {
                if binding_classification
                    .as_ref()
                    .map(|classification| classification.label == expected_label)
                    .unwrap_or(false)
                {
                    if let Some(classification) = binding_classification.take() {
                        channel = Some(classification.channel);
                    }
                } else if binding_classification.as_ref().and_then(|classification| {
                    Self::scope_label_to_arm(label_meta, classification.label)
                }) == Some(selected_arm)
                {
                    if let Some(classification) = binding_classification.take() {
                        channel = Some(classification.channel);
                    }
                } else if let Some(classification) =
                    self.take_matching_binding_for_lane(lane_idx, expected_label)
                {
                    channel = Some(classification.channel);
                }
            } else {
                (channel, _, _) = self.take_binding_for_selected_arm(
                    lane_idx,
                    selected_arm,
                    label_meta,
                    &mut binding_classification,
                );
            }
            channel
        } else {
            binding_channel
        };
        if transport_payload_len != 0
            && (!matches!(branch_kind, BranchKind::WireRecv) || binding_channel.is_some())
        {
            let port = self.port_for_lane(transport_payload_lane as usize);
            lane_port::requeue_recv(port);
            transport_payload_len = 0;
        }
        let branch_progress_eff = self
            .cursor
            .scope_lane_last_eff_for_arm(scope_id, selected_arm, lane_wire)
            .or_else(|| self.cursor.scope_lane_last_eff(scope_id, lane_wire))
            .unwrap_or(meta.eff_index);
        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            eff_index: branch_progress_eff,
            kind: branch_kind,
            route_source: route_token.source(),
        };
        Ok(RouteBranch {
            label: meta.label,
            cursor_index: preview_meta.cursor_index,
            transport_payload_len,
            transport_payload_lane,
            binding_channel,
            branch_meta,
            _cfg: core::marker::PhantomData,
        })
    }

    // Stage 2 of the offer kernel: resolve arm authority in fixed order
    // Ack -> Resolver -> Poll. This stage may defer/yield/restart frontier
    // evaluation, but it must not materialize the selected branch.
    pub(super) async fn resolve_token(
        &mut self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        is_dynamic_route_scope: bool,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
        frontier_visited: &mut FrontierVisitSet,
    ) -> RecvResult<ResolveTokenOutcome> {
        let scope_id = selection.scope_id;
        let frontier_parallel_root = selection.frontier_parallel_root;
        let offer_lanes = selection.offer_lanes;
        let offer_lane_mask = selection.offer_lane_mask;
        let offer_lanes_len = selection.offer_lanes_len as usize;
        let offer_lane = selection.offer_lane;
        let offer_lane_idx = selection.offer_lane_idx as usize;
        let at_route_offer_entry = selection.at_route_offer_entry;

        let mut resolved_label_hint = self
            .peek_scope_hint(scope_id)
            .and_then(ScopeHint::new)
            .map(ScopeHint::label);
        if *transport_payload_len != 0
            && let Some(label) = resolved_label_hint
        {
            let label_meta = self.selection_label_meta(selection);
            self.mark_scope_ready_arm_from_label(scope_id, label, label_meta);
        }

        let mut liveness = OfferLivenessState::new(self.liveness_policy);
        let mut liveness_exhausted = false;

        let mut route_token = self.peek_scope_ack(scope_id);
        if route_token.is_none() && is_route_controller && is_dynamic_route_scope {
            let is_self_send_route = !Self::scope_has_controller_arm_entry(&self.cursor, scope_id);
            loop {
                let route_signals = self.policy_signals_for_slot(Slot::Route).into_owned();
                let resolver_step = if is_self_send_route {
                    self.prepare_route_decision_from_resolver_via_arm_entry(
                        scope_id,
                        &route_signals,
                    )?
                } else {
                    self.prepare_route_decision_from_resolver(scope_id, &route_signals)?
                };
                match resolver_step {
                    RouteResolveStep::Resolved(resolver_arm) => {
                        route_token = Some(RouteDecisionToken::from_resolver(resolver_arm));
                        break;
                    }
                    RouteResolveStep::Abort(reason) => {
                        return Err(RecvError::PolicyAbort { reason });
                    }
                    RouteResolveStep::Deferred { retry_hint, source } => {
                        match self.on_frontier_defer(
                            &mut liveness,
                            scope_id,
                            frontier_parallel_root,
                            source,
                            DeferReason::Unsupported,
                            retry_hint,
                            offer_lane,
                            binding_classification.is_some(),
                            None,
                            frontier_visited,
                        ) {
                            FrontierDeferOutcome::Continue => {}
                            FrontierDeferOutcome::Yielded => {
                                return Ok(ResolveTokenOutcome::RestartFrontier);
                            }
                            FrontierDeferOutcome::Exhausted => {
                                liveness_exhausted = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if route_token.is_none() && !is_route_controller {
            let mut passive_waited_for_wire = false;
            loop {
                let staged_payload_for_offer_lane =
                    *transport_payload_len != 0 && *transport_payload_lane == offer_lane;
                if !staged_payload_for_offer_lane {
                    let label_meta = self.selection_label_meta(selection);
                    let materialization_meta = self.selection_materialization_meta(selection);
                    self.cache_binding_classification_for_offer(
                        scope_id,
                        offer_lane_idx,
                        offer_lane_mask,
                        label_meta,
                        materialization_meta,
                        binding_classification,
                    );

                    self.ingest_scope_evidence_for_offer(
                        scope_id,
                        offer_lane_idx,
                        offer_lane_mask,
                        is_dynamic_route_scope,
                        label_meta,
                    );
                    if let Some(classification) = binding_classification.as_ref() {
                        self.ingest_binding_scope_evidence(
                            scope_id,
                            classification.label,
                            is_dynamic_route_scope,
                            label_meta,
                        );
                    }
                    if self.scope_evidence_conflicted(scope_id)
                        && !self.recover_scope_evidence_conflict(
                            scope_id,
                            is_dynamic_route_scope,
                            is_route_controller,
                        )
                    {
                        return Err(RecvError::PhaseInvariant);
                    }

                    if let Some(label) = self
                        .peek_scope_hint(scope_id)
                        .and_then(ScopeHint::new)
                        .map(ScopeHint::label)
                    {
                        resolved_label_hint = Some(label);
                    }
                }
                if let Some(token) = self.peek_scope_ack(scope_id) {
                    route_token = Some(token);
                    break;
                }

                if *transport_payload_len != 0 {
                    break;
                }

                if resolved_label_hint.is_some() && passive_waited_for_wire {
                    break;
                }

                if self.scope_has_ready_arm_evidence(scope_id) {
                    let needs_wire_turn_for_materialization = !passive_waited_for_wire
                        && *transport_payload_len == 0
                        && binding_classification.is_none();
                    if !needs_wire_turn_for_materialization {
                        break;
                    }
                }

                if !passive_waited_for_wire {
                    let recv_lane_idx = offer_lane as usize;
                    let recv_lane = recv_lane_idx as u8;
                    let port = self.port_for_lane(recv_lane_idx);
                    let mut recv_fut = core::pin::pin!(lane_port::recv_future(port));
                    let payload = poll_fn(|cx| match recv_fut.as_mut().poll(cx) {
                        Poll::Ready(result) => Poll::Ready(Some(result)),
                        Poll::Pending => Poll::Ready(None),
                    })
                    .await;
                    if let Some(payload) = payload {
                        let payload = payload.map_err(RecvError::Transport)?;
                        if *transport_payload_len == 0 && !payload.as_bytes().is_empty() {
                            *transport_payload_len =
                                lane_port::copy_payload_into_scratch(port, &payload)
                                    .map_err(|_| RecvError::PhaseInvariant)?;
                            *transport_payload_lane = recv_lane;
                        }
                    }
                    passive_waited_for_wire = true;
                    continue;
                }

                match self.on_frontier_defer(
                    &mut liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_classification.is_some(),
                    None,
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        break;
                    }
                    FrontierDeferOutcome::Yielded => {
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        liveness_exhausted = true;
                        break;
                    }
                }
            }
        }

        if route_token.is_none()
            && !is_route_controller
            && is_dynamic_route_scope
            && self.binding.policy_signals_provider().is_some()
        {
            let route_signals = self.policy_signals_for_slot(Slot::Route).into_owned();
            match self.prepare_route_decision_from_resolver(scope_id, &route_signals)? {
                RouteResolveStep::Resolved(resolver_arm) => {
                    route_token = Some(RouteDecisionToken::from_resolver(resolver_arm));
                }
                RouteResolveStep::Abort(reason) => {
                    if reason != 0 {
                        return Err(RecvError::PolicyAbort { reason });
                    }
                }
                RouteResolveStep::Deferred { retry_hint, source } => {
                    match self.on_frontier_defer(
                        &mut liveness,
                        scope_id,
                        frontier_parallel_root,
                        source,
                        DeferReason::Unsupported,
                        retry_hint,
                        offer_lane,
                        binding_classification.is_some(),
                        None,
                        frontier_visited,
                    ) {
                        FrontierDeferOutcome::Continue => {}
                        FrontierDeferOutcome::Yielded => {
                            yield_once().await;
                            return Ok(ResolveTokenOutcome::RestartFrontier);
                        }
                        FrontierDeferOutcome::Exhausted => {
                            liveness_exhausted = true;
                        }
                    }
                }
            }
        }

        if route_token.is_none()
            && !is_route_controller
            && *transport_payload_len == 0
            && binding_classification.is_none()
            && resolved_label_hint.is_none()
            && !liveness_exhausted
        {
            match self.on_frontier_defer(
                &mut liveness,
                scope_id,
                frontier_parallel_root,
                DeferSource::Resolver,
                DeferReason::NoEvidence,
                1,
                offer_lane,
                false,
                None,
                frontier_visited,
            ) {
                FrontierDeferOutcome::Continue => {
                    yield_once().await;
                    return Ok(ResolveTokenOutcome::RestartFrontier);
                }
                FrontierDeferOutcome::Yielded => {
                    yield_once().await;
                    return Ok(ResolveTokenOutcome::RestartFrontier);
                }
                FrontierDeferOutcome::Exhausted => {
                    liveness_exhausted = true;
                }
            }
        }

        if route_token.is_none() && liveness_exhausted {
            while route_token.is_none() && liveness.can_force_poll() {
                liveness.mark_forced_poll();
                if let Some(poll_arm) = self
                    .try_poll_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
                    .await
                {
                    route_token = Some(RouteDecisionToken::from_poll(poll_arm));
                    break;
                }
            }
            if route_token.is_none() {
                return Err(RecvError::PolicyAbort {
                    reason: liveness.exhaust_reason(),
                });
            }
        }

        if route_token.is_none() {
            if !is_route_controller
                && *transport_payload_len != 0
                && *transport_payload_lane != offer_lane
            {
                return Ok(ResolveTokenOutcome::RestartFrontier);
            }
            if let Some(poll_arm) = self
                .try_poll_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
                .await
            {
                route_token = Some(RouteDecisionToken::from_poll(poll_arm));
            } else {
                match self.on_frontier_defer(
                    &mut liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_classification.is_some(),
                    None,
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Yielded => {
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        while route_token.is_none() && liveness.can_force_poll() {
                            liveness.mark_forced_poll();
                            if let Some(poll_arm) = self
                                .try_poll_route_decision_for_offer(
                                    scope_id,
                                    &offer_lanes,
                                    offer_lanes_len,
                                )
                                .await
                            {
                                route_token = Some(RouteDecisionToken::from_poll(poll_arm));
                                break;
                            }
                        }
                        if route_token.is_none() {
                            return Err(RecvError::PolicyAbort {
                                reason: liveness.exhaust_reason(),
                            });
                        }
                    }
                }
            }
        }

        let mut route_token = route_token.ok_or(RecvError::PhaseInvariant)?;
        if let Some(classification) = binding_classification.as_ref()
            && let Some(binding_arm) = {
                let label_meta = self.selection_label_meta(selection);
                Self::scope_label_to_arm(label_meta, classification.label)
            }
            && binding_arm == route_token.arm().as_u8()
        {
            // Binding classification is demux-only. Once Ack/Resolver/Poll has
            // fixed the arm, matching classification may still contribute
            // readiness for branch materialization.
            self.mark_scope_ready_arm(scope_id, binding_arm);
        }
        if *transport_payload_len != 0 && *transport_payload_lane == offer_lane {
            if !is_route_controller
                && is_dynamic_route_scope
                && matches!(route_token.source(), RouteDecisionSource::Ack)
            {
                self.mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
            } else if is_route_controller
                && is_dynamic_route_scope
                && matches!(
                    route_token.source(),
                    RouteDecisionSource::Resolver | RouteDecisionSource::Poll
                )
            {
                self.mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
            }
        }

        let selected_arm = loop {
            let selected_arm = route_token.arm().as_u8();
            if self.selection_arm_requires_materialization_ready_evidence(
                selection,
                is_route_controller,
                selected_arm,
            ) && !self.scope_has_ready_arm(scope_id, selected_arm)
            {
                if matches!(route_token.source(), RouteDecisionSource::Resolver)
                    && let Some(poll_arm) = self
                        .try_poll_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
                        .await
                {
                    route_token = RouteDecisionToken::from_poll(poll_arm);
                    continue;
                }
                if *transport_payload_len != 0 {
                    let port = self.port_for_lane(*transport_payload_lane as usize);
                    lane_port::requeue_recv(port);
                }
                if matches!(route_token.source(), RouteDecisionSource::Resolver) {
                    let _ = self.take_scope_ack(scope_id);
                }
                let keep_current_scope = is_route_controller
                    && is_dynamic_route_scope
                    && !at_route_offer_entry
                    && matches!(route_token.source(), RouteDecisionSource::Resolver);
                if keep_current_scope {
                    yield_once().await;
                    return Ok(ResolveTokenOutcome::RestartFrontier);
                }
                match self.on_frontier_defer(
                    &mut liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_classification.is_some(),
                    Some(route_token.arm().as_u8()),
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        if !is_route_controller && !is_dynamic_route_scope {
                            self.await_static_passive_progress(
                                selection,
                                Some(route_token.arm().as_u8()),
                                binding_classification,
                                transport_payload_len,
                                transport_payload_lane,
                            )
                            .await?;
                            return Ok(ResolveTokenOutcome::RestartFrontier);
                        }
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Yielded => {
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        while liveness.can_force_poll() {
                            liveness.mark_forced_poll();
                            if self
                                .try_poll_route_decision_for_offer(
                                    scope_id,
                                    &offer_lanes,
                                    offer_lanes_len,
                                )
                                .await
                                .is_some()
                            {
                                return Ok(ResolveTokenOutcome::RestartFrontier);
                            }
                        }
                        return Err(RecvError::PolicyAbort {
                            reason: liveness.exhaust_reason(),
                        });
                    }
                }
            }
            break selected_arm;
        };
        Ok(ResolveTokenOutcome::Resolved(ResolvedRouteDecision {
            route_token,
            selected_arm,
            resolved_label_hint,
        }))
    }

    pub(crate) fn canonical_control_token<K>(&self, meta: &SendMeta) -> SendResult<CapFlowToken<K>>
    where
        K: ResourceKind + ControlMint,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsCanonical,
    {
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let shot = meta.shot.ok_or(SendError::PhaseInvariant)?;
        let cp_sid = SessionId::new(self.sid.raw());
        let port = self.port_for_lane(meta.lane as usize);
        let lane = port.lane();
        let cp_lane = Lane::new(lane.raw());
        let src_rv = RendezvousId::new(self.rendezvous_id().raw());
        port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let signals = self.policy_signals_for_slot(Slot::Route);
        let attrs = signals.attrs();
        let bytes = match tag {
            LoopContinueKind::TAG => {
                if K::TAG != LoopContinueKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                // Record loop decision before minting token
                let mut loop_scope = meta.scope;
                let mut recorded_via_loop_metadata = false;
                if let Some(metadata) = self.cursor.loop_metadata_inner()
                    && metadata.role == LoopRole::Controller
                    && metadata.controller == ROLE
                {
                    self.record_loop_decision(&metadata, LoopDecision::Continue, meta.lane)?;
                    loop_scope = metadata.scope;
                    recorded_via_loop_metadata = true;
                }
                if loop_scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
                    self.port_for_lane(meta.lane as usize)
                        .record_route_decision(loop_scope, 0);
                    self.emit_route_decision(loop_scope, 0, RouteDecisionSource::Ack, meta.lane);
                }
                let scope = loop_scope;
                let handle = LoopDecisionHandle {
                    sid: self.sid.raw(),
                    lane: lane.raw() as u16,
                    scope,
                };
                self.mint_control_token_with_handle::<LoopContinueKind>(
                    meta.peer, shot, lane, handle,
                )?
                .into_bytes()
            }
            LoopBreakKind::TAG => {
                if K::TAG != LoopBreakKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                // Record loop decision before minting token
                let mut loop_scope = meta.scope;
                let mut recorded_via_loop_metadata = false;
                if let Some(metadata) = self.cursor.loop_metadata_inner()
                    && metadata.role == LoopRole::Controller
                    && metadata.controller == ROLE
                {
                    self.record_loop_decision(&metadata, LoopDecision::Break, meta.lane)?;
                    loop_scope = metadata.scope;
                    recorded_via_loop_metadata = true;
                }
                if loop_scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
                    self.port_for_lane(meta.lane as usize)
                        .record_route_decision(loop_scope, 1);
                    self.emit_route_decision(loop_scope, 1, RouteDecisionSource::Ack, meta.lane);
                }
                let scope = loop_scope;
                let handle = LoopDecisionHandle {
                    sid: self.sid.raw(),
                    lane: lane.raw() as u16,
                    scope,
                };
                self.mint_control_token_with_handle::<LoopBreakKind>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
            RerouteKind::TAG => {
                if K::TAG != RerouteKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let policy = cluster
                    .policy_mode_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let handle = cluster
                    .prepare_reroute_handle_from_policy(
                        src_rv,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        policy,
                        transport_metrics,
                        signals.input,
                        attrs,
                    )
                    .map_err(|_| SendError::PhaseInvariant)?;
                self.mint_control_token_with_handle::<RerouteKind>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
            RouteDecisionKind::TAG => {
                if K::TAG != RouteDecisionKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let policy = cluster
                    .policy_mode_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let scope = meta.scope;
                let policy_scope = policy.scope();
                validate_route_decision_scope(scope, policy_scope)?;
                // Route arm is fixed by the offer/decode decision point.
                // Canonical route token minting must not re-evaluate policy.
                let arm = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
                if arm > 1 {
                    return Err(SendError::PhaseInvariant);
                }
                let handle = RouteDecisionHandle { scope, arm };
                self.port_for_lane(meta.lane as usize)
                    .record_route_decision(scope, arm);
                self.emit_route_decision(scope, arm, RouteDecisionSource::Resolver, meta.lane);
                self.mint_control_token_with_handle::<RouteDecisionKind>(
                    meta.peer, shot, lane, handle,
                )?
                .into_bytes()
            }
            SpliceIntentKind::TAG => {
                if K::TAG != SpliceIntentKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let policy = cluster
                    .policy_mode_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let operands = cluster
                    .prepare_splice_operands_from_policy(
                        src_rv,
                        cp_sid,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        policy,
                        transport_metrics,
                        signals.input,
                        attrs,
                    )
                    .map_err(|_| SendError::PhaseInvariant)?;
                self.mint_control_token_with_handle::<SpliceIntentKind>(
                    meta.peer,
                    shot,
                    lane,
                    Self::splice_handle_from_operands(operands),
                )?
                .into_bytes()
            }
            SpliceAckKind::TAG => {
                if K::TAG != SpliceAckKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let operands = cluster
                    .take_cached_splice_operands(cp_sid)
                    .or_else(|| cluster.distributed_operands(cp_sid))
                    .ok_or(SendError::PhaseInvariant)?;
                let token = self.mint_control_token_with_handle::<SpliceAckKind>(
                    meta.peer,
                    shot,
                    lane,
                    Self::splice_handle_from_operands(operands),
                )?;
                token.into_bytes()
            }
            CommitKind::TAG => {
                if K::TAG != CommitKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CommitKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            CheckpointKind::TAG => {
                if K::TAG != CheckpointKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CheckpointKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            RollbackKind::TAG => {
                if K::TAG != RollbackKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<RollbackKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            CancelKind::TAG => {
                if K::TAG != CancelKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CancelKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            CancelAckKind::TAG => {
                if K::TAG != CancelAckKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CancelAckKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            // Generic path for external control kinds (e.g., adapter AcceptHookKind).
            // Uses ControlMint trait for extensibility without modifying hibana core.
            _ => {
                let handle = K::mint_handle(self.sid, lane, meta.scope);
                self.mint_control_token_with_handle::<K>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
        };
        Ok(CapFlowToken::new(GenericCapToken::<K>::from_bytes(bytes)))
    }

    #[inline]
    pub(crate) fn settle_scope_after_action(
        &mut self,
        scope: ScopeId,
        route_arm: Option<u8>,
        _eff_index: Option<EffIndex>,
        lane: u8,
    ) {
        let region = if scope.kind() == ScopeKind::Route {
            self.cursor.scope_region_by_id(scope)
        } else {
            None
        };
        let linger = region.as_ref().map_or(false, |r| r.linger);
        let lane_wire = lane;
        let mut exited_scope = false;

        // For linger scopes (loops), if cursor has advanced past the region boundary,
        // rewind to region.start so the next offer() can find the recv node.
        // This is essential for passive observers whose projection has fewer steps.
        // BUT: do NOT rewind if we're in the Break arm (arm > 0 for standard 2-arm loops).
        // The Break arm should exit the loop, not loop back.
        if linger {
            if let Some(ref reg) = region {
                let current_arm = route_arm.or_else(|| self.route_arm_for(lane_wire, scope));
                let is_break_arm = current_arm.map_or(false, |arm| arm > 0);
                if self.cursor.index() >= reg.end {
                    self.clear_descendant_route_state_for_lane(lane_wire, scope);
                    if is_break_arm {
                        self.pop_route_arm(lane_wire, scope);
                        exited_scope = true;
                        let mut current_scope = scope;
                        while let Some(parent) = self.cursor.scope_parent(current_scope) {
                            if !matches!(parent.kind(), ScopeKind::Route | ScopeKind::Loop) {
                                break;
                            }

                            if let Some(parent_region) = self.cursor.scope_region_by_id(parent) {
                                if parent_region.linger {
                                    if let Some(parent_arm) = self.route_arm_for(lane_wire, parent)
                                    {
                                        if parent_arm == 0 {
                                            self.set_cursor_index(parent_region.start);
                                            break;
                                        }
                                    }
                                }
                                let should_advance = self.cursor.index() >= parent_region.end;

                                if should_advance {
                                    self.clear_descendant_route_state_for_lane(lane_wire, parent);
                                    if self.cursor.advance_scope_by_id_in_place(parent) {}
                                    self.pop_route_arm(lane_wire, parent);
                                    current_scope = parent;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        self.set_cursor_index(reg.start);
                    }
                }
                if !is_break_arm {
                    let at_scope_start = self.cursor.index() == reg.start;
                    let at_passive_branch = self.cursor.jump_reason()
                        == Some(JumpReason::PassiveObserverBranch)
                        && self
                            .cursor
                            .scope_region()
                            .map(|region| region.scope_id == scope)
                            .unwrap_or(false);
                    if at_scope_start || at_passive_branch {
                        if let Some(first_eff) = self.cursor.scope_lane_first_eff(scope, lane_wire)
                        {
                            let lane_idx = lane_wire as usize;
                            self.set_lane_cursor_to_eff_index(lane_idx, first_eff);
                        }
                    }
                }
            }
        } else if let Some(ref reg) = region {
            if self.cursor.index() >= reg.end {
                exited_scope = true;
            }
        }

        if exited_scope {
            if let Some(eff_index) = self.cursor.scope_lane_last_eff(scope, lane_wire) {
                let lane_idx = lane_wire as usize;
                self.advance_lane_cursor(lane_idx, eff_index);
            }
        }

        if scope.kind() == ScopeKind::Route {
            if exited_scope {
                self.pop_route_arm(lane_wire, scope);
            } else if let Some(arm) = route_arm {
                let _ = self.set_route_arm(lane_wire, scope, arm);
            }
            if exited_scope {
                self.clear_scope_evidence(scope);
            }
        }

        // If we rewound into a parent linger scope, sync its lane cursor to the
        // entry eff_index so offer()/flow() can locate the next iteration.
        let mut parent_scope = scope;
        loop {
            let Some(parent) = self.cursor.scope_parent(parent_scope) else {
                break;
            };
            if !matches!(parent.kind(), ScopeKind::Route | ScopeKind::Loop) {
                break;
            }
            if let Some(parent_region) = self.cursor.scope_region_by_id(parent) {
                if parent.kind() == ScopeKind::Route
                    && !parent_region.linger
                    && self.cursor.index() >= parent_region.end
                {
                    self.pop_route_arm(lane_wire, parent);
                    self.clear_scope_evidence(parent);
                }
                if parent_region.linger && self.cursor.index() == parent_region.start {
                    if let Some(parent_arm) = self.route_arm_for(lane_wire, parent) {
                        if parent_arm == 0 {
                            if let Some(first_eff) =
                                self.cursor.scope_lane_first_eff(parent, lane_wire)
                            {
                                let lane_idx = lane_wire as usize;
                                self.set_lane_cursor_to_eff_index(lane_idx, first_eff);
                            }
                        }
                    }
                }
            }
            parent_scope = parent;
        }
        self.prune_route_state_to_cursor_path_for_lane(lane_wire);
    }

    /// Rendezvous id for the primary port.
    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.port().rv_id()
    }

    /// Get the primary lane's port (typically Lane 0).
    ///
    /// # Safety invariant
    /// The primary port is always retained by construction. This is enforced
    /// at attach time and preserved throughout the endpoint's lifetime.
    fn port(&self) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[self.primary_lane].is_some(),
            "port: primary lane {} has no port (invariant violation)",
            self.primary_lane
        );
        // SAFETY: Primary port is always present by construction invariant.
        // In release builds, unwrap_unchecked could be used, but we keep
        // expect for defense-in-depth.
        self.ports[self.primary_lane]
            .as_ref()
            .expect("cursor endpoint retains primary port")
    }

    /// Get port for a specific lane.
    ///
    /// # Panics
    /// Panics if the port for `lane_idx` was not acquired.
    pub(super) fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T, E> {
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
    pub(super) fn frontier_scratch_view(&self) -> FrontierScratchView {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        frontier_scratch_view_from_storage(
            scratch_ptr,
            layout,
            self.cursor.logical_lane_count(),
            self.cursor.max_frontier_entries(),
        )
    }

    pub(super) fn loop_index(scope: ScopeId) -> Option<u8> {
        u8::try_from(scope.ordinal()).ok()
    }

    #[inline]
    pub(super) fn offer_lanes_for_scope(&self, scope_id: ScopeId) -> ([u8; MAX_LANES], usize) {
        self.cursor
            .route_scope_offer_lane_list(scope_id)
            .unwrap_or(([0; MAX_LANES], 0))
    }

    #[inline]
    pub(super) fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let (lanes, len) = self.offer_lanes_for_scope(scope_id);
        if len == 0 {
            self.primary_lane as u8
        } else {
            lanes[0]
        }
    }

    pub(super) fn propagate_recvless_parent_route_decision(
        &mut self,
        child_scope: ScopeId,
        arm: u8,
    ) {
        let Some(parent_scope) = self.cursor.scope_parent(child_scope) else {
            return;
        };
        if parent_scope.kind() != ScopeKind::Route {
            return;
        }
        let Some(parent_region) = self.cursor.scope_region_by_id(parent_scope) else {
            return;
        };
        if !parent_region.linger {
            return;
        }
        if self.cursor.is_route_controller(parent_scope) {
            return;
        }
        let parent_is_dynamic = self
            .cursor
            .route_scope_controller_policy(parent_scope)
            .map(|(policy, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        if parent_is_dynamic {
            return;
        }
        let parent_requires_wire_recv = {
            let mut arm = 0u8;
            let mut requires_wire = false;
            while arm <= 1 {
                if self.arm_has_recv(parent_scope, arm) {
                    let label = self
                        .cursor
                        .controller_arm_entry_by_arm(parent_scope, arm)
                        .map(|(_, label)| label);
                    if let Some(label) = label {
                        if !self.is_non_wire_loop_control_recv(parent_scope, arm, label) {
                            requires_wire = true;
                            break;
                        }
                    } else {
                        requires_wire = true;
                        break;
                    }
                }
                if arm == 1 {
                    break;
                }
                arm += 1;
            }
            requires_wire
        };
        if parent_requires_wire_recv {
            return;
        }
        let Some(parent_arm) = Arm::new(arm) else {
            return;
        };
        self.record_scope_ack(parent_scope, RouteDecisionToken::from_ack(parent_arm));
        let parent_lane = self.offer_lane_for_scope(parent_scope);
        self.record_route_decision_for_lane(parent_lane as usize, parent_scope, parent_arm.as_u8());
        self.emit_route_decision(
            parent_scope,
            parent_arm.as_u8(),
            RouteDecisionSource::Ack,
            parent_lane,
        );
    }

    #[inline]
    pub(super) fn controller_arm_at_cursor(&self, scope_id: ScopeId) -> Option<u8> {
        let idx = self.cursor.index();
        if let Some((entry, _label)) = self.cursor.controller_arm_entry_by_arm(scope_id, 0)
            && idx == state_index_to_usize(entry)
        {
            return Some(0);
        }
        if let Some((entry, _label)) = self.cursor.controller_arm_entry_by_arm(scope_id, 1)
            && idx == state_index_to_usize(entry)
        {
            return Some(1);
        }
        None
    }

    fn is_non_wire_loop_control_recv(&self, scope_id: ScopeId, arm: u8, label: u8) -> bool {
        let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
        else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        let Some(recv_meta) = self.cursor.try_recv_meta_at(entry_idx) else {
            return false;
        };
        if !recv_meta.is_control || recv_meta.label != label {
            return false;
        }
        if recv_meta.peer == ROLE {
            return true;
        }
        // Passive observers model controller self-send loop control as cross-role
        // control recv nodes; treat these labels as non-wire arm selectors.
        !self.cursor.is_route_controller(scope_id)
            && self
                .control_semantic_kind(recv_meta.label, recv_meta.resource)
                .is_loop()
    }

    fn take_binding_for_lane(
        &mut self,
        lane_idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        let classification = self.binding_inbox.take_or_poll(&mut self.binding, lane_idx);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
        classification
    }

    pub(super) fn put_back_binding_for_lane(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
    ) {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        self.binding_inbox.put_back(lane_idx, classification);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
    }

    fn take_matching_binding_for_lane(
        &mut self,
        lane_idx: usize,
        expected_label: u8,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        let classification =
            self.binding_inbox
                .take_matching_or_poll(&mut self.binding, lane_idx, expected_label);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
        classification
    }

    fn take_matching_mask_binding_for_lane<F: FnMut(u8) -> bool>(
        &mut self,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
        drop_mismatch: F,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        let classification = self.binding_inbox.take_matching_mask_or_poll(
            &mut self.binding,
            lane_idx,
            label_mask,
            drop_label_mask,
            drop_mismatch,
        );
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
        classification
    }

    #[inline]
    fn take_binding_mask_ignoring_loop_control(
        &mut self,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
    ) -> Option<crate::binding::IncomingClassification> {
        let semantics = self.control_semantics();
        self.take_matching_mask_binding_for_lane(
            lane_idx,
            label_mask,
            drop_label_mask,
            move |label| semantics.is_loop_label(label),
        )
    }

    fn take_binding_for_selected_arm(
        &mut self,
        lane_idx: usize,
        selected_arm: u8,
        label_meta: ScopeLabelMeta,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
    ) -> (Option<crate::binding::Channel>, Option<u16>, bool) {
        let label_mask = label_meta.binding_demux_label_mask_for_arm(selected_arm);
        let drop_label_mask = self.loop_control_drop_label_mask();
        let mut channel = None;
        let mut instance = None;
        let mut has_fin = false;

        if let Some(classification) = binding_classification.take() {
            let label_bit = ScopeLabelMeta::label_bit(classification.label);
            if (label_mask & label_bit) != 0 {
                channel = Some(classification.channel);
                instance = Some(classification.instance);
                has_fin = classification.has_fin;
            } else {
                self.put_back_binding_for_lane(lane_idx, classification);
            }
        }

        if (channel.is_none() || instance.is_none())
            && let Some(classification) =
                self.take_binding_mask_ignoring_loop_control(lane_idx, label_mask, drop_label_mask)
        {
            if channel.is_none() {
                channel = Some(classification.channel);
            }
            if instance.is_none() {
                instance = Some(classification.instance);
            }
            if classification.has_fin {
                has_fin = true;
            }
        }

        (channel, instance, has_fin)
    }

    pub(super) fn poll_binding_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lane_mask == 0 {
            return None;
        }
        let preferred_arm = self
            .peek_scope_ack(scope_id)
            .map(|token| token.arm().as_u8());
        let mut label_mask = label_meta.preferred_binding_label_mask(preferred_arm);
        if label_mask == 0 && self.static_passive_scope_evidence_materializes_poll(scope_id) {
            label_mask = label_meta.binding_demux_label_mask_for_arm(0)
                | label_meta.binding_demux_label_mask_for_arm(1);
        }
        if label_mask == 0 {
            return None;
        }
        let authoritative_lane_mask = preferred_arm
            .map(|arm| materialization_meta.binding_demux_lane_mask(Some(arm)) & offer_lane_mask)
            .unwrap_or(0);
        let label_lane_mask = materialization_meta
            .binding_demux_lane_mask_for_label_mask(label_meta, label_mask)
            & offer_lane_mask;
        let base_lane_mask = if authoritative_lane_mask != 0 {
            authoritative_lane_mask
        } else if label_lane_mask != 0 {
            label_lane_mask
        } else {
            offer_lane_mask
        };
        if let Some(expected_label) = label_meta.preferred_binding_label(preferred_arm) {
            let buffered_lane_mask = self
                .binding_inbox
                .buffered_lane_mask_for_labels(ScopeLabelMeta::label_bit(expected_label))
                & offer_lane_mask;
            if let Some(picked) = self.poll_binding_exact_for_offer(
                offer_lane_idx,
                base_lane_mask | buffered_lane_mask,
                expected_label,
            ) {
                return Some(picked);
            }
        }
        let binding_lane_mask = base_lane_mask
            | (self.binding_inbox.buffered_lane_mask_for_labels(label_mask) & offer_lane_mask);
        if let Some(classification) =
            self.poll_binding_mask_for_offer(offer_lane_idx, binding_lane_mask, label_mask)
        {
            return Some(classification);
        }
        if self.static_passive_scope_evidence_materializes_poll(scope_id)
            && let Some((lane_idx, classification)) =
                self.poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
        {
            if self
                .static_passive_dispatch_arm_from_exact_label(
                    scope_id,
                    classification.label,
                    label_meta,
                )
                .is_some()
            {
                return Some((lane_idx, classification));
            }
            self.put_back_binding_for_lane(lane_idx, classification);
        }
        None
    }

    fn poll_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_mask: u128,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let drop_label_mask = self.loop_control_drop_label_mask();
        let matching_buffered_lane_mask =
            self.binding_inbox.buffered_lane_mask_for_labels(label_mask) & offer_lane_mask;
        if let Some(classification) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            matching_buffered_lane_mask,
            label_mask,
            drop_label_mask,
        ) {
            return Some(classification);
        }
        let drop_buffered_lane_mask = (self
            .binding_inbox
            .buffered_lane_mask_for_labels(drop_label_mask)
            & offer_lane_mask)
            & !matching_buffered_lane_mask;
        if let Some(classification) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            drop_buffered_lane_mask,
            label_mask,
            drop_label_mask,
        ) {
            return Some(classification);
        }
        self.poll_binding_mask_in_lane_mask(
            offer_lane_idx,
            offer_lane_mask & !(matching_buffered_lane_mask | drop_buffered_lane_mask),
            label_mask,
            drop_label_mask,
        )
    }

    fn poll_buffered_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        lane_mask: u8,
        label_mask: u128,
        drop_label_mask: u128,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if lane_mask == 0 {
            return None;
        }
        let mut remaining_lane_mask = lane_mask;
        while let Some(lane_slot) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut remaining_lane_mask)
        {
            if let Some(classification) =
                self.take_binding_mask_ignoring_loop_control(lane_slot, label_mask, drop_label_mask)
            {
                return Some((lane_slot, classification));
            }
        }
        None
    }

    fn poll_binding_mask_in_lane_mask(
        &mut self,
        offer_lane_idx: usize,
        lane_mask: u8,
        label_mask: u128,
        drop_label_mask: u128,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if lane_mask == 0 {
            return None;
        }
        let mut selected_lane_mask = lane_mask;
        let Some(lane_slot) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut selected_lane_mask)
        else {
            return None;
        };
        if let Some(classification) =
            self.take_binding_mask_ignoring_loop_control(lane_slot, label_mask, drop_label_mask)
        {
            // Classification is demux evidence only.
            return Some((lane_slot, classification));
        }
        None
    }

    fn poll_binding_exact_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        expected_label: u8,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lane_mask == 0 {
            return None;
        }
        let buffered_lane_mask = self
            .binding_inbox
            .buffered_lane_mask_for_labels(ScopeLabelMeta::label_bit(expected_label))
            & offer_lane_mask;
        if let Some(classification) =
            self.poll_binding_exact_in_lane_mask(offer_lane_idx, buffered_lane_mask, expected_label)
        {
            return Some(classification);
        }
        self.poll_binding_exact_in_lane_mask(
            offer_lane_idx,
            offer_lane_mask & !buffered_lane_mask,
            expected_label,
        )
    }

    fn poll_binding_exact_in_lane_mask(
        &mut self,
        offer_lane_idx: usize,
        lane_mask: u8,
        expected_label: u8,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if lane_mask == 0 {
            return None;
        }
        let mut remaining_lane_mask = lane_mask;
        while let Some(lane_idx) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut remaining_lane_mask)
        {
            if let Some(classification) =
                self.take_matching_binding_for_lane(lane_idx, expected_label)
            {
                return Some((lane_idx, classification));
            }
        }
        None
    }

    pub(super) fn poll_binding_any_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lane_mask == 0 {
            return None;
        }
        let mut remaining_lane_mask = offer_lane_mask;
        while let Some(lane_idx) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut remaining_lane_mask)
        {
            if let Some(classification) = self.take_binding_for_lane(lane_idx) {
                return Some((lane_idx, classification));
            }
        }
        None
    }

    pub(super) fn try_recv_from_binding(
        &mut self,
        logical_lane: u8,
        expected_label: u8,
        buf: &mut [u8],
    ) -> RecvResult<Option<usize>> {
        let lane_idx = logical_lane as usize;
        if let Some(classification) = self.take_matching_binding_for_lane(lane_idx, expected_label)
        {
            let n = self
                .binding
                .on_recv(classification.channel, buf)
                .map_err(RecvError::Binding)?;
            return Ok(Some(n));
        }
        Ok(None)
    }

    fn is_loop_control_scope(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
    ) -> bool {
        matches!(
            (
                controller_arm_semantic_kind(cursor, semantics, scope_id, 0),
                controller_arm_semantic_kind(cursor, semantics, scope_id, 1)
            ),
            (
                Some(ControlSemanticKind::LoopContinue),
                Some(ControlSemanticKind::LoopBreak)
            ) | (
                Some(ControlSemanticKind::LoopBreak),
                Some(ControlSemanticKind::LoopContinue)
            )
        )
    }

    fn parallel_scope_root(cursor: &PhaseCursor, mut scope_id: ScopeId) -> Option<ScopeId> {
        loop {
            if scope_id.kind() == ScopeKind::Parallel {
                return Some(scope_id);
            }
            let Some(parent) = cursor.scope_parent(scope_id) else {
                return None;
            };
            scope_id = parent;
        }
    }

    #[inline]
    fn frontier_kind_for_cursor(
        cursor: &PhaseCursor,
        scope_id: ScopeId,
        is_controller: bool,
    ) -> FrontierKind {
        Self::frontier_kind_for_index(cursor, scope_id, is_controller, cursor.index())
    }

    #[inline]
    fn frontier_kind_for_index(
        cursor: &PhaseCursor,
        scope_id: ScopeId,
        is_controller: bool,
        idx: usize,
    ) -> FrontierKind {
        if cursor.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch) {
            return FrontierKind::PassiveObserver;
        }
        let has_controller_entry = cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some();
        if !is_controller && !has_controller_entry {
            return FrontierKind::PassiveObserver;
        }
        if let Some(region) = cursor.scope_region_by_id(scope_id)
            && region.linger
        {
            return FrontierKind::Loop;
        }
        if Self::parallel_scope_root(cursor, scope_id).is_some() {
            return FrontierKind::Parallel;
        }
        FrontierKind::Route
    }

    #[inline]
    pub(super) fn scope_loop_meta(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
    ) -> ScopeLoopMeta {
        Self::scope_loop_meta_at(cursor, semantics, scope_id, cursor.index())
    }

    #[inline]
    pub(super) fn scope_loop_meta_at(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        idx: usize,
    ) -> ScopeLoopMeta {
        let mut flags = 0u8;
        if cursor.node_loop_scope(idx).is_some() {
            flags |= ScopeLoopMeta::FLAG_SCOPE_ACTIVE;
        }
        if cursor
            .scope_region_by_id(scope_id)
            .map(|region| region.linger)
            .unwrap_or(false)
        {
            flags |= ScopeLoopMeta::FLAG_SCOPE_LINGER;
        }
        if Self::is_loop_control_scope(cursor, semantics, scope_id) {
            flags |= ScopeLoopMeta::FLAG_CONTROL_SCOPE;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 0).is_some() {
            flags |= ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 1).is_some() {
            flags |= ScopeLoopMeta::FLAG_BREAK_HAS_RECV;
        }
        ScopeLoopMeta { flags }
    }

    #[inline]
    pub(super) fn scope_label_meta(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
    ) -> ScopeLabelMeta {
        Self::scope_label_meta_at(cursor, semantics, scope_id, loop_meta, cursor.index())
    }

    #[inline]
    pub(super) fn scope_label_meta_at(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
        idx: usize,
    ) -> ScopeLabelMeta {
        let is_controller = cursor.is_route_controller(scope_id);
        let mut meta = ScopeLabelMeta {
            #[cfg(test)]
            scope_id,
            loop_meta,
            ..ScopeLabelMeta::EMPTY
        };
        if let Some(recv_meta) = cursor.try_recv_meta_at(idx)
            && recv_meta.scope == scope_id
        {
            meta.recv_label = recv_meta.label;
            meta.flags |= ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL;
            if let Some(arm) = recv_meta.route_arm {
                meta.recv_arm = arm;
                meta.flags |= ScopeLabelMeta::FLAG_CURRENT_RECV_ARM;
                meta.record_arm_label(arm, recv_meta.label);
                if !Self::current_recv_is_scope_local(
                    cursor,
                    semantics,
                    scope_id,
                    loop_meta,
                    recv_meta.label,
                    recv_meta.resource,
                    arm,
                ) {
                    meta.flags |= ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED;
                }
            }
        }
        if let Some((_, label)) = cursor.controller_arm_entry_by_arm(scope_id, 0) {
            meta.controller_labels[0] = label;
            meta.flags |= ScopeLabelMeta::FLAG_CONTROLLER_ARM0;
            meta.record_arm_label(0, label);
            if !is_controller {
                meta.clear_evidence_arm_label(0, label);
            }
        }
        if let Some((_, label)) = cursor.controller_arm_entry_by_arm(scope_id, 1) {
            meta.controller_labels[1] = label;
            meta.flags |= ScopeLabelMeta::FLAG_CONTROLLER_ARM1;
            meta.record_arm_label(1, label);
            if !is_controller {
                meta.clear_evidence_arm_label(1, label);
            }
        }
        if loop_meta.loop_label_scope() {
            if let Some(label) = controller_arm_label(cursor, scope_id, 0) {
                meta.record_arm_label(0, label);
            }
            if let Some(label) = controller_arm_label(cursor, scope_id, 1) {
                meta.record_arm_label(1, label);
            }
        }
        let mut dispatch_idx = 0usize;
        while let Some((label, arm, _)) =
            cursor.route_scope_first_recv_dispatch_entry(scope_id, dispatch_idx)
        {
            meta.record_dispatch_arm_label(arm, label);
            dispatch_idx += 1;
        }
        meta
    }

    #[inline]
    fn offer_scope_label_meta(&self, scope_id: ScopeId, offer_lane_idx: usize) -> ScopeLabelMeta {
        if offer_lane_idx < MAX_LANES {
            let info = self.route_state.lane_offer_state(offer_lane_idx);
            if info.scope == scope_id {
                let entry_idx = state_index_to_usize(info.entry);
                if let Some(cached) =
                    RouteFrontierMachine::offer_entry_label_meta(self, scope_id, entry_idx)
                {
                    return cached;
                }
                let loop_meta = Self::scope_loop_meta_at(
                    &self.cursor,
                    &self.control_semantics(),
                    scope_id,
                    entry_idx,
                );
                return Self::scope_label_meta_at(
                    &self.cursor,
                    &self.control_semantics(),
                    scope_id,
                    loop_meta,
                    entry_idx,
                );
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_max() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if let Some(cached) =
                RouteFrontierMachine::offer_entry_label_meta(self, scope_id, entry_idx)
            {
                return cached;
            }
            let loop_meta = Self::scope_loop_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                entry_idx,
            );
            return Self::scope_label_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                loop_meta,
                entry_idx,
            );
        }
        let loop_meta = Self::scope_loop_meta(&self.cursor, &self.control_semantics(), scope_id);
        Self::scope_label_meta(&self.cursor, &self.control_semantics(), scope_id, loop_meta)
    }

    #[inline]
    fn offer_scope_materialization_meta(
        &self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
    ) -> ScopeArmMaterializationMeta {
        if offer_lane_idx < MAX_LANES {
            let info = self.route_state.lane_offer_state(offer_lane_idx);
            if info.scope == scope_id {
                if let Some(cached) = self
                    .offer_entry_materialization_meta(scope_id, state_index_to_usize(info.entry))
                {
                    return cached;
                }
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_max() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if let Some(cached) = self.offer_entry_materialization_meta(scope_id, entry_idx) {
                return cached;
            }
        }
        self.compute_scope_arm_materialization_meta(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_label_meta(
        &self,
        selection: OfferScopeSelection,
    ) -> ScopeLabelMeta {
        self.offer_scope_label_meta(selection.scope_id, selection.offer_lane_idx as usize)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_materialization_meta(
        &self,
        selection: OfferScopeSelection,
    ) -> ScopeArmMaterializationMeta {
        self.offer_scope_materialization_meta(selection.scope_id, selection.offer_lane_idx as usize)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_passive_recv_meta(
        &self,
        selection: OfferScopeSelection,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> [CachedRecvMeta; 2] {
        self.compute_scope_passive_recv_meta(
            materialization_meta,
            selection.scope_id,
            selection.offer_lane,
        )
    }

    fn frontier_static_facts_at(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        is_controller: bool,
        is_dynamic: bool,
        idx: usize,
    ) -> FrontierStaticFacts {
        let loop_meta = Self::scope_loop_meta_at(cursor, semantics, scope_id, idx);
        let controller_local_ready =
            is_controller && Self::scope_has_controller_arm_entry(cursor, scope_id);
        let cursor_ready = cursor.is_recv_at(idx)
            || cursor.try_recv_meta_at(idx).is_some()
            || cursor.try_local_meta_at(idx).is_some();
        FrontierStaticFacts {
            frontier: Self::frontier_kind_for_index(cursor, scope_id, is_controller, idx),
            ready: loop_meta.recvless_ready()
                || controller_local_ready
                || is_dynamic
                || cursor_ready,
        }
    }

    #[inline]
    fn ack_is_progress_evidence(loop_meta: ScopeLoopMeta, has_ack: bool) -> bool {
        has_ack && !loop_meta.control_scope()
    }

    pub(super) fn skip_unselected_arm_lanes(
        &mut self,
        scope: ScopeId,
        selected_arm: u8,
        skip_lane: u8,
    ) {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if lane_idx == skip_lane as usize {
                lane_idx += 1;
                continue;
            }
            let lane_wire = lane_idx as u8;
            if self
                .cursor
                .scope_lane_last_eff_for_arm(scope, selected_arm, lane_wire)
                .is_some()
            {
                lane_idx += 1;
                continue;
            }
            if let Some(eff_index) = self.cursor.scope_lane_last_eff(scope, lane_wire) {
                self.advance_lane_cursor(lane_idx, eff_index);
            }
            lane_idx += 1;
        }
    }

    pub(super) fn maybe_skip_remaining_route_arm(
        &mut self,
        scope: ScopeId,
        lane: u8,
        arm: Option<u8>,
        eff_index: EffIndex,
    ) {
        let Some(arm) = arm else {
            return;
        };
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return;
        }
        if let Some(last_arm_eff) = self.cursor.scope_lane_last_eff_for_arm(scope, arm, lane) {
            if last_arm_eff == eff_index {
                if let Some(scope_last) = self.cursor.scope_lane_last_eff(scope, lane) {
                    if scope_last != last_arm_eff {
                        self.advance_lane_cursor(lane as usize, scope_last);
                    }
                }
            }
        }
    }

    #[inline]
    pub(super) fn maybe_advance_phase(&mut self) {
        if self.cursor.is_phase_complete() && !self.has_active_linger_route() {
            if self.has_ready_frontier_candidate() {
                return;
            }
            self.advance_phase_skipping_inactive();
        }
    }

    fn phase_guard_mismatch(&self) -> bool {
        let Some(guard) = self.cursor.current_phase_route_guard() else {
            return false;
        };
        if guard.is_empty() {
            return false;
        }
        let Some(selected) = self.selected_arm_for_scope(guard.scope()) else {
            return false;
        };
        selected != guard.arm
    }

    fn has_active_linger_route(&self) -> bool {
        let phase_mask = self.cursor.current_phase_lane_mask();
        ((self.route_state.lane_linger_mask | self.route_state.lane_offer_linger_mask) & phase_mask)
            != 0
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
            self.public_generation = 0;
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
    pub(crate) fn mint_control_token<K>(
        &self,
        dest_role: u8,
        shot: CapShot,
        lane: Lane,
    ) -> SendResult<GenericCapToken<K>>
    where
        K: ResourceKind + crate::control::cap::mint::SessionScopedKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsCanonical,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .canonical_session_token::<K, StoredMint<Mint>>(
                self.rendezvous_id(),
                self.sid,
                lane,
                dest_role,
                shot,
                self.mint,
            )
            .ok_or(SendError::PhaseInvariant)
    }

    pub(crate) fn mint_control_token_with_handle<K>(
        &self,
        dest_role: u8,
        shot: CapShot,
        lane: Lane,
        handle: K::Handle,
    ) -> SendResult<GenericCapToken<K>>
    where
        K: ResourceKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsCanonical,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .canonical_token_with_handle::<K, StoredMint<Mint>>(
                self.rendezvous_id(),
                self.sid,
                lane,
                dest_role,
                shot,
                handle,
                self.mint,
            )
            .ok_or(SendError::PhaseInvariant)
    }

    fn splice_handle_from_operands(operands: SpliceOperands) -> SpliceHandle {
        let mut flags = 0u16;
        if operands.seq_tx != 0 || operands.seq_rx != 0 {
            flags |= splice_flags::FENCES_PRESENT;
        }
        SpliceHandle {
            src_rv: operands.src_rv.raw(),
            dst_rv: operands.dst_rv.raw(),
            src_lane: operands.src_lane.raw() as u16,
            dst_lane: operands.dst_lane.raw() as u16,
            old_gen: operands.old_gen.raw(),
            new_gen: operands.new_gen.raw(),
            seq_tx: operands.seq_tx,
            seq_rx: operands.seq_rx,
            flags,
        }
    }
}
pub trait CanonicalTokenProvider<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, B>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    >;
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for NoControl
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline(always)]
    fn into_token(
        _endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        _meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    > {
        Ok(None)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, K, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for ExternalControl<K>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind:
        ResourceKind + ControlMint,
    K: ResourceKind,
    B: BindingSlot,
{
    /// External control: behavior depends on `K::AUTO_MINT_EXTERNAL`.
    ///
    /// - When `AUTO_MINT_EXTERNAL = true` (e.g., SpliceIntentKind):
    ///   Auto-mint a token with proper handle (sid/lane/scope) populated by the resolver.
    ///
    /// - When `AUTO_MINT_EXTERNAL = false` (default, e.g., LoadBeginKind):
    ///   Caller provides the token/payload directly; no auto-minting.
    #[inline(always)]
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    > {
        if K::AUTO_MINT_EXTERNAL {
            // Auto-mint for external splice kinds
            endpoint
                .canonical_control_token::<
                    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
                >(meta)
                .map(Some)
        } else {
            // Caller provides the payload directly
            Ok(None)
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, K, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for CanonicalControl<K>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind:
        ResourceKind + ControlMint,
    K: ResourceKind,
    B: BindingSlot,
{
    #[inline(always)]
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    > {
        endpoint
            .canonical_control_token::<
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >(meta)
            .map(Some)
    }
}
