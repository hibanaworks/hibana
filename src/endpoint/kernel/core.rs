//! Internal endpoint kernel built on top of `PhaseCursor`.
//!
//! The kernel endpoint owns the rendezvous port outright and advances
//! according to the typestate cursor obtained from `RoleProgram` projection.

use core::{convert::TryFrom, ops::ControlFlow, task::Poll};

use super::authority::{
    Arm, DeferReason, DeferSource, LoopDecision, RouteDecisionSource, RouteDecisionToken,
    RouteResolveStep, route_policy_input_arg0, validate_route_decision_scope,
};
use super::evidence::{ScopeEvidence, ScopeLabelMeta, ScopeLoopMeta};
use super::frontier::*;
use super::frontier_state::FrontierState;
use super::inbox::{BindingInbox, PackedIncomingClassification};
use super::lane_port;
use super::lane_slots::LaneSlotArray;
use super::layout::{EndpointArenaLayout, LeasedState};
use super::offer::RouteFrontierMachine;
use super::offer::*;
use super::route_state::RouteState;
use crate::binding::{BindingSlot, IncomingClassification, NoBinding};
use crate::eff::EffIndex;
use crate::global::ControlDesc;
#[cfg(test)]
use crate::global::LoopControlMeaning;
use crate::global::compiled::images::{ControlSemanticKind, ControlSemanticsTable};
use crate::global::const_dsl::{PolicyMode, ScopeId, ScopeKind};
use crate::global::role_program::LaneSetView;
use crate::global::typestate::{
    ARM_SHARED, JumpReason, LoopMetadata, LoopRole, PassiveArmNavigation, PhaseCursor, RecvMeta,
    SendMeta, StateIndex, state_index_to_usize,
};
#[cfg(test)]
use crate::global::{MessageSpec, SendableLabel};
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    control::{
        cap::atomic_codecs::{TopologyHandle, topology_flags},
        cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, LoopDecisionHandle, RouteArmHandle,
        },
        cap::{
            mint::{
                CAP_HANDLE_LEN, CAP_TOKEN_LEN, CapHeader, CapShot, ControlOp, ControlResourceKind,
                E0, EndpointEpoch, EpochTable, EpochTbl, GenericCapToken, MintConfigMarker, Owner,
                ResourceKind,
            },
            typed_tokens::{RawRegisteredCapToken, RegisteredTokenParts},
        },
        cluster::{
            core::{DynamicResolution, TopologyDescriptor, TopologyOperands},
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
    rendezvous::{
        capability::{CapEntry, CapReleaseCtx},
        core::EndpointLeaseId,
        port::Port,
        tables::LoopDisposition,
    },
    runtime::consts::LabelUniverse,
    transport::{
        Transport, TransportMetrics,
        trace::TapFrameMeta,
        wire::{FrameFlags, Payload},
    },
};

type PortStorage<'r, T, E> = LaneSlotArray<Port<'r, T, E>>;
type GuardStorage<'r, T, U, C> = LaneSlotArray<LaneGuard<'r, T, U, C>>;

type StoredMint<Mint> = crate::control::cap::mint::MintConfig<
    <Mint as MintConfigMarker>::Spec,
    <Mint as MintConfigMarker>::Policy,
>;

#[derive(Clone, Copy)]
pub(crate) struct PublicEndpointRevoke {
    revoke_for_session: unsafe fn(*mut (), SessionId, *mut Lane, usize) -> usize,
}

impl PublicEndpointRevoke {
    #[cfg(test)]
    pub(crate) const UNARMED: Self = Self {
        revoke_for_session: Self::unarmed,
    };

    #[inline]
    pub(crate) const fn new(
        revoke_for_session: unsafe fn(*mut (), SessionId, *mut Lane, usize) -> usize,
    ) -> Self {
        Self { revoke_for_session }
    }

    #[inline]
    pub(crate) unsafe fn revoke_for_session(
        self,
        endpoint: *mut (),
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize {
        unsafe { (self.revoke_for_session)(endpoint, sid, lanes, lane_capacity) }
    }

    #[inline]
    #[cfg(test)]
    unsafe fn unarmed(
        _endpoint: *mut (),
        _sid: SessionId,
        _lanes: *mut Lane,
        _lane_capacity: usize,
    ) -> usize {
        0
    }
}

#[derive(Clone, Copy)]
enum BindingLanePreference {
    Any,
    Arm(u8),
    LabelMask(u128),
}

#[cfg(test)]
use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

#[inline]
fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
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
    let (entry, _label) = cursor.shared_controller_arm_entry_by_arm(scope_id, arm)?;
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
fn endpoint_scope_label_meta<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    scope_id: ScopeId,
    loop_meta: ScopeLoopMeta,
) -> ScopeLabelMeta
where
    T: Transport,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
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
    fn route_policy_enforces_scope_match_before_route_handle() {
        let scope = ScopeId::generic(12);
        let err = validate_route_decision_scope(scope, ScopeId::generic(13))
            .expect_err("scope mismatch must fail");
        assert!(matches!(err, SendError::PhaseInvariant));
    }

    #[test]
    fn route_policy_rejects_empty_scope() {
        let err = validate_route_decision_scope(ScopeId::none(), ScopeId::none())
            .expect_err("route scope is required");
        assert!(matches!(err, SendError::PhaseInvariant));
    }

    #[test]
    fn route_policy_allows_static_route_scope_without_policy_scope() {
        let scope = ScopeId::generic(18);
        validate_route_decision_scope(scope, ScopeId::none())
            .expect("static route scope should remain valid without policy scope");
        validate_route_decision_scope(scope, scope)
            .expect("dynamic route scope should match policy scope");
    }
}

#[cfg(test)]
mod send_rollback_tests {
    use super::{PendingCapRelease, RawCapFlowToken, StagedDispatchToken};
    use crate::{
        control::cap::{
            mint::{CAP_NONCE_LEN, CAP_TOKEN_LEN, CapShot, ResourceKind},
            resource_kinds::{LoopContinueKind, LoopDecisionHandle},
        },
        global::const_dsl::ScopeId,
        rendezvous::{
            capability::{CapEntry, CapReleaseCtx, CapTable},
            tables::StateSnapshotTable,
        },
        substrate::{Lane, SessionId},
    };
    use core::cell::Cell;
    use std::vec;

    fn provisional_release_ctx(
        lane: Lane,
    ) -> (CapTable, StateSnapshotTable, Cell<u64>, std::vec::Vec<u8>) {
        let table = CapTable::new();
        let mut snapshot_storage = vec![0u8; StateSnapshotTable::storage_bytes(1)];
        let mut snapshots = StateSnapshotTable::empty();
        unsafe {
            snapshots.bind_from_storage(snapshot_storage.as_mut_ptr(), lane.raw(), 1);
        }
        (table, snapshots, Cell::new(0), snapshot_storage)
    }

