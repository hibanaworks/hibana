//! Cursor-driven endpoint implementation built on top of `PhaseCursor`.
//!
//! A cursor endpoint owns the rendezvous port outright and advances according
//! to the typestate cursor obtained from `RoleProgram` projection.

use core::{
    convert::TryFrom,
    future::poll_fn,
    task::Poll,
};

use super::flow::CapFlow;
use crate::binding::{BindingSlot, NoBinding};
use crate::eff::EffIndex;
use crate::g::{ControlHandling, ControlPayloadKind};
use crate::global::const_dsl::{HandlePlan, ScopeId, ScopeKind};
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{
    JumpReason, LoopMetadata, LoopRole, PassiveArmNavigation, PhaseCursor, RecvMeta, ScopeRecord,
    ScopeRegion, SendMeta, StateIndex, ARM_SHARED,
};
use crate::runtime::config::Clock;
use crate::{
    control::{
        CapFlowToken, CapRegisteredToken, CpEffect, CpError,
        cap::resource_kinds::{
            CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoopBreakKind, LoopContinueKind,
            LoopDecisionHandle, RerouteKind, RollbackKind, RouteDecisionKind,
            SpliceAckKind, SpliceHandle, SpliceIntentKind, splice_flags,
        },
        cap::{
            CAP_TOKEN_LEN, CapShot, ControlMint, E0, EndpointEpoch, EpochInit, EpochTable,
            GenericCapToken, MintConfigMarker, Owner, ResourceKind,
        },
        cluster::{DynamicResolution, SpliceOperands},
        types::{LaneId as CpLaneId, RendezvousId as CpRendezvousId, SessionId as CpSessionId},
    },
    endpoint::{
        RecvError, RecvResult, SendError, SendResult,
        affine::LaneGuard,
        control::{ControlOutcome, SessionControlCtx},
    },
    epf::{self, AbortInfo, Action as PolicyAction, Slot as VmSlot},
    observe::{RawEvent, ScopeTrace, TapEvent, emit, events, ids, policy_abort, policy_trap},
    rendezvous::{Lane, LoopDisposition, Port, RendezvousId, SessionId},
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_REROUTE, LABEL_SPLICE_ACK,
        LABEL_SPLICE_INTENT, LabelUniverse,
    },
    transport::{
        Transport, TransportMetrics,
        trace::TapFrameMeta,
        wire::{FrameFlags, Payload, WireDecodeOwned, WireEncode},
    },
};

/// Classification of control labels for dynamic plan evaluation dispatch.
///
/// This enum provides a clean abstraction over the raw label constants,
/// grouping them by their evaluation semantics:
/// - `Loop`: Labels that require loop-specific evaluation (continue/break decisions)
/// - `SpliceOrReroute`: Labels validated later in `mint_control_token_with_handle`
/// - `Route`: Standard route arm evaluation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DynamicLabelClass {
    /// Loop control labels (LABEL_LOOP_CONTINUE, LABEL_LOOP_BREAK)
    Loop,
    /// Splice and reroute labels (validated in mint_control_token_with_handle)
    SpliceOrReroute,
    /// Standard route decision labels
    Route,
}

/// Classify a label for dynamic plan evaluation dispatch.
///
/// This function maps raw label constants to their semantic classification,
/// providing a single point of truth for label-based dispatch logic.
#[inline]
const fn classify_dynamic_label(label: u8) -> DynamicLabelClass {
    match label {
        LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK => DynamicLabelClass::Loop,
        LABEL_SPLICE_INTENT | LABEL_SPLICE_ACK | LABEL_REROUTE => {
            DynamicLabelClass::SpliceOrReroute
        }
        _ => DynamicLabelClass::Route,
    }
}

/// Cursor-driven endpoint. Owns the rendezvous port as well as the lane
/// release handle. Dropping the endpoint releases the lane back to the
/// `SessionCluster` via the handle.
pub struct CursorEndpoint<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U = crate::runtime::consts::DefaultLabelUniverse,
    C = crate::runtime::config::CounterClock,
    E: EpochTable = EpochInit,
    const MAX_RV: usize = 8,
    Mint = crate::control::cap::DefaultMintConfig,
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
    ports: [Option<Port<'r, T, E>>; MAX_LANES],
    /// Multi-lane guard array. Each active lane has its own guard.
    guards: [Option<LaneGuard<'r, T, U, C>>; MAX_LANES],
    /// Primary lane index (first active lane, typically 0).
    primary_lane: usize,
    sid: SessionId,
    _owner: Owner<'r, E0>,
    _epoch: EndpointEpoch<'r, E>,
    /// Phase-aware cursor for multi-lane parallel execution.
    cursor: PhaseCursor<ROLE>,
    control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    /// Lane-local route arm stacks for parallel composition.
    ///
    /// Each lane maintains its own route arm stack to support independent
    /// `g::route`/`g::loop` scopes within `g::par` regions. When a lane enters
    /// a route scope, only that lane's stack is pushed. This enables:
    ///
    /// - **Shared Scope (Outer)**: Route scopes outside `g::par` are pushed to
    ///   all active lanes when entering the parallel region.
    /// - **Local Scope (Inner)**: Route scopes inside `g::par` are pushed only
    ///   to the specific lane executing that scope.
    /// - **Join**: When exiting `g::par`, each lane's local scopes are popped,
    ///   returning to shared scope state.
    lane_route_arms: [[RouteArmState; MAX_ROUTE_ARM_STACK]; MAX_LANES],
    lane_route_arm_lens: [u8; MAX_LANES],
    lane_linger_counts: [u8; MAX_LANES],
    lane_linger_mask: u8,
    pending_linger_mask: u8,
    pending_offer_mask: u8,
    pending_offer_info: [PendingOfferInfo; MAX_LANES],
    pending_binding: [Option<crate::binding::IncomingClassification>; MAX_LANES],
    mint: Mint,
    binding: B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopDecision {
    Continue,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteArmState {
    scope: ScopeId,
    arm: u8,
}

impl RouteArmState {
    const EMPTY: Self = Self {
        scope: ScopeId::none(),
        arm: 0,
    };
}

#[derive(Clone, Copy)]
struct PendingOfferInfo {
    scope: ScopeId,
    entry: StateIndex,
    parallel_root: ScopeId,
    flags: u8,
}

impl PendingOfferInfo {
    const FLAG_CONTROLLER: u8 = 1;
    const FLAG_DYNAMIC: u8 = 1 << 1;
    const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: u16::MAX,
        parallel_root: ScopeId::none(),
        flags: 0,
    };

    #[inline]
    fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }
}

const MAX_ROUTE_ARM_STACK: usize = 8;

#[derive(Clone, Copy)]
enum RouteInput {
    Decision(u8),
    Wire {
        arm: u8,
        channel: crate::binding::Channel,
        instance: u16,
    },
    Hint { arm: u8 },
    Mergeable,
    Resolver(u8),
    Poll(u8),
}

#[derive(Clone, Copy)]
struct OfferDebugger {
    enabled: bool,
}

impl OfferDebugger {
    #[cfg(feature = "std")]
    #[inline]
    fn new() -> Self {
        Self {
            enabled: std::env::var_os("HIBANA_OFFER_DEBUG").is_some(),
        }
    }

    #[cfg(not(feature = "std"))]
    #[inline]
    fn new() -> Self {
        Self { enabled: false }
    }

    #[inline]
    fn missing_scope_region(&self, idx: usize, scope: ScopeId) {
        let _ = (idx, scope);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] missing scope region: idx={} scope={:?}",
                idx, scope
            );
        }
    }

    #[inline]
    fn non_route_scope(&self, idx: usize, scope: ScopeId, kind: ScopeKind) {
        let _ = (idx, scope, kind);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] non-route scope: idx={} scope={:?} kind={:?}",
                idx, scope, kind
            );
        }
    }

    #[inline]
    fn offer_entry_mismatch(&self, idx: usize, expected: u16, scope: ScopeId) {
        let _ = (idx, expected, scope);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] offer entry mismatch: idx={} expected={} scope={:?}",
                idx, expected, scope
            );
        }
    }

    #[inline]
    fn resolver_disabled(
        &self,
        scope: ScopeId,
        plan: HandlePlan,
        entry: Option<&ScopeRecord>,
        arm_entry: bool,
    ) {
        let _ = (scope, plan, entry, arm_entry);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            let suffix = if arm_entry { " (arm entry)" } else { "" };
            if let Some(entry) = entry {
                eprintln!(
                    "[hibana-offer] resolver disabled{}: scope={:?} plan={:?} ctrl_labels={:?} ctrl_entries={:?} offer_entry={}",
                    suffix,
                    scope,
                    plan,
                    entry.controller_arm_label,
                    entry.controller_arm_entry,
                    entry.offer_entry
                );
            }
            eprintln!(
                "[hibana-offer] resolver disabled{}: scope={:?} plan={:?}",
                suffix, scope, plan
            );
        }
    }

    #[inline]
    fn resolver_unexpected(&self, scope: ScopeId, resolution: &DynamicResolution) {
        let _ = (scope, resolution);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] resolver unexpected resolution: scope={:?} res={:?}",
                scope, resolution
            );
        }
    }

    #[inline]
    fn select_current_scope(
        &self,
        scope: ScopeId,
        idx: usize,
        entry: Option<u16>,
        start: u16,
        controller: bool,
        dynamic: bool,
    ) {
        let _ = (scope, idx, entry, start, controller, dynamic);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] select_offer_entry: current scope={:?} idx={} entry={:?} start={} controller={} dynamic={}",
                scope, idx, entry, start, controller, dynamic
            );
        }
    }

    #[inline]
    fn lane_scan(&self, lane: usize, idx: usize, scope: ScopeId, controller: bool, hint: bool) {
        let _ = (lane, idx, scope, controller, hint);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] lane={} idx={} scope={:?} controller={} hint={}",
                lane, idx, scope, controller, hint
            );
        }
    }

    #[inline]
    fn current_match(&self, idx: usize, matched: bool) {
        let _ = (idx, matched);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] current_idx={} current_match={}",
                idx, matched
            );
        }
    }

    #[inline]
    fn select_choice(&self, label: &str, idx: usize) {
        let _ = (label, idx);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] select_offer_entry: choose={} idx={}",
                label, idx
            );
        }
    }

    #[inline]
    fn select_ambiguous(
        &self,
        idx: usize,
        candidates: usize,
        controllers: usize,
        hints: usize,
        current_route: bool,
        current_controller: bool,
    ) {
        let _ = (idx, candidates, controllers, hints, current_route, current_controller);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] select_offer_entry: ambiguous idx={} candidates={} controllers={} hints={} current_route={} current_controller={}",
                idx, candidates, controllers, hints, current_route, current_controller
            );
        }
    }

    #[inline]
    fn resolved_hint(&self, scope: ScopeId, label: u8, controller: bool) {
        let _ = (scope, label, controller);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] scope={:?} resolved_hint={} controller={}",
                scope, label, controller
            );
        }
    }

    #[inline]
    fn selected_arm(&self, scope: ScopeId, selected_arm: u8) {
        let _ = (scope, selected_arm);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] scope={:?} selected_arm={}",
                scope, selected_arm
            );
        }
    }

    #[inline]
    fn branch_label(&self, scope: ScopeId, label: u8, arm: Option<u8>, lane: u8) {
        let _ = (scope, label, arm, lane);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] scope={:?} branch_label={} route_arm={:?} lane={}",
                scope, label, arm, lane
            );
        }
    }

    #[inline]
    fn offer_start(
        &self,
        idx: usize,
        phase: usize,
        lanes: [usize; MAX_LANES],
        scope: Option<ScopeId>,
    ) {
        let _ = (idx, phase, lanes, scope);
        if !self.enabled {
            return;
        }
        #[cfg(feature = "std")]
        {
            eprintln!(
                "[hibana-offer] start: idx={} phase={} lanes={:?} scope={:?}",
                idx, phase, lanes, scope
            );
        }
    }
}

/// Branch metadata carried from `offer()` to `decode()`.
#[derive(Clone, Copy, Debug)]
pub struct BranchMeta {
    /// The scope this branch belongs to.
    pub scope_id: ScopeId,
    /// The selected arm (0, 1, ...).
    pub selected_arm: u8,
    /// Wire lane for this branch.
    pub lane_wire: u8,
    /// EffIndex for lane cursor advancement.
    pub eff_index: EffIndex,
    /// Classification of the branch for decode() dispatch.
    pub kind: BranchKind,
}