    #[test]
    fn dropping_staged_dispatch_token_releases_provisional_capability() {
        let sid = SessionId::new(42);
        let lane = Lane::new(3);
        let role = 0u8;
        let nonce = [0xAB; CAP_NONCE_LEN];
        let handle = LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.raw() as u16,
            scope: ScopeId::loop_scope(2),
        };
        let handle_bytes = LoopContinueKind::encode_handle(&handle);
        let (table, snapshots, revisions, _snapshot_storage) = provisional_release_ctx(lane);

        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: LoopContinueKind::TAG,
                shot_state: CapShot::Many.as_u8(),
                role,
                mint_revision: 1,
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: handle_bytes,
            })
            .expect("insert succeeds");

        drop(StagedDispatchToken {
            token: RawCapFlowToken::from_bytes([0u8; CAP_TOKEN_LEN]),
            rollback: PendingCapRelease::new(
                nonce,
                CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
            ),
        });

        assert!(
            table
                .claim_by_nonce(
                    &nonce,
                    sid,
                    lane,
                    LoopContinueKind::TAG,
                    role,
                    CapShot::Many,
                    2,
                )
                .is_err(),
            "dropping the staged token must release provisional authority"
        );
    }

    #[test]
    fn disarming_staged_dispatch_token_preserves_provisional_capability() {
        let sid = SessionId::new(43);
        let lane = Lane::new(4);
        let role = 1u8;
        let nonce = [0xCD; CAP_NONCE_LEN];
        let handle = LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.raw() as u16,
            scope: ScopeId::loop_scope(3),
        };
        let handle_bytes = LoopContinueKind::encode_handle(&handle);
        let (table, snapshots, revisions, _snapshot_storage) = provisional_release_ctx(lane);

        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: LoopContinueKind::TAG,
                shot_state: CapShot::Many.as_u8(),
                role,
                mint_revision: 1,
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: handle_bytes,
            })
            .expect("insert succeeds");

        let mut token = StagedDispatchToken {
            token: RawCapFlowToken::from_bytes([0u8; CAP_TOKEN_LEN]),
            rollback: PendingCapRelease::new(
                nonce,
                CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
            ),
        };
        token.rollback.disarm();
        drop(token);

        assert!(
            table
                .claim_by_nonce(
                    &nonce,
                    sid,
                    lane,
                    LoopContinueKind::TAG,
                    role,
                    CapShot::Many,
                    2,
                )
                .is_ok(),
            "disarming rollback must keep authority live for the registered owner"
        );
    }

    #[test]
    fn inert_explicit_dispatch_token_does_not_release_live_capability() {
        let sid = SessionId::new(44);
        let lane = Lane::new(5);
        let role = 0u8;
        let nonce = [0xEF; CAP_NONCE_LEN];
        let handle = LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.raw() as u16,
            scope: ScopeId::loop_scope(4),
        };
        let handle_bytes = LoopContinueKind::encode_handle(&handle);
        let (table, _snapshots, _revisions, _snapshot_storage) = provisional_release_ctx(lane);

        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: LoopContinueKind::TAG,
                shot_state: CapShot::Many.as_u8(),
                role,
                mint_revision: 1,
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: handle_bytes,
            })
            .expect("insert succeeds");

        let mut rollback = PendingCapRelease::inert();
        let parts = rollback.take_registered_parts([0u8; CAP_TOKEN_LEN]);
        assert!(
            parts.is_none(),
            "explicit payload tokens must not fabricate registered capability ownership"
        );
        drop(rollback);

        assert!(
            table
                .claim_by_nonce(
                    &nonce,
                    sid,
                    lane,
                    LoopContinueKind::TAG,
                    role,
                    CapShot::Many,
                    2,
                )
                .is_ok(),
            "inert explicit payload rollback must not release unrelated live authority"
        );
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
#[repr(C)]
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
    B: BindingSlot + 'r,
{
    pub(super) public_revoke: PublicEndpointRevoke,
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    pub(super) ports: PortStorage<'r, T, E>,
    /// Multi-lane guard array. Each active lane has its own guard.
    pub(super) guards: GuardStorage<'r, T, U, C>,
    /// Primary lane index (first live application lane, not always lane 0).
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
    pub(in crate::endpoint) public_offer_state: OfferState<'r>,
    pub(in crate::endpoint) public_route_branch: Option<MaterializedRouteBranch<'r>>,
    pub(in crate::endpoint) public_recv_state: super::recv::RecvState,
    pub(in crate::endpoint) public_decode_state: super::decode::DecodeState<'r>,
    pub(in crate::endpoint) public_send_state: SendState<'r>,
    pub(super) control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    pub(super) route_state: LeasedState<RouteState>,
    pub(super) frontier_state: LeasedState<FrontierState>,
    pub(super) binding_inbox: LeasedState<BindingInbox>,
    pub(super) restored_binding_payload: Option<RestoredBindingPayload<'r>>,
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
    B: BindingSlot + 'r,
> where
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
{
    pub(super) label: u8,
    pub(super) binding_classification: PackedIncomingClassification,
    pub(super) staged_payload: Option<StagedPayload<'r>>,
    pub(super) branch_meta: BranchMeta,
    pub(super) _cfg: core::marker::PhantomData<fn() -> (&'r T, U, C, E, Mint, B)>,
}

#[derive(Clone, Copy)]
pub(crate) struct MaterializedRouteBranch<'r> {
    pub(crate) label: u8,
    pub(in crate::endpoint::kernel) binding_classification: PackedIncomingClassification,
    pub(crate) staged_payload: Option<StagedPayload<'r>>,
    pub(crate) branch_meta: BranchMeta,
}

impl<'r> MaterializedRouteBranch<'r> {
    #[inline]
    pub(crate) const fn label(&self) -> u8 {
        self.label
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StagedPayload<'a> {
    Transport { lane: u8, payload: Payload<'a> },
    Binding { lane: u8, payload: Payload<'a> },
}

#[derive(Clone, Copy)]
pub(super) struct RestoredBindingPayload<'a> {
    lane: u8,
    classification: PackedIncomingClassification,
    payload: Payload<'a>,
}

impl<'a> RestoredBindingPayload<'a> {
    #[inline]
    fn matches(self, lane_idx: usize, classification: IncomingClassification) -> bool {
        let restored = self.classification.decode();
        self.lane as usize == lane_idx
            && restored.label == classification.label
            && restored.instance == classification.instance
            && restored.has_fin == classification.has_fin
            && restored.channel == classification.channel
    }
}

impl<'a> StagedPayload<'a> {
    #[inline]
    pub(super) const fn payload(self) -> Payload<'a> {
        match self {
            Self::Transport { payload, .. } | Self::Binding { payload, .. } => payload,
        }
    }
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
    B: BindingSlot + 'r,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            label: self.label,
            binding_classification: self.binding_classification,
            staged_payload: self.staged_payload,
            branch_meta: self.branch_meta,
            _cfg: core::marker::PhantomData,
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    From<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> for MaterializedRouteBranch<'r>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    fn from(branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>) -> Self {
        Self {
            label: branch.label,
            binding_classification: branch.binding_classification,
            staged_payload: branch.staged_payload,
            branch_meta: branch.branch_meta,
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> From<MaterializedRouteBranch<'r>>
    for RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    fn from(branch: MaterializedRouteBranch<'r>) -> Self {
        Self {
            label: branch.label,
            binding_classification: branch.binding_classification,
            staged_payload: branch.staged_payload,
            branch_meta: branch.branch_meta,
            _cfg: core::marker::PhantomData,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RawCapFlowToken {
    bytes: [u8; CAP_TOKEN_LEN],
}

impl RawCapFlowToken {
    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn from_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self { bytes }
    }

    #[inline(always)]
    pub(crate) fn bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.bytes
    }
}

struct PreparedSendControl {
    minted_control: Option<MintedControlToken>,
    dispatch: Option<DescriptorDispatch>,
    stage_payload: StageSendPayloadFn,
}

#[derive(Clone, Copy)]
struct DescriptorDispatch {
    desc: ControlDesc,
    scope_id: u16,
    epoch: u16,
}

impl DescriptorDispatch {
    #[inline(always)]
    const fn new(desc: ControlDesc, scope: ScopeId, epoch: u16) -> Self {
        Self {
            desc,
            scope_id: scope.local_ordinal(),
            epoch,
        }
    }
}

struct MintedControlToken {
    token: RawCapFlowToken,
    dispatch: DescriptorDispatch,
    rollback: PendingCapRelease,
}

struct PendingCapRelease {
    nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
    release_ctx: Option<CapReleaseCtx>,
}

impl PendingCapRelease {
    #[inline(always)]
    fn new(
        nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
        release_ctx: CapReleaseCtx,
    ) -> Self {
        Self {
            nonce,
            release_ctx: Some(release_ctx),
        }
    }

    #[cfg(test)]
    #[inline(always)]
    fn disarm(&mut self) {
        self.release_ctx = None;
        self.nonce.fill(0);
    }

    #[inline(always)]
    fn inert() -> Self {
        Self {
            nonce: [0u8; crate::control::cap::mint::CAP_NONCE_LEN],
            release_ctx: None,
        }
    }

    #[inline(always)]
    fn take_registered_parts(
        &mut self,
        bytes: [u8; CAP_TOKEN_LEN],
    ) -> Option<RegisteredTokenParts> {
        let release_ctx = self.release_ctx.take()?;
        let nonce = self.nonce;
        self.nonce.fill(0);
        Some(RegisteredTokenParts::from_registered_bytes(
            bytes,
            nonce,
            release_ctx,
        ))
    }
}

impl Drop for PendingCapRelease {
    fn drop(&mut self) {
        if let Some(release_ctx) = self.release_ctx.take() {
            release_ctx.release(&self.nonce);
        }
        self.nonce.fill(0);
    }
}

struct StagedDispatchToken {
    token: RawCapFlowToken,
    rollback: PendingCapRelease,
}

impl StagedDispatchToken {
    #[inline(always)]
    fn bytes(&self) -> [u8; CAP_TOKEN_LEN] {
        self.token.bytes()
    }
}

enum StagedControlEmission {
    None,
    Registered(StagedDispatchToken),
    Emitted {
        dispatch_token: StagedDispatchToken,
        return_emitted: bool,
    },
}

impl StagedControlEmission {
    #[inline(always)]
    fn dispatch_token_bytes(&self) -> Option<[u8; CAP_TOKEN_LEN]> {
        match self {
            Self::None => None,
            Self::Registered(token)
            | Self::Emitted {
                dispatch_token: token,
                ..
            } => Some(token.bytes()),
        }
    }
}

enum DispatchSendTokenResult {
    None,
    Emitted,
    Registered(RegisteredTokenParts),
}

type StageSendPayloadFn = for<'payload, 'scratch> fn(
    Option<MintedControlToken>,
    Option<lane_port::RawSendPayload>,
    &'scratch mut [u8],
) -> SendResult<StagedSendPayload>;

type EncodeControlHandleFn = fn(SessionId, Lane, ScopeId) -> [u8; CAP_HANDLE_LEN];

struct StagedSendPayload {
    encoded_len: usize,
    control: StagedControlEmission,
}

pub(crate) struct SendTransportEmission {
    control: StagedControlEmission,
    dispatch: Option<DescriptorDispatch>,
}

impl SendTransportEmission {
    #[inline(always)]
    const fn empty() -> Self {
        Self {
            control: StagedControlEmission::None,
            dispatch: None,
        }
    }
}

pub(crate) struct PendingSendIo<'r> {
    transport: lane_port::PendingSend<'r>,
    lane_idx: usize,
    control: Option<StagedControlEmission>,
    dispatch: Option<DescriptorDispatch>,
}

enum SendTransportStep<'r> {
    Immediate(SendTransportEmission),
    Pending(PendingSendIo<'r>),
}

enum SendInitOutcome<'r> {
    Ready(SendResult<SendControlOutcome<'r>>),
    Pending {
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        pending: PendingSendIo<'r>,
    },
    Commit {
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission,
    },
}

pub enum SendControlOutcome<'rv> {
    None,
    Registered(RawRegisteredCapToken<'rv>),
    Emitted(RawCapFlowToken),
}

#[derive(Clone, Copy)]
pub(crate) struct SendDesc {
    label: u8,
    expects_control: bool,
    control: Option<ControlDesc>,
    encode_control_handle: Option<EncodeControlHandleFn>,
}

impl SendDesc {
    #[inline]
    pub(crate) const fn new(
        label: u8,
        expects_control: bool,
        control: Option<ControlDesc>,
        encode_control_handle: Option<EncodeControlHandleFn>,
    ) -> Self {
        Self {
            label,
            expects_control,
            control,
            encode_control_handle,
        }
    }

    #[inline]
    pub(crate) const fn label(self) -> u8 {
        self.label
    }

    #[inline]
    pub(crate) const fn control(self) -> Option<ControlDesc> {
        self.control
    }

    #[inline]
    pub(crate) const fn encode_control_handle(self) -> Option<EncodeControlHandleFn> {
        self.encode_control_handle
    }
}

pub(crate) enum SendState<'r> {
    Init {
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        payload: Option<lane_port::RawSendPayload>,
    },
    Sending {
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        pending: PendingSendIo<'r>,
    },
    Committing {
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission,
    },
    Done,
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
    B: BindingSlot + 'r,
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
    pub(in crate::endpoint) fn restore_materialized_route_branch(
        &mut self,
        mut branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) {
        let binding_classification =
            PackedIncomingClassification::take(&mut branch.binding_classification);
        match branch.staged_payload {
            Some(StagedPayload::Binding { lane, payload }) => {
                if let Some(classification) = binding_classification {
                    self.restore_binding_payload_for_lane(lane as usize, classification, payload);
                } else {
                    debug_assert!(
                        false,
                        "binding staged payload must keep its classification until restore"
                    );
                }
            }
            Some(StagedPayload::Transport { lane, .. }) => {
                if let Some(classification) = binding_classification {
                    self.put_back_binding_for_lane(
                        branch.branch_meta.lane_wire as usize,
                        classification,
                    );
                }
                let port = self.port_for_lane(lane as usize);
                lane_port::requeue_recv(port);
            }
            None => {
                if let Some(classification) = binding_classification {
                    self.put_back_binding_for_lane(
                        branch.branch_meta.lane_wire as usize,
                        classification,
                    );
                }
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_offer_state(&mut self) {
        self.public_offer_state = OfferState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn restore_public_route_branch(&mut self) {
        if let Some(branch) = self.public_route_branch.take() {
            self.restore_materialized_route_branch(branch.into());
        }
    }

    #[inline]
    pub(in crate::endpoint) fn init_public_send_state(
        &mut self,
        preview: SendPreview,
        payload: Option<lane_port::RawSendPayload>,
    ) {
        let (meta, preview_cursor_index) = preview.into_parts();
        self.public_send_state = SendState::Init {
            meta,
            preview_cursor_index: Some(preview_cursor_index),
            payload,
        };
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_send_state(&mut self) {
        let state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        if let SendState::Sending { mut pending, .. } = state {
            let port = self.port_for_lane(pending.lane_idx);
            lane_port::cancel_send_outgoing(&mut pending.transport, port);
        }
    }

    #[inline]
    pub(in crate::endpoint) fn init_public_recv_state(&mut self) {
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_recv_state(&mut self) {
        self.public_recv_state = super::recv::RecvState::new();
    }

    #[inline]
    pub(in crate::endpoint) fn begin_public_decode_state(&mut self) {
        if let Some(branch) = self.public_route_branch.take() {
            self.public_decode_state = super::decode::DecodeState::new(branch);
        } else {
            self.public_decode_state = super::decode::DecodeState::empty();
        }
    }

    #[inline]
    pub(in crate::endpoint) fn reset_public_decode_state(&mut self) {
        if self.public_decode_state.restore_on_drop
            && let Some(branch) = self.public_decode_state.branch.take()
        {
            self.restore_materialized_route_branch(branch.into());
        }
        self.public_decode_state = super::decode::DecodeState::empty();
    }
    #[inline]
    pub(in crate::endpoint) fn poll_public_offer(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<u8>> {
        if let Some(branch) = self.public_route_branch.as_ref() {
            return Poll::Ready(Ok(branch.label()));
        }
        let mut offer_state = core::mem::replace(&mut self.public_offer_state, OfferState::new());
        let poll = self.poll_offer_state(&mut offer_state, cx);
        match poll {
            Poll::Pending => {
                self.public_offer_state = offer_state;
                Poll::Pending
            }
            Poll::Ready(Ok(branch)) => {
                self.public_offer_state = OfferState::new();
                debug_assert!(
                    self.public_route_branch.is_none(),
                    "public route branch slot must be empty before offer materializes a new branch"
                );
                if self.public_route_branch.is_some() {
                    Poll::Ready(Err(RecvError::PhaseInvariant))
                } else {
                    let label = branch.label();
                    self.public_route_branch = Some(branch);
                    Poll::Ready(Ok(label))
                }
            }
            Poll::Ready(Err(err)) => {
                self.public_offer_state = OfferState::new();
                Poll::Ready(Err(err))
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_recv(
        &mut self,
        descriptor: super::recv::RecvDesc,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        let mut recv_state =
            core::mem::replace(&mut self.public_recv_state, super::recv::RecvState::new());
        match self.poll_recv_state(descriptor, &mut recv_state, cx) {
            Poll::Pending => {
                self.public_recv_state = recv_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.public_recv_state = super::recv::RecvState::new();
                Poll::Ready(result)
            }
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_decode(
        &mut self,
        descriptor: super::decode::DecodeDesc,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        let mut decode_state = core::mem::replace(
            &mut self.public_decode_state,
            super::decode::DecodeState::empty(),
        );
        match self.poll_decode_state(descriptor, &mut decode_state, cx) {
            Poll::Pending => {
                self.public_decode_state = decode_state;
                Poll::Pending
            }
            Poll::Ready(result) => match result {
                Ok(payload) => {
                    self.public_decode_state = super::decode::DecodeState::empty();
                    Poll::Ready(Ok(payload))
                }
                Err(err) => {
                    self.public_decode_state = decode_state;
                    Poll::Ready(Err(err))
                }
            },
        }
    }

    #[inline]
    pub(in crate::endpoint) fn poll_public_send(
        &mut self,
        descriptor: SendDesc,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendControlOutcome<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut send_state = core::mem::replace(&mut self.public_send_state, SendState::Done);
        match self.poll_send_state(descriptor, &mut send_state, cx) {
            Poll::Pending => {
                self.public_send_state = send_state;
                Poll::Pending
            }
            Poll::Ready(result) => {
                self.public_send_state = SendState::Done;
                Poll::Ready(result)
            }
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
    pub(super) const fn control_semantic_kind(
        &self,
        semantic: ControlSemanticKind,
    ) -> ControlSemanticKind {
        semantic
    }

    #[inline]
    fn loop_control_drop_label_mask(&self) -> u128 {
        ScopeLabelMeta::label_bit(LoopContinueKind::LABEL)
            | ScopeLabelMeta::label_bit(LoopBreakKind::LABEL)
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
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let is_linger = self.is_linger_route(scope);
        let Some(scope_slot) = self.scope_slot_for_route(scope) else {
            return Err(RecvError::PhaseInvariant);
        };
        self.route_state
            .set_route_arm(lane_idx, scope, scope_slot, arm, is_linger)
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
            lane_idx < self.cursor.logical_lane_count(),
            "pop_route_arm: lane {} exceeds logical lane count {}",
            lane_idx,
            self.cursor.logical_lane_count()
        );
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        let is_linger = self.is_linger_route(scope);
        let Some(scope_slot) = self.scope_slot_for_route(scope) else {
            return;
        };
        if self
            .route_state
            .pop_route_arm(lane_idx, scope, scope_slot, is_linger)
        {
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    fn scope_is_descendant_of(&self, scope: ScopeId, ancestor: ScopeId) -> bool {
        self.route_ancestor_arm(scope, ancestor).is_some()
    }

    fn route_ancestor_arm(&self, scope: ScopeId, ancestor: ScopeId) -> Option<u8> {
        if scope.is_none() || ancestor.is_none() || scope == ancestor {
            return None;
        }
        let mut current = scope;
        while let Some(parent) = self.cursor.route_parent_scope(current) {
            let arm = self.cursor.route_parent_arm(current)?;
            if parent == ancestor {
                return Some(arm);
            }
            current = parent;
        }
        None
    }

    fn clear_descendant_route_state_for_lane(&mut self, lane: u8, ancestor_scope: ScopeId) {
        if ancestor_scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        if self.route_state.lane_route_arm_len(lane_idx) == 0 {
            return;
        }
        while let Some(scope) = self.route_state.last_lane_scope(lane_idx) {
            if scope.is_none()
                || scope.kind() != ScopeKind::Route
                || !self.scope_is_descendant_of(scope, ancestor_scope)
            {
                break;
            }
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    fn prune_route_state_to_cursor_path_for_lane(&mut self, lane: u8) {
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        if self.route_state.lane_route_arm_len(lane_idx) == 0 {
            return;
        }
        let cursor_scope = self.cursor.node_scope_id();
        while let Some(scope) = self.route_state.last_lane_scope(lane_idx) {
            let keep = !scope.is_none()
                && (scope == cursor_scope || self.scope_is_descendant_of(cursor_scope, scope));
            if keep || scope.is_none() {
                break;
            }
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    pub(in crate::endpoint::kernel) fn clear_scope_route_state_for_other_lanes(
        &mut self,
        scope: ScopeId,
        keep_lane: u8,
    ) {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if lane_idx != keep_lane as usize {
                let lane_wire = lane_idx as u8;
                self.clear_descendant_route_state_for_lane(lane_wire, scope);
                self.pop_route_arm(lane_wire, scope);
            }
            lane_idx += 1;
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
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        self.route_state.route_arm_for(lane_idx, scope)
    }

    pub(super) fn selected_arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let scope_slot = self.scope_slot_for_route(scope)?;
        self.route_state.selected_arm_for_scope_slot(scope_slot)
    }

    pub(super) fn route_scope_offer_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
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
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let Some(summary_lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) else {
            return None;
        };
        self.preview_scope_ack_token_non_consuming(scope_id, summary_lane_idx, offer_lanes)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    fn structural_arm_for_child_scope(
        &self,
        parent_scope: ScopeId,
        child_scope: ScopeId,
    ) -> Option<u8> {
        self.route_ancestor_arm(child_scope, parent_scope)
    }

    #[inline]
    pub(super) fn current_offer_scope_id(&self) -> ScopeId {
        let node_scope = self.cursor.node_scope_id();
        if node_scope.is_none() {
            return node_scope;
        }
        let mut child_scope = node_scope;
        while let Some(parent_scope) = self.cursor.route_parent_scope(child_scope) {
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
            if self.route_ancestor_arm(child_scope, parent_scope) != Some(parent_arm) {
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
                let Some(parent_scope) = self.cursor.route_parent_scope(child_scope) else {
                    break 'rebase;
                };
                if parent_scope == stop_scope {
                    break 'rebase;
                }
                if parent_scope.kind() == ScopeKind::Route
                    && let Some(parent_arm) = self
                        .selected_arm_for_scope(parent_scope)
                        .or_else(|| self.preview_selected_arm_for_scope(parent_scope))
                    && self.route_ancestor_arm(child_scope, parent_scope) != Some(parent_arm)
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

    pub(super) fn ensure_current_route_arm_state(&mut self) -> RecvResult<Option<bool>> {
        let Some(region) = self.cursor.scope_region() else {
            return Ok(None);
        };
        if region.kind != ScopeKind::Route {
            return Ok(None);
        }
        let Some(current_arm) = self.cursor.typestate_node(self.cursor.index()).route_arm() else {
            return Ok(None);
        };
        if self.cursor.index() == region.start && self.cursor.is_route_controller(region.scope_id) {
            return Ok(None);
        }
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
    pub(super) fn emit_policy_defer_event(
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

    pub(super) fn emit_endpoint_policy_audit(
        &self,
        slot: PolicySlot,
        event_id: u16,
        arg0: u32,
        arg1: u32,
        lane: Lane,
    ) {
        let port = self.port_for_lane(lane.raw() as usize);
        let event = events::RawEvent::new(port.now32(), event_id)
            .with_arg0(arg0)
            .with_arg1(arg1);
        let _ = port.flush_transport_events();
        let transport_attrs = port.transport().metrics().attrs();
        let signals = self.policy_signals_for_slot(slot);
        let mut policy_attrs = *signals.attrs();
        policy_attrs.copy_from(&transport_attrs);
        let policy_input = signals.input;
        let policy_digest = port.policy_digest(slot);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_attrs.hash32();
        let transport_snapshot_hash = policy_runtime::hash_transport_attrs(&policy_attrs);
        let replay_transport = policy_runtime::replay_transport_inputs(&policy_attrs);
        let replay_transport_presence = policy_runtime::replay_transport_presence(&policy_attrs);
        let slot_id = policy_runtime::slot_tag(slot);
        let mode_id = policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            policy_attrs_hash,
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
        let verdict = policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            policy_runtime::verdict_reason(verdict) as u32,
            policy_runtime::POLICY_FUEL_NONE as u32,
            lane,
        );
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
        if let Some((preview_lane, preview_scope, _)) = preview_route_arm
            && preview_scope == scope_id
            && (preview_lane as usize) < self.cursor.logical_lane_count()
        {
            return self.preview_route_arm_for(preview_lane, scope_id, preview_route_arm);
        }
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let Some(summary_lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) else {
            return None;
        };
        self.preview_scope_ack_token_non_consuming(scope_id, summary_lane_idx, offer_lanes)
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
    pub(crate) fn preview_flow_meta(
        &mut self,
        target_label: u8,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
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
                    let offer_lanes = self.offer_lane_set_for_scope(scope_id);
                    let preview_arm = offer_lanes
                        .first_set(self.cursor.logical_lane_count())
                        .and_then(|summary_lane_idx| {
                            self.preview_scope_ack_token_non_consuming(
                                scope_id,
                                summary_lane_idx,
                                offer_lanes,
                            )
                            .map(|token| token.arm().as_u8())
                        });
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
                    local.semantic,
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

    #[cfg(test)]
    pub(super) fn preview_flow<M>(&mut self) -> SendResult<crate::endpoint::kernel::SendPreview>
    where
        M: MessageSpec + SendableLabel,
        T: Transport + 'r,
        U: LabelUniverse,
        C: crate::runtime::config::Clock,
        E: EpochTable,
        Mint: MintConfigMarker,
    {
        self.preview_flow_meta(<M as MessageSpec>::LABEL)
    }

    fn evaluate_dynamic_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        control: Option<ControlDesc>,
    ) -> SendResult<()> {
        if !meta.policy().is_dynamic() {
            return Ok(());
        }
        if let Some(control) = control
            && control_policy_is_validated_during_handle_preparation(control.op())
        {
            return Ok(());
        }
        let dynamic_kind = self.control_semantic_kind(meta.semantic);
        let route_signals = self.policy_signals_for_slot(PolicySlot::Route).into_owned();
        match dynamic_kind {
            ControlSemanticKind::LoopContinue | ControlSemanticKind::LoopBreak => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_loop_policy(meta, op, &route_signals)
            }
            ControlSemanticKind::RouteArm => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_route_policy(meta, target_label, op, &route_signals)
            }
            ControlSemanticKind::Other => {
                if control.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                let op = if meta.scope.is_none() {
                    ControlOp::RouteDecision
                } else {
                    self.cursor
                        .route_scope_controller_policy(meta.scope)
                        .map(|(_, _, _, op)| op)
                        .unwrap_or(ControlOp::RouteDecision)
                };
                self.evaluate_route_policy(meta, target_label, op, &route_signals)
            }
        }
    }

    fn emit_route_policy_audit(
        &self,
        scope_id: ScopeId,
        lane: u8,
        policy_id: u16,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) {
        let port = self.port_for_lane(lane as usize);
        let _ = port.flush_transport_events();
        let transport_attrs = port.transport().metrics().attrs();
        let mut policy_attrs = *signals.attrs();
        policy_attrs.copy_from(&transport_attrs);
        let policy_input = signals.input;
        let arg0 = route_policy_input_arg0(&policy_input);
        let mut event = events::RawEvent::new(port.now32(), ids::ROUTE_DECISION)
            .with_arg0(arg0)
            .with_arg1(policy_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let policy_digest = port.policy_digest(PolicySlot::Route);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_attrs.hash32();
        let transport_snapshot_hash = policy_runtime::hash_transport_attrs(&policy_attrs);
        let replay_transport = policy_runtime::replay_transport_inputs(&policy_attrs);
        let replay_transport_presence = policy_runtime::replay_transport_presence(&policy_attrs);
        let mode_id = policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            policy_attrs_hash,
            transport_snapshot_hash,
            ((policy_runtime::slot_tag(PolicySlot::Route) as u32) << 24) | ((mode_id as u32) << 16),
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
        let verdict = policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            policy_runtime::verdict_reason(verdict) as u32,
            policy_runtime::POLICY_FUEL_NONE as u32,
            port.lane(),
        );
    }

    fn evaluate_route_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        op: ControlOp,
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

        self.emit_route_policy_audit(scope_id, meta.lane, policy_id, signals);

        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                op,
                signals.input,
                &attrs,
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
        op: ControlOp,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        // For local control (self-send), the caller explicitly chooses continue/break.
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
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                op,
                signals.input,
                &attrs,
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
                if !loop_control_kind_matches_disposition(meta.semantic, disposition) {
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
            semantic: meta.semantic,
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
            semantic: meta.semantic,
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
            semantic: meta.semantic,
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
        semantic: ControlSemanticKind,
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
            semantic,
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
        let semantic = controller_arm_semantic_kind(
            &self.cursor,
            &self.control_semantics(),
            scope_id,
            route_arm,
        )
        .unwrap_or(ControlSemanticKind::RouteArm);
        Self::synthetic_cached_recv_meta(
            cursor_index,
            scope_id,
            route_arm,
            label,
            semantic,
            next,
            lane,
        )
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
    pub(super) fn selection_arm_requires_materialization_ready_evidence(
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
                    && self.control_semantic_kind(passive_meta.semantic).is_loop()
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
    pub(super) fn selection_non_wire_loop_control_recv(
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
                    && self.control_semantic_kind(passive_meta.semantic).is_loop()))
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

    pub(super) fn preview_selected_arm_meta(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
        resolved_label_hint: Option<u8>,
    ) -> RecvResult<CachedRecvMeta> {
        let scope_id = selection.scope_id;
        let selected_label_meta = self.selection_label_meta(selection);
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let controller_arm_entry =
            if selection.at_route_offer_entry && self.cursor.is_route_controller(scope_id) {
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
                let semantic = controller_arm_semantic_kind(
                    &self.cursor,
                    &self.control_semantics(),
                    scope_id,
                    selected_arm,
                )
                .unwrap_or(ControlSemanticKind::RouteArm);
                Self::synthetic_cached_recv_meta(
                    arm_entry_idx,
                    scope_id,
                    selected_arm,
                    arm_entry_label,
                    semantic,
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
        RouteFrontierMachine::new(self).align_cursor_to_selected_scope()?;
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

    #[inline]
    fn record_route_decision_for_scope_lanes(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
        fallback_lane: u8,
    ) {
        if scope_id.is_none() || scope_id.kind() != ScopeKind::Route {
            self.record_route_decision_for_lane(fallback_lane as usize, scope_id, arm);
            return;
        }

        let logical_lane_count = self.cursor.logical_lane_count();
        let mut recorded = false;
        let mut lane_idx = 0usize;
        while lane_idx < logical_lane_count {
            if self
                .cursor
                .scope_lane_last_eff_for_arm(scope_id, arm, lane_idx as u8)
                .is_some()
            {
                self.record_route_decision_for_lane(lane_idx, scope_id, arm);
                recorded = true;
            }
            lane_idx += 1;
        }

        if !recorded && (fallback_lane as usize) < logical_lane_count {
            self.record_route_decision_for_lane(fallback_lane as usize, scope_id, arm);
        }
    }

    pub(super) fn prepare_route_decision_from_resolver(
        &mut self,
        scope_id: ScopeId,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> RecvResult<RouteResolveStep> {
        let (policy, eff_index, tag, op) = self
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
        self.emit_route_policy_audit(scope_id, offer_lane, policy_id, signals);
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(offer_lane as usize);
        let lane = Lane::new(port.lane().raw());
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            eff_index,
            tag,
            op,
            signals.input,
            &attrs,
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
        };
        let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
        self.record_route_decision_for_scope_lanes(scope_id, arm.as_u8(), offer_lane);
        self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
        self.emit_route_decision(
            scope_id,
            arm.as_u8(),
            RouteDecisionSource::Resolver,
            offer_lane,
        );
        Ok(RouteResolveStep::Resolved(arm))
    }

    fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }

    #[inline(never)]
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
    fn commit_send_route_selection(&mut self, meta: SendMeta) -> SendResult<()> {
        let Some(selected_arm) = meta.route_arm else {
            return Ok(());
        };
        let scope_id = meta.scope;
        let lane_wire = meta.lane;
        let route_source = self.peek_scope_ack(scope_id).map(|token| token.source());
        let is_route_controller = self.cursor.is_route_controller(scope_id);

        if !is_route_controller {
            self.propagate_recvless_parent_route_decision(scope_id, selected_arm);
        }

        match route_source {
            Some(RouteDecisionSource::Ack) if is_route_controller => {
                self.record_route_decision_for_lane(lane_wire as usize, scope_id, selected_arm);
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Ack,
                    lane_wire,
                );
            }
            Some(RouteDecisionSource::Poll) => {
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Poll,
                    self.offer_lane_for_scope(scope_id),
                );
            }
            _ => {}
        }

        if self.selected_arm_for_scope(scope_id) != Some(selected_arm) {
            self.clear_scope_route_state_for_other_lanes(scope_id, lane_wire);
        }
        self.skip_unselected_arm_lanes(scope_id, selected_arm, lane_wire);
        self.set_route_arm(lane_wire, scope_id, selected_arm)
            .map_err(|_| SendError::PhaseInvariant)?;
        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
        self.port_for_lane(lane_wire as usize).clear_route_hints();
        Ok(())
    }

    #[inline(never)]
    fn commit_send_preview(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<()> {
        self.commit_send_route_selection(meta)?;
        if let Some(preview_cursor_index) = preview_cursor_index {
            self.set_cursor_index(state_index_to_usize(preview_cursor_index));
        }
        self.advance_cursor_after_send()
    }

    #[inline(never)]
    fn advance_cursor_after_send(&mut self) -> SendResult<()> {
        self.cursor
            .try_advance_past_jumps_in_place()
            .map_err(|_| SendError::PhaseInvariant)
    }

    #[inline(never)]
    fn commit_send_progress(&mut self, meta: SendMeta) {
        let lane_idx = meta.lane as usize;
        if self
            .cursor
            .current_phase_contains_eff_index(lane_idx, meta.eff_index)
        {
            self.advance_lane_cursor(lane_idx, meta.eff_index);
        } else {
            self.complete_lane_phase(lane_idx);
        }
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
    }

    fn stage_data_send_payload(
        minted_token: Option<MintedControlToken>,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if minted_token.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let data = payload.ok_or(SendError::PhaseInvariant)?;
        Ok(StagedSendPayload {
            encoded_len: data.encode_into(scratch)?,
            control: StagedControlEmission::None,
        })
    }

    #[inline(always)]
    fn stage_explicit_wire_control_payload(
        minted_token: Option<MintedControlToken>,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if minted_token.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let data = payload.ok_or(SendError::PhaseInvariant)?;
        let encoded_len = data.encode_into(scratch)?;
        if encoded_len != CAP_TOKEN_LEN {
            return Err(SendError::PhaseInvariant);
        }
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes.copy_from_slice(&scratch[..CAP_TOKEN_LEN]);
        let token = GenericCapToken::<()>::from_bytes(bytes);
        if matches!(
            token
                .control_header()
                .map_err(|_| SendError::PhaseInvariant)?
                .shot(),
            CapShot::One
        ) {
            return Err(SendError::PhaseInvariant);
        }
        Ok(StagedSendPayload {
            encoded_len,
            control: StagedControlEmission::Emitted {
                dispatch_token: StagedDispatchToken {
                    token: RawCapFlowToken { bytes },
                    rollback: PendingCapRelease::inert(),
                },
                return_emitted: true,
            },
        })
    }

    #[inline(always)]
    fn stage_registered_send_payload(
        minted_token: Option<MintedControlToken>,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if payload.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let token = minted_token.ok_or(SendError::PhaseInvariant)?;
        let bytes = token.token.bytes();
        scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
        Ok(StagedSendPayload {
            encoded_len: CAP_TOKEN_LEN,
            control: StagedControlEmission::Registered(StagedDispatchToken {
                token: token.token,
                rollback: token.rollback,
            }),
        })
    }

    #[inline(always)]
    fn stage_emitted_send_payload(
        minted_token: Option<MintedControlToken>,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        if payload.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let token = minted_token.ok_or(SendError::PhaseInvariant)?;
        let bytes = token.token.bytes();
        scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
        Ok(StagedSendPayload {
            encoded_len: CAP_TOKEN_LEN,
            control: StagedControlEmission::Emitted {
                dispatch_token: StagedDispatchToken {
                    token: token.token,
                    rollback: token.rollback,
                },
                return_emitted: false,
            },
        })
    }

    #[inline(never)]
    fn mint_descriptor_token_bytes(
        &mut self,
        peer: u8,
        shot: CapShot,
        lane: Lane,
        scope: ScopeId,
        epoch: u16,
        control: ControlDesc,
        handle_bytes: [u8; CAP_HANDLE_LEN],
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let rendezvous = cluster
            .get_local(&self.rendezvous_id())
            .ok_or(SendError::PhaseInvariant)?;
        let strategy = self.mint.as_config().strategy();
        let nonce = strategy.derive_nonce(rendezvous.next_nonce_seed());
        let rollback = PendingCapRelease::new(nonce, rendezvous.cap_release_ctx(lane));
        rendezvous
            .caps()
            .insert_entry(CapEntry {
                sid: self.sid,
                lane_raw: lane.as_wire(),
                kind_tag: control.resource_tag(),
                shot_state: shot.as_u8(),
                role: peer,
                mint_revision: rendezvous.next_cap_revision(),
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: handle_bytes,
            })
            .map_err(|_| SendError::PhaseInvariant)?;

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        CapHeader::new(
            self.sid,
            lane,
            peer,
            control.resource_tag(),
            control.label(),
            control.op(),
            control.path(),
            shot,
            control.scope_kind(),
            control.header_flags(),
            scope.local_ordinal(),
            epoch,
            handle_bytes,
        )
        .encode(&mut header);
        let tag = strategy.derive_tag(&nonce, &header);
        Ok(MintedControlToken {
            token: RawCapFlowToken {
                bytes: GenericCapToken::<()>::from_parts(nonce, header, tag).bytes,
            },
            dispatch: DescriptorDispatch::new(control, scope, epoch),
            rollback,
        })
    }

    #[inline(never)]
    fn mint_send_control(
        &mut self,
        meta: SendMeta,
        descriptor: SendDesc,
    ) -> SendResult<Option<MintedControlToken>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let Some(control) = descriptor.control() else {
            return Ok(None);
        };
        if matches!(control.path(), crate::control::cap::mint::ControlPath::Wire)
            && !control.auto_mint_wire()
        {
            return Err(SendError::PhaseInvariant);
        }

        let lane = self.port_for_lane(meta.lane as usize).lane();
        let shot = meta.shot.ok_or(SendError::PhaseInvariant)?;
        let minted = match control.op() {
            ControlOp::LoopContinue => self.mint_local_loop_continue_control(&meta, shot, lane)?,
            ControlOp::LoopBreak => self.mint_local_loop_break_control(&meta, shot, lane)?,
            ControlOp::CapDelegate => {
                let cp_lane = Lane::new(lane.raw());
                let src_rv = RendezvousId::new(self.rendezvous_id().raw());
                self.mint_local_reroute_control(&meta, shot, lane, src_rv, cp_lane, control)?
            }
            ControlOp::RouteDecision => {
                let cp_lane = Lane::new(lane.raw());
                let src_rv = RendezvousId::new(self.rendezvous_id().raw());
                self.mint_local_route_decision_control(&meta, shot, lane, src_rv, cp_lane, control)?
            }
            ControlOp::TopologyBegin => {
                let cp_sid = SessionId::new(self.sid.raw());
                let cp_lane = Lane::new(lane.raw());
                let src_rv = RendezvousId::new(self.rendezvous_id().raw());
                let encode_control_handle = descriptor
                    .encode_control_handle()
                    .ok_or(SendError::PhaseInvariant)?;
                self.mint_local_topology_begin_control(
                    &meta,
                    shot,
                    lane,
                    src_rv,
                    cp_lane,
                    control,
                    encode_control_handle(cp_sid, cp_lane, meta.scope),
                )?
            }
            ControlOp::TopologyAck => {
                let cp_sid = SessionId::new(self.sid.raw());
                let encode_control_handle = descriptor
                    .encode_control_handle()
                    .ok_or(SendError::PhaseInvariant)?;
                self.mint_local_topology_ack_control(
                    &meta,
                    shot,
                    lane,
                    cp_sid,
                    control,
                    encode_control_handle(cp_sid, lane, meta.scope),
                )?
            }
            _ => {
                let encode_control_handle = descriptor
                    .encode_control_handle()
                    .ok_or(SendError::PhaseInvariant)?;
                let epoch = self.descriptor_send_epoch(control, lane)?;
                self.mint_descriptor_token_bytes(
                    meta.peer,
                    shot,
                    lane,
                    meta.scope,
                    epoch,
                    control,
                    encode_control_handle(self.sid, lane, meta.scope),
                )?
            }
        };
        Ok(Some(minted))
    }

    #[inline]
    fn descriptor_send_epoch(&self, control: ControlDesc, lane: Lane) -> SendResult<u16> {
        match control.op() {
            ControlOp::AbortAck | ControlOp::StateSnapshot => {
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let rendezvous = cluster
                    .get_local(&self.rendezvous_id())
                    .ok_or(SendError::PhaseInvariant)?;
                Ok(rendezvous.lane_generation(lane).raw())
            }
            ControlOp::StateRestore | ControlOp::TxCommit | ControlOp::TxAbort => {
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let rendezvous = cluster
                    .get_local(&self.rendezvous_id())
                    .ok_or(SendError::PhaseInvariant)?;
                rendezvous
                    .snapshot_generation(lane)
                    .map(|generation| generation.raw())
                    .ok_or(SendError::PhaseInvariant)
            }
            _ => Ok(0),
        }
    }

    #[inline(never)]
    fn dispatch_send_token(
        &self,
        dispatch: Option<DescriptorDispatch>,
        mut token: StagedDispatchToken,
    ) -> SendResult<DispatchSendTokenResult> {
        let Some(dispatch) = dispatch else {
            return Ok(DispatchSendTokenResult::None);
        };
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .dispatch_descriptor_control_frame(
                self.rendezvous_id(),
                token.bytes(),
                dispatch.desc,
                dispatch.scope_id,
                dispatch.epoch,
                None,
            )
            .map_err(|_| SendError::PhaseInvariant)?;

        match token.rollback.take_registered_parts(token.bytes()) {
            Some(parts) => Ok(DispatchSendTokenResult::Registered(parts)),
            None => Ok(DispatchSendTokenResult::Emitted),
        }
    }

    #[inline(never)]
    fn preflight_send_control_dispatch(
        &self,
        meta: SendMeta,
        emission: &SendTransportEmission,
    ) -> SendResult<()> {
        let (Some(dispatch), Some(bytes)) =
            (emission.dispatch, emission.control.dispatch_token_bytes())
        else {
            return Ok(());
        };
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .validate_send_bound_descriptor_control_frame(
                self.rendezvous_id(),
                bytes,
                dispatch.desc,
                self.sid,
                Lane::new(meta.lane as u32),
                meta.peer,
                dispatch.scope_id,
                dispatch.epoch,
            )
            .map_err(|_| SendError::PhaseInvariant)
    }

    #[inline(never)]
    fn prepare_send_control(
        &mut self,
        meta: SendMeta,
        descriptor: SendDesc,
        has_payload: bool,
    ) -> SendResult<PreparedSendControl>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        if meta.is_control != descriptor.expects_control {
            return Err(SendError::PhaseInvariant);
        }

        let control = descriptor.control();
        self.evaluate_dynamic_policy(&meta, descriptor.label, control)?;

        let lane = Lane::new(meta.lane as u32);
        self.emit_endpoint_policy_audit(
            PolicySlot::EndpointTx,
            ids::ENDPOINT_SEND,
            self.sid.raw(),
            Self::endpoint_policy_args(lane, meta.label, FrameFlags::empty()),
            lane,
        );

        let explicit_dispatch = match control {
            Some(control)
                if has_payload
                    && matches!(control.path(), crate::control::cap::mint::ControlPath::Wire) =>
            {
                Some(DescriptorDispatch::new(
                    control,
                    meta.scope,
                    self.descriptor_send_epoch(control, lane)?,
                ))
            }
            _ => None,
        };
        let minted_control = match control {
            Some(control)
                if has_payload
                    && matches!(control.path(), crate::control::cap::mint::ControlPath::Wire) =>
            {
                None
            }
            _ => self.mint_send_control(meta, descriptor)?,
        };
        let stage_payload = match control {
            None => Self::stage_data_send_payload,
            Some(control) => match control.path() {
                crate::control::cap::mint::ControlPath::Local => {
                    if has_payload {
                        return Err(SendError::PhaseInvariant);
                    }
                    Self::stage_registered_send_payload
                }
                crate::control::cap::mint::ControlPath::Wire => {
                    if has_payload {
                        Self::stage_explicit_wire_control_payload
                    } else {
                        Self::stage_emitted_send_payload
                    }
                }
            },
        };

        Ok(PreparedSendControl {
            dispatch: explicit_dispatch
                .or_else(|| minted_control.as_ref().map(|token| token.dispatch)),
            minted_control,
            stage_payload,
        })
    }

    #[inline(never)]
    fn begin_send_transport(
        &mut self,
        meta: SendMeta,
        payload: Option<lane_port::RawSendPayload>,
        prepared: PreparedSendControl,
    ) -> SendResult<SendTransportStep<'r>> {
        let dispatch = prepared.dispatch;
        let scratch_ptr = {
            let port = self.port_for_lane(meta.lane as usize);
            lane_port::scratch_ptr(port)
        };
        let staged_send = {
            let scratch = unsafe { &mut *scratch_ptr };
            (prepared.stage_payload)(prepared.minted_control, payload, scratch)?
        };
        if let (Some(dispatch), Some(bytes)) =
            (dispatch, staged_send.control.dispatch_token_bytes())
        {
            let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
            cluster
                .validate_send_bound_descriptor_control_frame(
                    self.rendezvous_id(),
                    bytes,
                    dispatch.desc,
                    self.sid,
                    Lane::new(meta.lane as u32),
                    meta.peer,
                    dispatch.scope_id,
                    dispatch.epoch,
                )
                .map_err(|_| SendError::PhaseInvariant)?;
        }
        let encoded_len = staged_send.encoded_len;

        let mut pending_transport = None;
        let is_remote_send = {
            let port = self.port_for_lane(meta.lane as usize);
            let payload_view = {
                let scratch = unsafe { &*scratch_ptr };
                Payload::new(&scratch[..encoded_len])
            };
            let outgoing = crate::transport::Outgoing {
                meta: crate::transport::SendMeta {
                    eff_index: meta.eff_index,
                    label: meta.label,
                    peer: meta.peer,
                    lane: port.lane().as_wire(),
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
                let mut transport = lane_port::PendingSend::new();
                lane_port::begin_send_outgoing(&mut transport, port, outgoing);
                pending_transport = Some(transport);
                true
            } else {
                false
            }
        };

        if is_remote_send {
            Ok(SendTransportStep::Pending(PendingSendIo {
                transport: pending_transport.ok_or(SendError::PhaseInvariant)?,
                lane_idx: meta.lane as usize,
                control: Some(staged_send.control),
                dispatch,
            }))
        } else {
            Ok(SendTransportStep::Immediate(SendTransportEmission {
                control: staged_send.control,
                dispatch,
            }))
        }
    }

    #[inline(never)]
    fn poll_send_init(
        &mut self,
        descriptor: SendDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        payload: Option<lane_port::RawSendPayload>,
    ) -> SendInitOutcome<'r>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let prepared = match self.prepare_send_control(meta, descriptor, payload.is_some()) {
            Ok(prepared) => prepared,
            Err(err) => return SendInitOutcome::Ready(Err(err)),
        };
        let step = match self.begin_send_transport(meta, payload, prepared) {
            Ok(step) => step,
            Err(err) => return SendInitOutcome::Ready(Err(err)),
        };
        match step {
            SendTransportStep::Immediate(emission) => SendInitOutcome::Commit {
                meta,
                preview_cursor_index,
                emission,
            },
            SendTransportStep::Pending(pending) => SendInitOutcome::Pending {
                meta,
                preview_cursor_index,
                pending,
            },
        }
    }

    #[inline(never)]
    fn poll_send_transport(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<()>> {
        let port = self.port_for_lane(pending.lane_idx);
        lane_port::poll_send_outgoing(&mut pending.transport, port, cx)
            .map_err(SendError::Transport)
    }

    #[inline(never)]
    fn finish_send_after_transport_runtime(
        &mut self,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission,
    ) -> SendResult<SendControlOutcome<'r>> {
        self.preflight_send_control_dispatch(meta, &emission)?;
        self.commit_send_after_emit(preview_cursor_index, meta)?;
        self.emit_send_after_transport_event(meta);
        self.resolve_send_control_outcome(emission)
    }

    #[inline(never)]
    fn emit_send_after_transport_event(&mut self, meta: SendMeta) {
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
    }

    #[inline(never)]
    fn resolve_send_control_outcome(
        &mut self,
        emission: SendTransportEmission,
    ) -> SendResult<SendControlOutcome<'r>> {
        match emission.control {
            StagedControlEmission::None => Ok(SendControlOutcome::None),
            StagedControlEmission::Registered(token) => {
                self.resolve_registered_send_control_outcome(emission.dispatch, token)
            }
            StagedControlEmission::Emitted {
                dispatch_token,
                return_emitted,
            } => self.resolve_emitted_send_control_outcome(
                emission.dispatch,
                dispatch_token,
                return_emitted,
            ),
        }
    }

    #[inline(never)]
    fn resolve_registered_send_control_outcome(
        &self,
        dispatch: Option<DescriptorDispatch>,
        token: StagedDispatchToken,
    ) -> SendResult<SendControlOutcome<'r>> {
        match self.dispatch_send_token(dispatch, token)? {
            DispatchSendTokenResult::Registered(parts) => Ok(SendControlOutcome::Registered(
                RawRegisteredCapToken::from_parts(parts),
            )),
            DispatchSendTokenResult::None | DispatchSendTokenResult::Emitted => {
                Err(SendError::PhaseInvariant)
            }
        }
    }

    #[inline(never)]
    fn resolve_emitted_send_control_outcome(
        &self,
        dispatch: Option<DescriptorDispatch>,
        dispatch_token: StagedDispatchToken,
        return_emitted: bool,
    ) -> SendResult<SendControlOutcome<'r>> {
        let emitted = dispatch_token.token;
        match self.dispatch_send_token(dispatch, dispatch_token)? {
            DispatchSendTokenResult::Registered(parts) => {
                if return_emitted {
                    drop(parts);
                    Ok(SendControlOutcome::Emitted(emitted))
                } else {
                    Ok(SendControlOutcome::Registered(
                        RawRegisteredCapToken::from_parts(parts),
                    ))
                }
            }
            DispatchSendTokenResult::Emitted => {
                if return_emitted {
                    Ok(SendControlOutcome::Emitted(emitted))
                } else {
                    Err(SendError::PhaseInvariant)
                }
            }
            DispatchSendTokenResult::None => Err(SendError::PhaseInvariant),
        }
    }

    #[inline(never)]
    fn poll_send_pending(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendTransportEmission>> {
        match self.poll_send_transport(pending, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => {
                let emission = SendTransportEmission {
                    control: pending
                        .control
                        .take()
                        .expect("send transport control must remain until completion"),
                    dispatch: pending.dispatch,
                };
                Poll::Ready(Ok(emission))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    #[inline(never)]
    pub(crate) fn poll_send_state(
        &mut self,
        descriptor: SendDesc,
        state: &mut SendState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendControlOutcome<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        loop {
            match state {
                SendState::Init {
                    meta,
                    preview_cursor_index,
                    payload,
                } => match self.poll_send_init(
                    descriptor,
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
                } => match self.poll_send_pending(pending, cx) {
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
                    let result = self.finish_send_after_transport_runtime(
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
    fn record_loop_decision(
        &mut self,
        metadata: &LoopMetadata,
        decision: LoopDecision,
        lane: u8,
    ) -> SendResult<u16> {
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
            self.record_route_decision_for_scope_lanes(metadata.scope, arm, lane);
            self.emit_route_decision(metadata.scope, arm, RouteDecisionSource::Ack, lane);
        }
        Ok(epoch)
    }

    #[inline(never)]
    fn mint_local_loop_continue_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut loop_scope = meta.scope;
        let mut epoch = 0;
        let mut recorded_via_loop_metadata = false;
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            epoch = self.record_loop_decision(&metadata, LoopDecision::Continue, meta.lane)?;
            loop_scope = metadata.scope;
            recorded_via_loop_metadata = true;
        }
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
            self.record_route_decision_for_scope_lanes(loop_scope, 0, meta.lane);
            self.emit_route_decision(loop_scope, 0, RouteDecisionSource::Ack, meta.lane);
            epoch = self.port_for_lane(meta.lane as usize).route_change_epoch();
        }
        self.mint_control_token_bytes_with_handle::<LoopContinueKind>(
            meta.peer,
            shot,
            lane,
            loop_scope,
            epoch,
            LoopDecisionHandle {
                sid: self.sid.raw(),
                lane: lane.raw() as u16,
                scope: loop_scope,
            },
        )
    }

    #[inline(never)]
    fn mint_local_loop_break_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut loop_scope = meta.scope;
        let mut epoch = 0;
        let mut recorded_via_loop_metadata = false;
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            epoch = self.record_loop_decision(&metadata, LoopDecision::Break, meta.lane)?;
            loop_scope = metadata.scope;
            recorded_via_loop_metadata = true;
        }
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
            self.record_route_decision_for_scope_lanes(loop_scope, 1, meta.lane);
            self.emit_route_decision(loop_scope, 1, RouteDecisionSource::Ack, meta.lane);
            epoch = self.port_for_lane(meta.lane as usize).route_change_epoch();
        }
        self.mint_control_token_bytes_with_handle::<LoopBreakKind>(
            meta.peer,
            shot,
            lane,
            loop_scope,
            epoch,
            LoopDecisionHandle {
                sid: self.sid.raw(),
                lane: lane.raw() as u16,
                scope: loop_scope,
            },
        )
    }

    #[inline(never)]
    fn mint_local_reroute_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        src_rv: RendezvousId,
        cp_lane: Lane,
        control: ControlDesc,
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let signals = self.policy_signals_for_slot(PolicySlot::Route);
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let policy = cluster
            .policy_mode_for(
                src_rv,
                cp_lane,
                meta.eff_index,
                control.resource_tag(),
                control.op(),
            )
            .map_err(Self::map_cp_error)?;
        let handle = cluster
            .prepare_reroute_handle_from_policy(
                src_rv,
                cp_lane,
                meta.eff_index,
                control.resource_tag(),
                control.op(),
                policy,
                signals.input,
                &attrs,
            )
            .map_err(Self::map_cp_error)?;
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            meta.scope,
            0,
            control,
            handle.encode(),
        )
    }

    #[inline(never)]
    fn mint_local_route_decision_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        src_rv: RendezvousId,
        cp_lane: Lane,
        control: ControlDesc,
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let policy = cluster
            .policy_mode_for(
                src_rv,
                cp_lane,
                meta.eff_index,
                control.resource_tag(),
                control.op(),
            )
            .map_err(|_| SendError::PhaseInvariant)?;
        let scope = meta.scope;
        validate_route_decision_scope(scope, policy.scope())?;
        let arm = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if arm > 1 {
            return Err(SendError::PhaseInvariant);
        }
        self.record_route_decision_for_scope_lanes(scope, arm, meta.lane);
        self.emit_route_decision(scope, arm, RouteDecisionSource::Resolver, meta.lane);
        let epoch = self.port_for_lane(meta.lane as usize).route_change_epoch();
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            scope,
            epoch,
            control,
            RouteArmHandle { scope, arm }.encode(),
        )
    }

    #[inline(never)]
    fn mint_local_topology_begin_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        src_rv: RendezvousId,
        cp_lane: Lane,
        control: ControlDesc,
        descriptor_handle: [u8; CAP_HANDLE_LEN],
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let descriptor =
            TopologyDescriptor::decode(descriptor_handle).map_err(Self::map_cp_error)?;
        let operands = cluster
            .prepare_topology_operands_from_descriptor(src_rv, cp_lane, control, descriptor)
            .map_err(Self::map_cp_error)?;
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            meta.scope,
            0,
            control,
            Self::topology_handle_from_operands(operands).encode(),
        )
    }

    #[inline(never)]
    fn mint_local_topology_ack_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        cp_sid: SessionId,
        control: ControlDesc,
        descriptor_handle: [u8; CAP_HANDLE_LEN],
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let cp_lane = Lane::new(lane.raw());
        let descriptor =
            TopologyDescriptor::decode(descriptor_handle).map_err(Self::map_cp_error)?;
        let preview_operands = cluster
            .cached_topology_operands(cp_sid)
            .or_else(|| cluster.distributed_topology_operands(cp_sid))
            .ok_or(SendError::PhaseInvariant)?;
        cluster
            .validate_topology_operands_from_descriptor(
                rv_id,
                cp_lane,
                control,
                descriptor,
                preview_operands,
            )
            .map_err(Self::map_cp_error)?;
        let operands = cluster
            .take_cached_topology_operands(cp_sid)
            .or_else(|| cluster.distributed_topology_operands(cp_sid))
            .ok_or(SendError::PhaseInvariant)?;
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            meta.scope,
            0,
            control,
            Self::topology_handle_from_operands(operands).encode(),
        )
    }

    #[inline(never)]
    fn mint_control_token_bytes_with_handle<K>(
        &mut self,
        peer: u8,
        shot: CapShot,
        lane: Lane,
        scope: ScopeId,
        epoch: u16,
        handle: K::Handle,
    ) -> SendResult<MintedControlToken>
    where
        K: ResourceKind + crate::control::cap::mint::ControlResourceKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        self.mint_descriptor_token_bytes(
            peer,
            shot,
            lane,
            scope,
            epoch,
            ControlDesc::of::<K>(),
            K::encode_handle(&handle),
        )
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
                        while let Some(parent) = self.cursor.control_parent_scope(current_scope) {
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
            let Some(parent) = self.cursor.control_parent_scope(parent_scope) else {
                break;
            };
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
    pub(super) fn offer_lane_set_for_scope(&self, scope_id: ScopeId) -> LaneSetView {
        self.cursor
            .route_scope_offer_lane_set(scope_id)
            .unwrap_or(LaneSetView::EMPTY)
    }

    #[inline]
    pub(super) fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if let Some(lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) {
            lane_idx as u8
        } else {
            self.primary_lane as u8
        }
    }

    pub(super) fn propagate_recvless_parent_route_decision(
        &mut self,
        child_scope: ScopeId,
        arm: u8,
    ) {
        let Some(parent_scope) = self.cursor.route_parent_scope(child_scope) else {
            return;
        };
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
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        if parent_is_dynamic {
            return;
        }
        let parent_requires_wire_recv = {
            let mut arm = 0u8;
            let mut requires_wire = false;
            while arm <= 1 {
                if self.arm_has_recv(parent_scope, arm)
                    && !self.is_non_wire_loop_control_arm(parent_scope, arm)
                {
                    requires_wire = true;
                    break;
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
        self.record_route_decision_for_scope_lanes(parent_scope, parent_arm.as_u8(), parent_lane);
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

    fn is_non_wire_loop_control_arm(&self, scope_id: ScopeId, arm: u8) -> bool {
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
        recv_meta.is_control
            && recv_meta.route_arm == Some(arm)
            && (recv_meta.peer == ROLE
                || (!self.cursor.is_route_controller(scope_id)
                    && self.control_semantic_kind(recv_meta.semantic).is_loop()))
    }

    #[cfg(test)]
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
            && self.control_semantic_kind(recv_meta.semantic).is_loop()
    }

    fn take_binding_for_lane(
        &mut self,
        lane_idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        let classification = self.binding_inbox.take_or_poll(&mut self.binding, lane_idx);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
        classification
    }

    #[inline]
    pub(super) fn take_restored_binding_payload(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
    ) -> Option<Payload<'r>> {
        match self.restored_binding_payload {
            Some(restored) if restored.matches(lane_idx, classification) => {
                self.restored_binding_payload = None;
                Some(restored.payload)
            }
            Some(_) | None => None,
        }
    }

    #[inline]
    fn restore_binding_payload_for_lane(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
        payload: Payload<'r>,
    ) {
        debug_assert!(
            self.restored_binding_payload.is_none(),
            "at most one restored binding payload may be staged per endpoint"
        );
        self.restored_binding_payload = Some(RestoredBindingPayload {
            lane: lane_idx as u8,
            classification: PackedIncomingClassification::encode(classification),
            payload,
        });
        self.put_back_binding_for_lane(lane_idx, classification);
    }

    pub(super) fn put_back_binding_for_lane(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
    ) {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        self.binding_inbox.put_back(lane_idx, classification);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
    }

    pub(super) fn take_matching_binding_for_lane(
        &mut self,
        lane_idx: usize,
        expected_label: u8,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        let classification =
            self.binding_inbox
                .take_matching_or_poll(&mut self.binding, lane_idx, expected_label);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
        classification
    }

    fn take_matching_mask_binding_for_lane<F: FnMut(u8) -> bool>(
        &mut self,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
        drop_mismatch: F,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        let classification = self.binding_inbox.take_matching_mask_or_poll(
            &mut self.binding,
            lane_idx,
            label_mask,
            drop_label_mask,
            drop_mismatch,
        );
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
        classification
    }

    #[inline]
    fn take_binding_mask_ignoring_loop_control(
        &mut self,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
    ) -> Option<crate::binding::IncomingClassification> {
        self.take_matching_mask_binding_for_lane(
            lane_idx,
            label_mask,
            drop_label_mask,
            move |label| label == LoopContinueKind::LABEL || label == LoopBreakKind::LABEL,
        )
    }

    pub(super) fn take_binding_for_selected_arm(
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
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        self.poll_binding_for_offer_lanes(
            scope_id,
            offer_lane_idx,
            self.offer_lane_set_for_scope(scope_id),
            label_meta,
            materialization_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn poll_binding_for_offer_lanes(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lanes.is_empty() {
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
        let preference = if let Some(arm) = preferred_arm
            && self.offer_lanes_contain_binding_preference(
                offer_lanes,
                label_meta,
                materialization_meta,
                BindingLanePreference::Arm(arm),
            ) {
            BindingLanePreference::Arm(arm)
        } else if self.offer_lanes_contain_binding_preference(
            offer_lanes,
            label_meta,
            materialization_meta,
            BindingLanePreference::LabelMask(label_mask),
        ) {
            BindingLanePreference::LabelMask(label_mask)
        } else {
            BindingLanePreference::Any
        };
        if let Some(expected_label) = label_meta.preferred_binding_label(preferred_arm) {
            if let Some(picked) = self.poll_binding_exact_for_offer(
                offer_lane_idx,
                offer_lanes,
                expected_label,
                label_meta,
                materialization_meta,
                preference,
            ) {
                return Some(picked);
            }
        }
        if let Some(classification) = self.poll_binding_mask_for_offer(
            offer_lane_idx,
            offer_lanes,
            label_mask,
            label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(classification);
        }
        if self.static_passive_scope_evidence_materializes_poll(scope_id)
            && let Some((lane_idx, classification)) =
                self.poll_binding_any_for_offer(offer_lane_idx, offer_lanes)
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
        offer_lanes: LaneSetView,
        label_mask: u128,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let drop_label_mask = self.loop_control_drop_label_mask();
        if let Some(classification) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            offer_lanes,
            label_mask,
            0,
            false,
            label_mask,
            drop_label_mask,
            label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(classification);
        }
        if let Some(classification) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            offer_lanes,
            drop_label_mask,
            label_mask,
            true,
            label_mask,
            drop_label_mask,
            label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(classification);
        }
        self.poll_binding_mask_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            label_mask,
            drop_label_mask,
            label_meta,
            materialization_meta,
            preference,
        )
    }

    fn poll_buffered_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        buffered_label_mask: u128,
        excluded_buffered_mask: u128,
        require_preference: bool,
        label_mask: u128,
        drop_label_mask: u128,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let lane_limit = self.cursor.logical_lane_count();
        let mut scan_idx = 0usize;
        while let Some(lane_slot) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            if !self
                .binding_inbox
                .lane_has_buffered_label(lane_slot, buffered_label_mask)
                || (excluded_buffered_mask != 0
                    && self
                        .binding_inbox
                        .lane_has_buffered_label(lane_slot, excluded_buffered_mask))
                || (require_preference
                    && !self.offer_lane_matches_binding_preference(
                        label_meta,
                        materialization_meta,
                        preference,
                        lane_slot,
                    ))
            {
                continue;
            }
            if let Some(classification) =
                self.take_binding_mask_ignoring_loop_control(lane_slot, label_mask, drop_label_mask)
            {
                return Some((lane_slot, classification));
            }
        }
        None
    }

    fn poll_binding_mask_in_lane_set(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        label_mask: u128,
        drop_label_mask: u128,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let lane_limit = self.cursor.logical_lane_count();
        let excluded_mask = label_mask | drop_label_mask;
        let mut scan_idx = 0usize;
        while let Some(lane_slot) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            if self
                .binding_inbox
                .lane_has_buffered_label(lane_slot, excluded_mask)
                || !self.offer_lane_matches_binding_preference(
                    label_meta,
                    materialization_meta,
                    preference,
                    lane_slot,
                )
            {
                continue;
            }
            return self
                .take_binding_mask_ignoring_loop_control(lane_slot, label_mask, drop_label_mask)
                .map(|classification| (lane_slot, classification));
        }
        None
    }

    fn poll_binding_exact_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        expected_label: u8,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let expected_label_mask = ScopeLabelMeta::label_bit(expected_label);
        if let Some(classification) = self.poll_binding_exact_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            expected_label,
            expected_label_mask,
            true,
            label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(classification);
        }
        self.poll_binding_exact_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            expected_label,
            expected_label_mask,
            false,
            label_meta,
            materialization_meta,
            preference,
        )
    }

    fn poll_binding_exact_in_lane_set(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        expected_label: u8,
        expected_label_mask: u128,
        buffered_only: bool,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let lane_limit = self.cursor.logical_lane_count();
        let mut scan_idx = 0usize;
        while let Some(lane_idx) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            let has_buffered = self
                .binding_inbox
                .lane_has_buffered_label(lane_idx, expected_label_mask);
            if buffered_only {
                if !has_buffered {
                    continue;
                }
            } else if has_buffered
                || !self.offer_lane_matches_binding_preference(
                    label_meta,
                    materialization_meta,
                    preference,
                    lane_idx,
                )
            {
                continue;
            }
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
        offer_lanes: LaneSetView,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lanes.is_empty() {
            return None;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut scan_idx = 0usize;
        while let Some(lane_idx) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            if let Some(classification) = self.take_binding_for_lane(lane_idx) {
                return Some((lane_idx, classification));
            }
        }
        None
    }

    #[inline]
    fn offer_lanes_contain_binding_preference(
        &self,
        offer_lanes: LaneSetView,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> bool {
        let lane_limit = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if offer_lanes.contains(lane_idx)
                && self.offer_lane_matches_binding_preference(
                    label_meta,
                    materialization_meta,
                    preference,
                    lane_idx,
                )
            {
                return true;
            }
            lane_idx += 1;
        }
        false
    }

    #[inline]
    fn offer_lane_matches_binding_preference(
        &self,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
        lane_idx: usize,
    ) -> bool {
        match preference {
            BindingLanePreference::Any => true,
            BindingLanePreference::Arm(arm) => {
                self.binding_demux_contains_lane(materialization_meta.scope_id, Some(arm), lane_idx)
            }
            BindingLanePreference::LabelMask(label_mask) => self
                .binding_demux_contains_lane_for_label_mask(
                    materialization_meta.scope_id,
                    label_meta,
                    label_mask,
                    lane_idx,
                ),
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
        let mut lane_idx = scan_idx.saturating_sub(1);
        while lane_idx < lane_limit {
            if lane_idx != preferred_lane_idx && offer_lanes.contains(lane_idx) {
                *scan_idx = lane_idx.saturating_add(2);
                return Some(lane_idx);
            }
            lane_idx += 1;
        }
        None
    }

    pub(super) fn try_recv_from_binding(
        &mut self,
        logical_lane: u8,
        expected_label: u8,
        scratch_ptr: *mut [u8],
    ) -> RecvResult<Option<Payload<'r>>> {
        let lane_idx = logical_lane as usize;
        if let Some(classification) = self.take_matching_binding_for_lane(lane_idx, expected_label)
        {
            if let Some(payload) = self.take_restored_binding_payload(lane_idx, classification) {
                return Ok(Some(payload));
            }
            let payload = lane_port::recv_from_binding(
                core::ptr::from_mut(&mut self.binding),
                classification.channel,
                scratch_ptr,
            )
            .map_err(RecvError::Binding)?;
            return Ok(Some(payload));
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

    pub(super) fn parallel_scope_root(cursor: &PhaseCursor, scope_id: ScopeId) -> Option<ScopeId> {
        cursor.parallel_scope_root(scope_id)
    }

    #[inline]
    pub(super) fn frontier_kind_for_cursor(
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
                    recv_meta.semantic,
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
        meta.record_dispatch_arm_label_mask(
            0,
            cursor.route_scope_first_recv_dispatch_arm_label_mask(scope_id, 0),
        );
        meta.record_dispatch_arm_label_mask(
            1,
            cursor.route_scope_first_recv_dispatch_arm_label_mask(scope_id, 1),
        );
        meta
    }

    #[inline]
    fn offer_scope_label_meta(&self, scope_id: ScopeId, offer_lane_idx: usize) -> ScopeLabelMeta {
        if offer_lane_idx < self.cursor.logical_lane_count() {
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
        if offer_lane_idx < self.cursor.logical_lane_count() {
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

    pub(super) fn frontier_static_facts_at(
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
        let lane_limit = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
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
                        self.complete_lane_phase(lane as usize);
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
        let phase_lanes = self.cursor.current_phase_lane_set();
        let logical_lane_count = self.cursor.logical_lane_count();
        let lane_linger = self.route_state.lane_linger_lanes();
        let offer_linger = self.route_state.lane_offer_linger_lanes();
        let mut lane_idx = 0usize;
        while lane_idx < logical_lane_count {
            if phase_lanes.contains(lane_idx)
                && (lane_linger.contains(lane_idx) || offer_linger.contains(lane_idx))
            {
                return true;
            }
            lane_idx += 1;
        }
        false
    }

    pub(crate) fn matches_session(&self, sid: SessionId) -> bool {
        self.sid == sid
    }

    pub(crate) fn for_each_physical_lane(&self, mut f: impl FnMut(Lane)) {
        let logical_lane_count = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < logical_lane_count {
            if let Some(port) = self.ports.get(lane_idx).and_then(|slot| slot.as_ref()) {
                f(port.lane);
            }
            lane_idx += 1;
        }
    }

    pub(crate) fn invalidate_public_owner(&mut self) {
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
    fn topology_handle_from_operands(operands: TopologyOperands) -> TopologyHandle {
        let mut flags = 0u16;
        if operands.seq_tx != 0 || operands.seq_rx != 0 {
            flags |= topology_flags::FENCES_PRESENT;
        }
        TopologyHandle {
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