/// Classification of branch types for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    /// Normal wire recv: payload comes from transport/binding.
    WireRecv,
    /// Synthetic local control: CanonicalControl self-send that doesn't go on wire.
    /// Decode from zero buffer; scope settlement uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should use `into_endpoint()` and `flow().send()` instead of `decode()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
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
    label: u8,
    payload: Payload<'r>,
    endpoint: CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    /// Channel from binding classification (for FlowBinder recv path).
    /// None when not using FlowBinder or when data comes from transport directly.
    binding_channel: Option<crate::binding::Channel>,
    /// Instance from binding classification (for multi-channel routing).
    binding_instance: Option<u16>,
    /// Branch metadata from offer() for decode() dispatch.
    /// Eliminates label→arm inference in decode().
    branch_meta: BranchMeta,
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
    pub async fn decode<M>(
        self,
    ) -> RecvResult<(
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        M::Payload,
    )>
    where
        M: crate::g::MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let RouteBranch {
            label,
            payload: payload_view,
            mut endpoint,
            binding_channel,
            binding_instance: _,
            branch_meta,
        } = self;

        let expected = <M as crate::g::MessageSpec>::LABEL;
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }

        match branch_meta.kind {
            BranchKind::LocalControl => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);

                endpoint.cursor = endpoint
                    .cursor
                    .try_advance_past_jumps()
                    .map_err(|_| RecvError::PhaseInvariant)?;

                let lane_idx = branch_meta.lane_wire as usize;
                endpoint.advance_lane_cursor(lane_idx, branch_meta.eff_index);
                endpoint.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    branch_meta.eff_index,
                );
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            BranchKind::EmptyArmTerminal => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);

                endpoint.cursor = endpoint
                    .cursor
                    .try_follow_jumps()
                    .map_err(|_| RecvError::PhaseInvariant)?;

                let lane_idx = branch_meta.lane_wire as usize;
                if let Some(eff_index) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, eff_index);
                } else {
                    endpoint.advance_lane_cursor(lane_idx, branch_meta.eff_index);
                }
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            BranchKind::ArmSendHint => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );

                return Ok((endpoint, payload));
            }

            BranchKind::WireRecv => {}
        }

        let meta = endpoint
            .cursor
            .try_recv_meta()
            .ok_or(RecvError::PhaseInvariant)?;
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(RecvError::PhaseInvariant);
        }

        if matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK) {
            if let Some(LoopMetadata {
                scope: scope_id,
                controller,
                target,
                role,
                ..
            }) = endpoint.cursor.loop_metadata_inner()
            {
                if role != LoopRole::Target || target != ROLE {
                    return Err(RecvError::PhaseInvariant);
                }

                if meta.peer != controller {
                    return Err(RecvError::PeerMismatch {
                        expected: controller,
                        actual: meta.peer,
                    });
                }

                let idx = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::loop_index(scope_id)
                    .ok_or(RecvError::PhaseInvariant)?;
                let port = endpoint.port_for_lane(meta.lane as usize);
                let lane = port.lane();
                port.loop_table().acknowledge(lane, ROLE, idx);
                let has_local_decision = port.loop_table().has_decision(lane, idx);
                if has_local_decision {
                    port.ack_loop_decision(idx, ROLE);
                }
            }
        }

        let payload = if let Some(channel) = binding_channel {
            let port = endpoint.port_mut();
            let scratch_ptr = port.scratch_ptr();
            let scratch = unsafe { &mut *scratch_ptr };
            let n = endpoint
                .binding
                .on_recv(channel, scratch)
                .map_err(|_| RecvError::PhaseInvariant)?;

            M::Payload::decode_owned(&scratch[..n]).map_err(RecvError::Codec)?
        } else if !payload_view.as_bytes().is_empty() {
            // Transport path: use payload view directly
            M::Payload::decode_owned(payload_view.as_bytes()).map_err(RecvError::Codec)?
        } else {
            // Empty payload (e.g., for marker types like HqResponseFin with no data)
            M::Payload::decode_owned(&[]).map_err(RecvError::Codec)?
        };

        endpoint.cursor = endpoint
            .cursor
            .try_advance_past_jumps()
            .map_err(|_| RecvError::PhaseInvariant)?;

        let decode_lane_idx = meta.lane as usize;
        endpoint.advance_lane_cursor(decode_lane_idx, meta.eff_index);
        endpoint.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        endpoint.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        if branch_meta.scope_id != meta.scope {
            endpoint.settle_scope_after_action(
                branch_meta.scope_id,
                Some(branch_meta.selected_arm),
                Some(meta.eff_index),
                branch_meta.lane_wire,
            );
        }
        let mut route_arm_updated = false;
        let mut linger_scope = meta.scope;
        loop {
            if endpoint.is_linger_route(linger_scope) {
                let mut arm = endpoint.route_arm_for(meta.lane, linger_scope);
                if arm.is_none() {
                    arm = endpoint
                        .cursor
                        .first_recv_target(linger_scope, label)
                        .map(|(arm, _)| if arm == ARM_SHARED { 0 } else { arm });
                    if let Some(selected) = arm {
                        endpoint.set_route_arm(meta.lane, linger_scope, selected)?;
                        route_arm_updated = true;
                    }
                }
                if let Some(arm) = arm {
                    if arm == 0 {
                        if let Some(last_eff) = endpoint
                            .cursor
                            .scope_lane_last_eff_for_arm(linger_scope, arm, meta.lane)
                        {
                            if last_eff == meta.eff_index {
                                if let Some(first_eff) = endpoint
                                    .cursor
                                    .scope_lane_first_eff(linger_scope, meta.lane)
                                {
                                    endpoint.set_lane_cursor_to_eff_index(
                                        meta.lane as usize,
                                        first_eff,
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            let Some(parent) = endpoint.cursor.scope_parent(linger_scope) else {
                break;
            };
            linger_scope = parent;
        }
        if route_arm_updated {
            endpoint.refresh_pending_offer_lane(meta.lane as usize);
        }
        if let Some(region) = endpoint.cursor.scope_region() {
            if region.kind == ScopeKind::Route && region.linger {
                let at_scope_start = endpoint.cursor.index() == region.start;
                let at_passive_branch = endpoint.cursor.jump_reason()
                    == Some(JumpReason::PassiveObserverBranch)
                    && endpoint
                        .cursor
                        .scope_region()
                        .map(|scope_region| scope_region.scope_id == region.scope_id)
                        .unwrap_or(false);
                if at_scope_start || at_passive_branch {
                    if let Some(arm) = endpoint.route_arm_for(meta.lane, region.scope_id) {
                        if arm == 0 {
                            if let Some(first_eff) =
                                endpoint.cursor.scope_lane_first_eff(region.scope_id, meta.lane)
                            {
                                endpoint.set_lane_cursor_to_eff_index(
                                    meta.lane as usize,
                                    first_eff,
                                );
                            }
                        }
                    }
                }
            }
        }
        endpoint.maybe_advance_phase();
        Ok((endpoint, payload))
    }

    /// Branch label.
    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    /// Binding instance, if available.
    #[inline]
    pub fn instance(&self) -> Option<u16> {
        self.binding_instance
    }

    /// Active scope id, if any.
    #[inline]
    pub fn scope_id(&self) -> Option<ScopeId> {
        self.endpoint.cursor.scope_id()
    }

    /// Active scope kind, if any.
    #[inline]
    pub fn scope_kind(&self) -> Option<ScopeKind> {
        self.endpoint.cursor.scope_kind()
    }

    /// Active scope region, if any.
    #[inline]
    pub fn scope_region(&self) -> Option<ScopeRegion> {
        self.endpoint.cursor.scope_region()
    }

    /// Consume the branch and recover the underlying endpoint.
    ///
    /// Note: This does NOT advance the typestate. Use `decode()` to properly
    /// advance the cursor after receiving a route arm message.
    #[inline]
    pub fn into_endpoint(self) -> CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        self.endpoint
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        ports: [Option<Port<'r, T, E>>; MAX_LANES],
        guards: [Option<LaneGuard<'r, T, U, C>>; MAX_LANES],
        primary_lane: usize,
        sid: SessionId,
        owner: Owner<'r, E0>,
        epoch: EndpointEpoch<'r, E>,
        cursor: PhaseCursor<ROLE>,
        control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
        mint: Mint,
        binding: B,
    ) -> Self {
        let mut endpoint = Self {
            ports,
            guards,
            primary_lane,
            sid,
            _owner: owner,
            _epoch: epoch,
            cursor,
            control,
            lane_route_arms: [[RouteArmState::EMPTY; MAX_ROUTE_ARM_STACK]; MAX_LANES],
            lane_route_arm_lens: [0; MAX_LANES],
            lane_linger_counts: [0; MAX_LANES],
            lane_linger_mask: 0,
            pending_linger_mask: 0,
            pending_offer_mask: 0,
            pending_offer_info: [PendingOfferInfo::EMPTY; MAX_LANES],
            pending_binding: [None; MAX_LANES],
            mint,
            binding,
        };
        endpoint.rebuild_pending_offers();
        endpoint
    }

    #[inline]
    fn scope_trace(&self, scope: ScopeId) -> Option<ScopeTrace> {
        if scope.is_none() {
            return None;
        }
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| ScopeTrace::new(region.range, region.nest))
    }

    /// Set route arm for (lane, scope) — update-in-place if exists, insert if not.
    ///
    /// Returns `Err(PhaseInvariant)` on capacity overflow or invalid lane.
    /// This prevents silent drops that could hide correctness bugs.
    fn set_route_arm(&mut self, lane: u8, scope: ScopeId, arm: u8) -> Result<(), RecvError> {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return Ok(());
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return Err(RecvError::PhaseInvariant);
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        let is_linger = self.is_linger_route(scope);

        // Check if (lane, scope) already exists — update in place
        for idx in 0..len {
            if self.lane_route_arms[lane_idx][idx].scope == scope {
                self.lane_route_arms[lane_idx][idx].arm = arm;
                return Ok(());
            }
        }

        // Not found — insert new entry
        if len >= MAX_ROUTE_ARM_STACK {
            return Err(RecvError::PhaseInvariant);
        }
        self.lane_route_arms[lane_idx][len] = RouteArmState { scope, arm };
        self.lane_route_arm_lens[lane_idx] = self.lane_route_arm_lens[lane_idx].saturating_add(1);
        if is_linger {
            self.increment_linger_count(lane_idx);
        }
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
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        if len == 0 {
            return;
        }
        let is_linger = self.is_linger_route(scope);
        if let Some(pos) = (0..len)
            .rev()
            .find(|&idx| self.lane_route_arms[lane_idx][idx].scope == scope)
        {
            let _removed = self.lane_route_arms[lane_idx][pos];
            let last = len - 1;
            for idx in pos..last {
                self.lane_route_arms[lane_idx][idx] = self.lane_route_arms[lane_idx][idx + 1];
            }
            self.lane_route_arms[lane_idx][last] = RouteArmState::EMPTY;
            self.lane_route_arm_lens[lane_idx] =
                self.lane_route_arm_lens[lane_idx].saturating_sub(1);
            if is_linger {
                self.decrement_linger_count(lane_idx);
            }
        }
    }

    fn is_linger_route(&self, scope: ScopeId) -> bool {
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

    fn increment_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count < u8::MAX);
        *count = count.saturating_add(1);
        if *count == 1 {
            self.lane_linger_mask |= 1u8 << lane_idx;
        }
    }

    fn decrement_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count > 0);
        if *count == 0 {
            return;
        }
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.lane_linger_mask &= !(1u8 << lane_idx);
        }
    }

    fn route_arm_for(&self, lane: u8, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        (0..self.lane_route_arm_lens[lane_idx] as usize)
            .rev()
            .find_map(|idx| {
                let slot = self.lane_route_arms[lane_idx][idx];
                (slot.scope == scope).then_some(slot.arm)
            })
    }

    fn selected_arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
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

    #[inline]
    fn endpoint_policy_args(lane: Lane, label: u8, flags: FrameFlags) -> u32 {
        ((ROLE as u32) << 24)
            | ((lane.as_wire() as u32) << 16)
            | ((label as u32) << 8)
            | flags.bits() as u32
    }

    /// Emit a policy-layer tap event associated with this endpoint.
    ///
    /// The event is tagged with the current lane and session, ensuring that
    /// downstream normalisers (e.g. `observe::normalise::policy_lane_trace`)
    /// can attribute POLICY_* events to the correct rendezvous lane. Use this
    /// for recording resolver / EPF decisions such as `policy_effect`,
    /// `policy_trap`, or `policy_abort`.
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
        let mut event = RawEvent::with_causal(port.now32(), id, causal, arg0, arg1);
        if let Some(trace) = self.scope_trace(scope) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    fn emit_endpoint_event(
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
        let mut event = RawEvent::new(port.now32(), id, meta.sid, packed);
        if let Some(scope) = scope_trace {
            event = event.with_arg2(scope.pack());
        }
        emit(port.tap(), event);
    }

    fn eval_endpoint_policy(
        &self,
        slot: VmSlot,
        event_id: u16,
        arg0: u32,
        arg1: u32,
        lane: Lane,
    ) -> PolicyAction {
        let port = self.port_for_lane(lane.raw() as usize);
        let event = RawEvent::new(port.now32(), event_id, arg0, arg1);
        let _ = port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        epf::run_with(
            port.host_slots(),
            slot,
            &event,
            port.caps_mask(),
            Some(self.sid),
            Some(lane),
            move |ctx| {
                ctx.set_transport_snapshot(transport_metrics);
            },
        )
    }

    fn apply_send_policy(&self, action: PolicyAction, scope: ScopeId, lane: Lane) -> SendResult<()> {
        match action {
            PolicyAction::Proceed => Ok(()),
            PolicyAction::Abort(info) => Err(self.policy_abort_send(info, scope, lane)),
            PolicyAction::Ra(_) => {
                self.emit_policy_event(policy_trap(), 0xFFFF, self.sid.raw(), scope, lane);
                self.emit_policy_event(policy_abort(), 0xFFFF, self.sid.raw(), scope, lane);
                Err(SendError::PolicyAbort { reason: 0xFFFF })
            }
            PolicyAction::Tap { id, arg0, arg1 } => {
                self.emit_policy_event(id, arg0, arg1, scope, lane);
                Ok(())
            }
            PolicyAction::Route { .. } => Ok(()),
        }
    }

    fn policy_abort_send(&self, info: AbortInfo, scope: ScopeId, lane: Lane) -> SendError {
        if info.trap.is_some() {
            self.emit_policy_event(policy_trap(), info.reason as u32, self.sid.raw(), scope, lane);
        }
        self.emit_policy_event(policy_abort(), info.reason as u32, self.sid.raw(), scope, lane);
        SendError::PolicyAbort {
            reason: info.reason,
        }
    }

    fn apply_recv_policy(&self, action: PolicyAction, scope: ScopeId, lane: Lane) -> RecvResult<()> {
        match action {
            PolicyAction::Proceed => Ok(()),
            PolicyAction::Abort(info) => Err(self.policy_abort_recv(info, scope, lane)),
            PolicyAction::Ra(_) => {
                self.emit_policy_event(policy_trap(), 0xFFFF, self.sid.raw(), scope, lane);
                self.emit_policy_event(policy_abort(), 0xFFFF, self.sid.raw(), scope, lane);
                Err(RecvError::PolicyAbort { reason: 0xFFFF })
            }
            PolicyAction::Tap { id, arg0, arg1 } => {
                self.emit_policy_event(id, arg0, arg1, scope, lane);
                Ok(())
            }
            PolicyAction::Route { .. } => Ok(()),
        }
    }

    fn policy_abort_recv(&self, info: AbortInfo, scope: ScopeId, lane: Lane) -> RecvError {
        if info.trap.is_some() {
            self.emit_policy_event(policy_trap(), info.reason as u32, self.sid.raw(), scope, lane);
        }
        self.emit_policy_event(policy_abort(), info.reason as u32, self.sid.raw(), scope, lane);
        RecvError::PolicyAbort {
            reason: info.reason,
        }
    }

    /// Create a CapFlow for the current send transition.
    ///
    /// This is the primary entry point for sending messages. Returns a `CapFlow`
    /// that must be consumed by calling `.send(arg).await`.
    ///
    /// Automatically handles routing: if the target label doesn't match the current
    /// cursor position, attempts to advance to the correct branch.
    pub fn flow<M>(mut self) -> SendResult<CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>>
    where
        M: crate::g::MessageSpec + crate::g::SendableLabel,
        T: Transport + 'r,
        U: LabelUniverse,
        C: Clock,
        E: EpochTable,
        Mint: MintConfigMarker,
    {
        let target_label = <M as crate::g::MessageSpec>::LABEL;
        self.try_select_lane_for_label(target_label);

        // For Route scopes, handle cursor repositioning at controller arm entry points.
        // This covers both linger (loops) and non-linger routes when the controller
        // needs to select a different arm than the current cursor position.
        if let Some(region) = self.cursor.scope_region() {
            if region.kind == ScopeKind::Route {
                // For linger scopes (loops), follow any pending Jump nodes first.
                // LoopContinue jumps back to loop_start, then we reposition to the target arm.
                if region.linger && self.cursor.is_jump() {
                    self.cursor = self
                        .cursor
                        .try_follow_jumps()
                        .map_err(|_| SendError::PhaseInvariant)?;
                }

                let scope_id = region.scope_id;
                if self.cursor.is_route_controller(scope_id) {
                    let at_route_start = self.cursor.index() == region.start;
                    let at_arm_entry = self.cursor.is_at_controller_arm_entry(scope_id);
                    let at_decision = at_arm_entry || at_route_start || self.cursor.label().is_none();
                    if at_decision {
                        // Use O(1) controller_arm_entry registry lookup to reposition
                        // cursor to the arm entry matching target_label.
                        if let Some(entry_idx) =
                            self.cursor.controller_arm_entry_for_label(scope_id, target_label)
                        {
                            self.cursor = self.cursor.with_index(entry_idx as usize);
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

            // Follow Jump nodes (LoopContinue, LoopBreak, RouteArmEnd are auto-followed).
            // Only PassiveObserverBranch stops here.
            if self.cursor.is_jump() {
                self.cursor = self
                    .cursor
                    .try_follow_jumps()
                    .map_err(|_| SendError::PhaseInvariant)?;
            }

            // Handle PassiveObserverBranch Jump: use structured arm navigation
            // instead of scanning the entire scope for the target label.
            if self.cursor.is_jump() {
                if let Some(JumpReason::PassiveObserverBranch) = self.cursor.jump_reason() {
                    // Find which arm contains the target label and follow the corresponding Jump
                    if let Some(new_cursor) =
                        self.cursor.follow_passive_observer_for_label(target_label)
                    {
                        self.cursor = new_cursor;
                        continue;
                    }
                }
            }

            // Accept both Send and Local actions
            if !self.cursor.is_send() && !self.cursor.is_local_action() {
                if let Some(region) = self.cursor.scope_region() {
                    if region.kind == ScopeKind::Route
                        && self.can_advance_route_scope(region.scope_id, target_label)
                    {
                        if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                            self.cursor = cursor;
                            continue;
                        }
                    }
                }
                return Err(SendError::PhaseInvariant);
            }

            // Get metadata: for Local actions, create SendMeta with peer=ROLE
            let current_meta = if self.cursor.is_local_action() {
                let local = self
                    .cursor
                    .try_local_meta()
                    .ok_or(SendError::PhaseInvariant)?;
                SendMeta {
                    eff_index: local.eff_index,
                    peer: ROLE,
                    label: local.label,
                    resource: local.resource,
                    is_control: local.is_control,
                    next: local.next,
                    scope: local.scope,
                    route_arm: local.route_arm,
                    shot: local.shot,
                    plan: local.plan,
                    lane: local.lane,
                }
            } else {
                self.cursor
                    .try_send_meta()
                    .ok_or(SendError::PhaseInvariant)?
            };

            if current_meta.label == target_label {
                self.evaluate_dynamic_plan(&current_meta, target_label)?;
                return Ok(CapFlow::new(self, current_meta));
            }

            // Label mismatch: try advancing past Route scope boundary.
            // No O(n) seek_label fallback - cursor must be at correct position.
            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route
                    && self.can_advance_route_scope(region.scope_id, target_label)
                {
                    if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                        self.cursor = cursor;
                        continue;
                    }
                }
            }

            return Err(SendError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            });
        }
    }

    fn evaluate_dynamic_plan(&mut self, meta: &SendMeta, target_label: u8) -> SendResult<()> {
        if !meta.plan.is_dynamic() {
            return Ok(());
        }
        match classify_dynamic_label(target_label) {
            DynamicLabelClass::Loop => self.evaluate_loop_plan(meta),
            DynamicLabelClass::SpliceOrReroute => {
                Ok(())
            }
            DynamicLabelClass::Route => self.evaluate_route_plan(meta, target_label),
        }
    }

    fn evaluate_route_plan(&mut self, meta: &SendMeta, target_label: u8) -> SendResult<()> {
        let plan = meta.plan;
        let (policy_id, _meta) = plan.dynamic_components().ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }

        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;

        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let metrics = port.transport().metrics().snapshot();
        let transport_ctx = self.transport_context_snapshot();
        let resolution = cluster
            .resolve_dynamic_plan(
                self.rendezvous_id(),
                Some(CpSessionId::new(self.sid.raw())),
                CpLaneId::new(port.lane().raw()),
                meta.eff_index,
                tag,
                metrics,
                transport_ctx,
            )
            .map_err(Self::map_cp_error)?;

        if scope_id.is_none() || scope_id != plan.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match resolution {
            DynamicResolution::RouteArm { arm } if arm == arm_index => Ok(()),
            DynamicResolution::RouteArm { .. } => {
                Err(SendError::PolicyAbort { reason: policy_id })
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    fn evaluate_loop_plan(&mut self, meta: &SendMeta) -> SendResult<()> {
        // For CanonicalControl (self-send), the caller explicitly chooses continue/break.
        // No resolver validation is needed - the caller's choice is authoritative.
        if meta.peer == ROLE {
            return Ok(());
        }

        let plan = meta.plan;
        let (policy_id, _meta) = plan.dynamic_components().ok_or(SendError::PhaseInvariant)?;
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;

        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let metrics = port.transport().metrics().snapshot();
        let transport_ctx = self.transport_context_snapshot();
        let resolution = cluster
            .resolve_dynamic_plan(
                self.rendezvous_id(),
                Some(CpSessionId::new(self.sid.raw())),
                CpLaneId::new(port.lane().raw()),
                meta.eff_index,
                tag,
                metrics,
                transport_ctx,
            )
            .map_err(Self::map_cp_error)?;

        if meta.scope.is_none() || meta.scope != plan.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match resolution {
            DynamicResolution::Loop { decision } => {
                let disposition = if decision {
                    LoopDisposition::Continue
                } else {
                    LoopDisposition::Break
                };
                let expected_label = match disposition {
                    LoopDisposition::Continue => LABEL_LOOP_CONTINUE,
                    LoopDisposition::Break => LABEL_LOOP_BREAK,
                };
                if expected_label != meta.label {
                    return Err(SendError::PolicyAbort { reason: policy_id });
                }
                Ok(())
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    /// Try to select route arm meta via recv index lookup.
    /// Returns None if the arm doesn't have a recv node for this role.
    fn try_select_route_arm_meta(&mut self, scope_id: ScopeId, target_arm: u8) -> Option<RecvMeta> {
        let idx = self.cursor.route_scope_arm_recv_index(scope_id, target_arm)?;
        self.cursor = self.cursor.with_index(idx);
        let mut meta = self.cursor.try_recv_meta()?;
        meta.route_arm = Some(target_arm);
        Some(meta)
    }

    fn emit_route_decision(&self, scope_id: ScopeId, arm: u8, decision: u8, lane: u8) {
        let port = self.port_for_lane(lane as usize);
        let causal = TapEvent::make_causal_key(port.lane().as_wire(), decision);
        let arg0 = self.sid.raw();
        let arg1 = ((scope_id.raw() as u32) << 16) | (arm as u32);
        let mut event = events::RouteDecision::with_causal(port.now32(), causal, arg0, arg1);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    fn prepare_route_decision_from_resolver(&self, scope_id: ScopeId) -> RecvResult<u8> {
        let debug = OfferDebugger::new();
        let (plan, eff_index, tag) = self
            .cursor
            .route_scope_controller_plan(scope_id)
            .ok_or(RecvError::PhaseInvariant)?;
        if !plan.is_dynamic() {
            debug.resolver_disabled(
                scope_id,
                plan,
                self.cursor.scope_entry_for_test(scope_id),
                false,
            );
            return Err(RecvError::PhaseInvariant);
        }
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = CpRendezvousId::new(self.rendezvous_id().raw());
        let offer_lane = self.offer_lane_for_scope(scope_id);
        let port = self.port_for_lane(offer_lane as usize);
        let lane = CpLaneId::new(port.lane().raw());
        let metrics = port.transport().metrics().snapshot();
        let transport_ctx = self.transport_context_snapshot();
        let resolution = cluster
            .resolve_dynamic_plan(rv_id, None, lane, eff_index, tag, metrics, transport_ctx)
            .map_err(Self::map_recv_cp_error)?;
        let arm = match resolution {
            DynamicResolution::RouteArm { arm } => arm,
            DynamicResolution::Loop { decision } => {
                if decision { 0 } else { 1 }
            }
            _ => {
                debug.resolver_unexpected(scope_id, &resolution);
                return Err(RecvError::PhaseInvariant);
            }
        };
        port.record_route_decision(scope_id, arm);
        self.emit_route_decision(scope_id, arm, 2, offer_lane);
        Ok(arm)
    }

    /// Route decision via controller_arm_entry labels.
    fn prepare_route_decision_from_resolver_via_arm_entry(
        &self,
        scope_id: ScopeId,
    ) -> RecvResult<u8> {
        let debug = OfferDebugger::new();
        // Get arm 0's entry to find the label used for resolver lookup
        let (arm0_entry, _arm0_label) = self
            .cursor
            .controller_arm_entry_by_arm(scope_id, 0)
            .ok_or(RecvError::PhaseInvariant)?;

        // Navigate to arm0_entry to get the node's metadata
        let arm0_cursor = self.cursor.with_index(arm0_entry as usize);

        // The arm entry node should be a Local (self-send) node with a HandlePlan
        let local_meta = arm0_cursor
            .try_local_meta()
            .ok_or(RecvError::PhaseInvariant)?;

        let plan = local_meta.plan;
        if !plan.is_dynamic() {
            debug.resolver_disabled(
                scope_id,
                plan,
                self.cursor.scope_entry_for_test(scope_id),
                true,
            );
            return Err(RecvError::PolicyAbort { reason: 0 });
        }

        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = CpRendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(local_meta.lane as usize);
        let lane = CpLaneId::new(port.lane().raw());
        let metrics = port.transport().metrics().snapshot();
        let transport_ctx = self.transport_context_snapshot();
        let tag = local_meta.resource.unwrap_or(0);
        let resolution = cluster
            .resolve_dynamic_plan(
                rv_id,
                None,
                lane,
                local_meta.eff_index,
                tag,
                metrics,
                transport_ctx,
            )
            .map_err(Self::map_recv_cp_error)?;

        let arm = match resolution {
            DynamicResolution::RouteArm { arm } => arm,
            DynamicResolution::Loop { decision } => {
                if decision { 0 } else { 1 }
            }
            _ => {
                debug.resolver_unexpected(scope_id, &resolution);
                return Err(RecvError::PhaseInvariant);
            }
        };
        port.record_route_decision(scope_id, arm);
        self.emit_route_decision(scope_id, arm, 2, local_meta.lane);
        Ok(arm)
    }

    fn route_arm_from_label(&self, scope_id: ScopeId, label: u8) -> Option<u8> {
        match label {
            LABEL_LOOP_CONTINUE => return Some(0),
            LABEL_LOOP_BREAK => return Some(1),
            _ => {}
        }

        if self.cursor.is_route_controller(scope_id) {
            return None;
        }

        if let Some(arm) = self.cursor.find_arm_for_recv_label(label) {
            if arm < 2 {
                return Some(arm);
            }
        }

        if Self::is_loop_control_scope(&self.cursor, scope_id)
            && self
                .cursor
                .route_scope_arm_recv_index(scope_id, 1)
                .is_none()
        {
            return Some(0);
        }
        None
    }

    fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }
    fn map_recv_cp_error(err: CpError) -> RecvError {
        match err {
            CpError::PolicyAbort { reason } => RecvError::PolicyAbort { reason },
            _ => RecvError::PhaseInvariant,
        }
    }

    pub(crate) async fn send_with_meta<M>(
        mut self,
        meta: &SendMeta,
        payload: Option<&<M as crate::g::MessageSpec>::Payload>,
    ) -> SendResult<(Self, ControlOutcome<'r, ControlResource<M>>)>
    where
        M: crate::g::MessageSpec + crate::g::SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
    {
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(SendError::PhaseInvariant);
        }

        let mut control_outcome = ControlOutcome::<'r, ControlResource<M>>::None;
        let mut canonical_fallback: Option<GenericCapToken<ControlResource<M>>> = None;

        let policy_action = self.eval_endpoint_policy(
            VmSlot::EndpointTx,
            ids::ENDPOINT_SEND,
            self.sid.raw(),
            Self::endpoint_policy_args(Lane::new(meta.lane as u32), meta.label, FrameFlags::empty()),
            Lane::new(meta.lane as u32),
        );
        self.apply_send_policy(policy_action, meta.scope, Lane::new(meta.lane as u32))?;

        let cluster_ref = self.control.cluster();
        let rv_id = self.rendezvous_id();
        let sid_raw = self.sid.raw();
        let lane_wire = self
            .port_for_lane(meta.lane as usize)
            .lane()
            .as_wire();
        let scope_trace = self.scope_trace(meta.scope);
        let logical_meta =
            TapFrameMeta::new(sid_raw, lane_wire, ROLE, meta.label, FrameFlags::empty());

        let route_info = if meta.scope.kind() == ScopeKind::Route {
            if let Some(arm) = meta.route_arm {
                Some((meta.scope, arm))
            } else {
                None
            }
        } else {
            None
        };

        // Auto-mint tokens for both Canonical and External control handling.
        // Canonical: token is used internally, not transmitted over wire.
        // External: token is minted AND transmitted over wire (cross-role).
        let minted_token = if matches!(
            control_handling,
            ControlHandling::Canonical | ControlHandling::External
        ) {
            let token_result = <M::ControlKind as CanonicalTokenProvider<
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
            >>::into_token(&self, meta);
            token_result?
        } else {
            None
        };

        let mut dispatch_frame = None;
        let mut route_tap: Option<(ScopeId, u8)> = None;
        {
            // Use lane-specific port for multi-lane parallel execution
            let port = self.port_for_lane_mut(meta.lane as usize);
            let payload_view = unsafe {
                let scratch = &mut *port.scratch_ptr();
                let len = match control_handling {
                    ControlHandling::None => {
                        let data = payload.ok_or(SendError::PhaseInvariant)?;
                        data.encode_into(scratch).map_err(SendError::Codec)?
                    }
                    ControlHandling::Canonical => {
                        let token = minted_token.ok_or(SendError::PhaseInvariant)?;
                        let frame = token.into_frame();
                        let bytes = *frame.bytes();
                        scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
                        canonical_fallback = Some(frame.as_generic());
                        dispatch_frame = Some(frame);
                        CAP_TOKEN_LEN
                    }
                    ControlHandling::External => {
                        // External control: behavior depends on AUTO_MINT_EXTERNAL.
                        // - If auto-minted (e.g., splice): use minted token
                        // - Otherwise (e.g., management): use caller-provided payload
                        if let Some(token) = minted_token {
                            let frame = token.into_frame();
                            let bytes = *frame.bytes();
                            scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
                            control_outcome = ControlOutcome::External(frame.as_generic());
                            dispatch_frame = Some(frame);
                            CAP_TOKEN_LEN
                        } else {
                            let data = payload.ok_or(SendError::PhaseInvariant)?;
                            data.encode_into(scratch).map_err(SendError::Codec)?
                        }
                    }
                };
                let slice = &scratch[..len];
                Payload::new(slice)
            };

            if let Some((scope_id, arm)) = route_info {
                port.record_route_decision(scope_id, arm);
                route_tap = Some((scope_id, arm));
            }

            let transport = port.transport();
            let tx_ptr = port.tx_ptr();

            // Invoke binding hook before transport send
            // Build SendMetadata from SendMeta for the new API
            let direction = if meta.peer == ROLE {
                // Self-send (CanonicalControl)
                crate::binding::LocalDirection::Local
            } else {
                // Cross-role send
                crate::binding::LocalDirection::Send
            };
            let binding_meta = crate::binding::SendMetadata {
                eff_index: meta.eff_index,
                label: meta.label,
                peer: meta.peer,
                lane: meta.lane,
                direction,
                is_control: meta.is_control,
            };
            let disposition = self
                .binding
                .on_send_with_meta(binding_meta, payload_view.as_bytes())
                .map_err(|_| SendError::Binding)?;

            // Skip wire transmission if:
            // 1. Self-send (CanonicalControl) - Local messages never go to wire
            // 2. Binder returned Handled - Binder already did wire I/O
            let should_bypass = direction == crate::binding::LocalDirection::Local
                || disposition == crate::binding::SendDisposition::Handled;

            if !should_bypass {
                unsafe {
                    transport
                        .send(&mut *tx_ptr, payload_view, meta.peer)
                        .await
                        .map_err(|err| SendError::Transport(err.into()))?;
                }
            }
        }

        if let Some((scope_id, arm)) = route_tap {
            self.emit_route_decision(scope_id, arm, 1, meta.lane);
        }

        // Advance typestate cursor (delegates to RoleTypestate).
        // Use try_advance_past_jumps to follow any Jump nodes (explicit control flow).
        self.cursor = self
            .cursor
            .try_advance_past_jumps()
            .map_err(|_| SendError::PhaseInvariant)?;

        let lane_idx = meta.lane as usize;
        self.advance_lane_cursor(lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();

        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_SEND
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);

        if let Some(frame) = dispatch_frame {
            let cluster = cluster_ref.ok_or(SendError::PhaseInvariant)?;
            match cluster.dispatch_typed_control_frame(rv_id, frame, None) {
                Ok(result) => {
                    if matches!(control_handling, ControlHandling::Canonical) {
                        let registered = result.ok_or(SendError::PhaseInvariant)?;
                        control_outcome = ControlOutcome::Canonical(registered);
                    }
                }
                Err(err) => {
                    match err {
                        CpError::Authorisation {
                            effect: CpEffect::SpliceAck,
                        } => {
                            if let Some(token) = canonical_fallback.take() {
                                control_outcome = ControlOutcome::Canonical(
                                    CapRegisteredToken::from_bytes(token.into_bytes()),
                                );
                            }
                        }
                        _ => return Err(SendError::PhaseInvariant),
                    }
                }
            }
        } else if matches!(control_handling, ControlHandling::Canonical) {
            return Err(SendError::PhaseInvariant);
        }

        Ok((self, control_outcome))
    }

    /// Receive a payload of type `M` according to the current typestate step.
    pub async fn recv<M>(mut self) -> RecvResult<(Self, <M as crate::g::MessageSpec>::Payload)>
    where
        M: crate::g::MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let target_label = <M as crate::g::MessageSpec>::LABEL;
        self.try_select_lane_for_label(target_label);

        // Navigate to the correct recv position, handling route scopes via resolver
        let mut _iter_count = 0u32;
        loop {
            _iter_count += 1;
            // Defensive check: recv() should converge within 3 iterations.
            // If we exceed this, it indicates a bug in control flow logic.
            debug_assert!(
                _iter_count <= 3,
                "recv() infinite loop detected at iter={}",
                _iter_count
            );
            if _iter_count > 3 {
                return Err(RecvError::PhaseInvariant);
            }

            // Handle loop decision Jump nodes (passive observer at loop boundary).
            // When we land on a Jump(LoopContinue), we need to consult the resolver
            // to decide which arm to take for the next iteration.
            // Note: LoopBreak is NOT handled here - it's followed automatically by
            // advance_past_jumps() since Break arm is already selected.
            if let Some(reason) = self.cursor.jump_reason() {
                if matches!(reason, JumpReason::LoopContinue) {
                    if let Some(region) = self.cursor.scope_region() {
                        if region.kind == ScopeKind::Route && region.linger {
                            let scope_id = region.scope_id;
                            // Consult resolver for loop decision
                            if let Ok(arm) = self.prepare_route_decision_from_resolver(scope_id) {
                                // Navigate based on resolver decision using O(1) registry lookup
                                if arm == 0 {
                                    // Continue: follow LoopContinue jump to loop start
                                    self.cursor = self.cursor.advance();
                                } else {
                                    // Break: use PassiveObserverBranch registry for O(1) lookup
                                    if let Some(nav) =
                                        self.cursor.follow_passive_observer_arm(arm)
                                    {
                                        let PassiveArmNavigation::WithinArm { entry } = nav;
                                        self.cursor = self.cursor.with_index(entry as usize);
                                    }
                                }
                                continue;
                            }
                        }
                    }
                }
            }

            // Check if we're at a route scope boundary where we need resolver to select arm
            // This must be checked BEFORE is_recv() because we might be at recv position
            // but in the wrong arm of the route
            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route && self.cursor.index() == region.start {
                    let scope_id = region.scope_id;
                    let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
                    // Check if we already have arm info for this scope
                    let existing_arm = self.route_arm_for(lane_wire, scope_id);
                    if let Some(arm) = existing_arm {
                        // Navigate to the recv position for this arm.
                        // Controller roles use route_recv_indices O(1) lookup.
                        // Passive observers use PassiveObserverBranch O(1) registry.
                        let recv_idx = self.cursor.route_scope_arm_recv_index(scope_id, arm);
                        if let Some(idx) = recv_idx {
                            self.cursor = self.cursor.with_index(idx);
                            self.set_route_arm(lane_wire, scope_id, arm)?;
                            continue;
                        }
                        // Passive observer: use PassiveObserverBranch registry for O(1) lookup
                        if let Some(nav) = self.cursor.follow_passive_observer_arm(arm) {
                            let PassiveArmNavigation::WithinArm { entry } = nav;
                            self.cursor = self.cursor.with_index(entry as usize);
                            self.set_route_arm(lane_wire, scope_id, arm)?;
                            continue;
                        }
                        // If arm has no recv (e.g., Break arm of loop), advance past route scope
                        if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                            self.cursor = cursor;
                            continue;
                        }
                    } else {
                        return Err(RecvError::PhaseInvariant);
                    }
                }
            }

            if self.cursor.is_recv() {
                break;
            }

            // If not at recv and not at route start, try to advance past route scope
            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route
                    && self.can_advance_route_scope(region.scope_id, target_label)
                {
                    if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                        self.cursor = cursor;
                        continue;
                    }
                }
            }
            return Err(RecvError::PhaseInvariant);
        }

        let meta = self
            .cursor
            .try_recv_meta()
            .ok_or(RecvError::PhaseInvariant)?;
        if meta.label != target_label {
            return Err(RecvError::LabelMismatch {
                expected: meta.label,
                actual: target_label,
            });
        }

        let sid_raw = self.sid.raw();
        let lane_wire = self
            .port_for_lane(meta.lane as usize)
            .lane()
            .as_wire();

        // Try binding-based recv first (for FlowBinderSlot with TransportOps).
        // This path reads from stream buffers (e.g., QUIC STREAM data) via on_recv(),
        // which extracts the payload from framing. Falls back to transport.recv()
        // for raw protocol frames (e.g., handshake CRYPTO frames).
        //
        let mut binding_buf: [u8; 65536] = [0; 65536];
        let logical_lane = meta.lane;

        let binding_data = self.try_recv_from_binding(logical_lane, meta.label, &mut binding_buf)?;

        let payload_bytes: &[u8] = if let Some(n) = binding_data {
            &binding_buf[..n]
        } else {
            'recv_loop: loop {
                let payload = {
                    let port = self.port_for_lane_mut(meta.lane as usize);
                    let transport = port.transport();
                    let rx_ptr = port.rx_ptr();
                    unsafe {
                        transport
                            .recv(&mut *rx_ptr)
                            .await
                            .map_err(|err| RecvError::Transport(err.into()))?
                    }
                };

                if let Some(n) =
                    self.try_recv_from_binding(logical_lane, meta.label, &mut binding_buf)?
                {
                    break 'recv_loop &binding_buf[..n];
                }

                if payload.as_bytes().is_empty() {
                    let binding_active = self.binding.transport_context().is_some();
                    if !binding_active {
                        break 'recv_loop payload.as_bytes();
                    }
                    if M::Payload::decode_owned(&[]).is_ok() {
                        break 'recv_loop payload.as_bytes();
                    }
                    // Empty payload likely signals stream data queued for binding path.
                    continue;
                }

                break 'recv_loop payload.as_bytes();
            }
        };

        let policy_action = self.eval_endpoint_policy(
            VmSlot::EndpointRx,
            ids::ENDPOINT_RECV,
            sid_raw,
            Self::endpoint_policy_args(Lane::new(meta.lane as u32), meta.label, FrameFlags::empty()),
            Lane::new(meta.lane as u32),
        );
        self.apply_recv_policy(policy_action, meta.scope, Lane::new(meta.lane as u32))?;

        let logical_meta =
            TapFrameMeta::new(sid_raw, lane_wire, ROLE, meta.label, FrameFlags::empty());
        let payload =
            M::Payload::decode_owned(payload_bytes).map_err(RecvError::Codec)?;

        let scope_trace = self.scope_trace(meta.scope);
        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_RECV
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);

        // Advance typestate cursor (delegates to RoleTypestate).
        // Use try_advance_past_jumps to follow any Jump nodes (explicit control flow).
        self.cursor = self
            .cursor
            .try_advance_past_jumps()
            .map_err(|_| RecvError::PhaseInvariant)?;

        let recv_lane_idx = meta.lane as usize;
        self.advance_lane_cursor(recv_lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
        Ok((self, payload))
    }

    fn record_loop_decision(
        &self,
        metadata: &LoopMetadata<ROLE>,
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
            ts, causal, self.sid.raw(), arg1,
            self.scope_trace(metadata.scope).map(|t| t.pack()).unwrap_or(0),
        );
        emit(port.tap(), event);
        if metadata.scope.kind() == ScopeKind::Route {
            port.record_route_decision(metadata.scope, arm);
            self.emit_route_decision(metadata.scope, arm, 1, lane);
        }
        Ok(())
    }

    /// Observe an inbound route branch.
    ///
    /// Route hints are drained once per call and consumed only when they match
    /// the current route scope.
    /// Loop control hints that resolve a recv-less arm are treated as
    /// EmptyArmTerminal and skip decode.
    pub async fn offer(self) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let mut self_endpoint = self;
        let debug = OfferDebugger::new();
        debug.offer_start(
            self_endpoint.cursor.index(),
            self_endpoint.phase_index(),
            self_endpoint.lane_cursors(),
            self_endpoint.cursor.scope_region().map(|r| r.scope_id),
        );
        self_endpoint.select_offer_entry()?;
        // O(1) entry: offer() must be called at a Route decision point.
        // Use the node's scope directly (no parent traversal).
        let node_scope = self_endpoint.cursor.node_scope_id();
        let region = match self_endpoint.cursor.scope_region_by_id(node_scope) {
            Some(region) => region,
            None => {
                debug.missing_scope_region(self_endpoint.cursor.index(), node_scope);
                return Err(RecvError::PhaseInvariant);
            }
        };
        if region.kind != ScopeKind::Route {
            debug.non_route_scope(
                self_endpoint.cursor.index(),
                region.scope_id,
                region.kind,
            );
            return Err(RecvError::PhaseInvariant);
        }
        let scope_id = region.scope_id;
        if let Some(offer_entry) = self_endpoint.cursor.route_scope_offer_entry(scope_id) {
            if offer_entry != u16::MAX && self_endpoint.cursor.index() != offer_entry as usize {
                debug.offer_entry_mismatch(
                    self_endpoint.cursor.index(),
                    offer_entry,
                    scope_id,
                );
                return Err(RecvError::PhaseInvariant);
            }
        }
        // Route hints are offer-scoped; hints are consumed per-offer via take_hint_for_offer().
        let (offer_lanes, offer_lanes_len) = self_endpoint.offer_lanes_for_scope(scope_id);
        let offer_lane = offer_lanes[0];
        let offer_lane_idx = offer_lane as usize;

        // Self-send controller routes have no recv nodes in this scope.
        let cursor_is_not_recv = !self_endpoint.cursor.is_recv();
        let is_route_controller = self_endpoint.cursor.is_route_controller(scope_id);
        let loop_scope_active = self_endpoint
            .cursor
            .typestate_node(self_endpoint.cursor.index())
            .loop_scope()
            .is_some();
        let loop_continue_has_recv = self_endpoint
            .cursor
            .route_scope_arm_recv_index(scope_id, 0)
            .is_some();
        let loop_break_has_recv = self_endpoint
            .cursor
            .route_scope_arm_recv_index(scope_id, 1)
            .is_some();

        // Fast path: Peek for a matching recv_label_hint BEFORE waiting for wire data.
        // Hints are consumed only after a payload is ready to decode.
        let hint_label = self_endpoint.peek_hint_for_offer(
            scope_id,
            &offer_lanes,
            offer_lanes_len,
            is_route_controller,
            loop_scope_active,
            loop_continue_has_recv,
            loop_break_has_recv,
        );
        let has_matching_hint = hint_label.is_some();
        let hint_arm = hint_label.and_then(|label| self_endpoint.arm_from_hint_label(scope_id, label));
        debug_assert!(
            hint_label.is_none() || hint_arm.is_some(),
            "route hint label missing arm mapping"
        );
        let hint_arm_has_recv = match hint_arm {
            Some(arm) => self_endpoint.arm_has_recv(scope_id, arm),
            None => true,
        };
        let arm_count = self_endpoint.cursor.route_scope_arm_count(scope_id);
        let is_self_send_controller =
            cursor_is_not_recv && is_route_controller && arm_count.map_or(true, |c| c == 0);

        // Use RouteTable early when the selected arm has no recv for this role.
        let early_route_decision =
            self_endpoint.ack_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len);

        // Skip recv loop when the selected arm has no recv node for this role.
        let early_decision_arm_has_no_recv =
            early_route_decision.map(|arm| !self_endpoint.arm_has_recv(scope_id, arm)).unwrap_or(false);

        // Skip recv loop when the decision is available without wire data.
        let skip_recv_loop = is_route_controller
            || is_self_send_controller
            || early_decision_arm_has_no_recv
            || (has_matching_hint && !hint_arm_has_recv);
        let (binding_classification, payload_view): (
            Option<crate::binding::IncomingClassification>,
            crate::transport::wire::Payload<'_>,
        ) = if skip_recv_loop {
            (None, crate::transport::wire::Payload::new(&[]))
        } else {
            'offer_recv: loop {
                if !is_route_controller {
                    if let Some(classification) = self_endpoint.poll_binding_for_offer(
                        scope_id,
                        &offer_lanes,
                        offer_lanes_len,
                        loop_scope_active,
                        loop_continue_has_recv,
                        loop_break_has_recv,
                    )
                    {
                        break 'offer_recv (
                            Some(classification),
                            crate::transport::wire::Payload::new(&[]),
                        );
                    }
                }

                let payload = {
                    let port = self_endpoint.port_for_lane(offer_lane_idx);
                    let transport = port.transport();
                    let rx_ptr = port.rx_ptr();
                    unsafe {
                        transport
                            .recv(&mut *rx_ptr)
                            .await
                            .map_err(|err| RecvError::Transport(err.into()))?
                    }
                };

                if !is_route_controller {
                    if let Some(classification) = self_endpoint.poll_binding_for_offer(
                        scope_id,
                        &offer_lanes,
                        offer_lanes_len,
                        loop_scope_active,
                        loop_continue_has_recv,
                        loop_break_has_recv,
                    )
                    {
                        break 'offer_recv (
                            Some(classification),
                            crate::transport::wire::Payload::new(&[]),
                        );
                    }
                }

                if payload.as_bytes().is_empty() {
                    let pending_payload_hint = if hint_label.is_some() {
                        hint_arm_has_recv
                    } else {
                        self_endpoint
                            .peek_hint_for_offer(
                                scope_id,
                                &offer_lanes,
                                offer_lanes_len,
                                is_route_controller,
                                loop_scope_active,
                                loop_continue_has_recv,
                                loop_break_has_recv,
                            )
                            .map(|label| {
                                let hint_arm =
                                    self_endpoint.arm_from_hint_label(scope_id, label);
                                match hint_arm {
                                    Some(arm) => self_endpoint.arm_has_recv(scope_id, arm),
                                    None => true,
                                }
                            })
                            .unwrap_or(false)
                    };
                    if pending_payload_hint {
                        continue 'offer_recv;
                    }
                }

                break 'offer_recv (None, payload);
            }
        };

        // Resolution order: RouteTable → wire (binding) → hint → mergeable → resolver → poll_route_decision.
        let route_decision = early_route_decision.or_else(|| {
            self_endpoint.ack_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
        });
        let resolved_label_hint = self_endpoint.take_hint_for_offer(
            scope_id,
            &offer_lanes,
            offer_lanes_len,
            is_route_controller,
            loop_scope_active,
            loop_continue_has_recv,
            loop_break_has_recv,
        );
        if let Some(label) = resolved_label_hint {
            debug.resolved_hint(scope_id, label, is_route_controller);
        }

        let route_mergeable = self_endpoint
            .cursor
            .route_scope_mergeable(scope_id)
            .unwrap_or(false);

        let mut route_input = route_decision.map(RouteInput::Decision);
        if route_input.is_none() && !is_route_controller {
            if let Some(classification) = binding_classification {
                if let Some(arm) =
                    self_endpoint.route_arm_from_label(scope_id, classification.label)
                {
                    route_input = Some(RouteInput::Wire {
                        arm,
                        channel: classification.channel,
                        instance: classification.instance,
                    });
                }
            }
        }
        if route_input.is_none() && !is_route_controller {
            if let Some(label) = resolved_label_hint {
                if let Some(arm) = self_endpoint.arm_from_hint_label(scope_id, label) {
                    route_input = Some(RouteInput::Hint { arm });
                }
            }
        }
        if route_input.is_none() && !is_route_controller && route_mergeable {
            route_input = Some(RouteInput::Mergeable);
        }
        if route_input.is_none() && is_route_controller {
            let arm_count_step3 = self_endpoint.cursor.route_scope_arm_count(scope_id);
            let is_self_send_route = arm_count_step3.map_or(true, |c| c == 0);
            let resolver_result = if is_self_send_route {
                self_endpoint.prepare_route_decision_from_resolver_via_arm_entry(scope_id)
            } else {
                self_endpoint.prepare_route_decision_from_resolver(scope_id)
            };
            match resolver_result {
                Ok(arm) => {
                    route_input = Some(RouteInput::Resolver(arm));
                }
                Err(RecvError::PolicyAbort { reason: _ }) => {}
                Err(err) => {
                    return Err(err);
                }
            }
        }
        if route_input.is_none() {
            let resolved = poll_fn(|cx| {
                let mut lane_idx = 0usize;
                while lane_idx < offer_lanes_len {
                    let lane = offer_lanes[lane_idx];
                    let port = self_endpoint.port_for_lane(lane as usize);
                    match port.poll_route_decision(scope_id, ROLE, cx) {
                        Poll::Ready(arm) => return Poll::Ready(arm),
                        Poll::Pending => {}
                    }
                    lane_idx += 1;
                }
                Poll::Pending
            })
            .await;
            route_input = Some(RouteInput::Poll(resolved));
        }
        let route_input = route_input.ok_or(RecvError::PhaseInvariant)?;
        let mut binding_channel: Option<crate::binding::Channel> = None;
        let mut binding_instance: Option<u16> = None;
        let mut resolved_from_hint = false;
        let selected_arm = match route_input {
            RouteInput::Decision(arm)
            | RouteInput::Resolver(arm)
            | RouteInput::Poll(arm) => arm,
            RouteInput::Wire {
                arm,
                channel,
                instance,
            } => {
                binding_channel = Some(channel);
                binding_instance = Some(instance);
                arm
            }
            RouteInput::Hint { arm } => {
                resolved_from_hint = true;
                arm
            }
            RouteInput::Mergeable => {
                self_endpoint
                    .port_for_lane(offer_lane_idx)
                    .record_route_decision(scope_id, 0);
                self_endpoint.emit_route_decision(scope_id, 0, 2, offer_lane);
                0
            }
        };
        debug.selected_arm(scope_id, selected_arm);

        let controller_arm_entry = self_endpoint
            .cursor
            .controller_arm_entry_by_arm(scope_id, selected_arm);
        let max_arms = self_endpoint
            .cursor
            .route_scope_arm_count(scope_id)
            .filter(|&count| count > 0);

        // Priority: controller arm entry, then recv registry, then passive observer.
        let meta = if let Some((arm_entry_idx, arm_entry_label)) = controller_arm_entry {
            let target_cursor = self_endpoint.cursor.with_index(arm_entry_idx as usize);
            self_endpoint.cursor = target_cursor;

            if let Some(local_meta) = target_cursor.try_local_meta() {
                Some(RecvMeta {
                    eff_index: local_meta.eff_index,
                    label: local_meta.label,
                    peer: ROLE,
                    resource: local_meta.resource,
                    is_control: local_meta.is_control,
                    next: local_meta.next,
                    scope: local_meta.scope,
                    route_arm: Some(selected_arm),
                    is_choice_determinant: false,
                    shot: local_meta.shot,
                    plan: local_meta.plan,
                    lane: local_meta.lane,
                })
            } else {
                Some(RecvMeta {
                    eff_index: 0,
                    label: arm_entry_label,
                    peer: ROLE,
                    resource: None,
                    is_control: true,
                    next: target_cursor.index(),
                    scope: scope_id,
                    route_arm: Some(selected_arm),
                    is_choice_determinant: false,
                    shot: None,
                    plan: crate::global::const_dsl::HandlePlan::none(),
                    lane: offer_lane,
                })
            }
        } else if let Some(max) = max_arms {
            if selected_arm >= max {
                None
            } else {
                self_endpoint.try_select_route_arm_meta(scope_id, selected_arm)
            }
        } else {
            None
        };

        let mut meta = if let Some(m) = meta {
            m
        } else {
            let region = self_endpoint
                .cursor
                .scope_region_by_id(scope_id)
                .ok_or(RecvError::PhaseInvariant)?;

            if let Some(hint_label) = resolved_label_hint {
                if let Some((dispatch_arm, target_idx)) = self_endpoint.cursor.first_recv_target(scope_id, hint_label) {
                    let target_cursor = self_endpoint.cursor.with_index(target_idx as usize);
                    if dispatch_arm < 2 && dispatch_arm != selected_arm {
                        return Err(RecvError::PhaseInvariant);
                    }
                    if let Some(recv_meta) = target_cursor.try_recv_meta() {
                        self_endpoint.cursor = target_cursor;
                        recv_meta
                    } else {
                        return Err(RecvError::PhaseInvariant);
                    }
                } else {
                    let nav_result = self_endpoint
                        .cursor
                        .follow_passive_observer_arm_for_scope(scope_id, selected_arm)
                        .ok_or(RecvError::PhaseInvariant)?;

                    let PassiveArmNavigation::WithinArm { entry } = nav_result;
                    let target_cursor = self_endpoint.cursor.with_index(entry as usize);
                    self_endpoint.cursor = target_cursor;

                    if let Some(recv_meta) = target_cursor.try_recv_meta() {
                        recv_meta
                    } else if let Some(send_meta) = target_cursor.try_send_meta() {
                        RecvMeta {
                            eff_index: send_meta.eff_index,
                            label: send_meta.label,
                            peer: send_meta.peer,
                            resource: send_meta.resource,
                            is_control: send_meta.is_control,
                            next: target_cursor.index(),
                            scope: scope_id,
                            route_arm: Some(selected_arm),
                            is_choice_determinant: false,
                            shot: send_meta.shot,
                            plan: send_meta.plan,
                            lane: send_meta.lane,
                        }
                    } else if target_cursor.is_jump() {
                        let scope_end = target_cursor.jump_target().unwrap_or(0);
                        let scope_end_cursor = self_endpoint.cursor.with_index(scope_end);
                        self_endpoint.cursor = scope_end_cursor;

                        if region.linger {
                            let synthetic_label = match selected_arm {
                                0 => LABEL_LOOP_CONTINUE,
                                1 => LABEL_LOOP_BREAK,
                                _ => return Err(RecvError::PhaseInvariant),
                            };
                            RecvMeta {
                                eff_index: 0,
                                label: synthetic_label,
                                peer: ROLE,
                                resource: None,
                                is_control: true,
                                next: scope_end,
                                scope: scope_id,
                                route_arm: Some(selected_arm),
                                is_choice_determinant: false,
                                shot: None,
                                plan: crate::global::const_dsl::HandlePlan::none(),
                                lane: offer_lane,
                            }
                        } else if let Some(recv_meta) = scope_end_cursor.try_recv_meta() {
                            recv_meta
                        } else if let Some(send_meta) = scope_end_cursor.try_send_meta() {
                            RecvMeta {
                                eff_index: send_meta.eff_index,
                                label: send_meta.label,
                                peer: send_meta.peer,
                                resource: send_meta.resource,
                                is_control: send_meta.is_control,
                                next: scope_end,
                                scope: scope_id,
                                route_arm: Some(selected_arm),
                                is_choice_determinant: false,
                                shot: send_meta.shot,
                                plan: send_meta.plan,
                                lane: send_meta.lane,
                            }
                        } else {
                            return Err(RecvError::PhaseInvariant);
                        }
                    } else if region.linger {
                        let synthetic_label = match selected_arm {
                            0 => LABEL_LOOP_CONTINUE,
                            1 => LABEL_LOOP_BREAK,
                            _ => return Err(RecvError::PhaseInvariant),
                        };
                        RecvMeta {
                            eff_index: 0,
                            label: synthetic_label,
                            peer: ROLE,
                            resource: None,
                            is_control: true,
                            next: self_endpoint.cursor.index(),
                            scope: scope_id,
                            route_arm: Some(selected_arm),
                            is_choice_determinant: false,
                            shot: None,
                            plan: crate::global::const_dsl::HandlePlan::none(),
                            lane: offer_lane,
                        }
                    } else {
                        return Err(RecvError::PhaseInvariant);
                    }
                }
            } else {
                let nav_result = self_endpoint
                    .cursor
                    .follow_passive_observer_arm_for_scope(scope_id, selected_arm)
                    .ok_or(RecvError::PhaseInvariant)?;

                let PassiveArmNavigation::WithinArm { entry } = nav_result;
                let target_cursor = self_endpoint.cursor.with_index(entry as usize);
                self_endpoint.cursor = target_cursor;

                if let Some(recv_meta) = target_cursor.try_recv_meta() {
                    recv_meta
                } else if let Some(send_meta) = target_cursor.try_send_meta() {
                    RecvMeta {
                        eff_index: send_meta.eff_index,
                        label: send_meta.label,
                        peer: send_meta.peer,
                        resource: send_meta.resource,
                        is_control: send_meta.is_control,
                        next: target_cursor.index(),
                        scope: scope_id,
                        route_arm: Some(selected_arm),
                        is_choice_determinant: false,
                        shot: send_meta.shot,
                        plan: send_meta.plan,
                        lane: send_meta.lane,
                    }
                } else if region.linger {
                    let synthetic_label = match selected_arm {
                        0 => LABEL_LOOP_CONTINUE,
                        1 => LABEL_LOOP_BREAK,
                        _ => return Err(RecvError::PhaseInvariant),
                    };
                    RecvMeta {
                        eff_index: 0,
                        label: synthetic_label,
                        peer: ROLE,
                        resource: None,
                        is_control: true,
                        next: self_endpoint.cursor.index(),
                        scope: scope_id,
                        route_arm: Some(selected_arm),
                        is_choice_determinant: false,
                        shot: None,
                        plan: crate::global::const_dsl::HandlePlan::none(),
                        lane: offer_lane,
                    }
                } else {
                    return Err(RecvError::PhaseInvariant);
                }
            }
        };

        if meta.scope != scope_id {
            // Nested route: ensure route_arm matches the recv node's scope.
            meta.route_arm = self_endpoint.cursor.find_arm_for_recv_label(meta.label);
        }

        debug.branch_label(scope_id, meta.label, meta.route_arm, meta.lane);

        self_endpoint.skip_unselected_arm_lanes(scope_id, selected_arm, meta.lane);

        let policy_action = self_endpoint.eval_endpoint_policy(
            VmSlot::EndpointRx,
            ids::ENDPOINT_RECV,
            self_endpoint.sid.raw(),
            Self::endpoint_policy_args(Lane::new(meta.lane as u32), meta.label, FrameFlags::empty()),
            Lane::new(meta.lane as u32),
        );
        self_endpoint.apply_recv_policy(policy_action, meta.scope, Lane::new(meta.lane as u32))?;

        let lane_wire = meta.lane;
        self_endpoint.set_route_arm(lane_wire, scope_id, selected_arm)?;

        // Late binding channel resolution: if payload_view is empty and binding_channel
        // is still None (because RouteTable/recv_label_hint provided the arm before
        // poll_incoming_for_lane was called), try to get the channel now. This allows decode()
        // to read data via on_recv() even when the route arm was resolved via other means.
        let (binding_channel, binding_instance) = if payload_view.as_bytes().is_empty() {
            let mut channel = binding_channel;
            let mut instance = binding_instance;
            if let Some(classification) = binding_classification {
                if classification.label != meta.label {
                    return Err(RecvError::LabelMismatch {
                        expected: meta.label,
                        actual: classification.label,
                    });
                }
                if channel.is_none() {
                    channel = Some(classification.channel);
                }
                if instance.is_none() {
                    instance = Some(classification.instance);
                }
            } else if channel.is_none() {
                if let Some(classification) =
                    self_endpoint.take_binding_for_lane(meta.lane as usize)
                {
                    if classification.label != meta.label {
                        return Err(RecvError::LabelMismatch {
                            expected: meta.label,
                            actual: classification.label,
                        });
                    }
                    channel = Some(classification.channel);
                    if instance.is_none() {
                        instance = Some(classification.instance);
                    }
                }
            }
            (channel, instance)
        } else {
            (binding_channel, binding_instance)
        };

        // Determine BranchKind based on cursor state after arm selection.
        // This eliminates the need for label→arm inference in decode().
        //
        // BranchKind determination order:
        // 1. WireRecv: cursor is at a Recv node → normal wire recv
        // 2. ArmSendHint: cursor is at a Send node → driver should use flow().send()
        // 3. LocalControl: cursor is at a Local node → synthetic recv from zero buffer
        // 4. EmptyArmTerminal: cursor is at Jump/None/Terminate → empty arm or scope end
        // 5. LocalControl (fallback): any other case → synthetic control
        let mut branch_kind = if self_endpoint.cursor.is_recv() {
            BranchKind::WireRecv
        } else if self_endpoint.cursor.is_send() {
            BranchKind::ArmSendHint
        } else if self_endpoint.cursor.is_local_action() {
            BranchKind::LocalControl
        } else if self_endpoint.cursor.is_jump() {
            // Jump node (e.g., LoopBreak Jump to terminal, or PassiveObserverBranch entry)
            // Treat as LocalControl for synthetic decode
            BranchKind::LocalControl
        } else {
            // None or Terminate action → empty arm leading to terminal
            BranchKind::EmptyArmTerminal
        };
        if resolved_from_hint && matches!(meta.label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK) {
            branch_kind = BranchKind::EmptyArmTerminal;
        }

        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            eff_index: meta.eff_index,
            kind: branch_kind,
        };

        Ok(RouteBranch {
            label: meta.label,
            payload: payload_view,
            endpoint: self_endpoint,
            binding_channel,
            binding_instance,
            branch_meta,
        })
    }

    pub(crate) fn canonical_control_token<K>(&self, meta: &SendMeta) -> SendResult<CapFlowToken<K>>
    where
        K: ResourceKind + ControlMint,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::AllowsCanonical,
    {
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let shot = meta.shot.ok_or(SendError::PhaseInvariant)?;
        let cp_sid = CpSessionId::new(self.sid.raw());
        let port = self.port_for_lane(meta.lane as usize);
        let lane = port.lane();
        let cp_lane = CpLaneId::new(lane.raw());
        let src_rv = CpRendezvousId::new(self.rendezvous_id().raw());
        port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let transport_ctx = self.transport_context_snapshot();
        let bytes = match tag {
            LoopContinueKind::TAG => {
                if K::TAG != LoopContinueKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                // Record loop decision before minting token
                let mut loop_scope = meta.scope;
                if let Some(metadata) = self.cursor.loop_metadata_inner()
                    && metadata.role == LoopRole::Controller
                    && metadata.controller == ROLE
                {
                    self.record_loop_decision(&metadata, LoopDecision::Continue, meta.lane)?;
                    loop_scope = metadata.scope;
                }
                if loop_scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                let scope = loop_scope;
                let handle = LoopDecisionHandle::new(self.sid.raw(), lane.raw() as u16, scope);
                self.mint_control_token_with_handle::<LoopContinueKind>(
                    meta.peer,
                    shot,
                    lane,
                    handle,
                )?
                    .into_bytes()
            }
            LoopBreakKind::TAG => {
                if K::TAG != LoopBreakKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                // Record loop decision before minting token
                let mut loop_scope = meta.scope;
                if let Some(metadata) = self.cursor.loop_metadata_inner()
                    && metadata.role == LoopRole::Controller
                    && metadata.controller == ROLE
                {
                    self.record_loop_decision(&metadata, LoopDecision::Break, meta.lane)?;
                    loop_scope = metadata.scope;
                }
                if loop_scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                let scope = loop_scope;
                let handle = LoopDecisionHandle::new(self.sid.raw(), lane.raw() as u16, scope);
                self.mint_control_token_with_handle::<LoopBreakKind>(
                    meta.peer,
                    shot,
                    lane,
                    handle,
                )?
                    .into_bytes()
            }
            RerouteKind::TAG => {
                if K::TAG != RerouteKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let plan = cluster
                    .control_plan_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let handle = cluster
                    .prepare_reroute_handle_from_plan(
                        src_rv,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        plan,
                        transport_metrics,
                        transport_ctx,
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
                let plan = cluster
                    .control_plan_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let scope = meta.scope;
                if scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                let handle = cluster
                    .prepare_route_decision_from_plan(
                        src_rv,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        plan,
                        transport_metrics,
                        transport_ctx,
                    )
                    .map_err(|_| SendError::PhaseInvariant)?;
                port.record_route_decision(scope, handle.arm);
                self.emit_route_decision(scope, handle.arm, 2, meta.lane);
                self.mint_control_token_with_handle::<RouteDecisionKind>(
                    meta.peer,
                    shot,
                    lane,
                    handle,
                )?
                    .into_bytes()
            }
            SpliceIntentKind::TAG => {
                if K::TAG != SpliceIntentKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let plan = cluster
                    .control_plan_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let operands = cluster
                    .prepare_splice_operands_from_plan(
                        src_rv,
                        cp_sid,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        plan,
                        transport_metrics,
                        transport_ctx,
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
            // Generic fallback for external control kinds (e.g., hibana-quic's AcceptHookKind).
            // Uses ControlMint trait for extensibility without modifying hibana core.
            _ => {
                let handle = K::mint_handle(self.sid, lane, meta.scope);
                self.mint_control_token_with_handle::<K>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
        };
        Ok(CapFlowToken::new(
            *meta,
            GenericCapToken::<K>::from_bytes(bytes),
        ))
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
                                if let Some(parent_arm) = self.route_arm_for(lane_wire, parent) {
                                    if parent_arm == 0 {
                                        self.cursor = self.cursor.with_index(parent_region.start);
                                        break;
                                    }
                                }
                            }
                            let should_advance = self.cursor.index() >= parent_region.end;

                            if should_advance {
                                if let Some(cursor) = self.cursor.advance_scope_by_id(parent) {
                                    self.cursor = cursor;
                                    }
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
                        self.cursor = self.cursor.with_index(reg.start);
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
            if let Some(arm) = route_arm {
                if !(linger && exited_scope) {
                    let _ = self.set_route_arm(lane_wire, scope, arm);
                }
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
    }

    /// Session id for this endpoint.
    #[inline]
    pub fn session_id(&self) -> SessionId {
        self.sid
    }

    /// Mint configuration marker for control tokens.
    #[inline]
    pub const fn mint_config(&self) -> Mint {
        self.mint
    }

    /// Lane bound to the primary port.
    #[inline]
    pub fn lane(&self) -> Lane {
        self.port().lane()
    }

    /// Rendezvous id for the primary port.
    #[inline]
    pub fn rendezvous_id(&self) -> RendezvousId {
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

    /// Get the primary lane's port mutably.
    ///
    /// # Safety invariant
    /// The primary port is always retained by construction.
    fn port_mut(&mut self) -> &mut Port<'r, T, E> {
        debug_assert!(
            self.ports[self.primary_lane].is_some(),
            "port_mut: primary lane {} has no port (invariant violation)",
            self.primary_lane
        );
        self.ports[self.primary_lane]
            .as_mut()
            .expect("cursor endpoint retains primary port")
    }

    /// Get port for a specific lane.
    ///
    /// # Panics
    /// Panics if the port for `lane_idx` was not acquired.
    fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[lane_idx].is_some(),
            "port_for_lane: lane {} has no port",
            lane_idx
        );
        self.ports[lane_idx]
            .as_ref()
            .expect("port not acquired for lane")
    }

    /// Get port for a specific lane mutably.
    ///
    /// # Panics
    /// Panics if the port for `lane_idx` was not acquired.
    fn port_for_lane_mut(&mut self, lane_idx: usize) -> &mut Port<'r, T, E> {
        debug_assert!(
            self.ports[lane_idx].is_some(),
            "port not acquired for lane {}",
            lane_idx
        );
        self.ports[lane_idx]
            .as_mut()
            .expect("port not acquired for lane")
    }

    fn loop_index(scope: ScopeId) -> Option<u8> {
        u8::try_from(scope.ordinal()).ok()
    }

    #[inline]
    fn offer_lanes_for_scope(&self, scope_id: ScopeId) -> ([u8; MAX_LANES], usize) {
        let (mut lanes, mut len) = self
            .cursor
            .route_scope_offer_lane_list(scope_id)
            .unwrap_or(([0; MAX_LANES], 0));
        if len == 0 {
            lanes[0] = self.primary_lane as u8;
            len = 1;
        }
        (lanes, len)
    }

    #[inline]
    fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let (lanes, _) = self.offer_lanes_for_scope(scope_id);
        lanes[0]
    }

    #[inline]
    fn can_advance_route_scope(&self, scope_id: ScopeId, target_label: u8) -> bool {
        let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
        self.route_arm_for(lane_wire, scope_id).is_some()
    }

    #[inline]
    fn lane_for_label_or_offer(&self, scope_id: ScopeId, target_label: u8) -> u8 {
        if let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label) {
            lane_idx as u8
        } else {
            self.offer_lane_for_scope(scope_id)
        }
    }

    /// Create a ContextSnapshot from the binding's transport context.
    ///
    /// If the binding provides a transport context provider, queries it for all
    /// keys returned by `supported_keys()` and creates a snapshot.
    /// Otherwise returns an empty snapshot.
    fn transport_context_snapshot(&self) -> crate::transport::context::ContextSnapshot {
        match self.binding.transport_context() {
            Some(provider) => {
                crate::transport::context::ContextSnapshot::from_provider(
                    provider,
                    provider.supported_keys(),
                )
            }
            None => crate::transport::context::ContextSnapshot::new(),
        }
    }

    /// Current phase index in the multi-lane execution model.
    #[inline]
    pub fn phase_index(&self) -> usize {
        self.cursor.phase_index()
    }

    #[cfg(feature = "test-utils")]
    pub fn assert_terminal(&self) {
        self.cursor.assert_terminal();
    }

    fn try_select_lane_for_label(&mut self, target_label: u8) -> bool {
        if let Some(meta) = self.cursor.try_recv_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(meta) = self.cursor.try_send_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(meta) = self.cursor.try_local_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label) else {
            return false;
        };
        let Some(idx) = self.cursor.index_for_lane_step(lane_idx) else {
            return false;
        };
        if idx != self.cursor.index() {
            self.cursor = self.cursor.with_index(idx);
        }
        true
    }

    fn hint_matches_scope(
        cursor: &PhaseCursor<ROLE>,
        scope_id: ScopeId,
        label: u8,
        is_route_controller: bool,
        loop_scope_active: bool,
        loop_continue_has_recv: bool,
        loop_break_has_recv: bool,
    ) -> bool {
        if is_route_controller {
            return false;
        }
        let _ = loop_scope_active;
        let is_loop_control_scope = Self::is_loop_control_scope(cursor, scope_id);
        match label {
            LABEL_LOOP_CONTINUE => {
                !loop_continue_has_recv
                    && (is_loop_control_scope || cursor.first_recv_target(scope_id, label).is_some())
            }
            LABEL_LOOP_BREAK => {
                !loop_break_has_recv
                    && (is_loop_control_scope || cursor.first_recv_target(scope_id, label).is_some())
            }
            _ => cursor.first_recv_target(scope_id, label).is_some(),
        }
    }

    fn peek_hint_for_lane(
        &mut self,
        cursor: &PhaseCursor<ROLE>,
        lane_idx: usize,
        scope_id: ScopeId,
        is_route_controller: bool,
        loop_scope_active: bool,
        loop_continue_has_recv: bool,
        loop_break_has_recv: bool,
    ) -> Option<u8> {
        if is_route_controller {
            return None;
        }
        // Route hints are lane-scoped; use the lane port to avoid cross-lane leakage.
        let port = self.port_for_lane(lane_idx);
        port.ingest_route_hints();
        port.peek_route_hint(|label| {
            Self::hint_matches_scope(
                cursor,
                scope_id,
                label,
                is_route_controller,
                loop_scope_active,
                loop_continue_has_recv,
                loop_break_has_recv,
            )
        })
    }

    fn take_hint_for_lane(
        &mut self,
        cursor: &PhaseCursor<ROLE>,
        lane_idx: usize,
        scope_id: ScopeId,
        is_route_controller: bool,
        loop_scope_active: bool,
        loop_continue_has_recv: bool,
        loop_break_has_recv: bool,
    ) -> Option<u8> {
        if is_route_controller {
            return None;
        }
        let port = self.port_for_lane(lane_idx);
        port.ingest_route_hints();
        port.take_route_hint(|label| {
            Self::hint_matches_scope(
                cursor,
                scope_id,
                label,
                is_route_controller,
                loop_scope_active,
                loop_continue_has_recv,
                loop_break_has_recv,
            )
        })
    }

    fn peek_hint_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
        is_route_controller: bool,
        loop_scope_active: bool,
        loop_continue_has_recv: bool,
        loop_break_has_recv: bool,
    ) -> Option<u8> {
        let cursor = self.cursor;
        let mut lane_idx = 0usize;
        while lane_idx < offer_lanes_len {
            let lane = offer_lanes[lane_idx] as usize;
            if let Some(label) = self.peek_hint_for_lane(
                &cursor,
                lane,
                scope_id,
                is_route_controller,
                loop_scope_active,
                loop_continue_has_recv,
                loop_break_has_recv,
            ) {
                return Some(label);
            }
            lane_idx += 1;
        }
        None
    }

    fn take_hint_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
        is_route_controller: bool,
        loop_scope_active: bool,
        loop_continue_has_recv: bool,
        loop_break_has_recv: bool,
    ) -> Option<u8> {
        let cursor = self.cursor;
        let mut resolved = None;
        let mut lane_idx = 0usize;
        while lane_idx < offer_lanes_len {
            let lane = offer_lanes[lane_idx] as usize;
            let hint = self.take_hint_for_lane(
                &cursor,
                lane,
                scope_id,
                is_route_controller,
                loop_scope_active,
                loop_continue_has_recv,
                loop_break_has_recv,
            );
            if resolved.is_none() {
                resolved = hint;
            }
            lane_idx += 1;
        }
        resolved
    }

    fn ack_route_decision_for_offer(
        &self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
    ) -> Option<u8> {
        let mut lane_idx = 0usize;
        while lane_idx < offer_lanes_len {
            let lane = offer_lanes[lane_idx];
            if let Some(arm) = self
                .port_for_lane(lane as usize)
                .ack_route_decision(scope_id, ROLE)
            {
                return Some(arm);
            }
            lane_idx += 1;
        }
        None
    }

    fn arm_has_recv(&self, scope_id: ScopeId, arm: u8) -> bool {
        if self.cursor.route_scope_arm_recv_index(scope_id, arm).is_some() {
            return true;
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) =
            self.cursor.follow_passive_observer_arm_for_scope(scope_id, arm)
        {
            let target_cursor = self.cursor.with_index(entry as usize);
            return target_cursor.is_recv();
        }
        false
    }

    fn arm_from_hint_label(&self, scope_id: ScopeId, label: u8) -> Option<u8> {
        match label {
            LABEL_LOOP_CONTINUE => Some(0),
            LABEL_LOOP_BREAK => Some(1),
            _ => self.route_arm_from_label(scope_id, label),
        }
    }

    fn peek_binding_for_lane(
        &mut self,
        lane_idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        if let Some(classification) = self.pending_binding[lane_idx] {
            return Some(classification);
        }
        let lane = lane_idx as u8;
        if let Some(classification) = self.binding.poll_incoming_for_lane(lane) {
            self.pending_binding[lane_idx] = Some(classification);
            return Some(classification);
        }
        None
    }

    fn take_binding_for_lane(
        &mut self,
        lane_idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        if let Some(classification) = self.pending_binding[lane_idx].take() {
            return Some(classification);
        }
        let lane = lane_idx as u8;
        self.binding.poll_incoming_for_lane(lane)
    }

    fn poll_binding_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
        loop_scope_active: bool,
        loop_continue_has_recv: bool,
        loop_break_has_recv: bool,
    ) -> Option<crate::binding::IncomingClassification> {
        let cursor = self.cursor;
        let mut lane_idx = 0usize;
        while lane_idx < offer_lanes_len {
            let lane = offer_lanes[lane_idx];
            let lane_slot = lane as usize;
            if let Some(classification) = self.peek_binding_for_lane(lane_slot) {
                let matches_scope = Self::hint_matches_scope(
                    &cursor,
                    scope_id,
                    classification.label,
                    false,
                    loop_scope_active,
                    loop_continue_has_recv,
                    loop_break_has_recv,
                );
                if matches_scope {
                    let _ = self.pending_binding[lane_slot].take();
                    return Some(classification);
                }
            }
            lane_idx += 1;
        }
        None
    }

    fn try_recv_from_binding(
        &mut self,
        logical_lane: u8,
        expected_label: u8,
        buf: &mut [u8],
    ) -> RecvResult<Option<usize>> {
        let Some(classification) = self.take_binding_for_lane(logical_lane as usize) else {
            return Ok(None);
        };
        if classification.label != expected_label {
            return Err(RecvError::LabelMismatch {
                expected: expected_label,
                actual: classification.label,
            });
        }
        let n = self
            .binding
            .on_recv(classification.channel, buf)
            .map_err(RecvError::Binding)?;
        Ok(Some(n))
    }

    fn is_loop_control_scope(cursor: &PhaseCursor<ROLE>, scope_id: ScopeId) -> bool {
        if let Some(region) = cursor.scope_region_by_id(scope_id) {
            if region.kind == ScopeKind::Route && region.linger {
                return true;
            }
        }
        let Some((_, label0)) = cursor.controller_arm_entry_by_arm(scope_id, 0) else {
            return false;
        };
        let Some((_, label1)) = cursor.controller_arm_entry_by_arm(scope_id, 1) else {
            return false;
        };
        (label0 == LABEL_LOOP_CONTINUE && label1 == LABEL_LOOP_BREAK)
            || (label0 == LABEL_LOOP_BREAK && label1 == LABEL_LOOP_CONTINUE)
    }

    fn parallel_scope_root(cursor: &PhaseCursor<ROLE>, mut scope_id: ScopeId) -> Option<ScopeId> {
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

    // Select a route entry when multiple parallel route decisions are pending.
    fn select_offer_entry(&mut self) -> RecvResult<()> {
        let debug = OfferDebugger::new();
        let node_scope = self.cursor.node_scope_id();
        let current_idx = self.cursor.index();
        let mut current_parallel = Self::parallel_scope_root(&self.cursor, node_scope);
        if let Some(root) = current_parallel {
            if !self.pending_offers_in_parallel_root(root) {
                current_parallel = None;
            }
        }
        let mut current_is_route_entry = false;
        let mut current_is_controller = false;
        let mut current_is_dynamic = false;
        if let Some(region) = self.cursor.scope_region_by_id(node_scope) {
            if region.kind == ScopeKind::Route {
                if let Some(entry) = self.cursor.route_scope_offer_entry(region.scope_id) {
                    let entry_idx =
                        if entry == u16::MAX { current_idx } else { entry as usize };
                    if entry == u16::MAX || entry_idx == current_idx {
                        current_is_route_entry = true;
                        current_is_controller = self.cursor.is_route_controller(region.scope_id);
                        if current_is_controller {
                            if let Some((plan, _, _)) =
                                self.cursor.route_scope_controller_plan(region.scope_id)
                            {
                                if plan.is_dynamic()
                                    && !Self::is_loop_control_scope(&self.cursor, region.scope_id)
                                {
                                    current_is_dynamic = true;
                                }
                            }
                        }
                        debug.select_current_scope(
                            region.scope_id,
                            current_idx,
                            Some(entry),
                            region.start as u16,
                            current_is_controller,
                            current_is_dynamic,
                        );
                    }
                } else {
                    return Ok(());
                }
            }
        }
        if self.cursor.scope_region().is_none() {
            if self.try_sync_pending_route_from_lanes() {
                return Ok(());
            }
        }

        let mut candidate_idx: Option<usize> = None;
        let mut candidate_count = 0usize;
        let mut controller_idx: Option<usize> = None;
        let mut controller_count = 0usize;
        let mut dynamic_controller_idx: Option<usize> = None;
        let mut dynamic_controller_count = 0usize;
        let mut hinted_idx: Option<usize> = None;
        let mut hinted_count = 0usize;
        let mut passive_idx: Option<usize> = None;
        let mut passive_count = 0usize;
        let mut current_has_hint = false;
        let mut current_matches_candidate = false;
        let mut loop_control_controller_count = 0usize;
        let mut non_loop_control_controller_count = 0usize;
        let mut lane_mask = self.pending_offer_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            let info = self.pending_offer_info[lane_idx];
            if info.scope.is_none() {
                continue;
            }
            if let Some(root) = current_parallel {
                if info.parallel_root != root {
                    continue;
                }
            }
            let entry_idx = info.entry as usize;
            if entry_idx == current_idx {
                current_matches_candidate = true;
            }
            let lane_cursor = self.cursor.with_index(entry_idx);
            let is_controller = info.is_controller();
            let loop_scope_active = lane_cursor
                .typestate_node(lane_cursor.index())
                .loop_scope()
                .is_some();
            let loop_continue_has_recv = lane_cursor
                .route_scope_arm_recv_index(info.scope, 0)
                .is_some();
            let loop_break_has_recv = lane_cursor
                .route_scope_arm_recv_index(info.scope, 1)
                .is_some();
            let binding_hint = self.peek_binding_for_lane(lane_idx);
            let has_binding_hint = binding_hint
                .map(|classification| {
                    Self::hint_matches_scope(
                        &lane_cursor,
                        info.scope,
                        classification.label,
                        is_controller,
                        loop_scope_active,
                        loop_continue_has_recv,
                        loop_break_has_recv,
                    )
                })
                .unwrap_or(false);
            let has_hint = has_binding_hint
                || self
                    .peek_hint_for_lane(
                        &lane_cursor,
                        lane_idx,
                        info.scope,
                        is_controller,
                        loop_scope_active,
                        loop_continue_has_recv,
                        loop_break_has_recv,
                    )
                    .is_some();
            let is_loop_control =
                is_controller && Self::is_loop_control_scope(&lane_cursor, info.scope);
            if !is_controller {
                passive_count += 1;
                if passive_idx.is_none() {
                    passive_idx = Some(entry_idx);
                }
            }
            debug.lane_scan(lane_idx, entry_idx, info.scope, is_controller, has_hint);
            candidate_count += 1;
            if candidate_idx.is_none() {
                candidate_idx = Some(entry_idx);
            }
            if is_controller {
                controller_count += 1;
                if controller_idx.is_none() {
                    controller_idx = Some(entry_idx);
                }
                if is_loop_control {
                    loop_control_controller_count += 1;
                } else {
                    non_loop_control_controller_count += 1;
                }
                if info.is_dynamic() {
                    dynamic_controller_count += 1;
                    if dynamic_controller_idx.is_none() {
                        dynamic_controller_idx = Some(entry_idx);
                    }
                }
            }
            if has_hint {
                hinted_count += 1;
                if hinted_idx.is_none() {
                    hinted_idx = Some(entry_idx);
                }
                if entry_idx == current_idx {
                    current_has_hint = true;
                }
            }
        }
        debug.current_match(current_idx, current_matches_candidate);
        if hinted_count >= 1 {
            if current_has_hint {
                debug.select_choice("current_hint", current_idx);
                return Ok(());
            }
            if let Some(entry_idx) = hinted_idx {
                debug.select_choice("hint", entry_idx);
                self.cursor = self.cursor.with_index(entry_idx);
                return Ok(());
            }
        }
        let current_is_candidate = current_matches_candidate;
        let current_is_active_controller =
            current_is_candidate && current_is_route_entry && current_is_controller;
        let current_is_active_dynamic = current_is_active_controller && current_is_dynamic;
        if dynamic_controller_count == 1 && !current_is_active_dynamic {
            if let Some(entry_idx) = dynamic_controller_idx {
                debug.select_choice("dynamic_controller", entry_idx);
                self.cursor = self.cursor.with_index(entry_idx);
                return Ok(());
            }
        }
        // Prefer passive offers when only loop-control controllers are pending.
        if hinted_count == 0
            && passive_count > 0
            && loop_control_controller_count > 0
            && non_loop_control_controller_count == 0
        {
            if let Some(entry_idx) = passive_idx {
                debug.select_choice("passive_loop_control", entry_idx);
                self.cursor = self.cursor.with_index(entry_idx);
                return Ok(());
            }
        }
        if current_is_candidate {
            debug.select_choice("current", current_idx);
            return Ok(());
        }
        if controller_count == 1 && !current_is_active_controller {
            if let Some(entry_idx) = controller_idx {
                debug.select_choice("controller", entry_idx);
                self.cursor = self.cursor.with_index(entry_idx);
                return Ok(());
            }
        }
        if candidate_count == 1 {
            if let Some(entry_idx) = candidate_idx {
                debug.select_choice("candidate", entry_idx);
                self.cursor = self.cursor.with_index(entry_idx);
                return Ok(());
            }
        }
        debug.select_ambiguous(
            current_idx,
            candidate_count,
            controller_count,
            hinted_count,
            current_is_route_entry,
            current_is_controller,
        );
        #[cfg(feature = "std")]
        if debug.enabled {
            let action = if self.cursor.is_recv() {
                "recv"
            } else if self.cursor.is_send() {
                "send"
            } else if self.cursor.is_local_action() {
                "local"
            } else if self.cursor.is_jump() {
                "jump"
            } else {
                "other"
            };
            eprintln!(
                "[hibana-offer] ambiguous current action: kind={} label={:?} scope={:?}",
                action,
                self.cursor.label(),
                self.cursor.scope_region().map(|r| r.scope_id)
            );
        }
        Err(RecvError::PhaseInvariant)
    }

    fn try_sync_pending_route_from_lanes(&mut self) -> bool {
        if self.try_sync_pending_route_from_lane(self.primary_lane) {
            return true;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if lane_idx != self.primary_lane && self.try_sync_pending_route_from_lane(lane_idx) {
                return true;
            }
            lane_idx += 1;
        }
        false
    }

    fn pending_offers_in_parallel_root(&self, root: ScopeId) -> bool {
        let mut lane_mask = self.pending_offer_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            let info = self.pending_offer_info[lane_idx];
            if !info.scope.is_none() && info.parallel_root == root {
                return true;
            }
        }
        false
    }

    fn try_sync_pending_route_from_lane(&mut self, lane_idx: usize) -> bool {
        let Some(idx) = self.cursor.index_for_lane_step(lane_idx) else {
            return false;
        };
        let lane_wire = lane_idx as u8;
        let lane_cursor = self.cursor.with_index(idx);
        let scope = lane_cursor.node_scope_id();
        if scope.is_none() {
            return false;
        }
        let mut candidate_idx: Option<usize> = None;
        let mut current = Some(scope);
        while let Some(scope_id) = current {
            if scope_id.kind() == ScopeKind::Route && self.route_arm_for(lane_wire, scope_id).is_none()
            {
                if let Some(entry) = lane_cursor.route_scope_offer_entry(scope_id) {
                    let entry_idx = if entry == u16::MAX { idx } else { entry as usize };
                    candidate_idx = Some(entry_idx);
                }
            }
            current = lane_cursor.scope_parent(scope_id);
        }
        let Some(entry_idx) = candidate_idx else {
            return false;
        };
        if entry_idx != self.cursor.index() {
            self.cursor = self.cursor.with_index(entry_idx);
        }
        true
    }

    /// Per-lane step progress within the current phase.
    #[inline]
    pub fn lane_cursors(&self) -> [usize; MAX_LANES] {
        *self.cursor.lane_cursors()
    }

    /// Get the step progress for a specific lane.
    #[inline]
    pub fn lane_cursor(&self, lane_idx: usize) -> usize {
        self.cursor.lane_cursor(lane_idx)
    }

    fn skip_unselected_arm_lanes(&mut self, scope: ScopeId, selected_arm: u8, skip_lane: u8) {
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

    fn maybe_skip_remaining_route_arm(
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

    fn rebuild_pending_offers(&mut self) {
        self.pending_offer_mask = 0;
        self.pending_linger_mask = 0;
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            self.refresh_pending_offer_lane(lane_idx);
            lane_idx += 1;
        }
    }

    fn refresh_pending_offer_lane(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let bit = 1u8 << lane_idx;
        if let Some(info) = self.compute_pending_offer_info(lane_idx) {
            self.pending_offer_info[lane_idx] = info;
            self.pending_offer_mask |= bit;
            if self.is_linger_route(info.scope) {
                self.pending_linger_mask |= bit;
            } else {
                self.pending_linger_mask &= !bit;
            }
        } else {
            self.pending_offer_info[lane_idx] = PendingOfferInfo::EMPTY;
            self.pending_offer_mask &= !bit;
            self.pending_linger_mask &= !bit;
        }
    }

    fn compute_pending_offer_info(&self, lane_idx: usize) -> Option<PendingOfferInfo> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let (mut entry_idx, mut scope_id, mut lane_cursor) = if let Some(idx) =
            self.cursor.index_for_lane_step(lane_idx)
        {
            let lane_cursor = self.cursor.with_index(idx);
            let scope_id = lane_cursor.node_scope_id();
            (idx, scope_id, lane_cursor)
        } else {
            let (scope_id, entry) = self.active_linger_offer_for_lane(lane_idx)?;
            let entry_idx = entry as usize;
            let lane_cursor = self.cursor.with_index(entry_idx);
            (entry_idx, scope_id, lane_cursor)
        };
        let mut region = lane_cursor.scope_region_by_id(scope_id)?;
        if region.kind != ScopeKind::Route {
            return None;
        }
        let mut entry = lane_cursor
            .route_scope_offer_entry(region.scope_id)
            .unwrap_or(u16::MAX);
        if entry != u16::MAX && entry as usize != entry_idx {
            if let Some((linger_scope, linger_entry)) = self.active_linger_offer_for_lane(lane_idx)
            {
                scope_id = linger_scope;
                entry_idx = linger_entry as usize;
                lane_cursor = self.cursor.with_index(entry_idx);
                region = lane_cursor.scope_region_by_id(scope_id)?;
                if region.kind != ScopeKind::Route {
                    return None;
                }
                entry = lane_cursor
                    .route_scope_offer_entry(region.scope_id)
                    .unwrap_or(u16::MAX);
                if entry != u16::MAX && entry as usize != entry_idx {
                    return None;
                }
            } else {
                return None;
            }
        }
        let entry_idx = if entry == u16::MAX { entry_idx } else { entry as usize };
        let is_controller = lane_cursor.is_route_controller(region.scope_id);
        let mut flags = 0u8;
        if is_controller {
            flags |= PendingOfferInfo::FLAG_CONTROLLER;
            if let Some((plan, _, _)) = lane_cursor.route_scope_controller_plan(region.scope_id) {
                if plan.is_dynamic() && !Self::is_loop_control_scope(&lane_cursor, region.scope_id)
                {
                    flags |= PendingOfferInfo::FLAG_DYNAMIC;
                }
            }
        }
        let parallel_root =
            Self::parallel_scope_root(&lane_cursor, region.scope_id).unwrap_or(ScopeId::none());
        Some(PendingOfferInfo {
            scope: region.scope_id,
            entry: entry_idx as StateIndex,
            parallel_root,
            flags,
        })
    }

    fn active_linger_offer_for_lane(&self, lane_idx: usize) -> Option<(ScopeId, StateIndex)> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        let mut idx = len;
        while idx > 0 {
            idx -= 1;
            let slot = self.lane_route_arms[lane_idx][idx];
            if slot.scope.is_none() || slot.arm != 0 {
                continue;
            }
            if !self.is_linger_route(slot.scope) {
                continue;
            }
            if let Some(region) = self.cursor.scope_region_by_id(slot.scope) {
                if region.kind == ScopeKind::Route {
                    let start = u16::try_from(region.start).ok()?;
                    return Some((slot.scope, start));
                }
            }
        }
        None
    }

    fn set_lane_cursor_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        self.cursor.set_lane_cursor_to_eff_index(lane_idx, eff_index);
        self.refresh_pending_offer_lane(lane_idx);
    }

    /// Advance the cursor for a specific lane by one step.
    #[inline]
    fn advance_lane_cursor(&mut self, lane_idx: usize, eff_index: EffIndex) {
        self.cursor.advance_lane_to_eff_index(lane_idx, eff_index);
        self.refresh_pending_offer_lane(lane_idx);
    }

    fn advance_phase_skipping_inactive(&mut self) {
        self.cursor.advance_phase_without_sync();
        while self.phase_guard_mismatch() {
            self.cursor.advance_phase_without_sync();
        }
        self.cursor.sync_idx_to_phase_start();
        self.rebuild_pending_offers();
    }

    #[inline]
    fn maybe_advance_phase(&mut self) {
        if self.cursor.is_phase_complete() && !self.has_active_linger_route() {
            self.advance_phase_skipping_inactive();
        }
    }

    fn phase_guard_mismatch(&self) -> bool {
        let Some(phase) = self.cursor.current_phase() else {
            return false;
        };
        let guard = phase.route_guard;
        if guard.is_empty() {
            return false;
        }
        let Some(selected) = self.selected_arm_for_scope(guard.scope) else {
            return false;
        };
        selected != guard.arm
    }

    fn has_active_linger_route(&self) -> bool {
        let phase_mask = self
            .cursor
            .current_phase()
            .map(|phase| phase.lane_mask)
            .unwrap_or(0);
        ((self.lane_linger_mask | self.pending_linger_mask) & phase_mask) != 0
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
        K: ResourceKind + crate::control::cap::SessionScopedKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::AllowsCanonical,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .canonical_session_token::<K, Mint>(
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
        <Mint as MintConfigMarker>::Policy: crate::control::cap::AllowsCanonical,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .canonical_token_with_handle::<K, Mint>(
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
        SpliceHandle::new(
            operands.src_rv.raw(),
            operands.dst_rv.raw(),
            operands.src_lane.raw() as u16,
            operands.dst_lane.raw() as u16,
            operands.old_gen.raw(),
            operands.new_gen.raw(),
            operands.seq_tx,
            operands.seq_rx,
            flags,
        )
    }
}
type ControlResource<M> =
    <<M as crate::g::MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind;

pub trait CanonicalTokenProvider<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, B>
where
    M: crate::g::MessageSpec + crate::g::SendableLabel,
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
    ) -> SendResult<Option<CapFlowToken<ControlResource<M>>>>;
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for crate::g::NoControl
where
    M: crate::g::MessageSpec + crate::g::SendableLabel,
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
    ) -> SendResult<Option<CapFlowToken<ControlResource<M>>>> {
        Ok(None)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, K, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for crate::g::ExternalControl<K>
where
    M: crate::g::MessageSpec + crate::g::SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    Mint::Policy: crate::control::cap::AllowsCanonical,
    ControlResource<M>: ResourceKind + ControlMint,
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
    ) -> SendResult<Option<CapFlowToken<ControlResource<M>>>> {
        if K::AUTO_MINT_EXTERNAL {
            // Auto-mint for external splice kinds
            endpoint
                .canonical_control_token::<ControlResource<M>>(meta)
                .map(Some)
        } else {
            // Caller provides the payload directly
            Ok(None)
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, K, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for crate::g::CanonicalControl<K>
where
    M: crate::g::MessageSpec + crate::g::SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    Mint::Policy: crate::control::cap::AllowsCanonical,
    ControlResource<M>: ResourceKind + ControlMint,
    K: ResourceKind,
    B: BindingSlot,
{
    #[inline(always)]
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<Option<CapFlowToken<ControlResource<M>>>> {
        endpoint
            .canonical_control_token::<ControlResource<M>>(meta)
            .map(Some)
    }
}
