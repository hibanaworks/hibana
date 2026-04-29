//! Offer-path helpers for scope selection and branch materialization.

use core::{ops::ControlFlow, task::Poll};

use super::authority::{
    Arm, DeferReason, DeferSource, RouteDecisionSource, RouteDecisionToken, RouteResolveStep,
};
use super::core::{BranchPreviewView, CursorEndpoint, RouteBranch, StagedPayload};
use super::evidence::ScopeFrameLabelMeta;
#[cfg(test)]
use super::frontier::FrontierCandidate;
use super::frontier::{
    ActiveEntrySet, DeferBudgetOutcome, FrontierDeferOutcome, FrontierKind, FrontierObservationKey,
    FrontierObservationSlot, FrontierVisitSet, LaneOfferState, ObservedEntrySet,
    OfferEntryObservedState, OfferEntryState, OfferLaneEntrySlotMasks, OfferLivenessState,
    OfferSelectPriority, checked_state_index, choose_offer_priority, current_entry_is_candidate,
    current_entry_matches_after_filter, frontier_observation_key_view_from_storage,
    frontier_observed_entries_view_from_storage,
    frontier_offer_lane_entry_slot_masks_view_from_storage, frontier_snapshot_from_scratch,
    frontier_working_observation_key_view_from_storage,
    should_suppress_current_passive_without_evidence,
};
use super::inbox::PackedIngressEvidence;
use super::lane_port;
use super::route_state::RouteArmCommitProof;
use crate::binding::BindingSlot;
use crate::control::cap::mint::{CapShot, EpochTable, MintConfigMarker};
use crate::eff::EffIndex;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::compiled::images::ControlSemanticKind;
use crate::global::const_dsl::{PolicyMode, ScopeId, ScopeKind};
use crate::global::role_program::LaneSetView;
use crate::global::typestate::{
    ARM_SHARED, MAX_FIRST_RECV_DISPATCH, RecvMeta, StateIndex, state_index_to_usize,
};
use crate::policy_runtime::PolicySlot;
use crate::rendezvous::port::Port;
use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::transport::{Transport, wire::Payload};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct LaneIngressEvidence {
    pub(in crate::endpoint::kernel) lane_idx: usize,
    pub(in crate::endpoint::kernel) evidence: crate::binding::IngressEvidence,
}

impl LaneIngressEvidence {
    #[inline]
    pub(in crate::endpoint::kernel) const fn new(
        lane_idx: usize,
        evidence: crate::binding::IngressEvidence,
    ) -> Self {
        Self { lane_idx, evidence }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn frame_label(self) -> u8 {
        self.evidence.frame_label.raw()
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn lane(self) -> u8 {
        self.lane_idx as u8
    }

    #[inline]
    const fn into_parts(self) -> (usize, crate::binding::IngressEvidence) {
        (self.lane_idx, self.evidence)
    }
}

#[derive(Clone, Copy)]
pub(super) struct OfferScopeSelection {
    pub(super) scope_id: ScopeId,
    pub(super) frontier_parallel_root: Option<ScopeId>,
    pub(super) offer_lane: u8,
    pub(super) offer_lane_idx: u8,
    pub(super) at_route_offer_entry: bool,
}

#[derive(Clone, Copy)]
struct OfferFrontierFacts {
    selection: OfferScopeSelection,
    scope_id: ScopeId,
    offer_lane: u8,
    offer_lane_idx: usize,
    offer_lanes: LaneSetView,
    suppress_scope_frame_hint: bool,
    is_route_controller: bool,
    is_dynamic_route_scope: bool,
    recvless_loop_control_scope: bool,
    controller_selected_recv_step: bool,
    skip_recv_loop: bool,
}

#[derive(Clone, Copy)]
enum ResolvePendingAction {
    YieldRestart,
    StaticPassiveProgress { selected_arm: u8 },
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct BranchCommitPlan {
    preview: BranchPreviewView,
    meta: Option<RecvMeta>,
    route_arm_proof: Option<RouteArmCommitProof>,
    clear_other_lanes: bool,
}

impl BranchCommitPlan {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn meta(&self) -> Option<RecvMeta> {
        self.meta
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn route_arm_proof(&self) -> Option<RouteArmCommitProof> {
        self.route_arm_proof
    }
}

#[derive(Clone, Copy)]
struct OfferCollectState<'a> {
    selection: OfferScopeSelection,
    facts: OfferFrontierFacts,
    binding_evidence: Option<LaneIngressEvidence>,
    transport_payload_len: usize,
    transport_payload_lane: u8,
    transport_payload: Option<Payload<'a>>,
}

#[derive(Clone, Copy)]
struct OfferResolveState<'a> {
    selection: OfferScopeSelection,
    facts: OfferFrontierFacts,
    binding_evidence: Option<LaneIngressEvidence>,
    transport_payload_len: usize,
    transport_payload_lane: u8,
    transport_payload: Option<Payload<'a>>,
    liveness: OfferLivenessState,
    pending_action: Option<ResolvePendingAction>,
    yield_armed: bool,
}

#[derive(Clone, Copy)]
enum OfferRunStage<'a> {
    CollectEvidence(OfferCollectState<'a>),
    ResolveToken(OfferResolveState<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CachedRecvMeta {
    pub(super) cursor_index: StateIndex,
    pub(super) eff_index: EffIndex,
    pub(super) peer: u8,
    pub(super) label: u8,
    pub(super) frame_label: u8,
    pub(super) resource: Option<u8>,
    pub(super) semantic: ControlSemanticKind,
    pub(super) is_control: bool,
    pub(super) next: StateIndex,
    pub(super) scope: ScopeId,
    pub(super) route_arm: u8,
    pub(super) is_choice_determinant: bool,
    pub(super) shot: Option<CapShot>,
    pub(super) policy: PolicyMode,
    pub(super) lane: u8,
    pub(super) flags: u8,
}

impl CachedRecvMeta {
    pub(super) const FLAG_RECV_STEP: u8 = 1;

    pub(super) const EMPTY: Self = Self {
        cursor_index: StateIndex::MAX,
        eff_index: EffIndex::ZERO,
        peer: 0,
        label: 0,
        frame_label: 0,
        resource: None,
        semantic: ControlSemanticKind::Other,
        is_control: false,
        next: StateIndex::MAX,
        scope: ScopeId::none(),
        route_arm: u8::MAX,
        is_choice_determinant: false,
        shot: None,
        policy: PolicyMode::static_mode(),
        lane: 0,
        flags: 0,
    };

    #[inline]
    pub(super) const fn is_empty(&self) -> bool {
        self.cursor_index.is_max() || self.next.is_max()
    }

    #[inline]
    pub(super) fn recv_meta(&self) -> Option<(usize, RecvMeta)> {
        if self.is_empty() {
            return None;
        }
        Some((
            state_index_to_usize(self.cursor_index),
            RecvMeta {
                eff_index: self.eff_index,
                peer: self.peer,
                label: self.label,
                frame_label: self.frame_label,
                resource: self.resource,
                semantic: self.semantic,
                is_control: self.is_control,
                next: state_index_to_usize(self.next),
                scope: self.scope,
                route_arm: (self.route_arm != u8::MAX).then_some(self.route_arm),
                is_choice_determinant: self.is_choice_determinant,
                shot: self.shot,
                policy: self.policy,
                lane: self.lane,
            },
        ))
    }

    #[inline]
    pub(super) fn is_recv_step(&self) -> bool {
        (self.flags & Self::FLAG_RECV_STEP) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeArmMaterializationMeta {
    pub(super) scope_id: ScopeId,
    pub(super) arm_count: u8,
    pub(super) controller_arm_entry: [StateIndex; 2],
    pub(super) controller_arm_label: [u8; 2],
    pub(super) controller_cross_role_recv_mask: u8,
    pub(super) recv_entry: [StateIndex; 2],
    pub(super) passive_arm_entry: [StateIndex; 2],
    pub(super) passive_arm_scope: [ScopeId; 2],
    pub(super) first_recv_dispatch: [(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub(super) first_recv_len: u8,
    pub(super) first_recv_frame_label_mask: crate::transport::FrameLabelMask,
    pub(super) first_recv_dispatch_arm_mask: u8,
}

impl ScopeArmMaterializationMeta {
    pub(super) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        arm_count: 0,
        controller_arm_entry: [StateIndex::MAX; 2],
        controller_arm_label: [0; 2],
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::MAX; 2],
        passive_arm_entry: [StateIndex::MAX; 2],
        passive_arm_scope: [ScopeId::none(); 2],
        first_recv_dispatch: [(0, 0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
        first_recv_frame_label_mask: crate::transport::FrameLabelMask::EMPTY,
        first_recv_dispatch_arm_mask: 0,
    };

    #[inline]
    pub(super) fn controller_arm_entry(&self, arm: u8) -> Option<(StateIndex, u8)> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.controller_arm_entry[arm];
        (!entry.is_max()).then_some((entry, self.controller_arm_label[arm]))
    }

    #[inline]
    pub(super) fn recv_entry(&self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.recv_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    pub(super) fn passive_arm_entry(&self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.passive_arm_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    pub(super) fn passive_arm_scope(&self, arm: u8) -> Option<ScopeId> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let scope = self.passive_arm_scope[arm];
        (!scope.is_none()).then_some(scope)
    }

    #[inline]
    pub(super) fn first_recv_target_for_lane_frame_label(
        &self,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (entry_frame_label, entry_lane, arm, target) = self.first_recv_dispatch[idx];
            if entry_frame_label == frame_label && entry_lane == lane && !target.is_max() {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn arm_has_first_recv_dispatch(&self, arm: u8) -> bool {
        arm < 2 && (self.first_recv_dispatch_arm_mask & (1u8 << arm)) != 0
    }

    #[inline]
    pub(super) fn controller_arm_requires_ready_evidence(&self, arm: u8) -> bool {
        arm < 2 && (self.controller_cross_role_recv_mask & (1u8 << arm)) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct ResolvedRouteDecision {
    pub(super) route_token: RouteDecisionToken,
    pub(super) selected_arm: u8,
    pub(super) resolved_hint_frame_label: Option<u8>,
}

pub(super) enum ResolveTokenOutcome {
    RestartFrontier,
    Resolved(ResolvedRouteDecision),
}

#[derive(Clone, Copy)]
pub(super) struct CurrentScopeSelectionMeta {
    pub(super) flags: u8,
}

impl CurrentScopeSelectionMeta {
    pub(super) const FLAG_ROUTE_ENTRY: u8 = 1;
    pub(super) const FLAG_HAS_OFFER_LANES: u8 = 1 << 1;
    pub(super) const FLAG_CONTROLLER: u8 = 1 << 2;

    pub(super) const EMPTY: Self = Self { flags: 0 };

    #[inline]
    pub(super) fn is_route_entry(self) -> bool {
        (self.flags & Self::FLAG_ROUTE_ENTRY) != 0
    }

    #[inline]
    pub(super) fn has_offer_lanes(self) -> bool {
        !self.is_route_entry() || (self.flags & Self::FLAG_HAS_OFFER_LANES) != 0
    }

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct CurrentFrontierSelectionState {
    pub(super) frontier: FrontierKind,
    pub(super) parallel_root: ScopeId,
    pub(super) ready: bool,
    pub(super) has_progress_evidence: bool,
    pub(super) flags: u8,
}

impl CurrentFrontierSelectionState {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn parallel(self) -> Option<ScopeId> {
        if self.parallel_root.is_none() {
            None
        } else {
            Some(self.parallel_root)
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn observe_candidate(
        &mut self,
        current_scope: ScopeId,
        current_idx: usize,
        candidate: FrontierCandidate,
    ) {
        if candidate.scope_id == current_scope && candidate.entry_idx as usize == current_idx {
            self.ready = candidate.ready();
            self.has_progress_evidence = candidate.has_evidence();
        }
    }

    #[inline]
    pub(super) fn loop_controller_without_evidence(self) -> bool {
        self.frontier == FrontierKind::Loop
            && self.is_controller()
            && self.ready
            && !self.has_progress_evidence
    }
}

#[derive(Clone, Copy)]
pub(super) struct FrontierStaticFacts {
    pub(super) frontier: FrontierKind,
    pub(super) ready: bool,
}

/// Branch metadata carried from `offer()` to `decode()`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BranchMeta {
    /// The scope this branch belongs to.
    pub(crate) scope_id: ScopeId,
    /// The selected arm (0, 1, ...).
    pub(crate) selected_arm: u8,
    /// Wire lane for this branch.
    pub(crate) lane_wire: u8,
    /// EffIndex for lane cursor advancement.
    pub(crate) eff_index: EffIndex,
    /// Transport/binding discriminator expected for this branch.
    pub(crate) frame_label: u8,
    /// Branch dispatch category for decode() dispatch.
    pub(crate) kind: BranchKind,
    /// Route decision source used when commit emits route-decision events.
    pub(in crate::endpoint::kernel) route_source: RouteDecisionSource,
}

/// Branch type taxonomy for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/binding.
    WireRecv,
    /// Synthetic local control: self-send that doesn't go on wire.
    /// Decode from zero buffer; scope settlement uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should continue on the same borrowed endpoint with `flow().send()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
}

pub(super) struct RouteFrontierMachine<
    'endpoint,
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
    C: Clock,
    Mint: MintConfigMarker,
{
    endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    frontier_visited: Option<FrontierVisitSet>,
    carried_binding_evidence: Option<LaneIngressEvidence>,
    carried_transport_payload: Option<(usize, u8, Payload<'r>)>,
    run_stage: Option<OfferRunStage<'r>>,
    pending_recv: lane_port::PendingRecv,
}

pub(crate) struct OfferState<'r> {
    frontier_visited: Option<FrontierVisitSet>,
    carried_binding_evidence: Option<LaneIngressEvidence>,
    carried_transport_payload: Option<(usize, u8, Payload<'r>)>,
    run_stage: Option<OfferRunStage<'r>>,
    pending_recv: lane_port::PendingRecv,
}

impl<'r> OfferState<'r> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            frontier_visited: None,
            carried_binding_evidence: None,
            carried_transport_payload: None,
            run_stage: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }
}

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    pub(super) const fn new(
        endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> Self {
        Self {
            endpoint,
            frontier_visited: None,
            carried_binding_evidence: None,
            carried_transport_payload: None,
            run_stage: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_frame_label_meta(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeFrameLabelMeta> {
        let state = endpoint.offer_entry_state_snapshot(entry_idx)?;
        if !endpoint.offer_entry_has_active_lanes(entry_idx)
            || endpoint.offer_entry_scope_id(entry_idx, state) != scope_id
        {
            return None;
        }
        if let Some(info) = endpoint.offer_entry_lane_state(scope_id, entry_idx) {
            let representative_idx = state_index_to_usize(info.entry);
            let loop_meta = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_loop_meta_at(
                &endpoint.cursor,
                &endpoint.control_semantics(),
                scope_id,
                representative_idx,
            );
            return Some(
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_meta_at(
                    &endpoint.cursor,
                    &endpoint.control_semantics(),
                    scope_id,
                    loop_meta,
                    representative_idx,
                ),
            );
        }
        let loop_meta = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_loop_meta(
            &endpoint.cursor,
            &endpoint.control_semantics(),
            scope_id,
        );
        #[cfg(test)]
        {
            if !state.frame_label_meta.scope_id().is_none() {
                return Some(state.frame_label_meta);
            }
        }
        Some(
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_meta(
                &endpoint.cursor,
                &endpoint.control_semantics(),
                scope_id,
                loop_meta,
            ),
        )
    }

    #[inline]
    fn offer_refresh_mask(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        lane_idx: usize,
    ) -> bool {
        endpoint.cursor.current_phase_lane_set().contains(lane_idx)
            || endpoint.route_state.lane_linger_lanes().contains(lane_idx)
            || endpoint
                .route_state
                .lane_offer_linger_lanes()
                .contains(lane_idx)
    }

    #[inline]
    fn for_each_set_lane(lane_set: LaneSetView, lane_limit: usize, mut f: impl FnMut(usize)) {
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            f(lane_idx);
            next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_offer_lane_entry_slot_masks(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> OfferLaneEntrySlotMasks {
        let active_entries = if use_root_observed_entries {
            endpoint.root_frontier_active_entries(current_parallel_root)
        } else {
            endpoint.global_active_entries()
        };
        let port = endpoint.port_for_lane(endpoint.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = endpoint.cursor.frontier_scratch_layout();
        let mut slot_masks =
            frontier_offer_lane_entry_slot_masks_view_from_storage(scratch_ptr, layout);
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(_state) = endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !endpoint.offer_entry_has_active_lanes(entry_idx) {
                continue;
            }
            let logical_lane_count = endpoint.cursor.logical_lane_count();
            let active_offer_lanes = endpoint.route_state.active_offer_lanes();
            Self::for_each_set_lane(active_offer_lanes, logical_lane_count, |lane_idx| {
                if state_index_to_usize(endpoint.route_state.lane_offer_state(lane_idx).entry)
                    == entry_idx
                {
                    slot_masks.set_logical_mask(lane_idx, slot_masks[lane_idx] | (1u8 << slot_idx));
                }
            });
        }
        slot_masks
    }

    pub(in crate::endpoint::kernel) fn frontier_observation_key(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> FrontierObservationKey {
        let active_entries = if use_root_observed_entries {
            endpoint.root_frontier_active_entries(current_parallel_root)
        } else {
            endpoint.global_active_entries()
        };
        let port = endpoint.port_for_lane(endpoint.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = endpoint.cursor.frontier_scratch_layout();
        let mut key = frontier_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            endpoint.cursor.max_frontier_entries(),
        );
        key.clear();
        key.set_active_entries_from(active_entries);
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            let summary = endpoint.compute_offer_entry_static_summary(entry_idx);
            let slot = key.slot_mut(slot_idx);
            slot.entry_summary_fingerprint = summary.observation_fingerprint();
            slot.scope_generation = endpoint.scope_evidence_generation_for_scope(
                endpoint.offer_entry_scope_id(entry_idx, entry_state),
            );
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            let Some(lane_idx) =
                endpoint.offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                continue;
            };
            key.slot_mut(slot_idx).route_change_epoch = endpoint
                .ports
                .get(lane_idx)
                .and_then(Option::as_ref)
                .map(Port::route_change_epoch)
                .unwrap_or(0);
        }
        let logical_lane_count = endpoint.cursor.logical_lane_count();
        let binding_nonempty_lanes = endpoint.binding_inbox.nonempty_lanes();
        let active_offer_lanes = endpoint.route_state.active_offer_lanes();
        Self::for_each_set_lane(active_offer_lanes, logical_lane_count, |lane_idx| {
            let info = endpoint.route_state.lane_offer_state(lane_idx);
            if !info.entry.is_max()
                && active_entries
                    .slot_for_entry(state_index_to_usize(info.entry))
                    .is_some()
            {
                key.insert_offer_lane(lane_idx);
                if binding_nonempty_lanes.contains(lane_idx) {
                    key.insert_binding_nonempty_lane(lane_idx);
                }
            }
        });
        key
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn ensure_global_frontier_scratch_initialized(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) {
        endpoint.init_global_frontier_scratch_if_needed();
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_cache(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        endpoint
            .frontier_observation_cache_snapshot(current_parallel_root, use_root_observed_entries)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn store_frontier_observation(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        endpoint.write_frontier_observation_snapshot(
            current_parallel_root,
            use_root_observed_entries,
            key,
            observed_entries,
        );
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn cached_offer_entry_observed_state_for_rebuild(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        entry_idx: usize,
        entry_state: &OfferEntryState,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<OfferEntryObservedState> {
        endpoint.reusable_cached_offer_entry_observed_state(
            entry_idx,
            entry_state,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache(
        endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) {
        let mut machine = Self::new(endpoint);
        machine.refresh_frontier_observation_cache_impl(
            current_parallel_root,
            use_root_observed_entries,
        )
    }

    pub(super) fn select_scope(&mut self) -> RecvResult<OfferScopeSelection> {
        self.align_cursor_to_selected_scope()?;
        // O(1) entry: offer() must be called at a Route decision point.
        // Use the node's scope directly (no parent traversal).
        let node_scope = self.endpoint.current_offer_scope_id();
        let Some(region) = self.endpoint.cursor.scope_region_by_id(node_scope) else {
            return Err(RecvError::PhaseInvariant);
        };
        if region.kind != ScopeKind::Route {
            return Err(RecvError::PhaseInvariant);
        }
        let scope_id = region.scope_id;
        if let Some(offer_entry) = self.endpoint.cursor.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
            && self.endpoint.cursor.index() != state_index_to_usize(offer_entry)
        {
            let selected_arm = self.endpoint.selected_arm_for_scope(scope_id);
            let current_arm = self
                .endpoint
                .cursor
                .typestate_node(self.endpoint.cursor.index())
                .route_arm();
            if selected_arm.is_none() || current_arm != selected_arm {
                return Err(RecvError::PhaseInvariant);
            }
        }
        let current_idx = self.endpoint.cursor.index();
        let cached_entry_state = self
            .endpoint
            .offer_entry_state_snapshot(current_idx)
            .filter(|state| {
                self.endpoint.offer_entry_has_active_lanes(current_idx)
                    && self.endpoint.offer_entry_scope_id(current_idx, *state) == scope_id
            });
        // Route hints are offer-scoped; preview only inspects them here.
        let offer_lane = if let Some(entry_state) = cached_entry_state {
            self.endpoint
                .offer_entry_representative_lane_idx(current_idx, entry_state)
                .map(|lane_idx| lane_idx as u8)
        } else {
            self.endpoint
                .offer_lane_set_for_scope(scope_id)
                .first_set(self.endpoint.cursor.logical_lane_count())
                .map(|lane_idx| lane_idx as u8)
        };
        let Some(offer_lane) = offer_lane else {
            return Err(RecvError::PhaseInvariant);
        };
        let offer_lane_idx = offer_lane;
        let at_route_offer_entry = self
            .endpoint
            .cursor
            .route_scope_offer_entry(scope_id)
            .map(|entry| entry.is_max() || current_idx == state_index_to_usize(entry))
            .unwrap_or(true);
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root:
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::parallel_scope_root(
                    &self.endpoint.cursor,
                    scope_id,
                ),
            offer_lane,
            offer_lane_idx,
            at_route_offer_entry,
        })
    }

    #[inline]
    fn record_scope_ack(&mut self, scope_id: ScopeId, token: RouteDecisionToken) {
        if let Some(slot) = self.endpoint.scope_slot_for_route(scope_id)
            && self
                .endpoint
                .route_state
                .scope_evidence
                .record_ack(slot, token)
        {
            self.endpoint
                .bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.endpoint
            .mark_scope_ready_arm_inner(scope_id, arm, true);
    }

    #[inline]
    fn mark_scope_materialization_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.endpoint
            .mark_scope_ready_arm_inner(scope_id, arm, false);
    }

    #[inline]
    fn mark_scope_ready_arm_from_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        let exact_static_passive_arm = self
            .endpoint
            .static_passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        let arm = exact_static_passive_arm.or_else(|| {
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_evidence_frame_label_to_arm(
                frame_label_meta,
                frame_label,
            )
        });
        if let Some(arm) = arm {
            if self
                .endpoint
                .loop_control_evidence_only(frame_label_meta, arm)
            {
                return;
            }
            if self
                .endpoint
                .static_passive_scope_evidence_materializes_poll(scope_id)
            {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_static_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, lane, frame_label);
            }
        }
    }

    #[inline]
    fn mark_scope_ready_arm_from_binding_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        let exact_static_passive_arm = self
            .endpoint
            .static_passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        let arm = exact_static_passive_arm.or_else(|| {
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::binding_scope_evidence_frame_label_to_arm(
                frame_label_meta,
                frame_label,
            )
        });
        if let Some(arm) = arm {
            if self
                .endpoint
                .loop_control_evidence_only(frame_label_meta, arm)
            {
                return;
            }
            if self
                .endpoint
                .static_passive_scope_evidence_materializes_poll(scope_id)
            {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_static_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, lane, frame_label);
            }
        }
    }

    #[inline]
    fn mark_static_passive_descendant_path_ready(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) {
        let mut current_scope = scope_id;
        let mut depth = 0usize;
        let depth_bound = self.endpoint.route_scope_depth_bound();
        while depth < depth_bound {
            let Some(arm) = self
                .endpoint
                .static_passive_descendant_dispatch_arm_from_exact_frame_label(
                    current_scope,
                    lane,
                    frame_label,
                )
            else {
                break;
            };
            self.mark_scope_ready_arm(current_scope, arm);
            let Some(child_scope) = self
                .endpoint
                .cursor
                .passive_arm_scope_by_arm(current_scope, arm)
            else {
                break;
            };
            if child_scope == current_scope {
                break;
            }
            current_scope = child_scope;
            depth += 1;
        }
    }

    fn on_frontier_defer(
        &mut self,
        liveness: &mut OfferLivenessState,
        scope_id: ScopeId,
        current_parallel: Option<ScopeId>,
        source: DeferSource,
        reason: DeferReason,
        retry_hint: u8,
        offer_lane: u8,
        binding_ready: bool,
        selected_arm: Option<u8>,
        visited: &mut FrontierVisitSet,
    ) -> FrontierDeferOutcome {
        let fingerprint = self.endpoint.evidence_fingerprint(scope_id, binding_ready);
        let budget = liveness.on_defer(fingerprint);
        let exhausted = matches!(budget, DeferBudgetOutcome::Exhausted);
        let is_controller = self.endpoint.cursor.is_route_controller(scope_id);
        let frontier =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::frontier_kind_for_cursor(
                &self.endpoint.cursor,
                scope_id,
                is_controller,
            );
        let hint = self.endpoint.peek_scope_frame_hint(scope_id);
        let ready_arm_mask = self.endpoint.scope_ready_arm_mask(scope_id);
        self.endpoint.emit_policy_defer_event(
            source,
            reason,
            scope_id,
            frontier,
            selected_arm,
            hint,
            retry_hint,
            *liveness,
            ready_arm_mask,
            binding_ready,
            exhausted,
            offer_lane,
        );
        visited.record(scope_id);
        let current_entry_idx = self.endpoint.cursor.index();
        let current_is_controller = self.endpoint.cursor.is_route_controller(scope_id);
        let mut scratch = self.endpoint.frontier_scratch_view();
        let mut snapshot = frontier_snapshot_from_scratch(
            &mut scratch,
            scope_id,
            current_entry_idx,
            current_parallel.unwrap_or(ScopeId::none()),
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::frontier_kind_for_cursor(
                &self.endpoint.cursor,
                scope_id,
                current_is_controller,
            ),
        );
        self.endpoint
            .for_each_active_offer_candidate(current_parallel, |candidate| {
                let _ = snapshot.push_candidate(candidate);
                ControlFlow::<()>::Continue(())
            });
        if exhausted {
            let Some(candidate) = snapshot.select_exhausted_controller_candidate(*visited) else {
                return FrontierDeferOutcome::Exhausted;
            };
            visited.record(candidate.scope_id);
            if candidate.entry_idx as usize != self.endpoint.cursor.index() {
                self.endpoint.set_cursor_index(candidate.entry_idx as usize);
            }
            return FrontierDeferOutcome::Yielded;
        }
        let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
            return FrontierDeferOutcome::Continue;
        };
        visited.record(candidate.scope_id);
        if candidate.entry_idx as usize != self.endpoint.cursor.index() {
            self.endpoint.set_cursor_index(candidate.entry_idx as usize);
        }
        FrontierDeferOutcome::Yielded
    }
    fn current_scope_selection_meta(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        current_frontier: CurrentFrontierSelectionState,
    ) -> Option<CurrentScopeSelectionMeta> {
        if let Some(meta) = self
            .endpoint
            .offer_entry_selection_meta(scope_id, current_idx)
        {
            return Some(meta);
        }
        let Some(region) = self.endpoint.cursor.scope_region_by_id(scope_id) else {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        };
        if region.kind != ScopeKind::Route {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let offer_entry = self
            .endpoint
            .cursor
            .route_scope_offer_entry(region.scope_id)?;
        let route_entry_idx = if offer_entry.is_max() {
            current_idx
        } else {
            state_index_to_usize(offer_entry)
        };
        if !offer_entry.is_max() && route_entry_idx != current_idx {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let mut flags = CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY;
        if !self
            .endpoint
            .offer_lane_set_for_scope(region.scope_id)
            .is_empty()
        {
            flags |= CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES;
        }
        if current_frontier.is_controller() {
            flags |= CurrentScopeSelectionMeta::FLAG_CONTROLLER;
        }
        Some(CurrentScopeSelectionMeta { flags })
    }

    #[inline]
    fn entry_has_route_scope(&self, entry_idx: usize) -> bool {
        if let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx)
            && self.endpoint.offer_entry_has_active_lanes(entry_idx)
        {
            let scope_id = self.endpoint.offer_entry_scope_id(entry_idx, entry_state);
            if !scope_id.is_none() {
                return self
                    .endpoint
                    .cursor
                    .scope_region_by_id(scope_id)
                    .map(|region| region.kind == ScopeKind::Route)
                    .unwrap_or(false);
            }
        }
        let scope_id = self.endpoint.cursor.node_scope_id_at(entry_idx);
        !scope_id.is_none()
            && self
                .endpoint
                .cursor
                .scope_region_by_id(scope_id)
                .map(|region| region.kind == ScopeKind::Route)
                .unwrap_or(false)
    }

    fn current_frontier_selection_state(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
    ) -> CurrentFrontierSelectionState {
        if let Some(info) = self.endpoint.offer_entry_lane_state(scope_id, current_idx) {
            let entry_state = self
                .endpoint
                .offer_entry_state_snapshot(current_idx)
                .unwrap_or_else(|| unreachable!("active offer entry must have a runtime snapshot"));
            let summary = self
                .endpoint
                .compute_offer_entry_static_summary(current_idx);
            let entry_parallel = self
                .endpoint
                .offer_entry_parallel_root_from_state(current_idx, entry_state);
            let parallel_root = info.parallel_root;
            let current_parallel = if !parallel_root.is_none()
                && self.endpoint.root_frontier_active_mask(parallel_root) != 0
            {
                Some(parallel_root)
            } else {
                entry_parallel
            };
            let mut flags = 0u8;
            if summary.is_controller() {
                flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
            }
            if summary.is_dynamic() {
                flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
            }
            return CurrentFrontierSelectionState {
                frontier: self.endpoint.offer_entry_frontier(current_idx, entry_state),
                parallel_root: current_parallel.unwrap_or(ScopeId::none()),
                ready: summary.static_ready(),
                has_progress_evidence: false,
                flags,
            };
        }
        let current_is_controller = self.endpoint.cursor.is_route_controller(scope_id);
        let current_is_dynamic = current_is_controller
            && self
                .endpoint
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _, _)| policy.is_dynamic())
                .unwrap_or(false);
        let static_facts =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::frontier_static_facts_at(
                &self.endpoint.cursor,
                &self.endpoint.control_semantics(),
                scope_id,
                current_is_controller,
                current_is_dynamic,
                current_idx,
            );
        let cursor_parallel =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::parallel_scope_root(
                &self.endpoint.cursor,
                scope_id,
            );
        let cursor_parallel_has_offer = cursor_parallel
            .map(|root| self.endpoint.root_frontier_active_mask(root) != 0)
            .unwrap_or(false);
        let current_entry_has_offer = self.endpoint.offer_entry_has_active_lanes(current_idx);
        let current_entry_parallel = if cursor_parallel_has_offer || !current_entry_has_offer {
            None
        } else {
            self.endpoint
                .offer_entry_state_snapshot(current_idx)
                .and_then(|entry_state| {
                    self.endpoint
                        .offer_entry_parallel_root_from_state(current_idx, entry_state)
                })
        };
        let current_parallel = if cursor_parallel_has_offer {
            cursor_parallel
        } else {
            current_entry_parallel
        };
        let mut flags = 0u8;
        if current_is_controller {
            flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
        }
        if current_is_dynamic {
            flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
        }
        CurrentFrontierSelectionState {
            frontier: static_facts.frontier,
            parallel_root: current_parallel.unwrap_or(ScopeId::none()),
            ready: static_facts.ready,
            has_progress_evidence: false,
            flags,
        }
    }
    pub(super) fn align_cursor_to_selected_scope(&mut self) -> RecvResult<()> {
        let node_scope = self.endpoint.cursor.node_scope_id();
        let current_scope = self.endpoint.current_offer_scope_id();
        if current_scope != node_scope
            && let Some(entry_idx) = self.endpoint.route_scope_offer_entry_index(current_scope)
            && entry_idx != self.endpoint.cursor.index()
        {
            self.endpoint.set_cursor_index(entry_idx);
            self.endpoint.sync_lane_offer_state();
            return self.align_cursor_to_selected_scope();
        }
        let node_scope = self.endpoint.current_offer_scope_id();
        let current_idx = self.endpoint.cursor.index();
        let mut current_frontier_state =
            self.current_frontier_selection_state(node_scope, current_idx);
        let current_frontier = current_frontier_state.frontier;
        let current_parallel = current_frontier_state.parallel();
        let current_parallel_root = current_frontier_state.parallel_root;
        let current_scope_selected = self.endpoint.selected_arm_for_scope(node_scope).is_some();
        if current_scope_selected
            && self
                .current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
                .map(|meta| meta.is_route_entry())
                .unwrap_or(false)
        {
            return Ok(());
        }
        let use_root_observed_entries = current_parallel.is_some();
        let active_entries = self.endpoint.active_frontier_entries(current_parallel);
        if active_entries.contains_only(current_idx) {
            let Some(current_scope_meta) =
                self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
            else {
                return Ok(());
            };
            if current_scope_meta.is_route_entry() && current_scope_meta.has_offer_lanes() {
                return Ok(());
            }
        }
        let observation_key = RouteFrontierMachine::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let mut observed_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_observed_entries(current_parallel_root)
        } else {
            self.endpoint.global_frontier_observed_entries()
        };
        let cached_entries = self.endpoint.cached_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
        );
        if cached_entries.is_none() && observed_entries.len() != 0 {
            RouteFrontierMachine::refresh_frontier_observation_cache(
                self.endpoint,
                current_parallel_root,
                use_root_observed_entries,
            );
            observed_entries = if use_root_observed_entries {
                self.endpoint
                    .root_frontier_observed_entries(current_parallel_root)
            } else {
                self.endpoint.global_frontier_observed_entries()
            };
        }
        let reentry_ready_entry_idx =
            self.endpoint
                .observed_reentry_entry_idx(observed_entries, current_idx, true);
        let reentry_any_entry_idx =
            self.endpoint
                .observed_reentry_entry_idx(observed_entries, current_idx, false);
        let loop_controller_without_evidence =
            current_frontier_state.loop_controller_without_evidence();
        let progress_sibling_exists = if current_parallel_root.is_none() {
            self.endpoint.global_frontier_progress_sibling_exists(
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        } else {
            self.endpoint.root_frontier_progress_sibling_exists(
                current_parallel_root,
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        };
        let Some(current_scope_meta) =
            self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
        else {
            return Ok(());
        };
        let current_is_route_entry = current_scope_meta.is_route_entry();
        let current_has_offer_lanes = current_scope_meta.has_offer_lanes();
        let current_is_controller = current_scope_meta.is_controller();
        let mut selectable_mask = 0u8;
        let mut slot_idx = 0usize;
        let observed_len = observed_entries.len();
        while slot_idx < observed_len {
            let slot_bit = 1u8 << slot_idx;
            if let Some(entry_idx) = observed_entries.first_entry_idx(slot_bit)
                && self.entry_has_route_scope(entry_idx)
            {
                selectable_mask |= slot_bit;
            }
            slot_idx += 1;
        }
        let observed_mask = observed_entries.occupancy_mask() & selectable_mask;
        let ready_mask = observed_entries.ready_mask & observed_mask;
        let ready_arm_mask = observed_entries.ready_arm_mask & observed_mask;
        let controller_mask = observed_entries.controller_mask & observed_mask;
        let dynamic_controller_mask = observed_entries.dynamic_controller_mask & observed_mask;
        let current_entry_bit = observed_entries.entry_bit(current_idx);
        let progress_mask = if current_is_route_entry {
            observed_entries.progress_mask & observed_mask
        } else {
            (observed_entries.progress_mask & observed_mask) & !current_entry_bit
        };
        if current_entry_bit != 0 {
            current_frontier_state.ready |= (current_entry_bit & ready_mask) != 0;
            current_frontier_state.has_progress_evidence |=
                (current_entry_bit & progress_mask) != 0;
        }
        let current_matches_candidate = current_entry_bit != 0 && current_is_route_entry;
        let mut current_has_evidence = (current_entry_bit & observed_entries.progress_mask) != 0;
        let suppress_current_controller_without_evidence = current_is_controller
            && current_matches_candidate
            && (current_entry_bit & observed_entries.ready_arm_mask) == 0
            && (current_entry_bit & observed_entries.progress_mask) == 0
            && progress_sibling_exists;
        let controller_progress_sibling_exists =
            (progress_mask & controller_mask & !current_entry_bit) != 0;
        let mut static_controller_ready_mask = observed_mask & !controller_mask;
        static_controller_ready_mask |= current_entry_bit & controller_mask;
        static_controller_ready_mask |= progress_mask & controller_mask;
        if suppress_current_controller_without_evidence {
            static_controller_ready_mask &= !current_entry_bit;
        }
        let current_entry_unrunnable = current_is_route_entry && !current_has_offer_lanes;
        let mut candidate_mask = progress_mask;
        if current_matches_candidate {
            candidate_mask |= current_entry_bit;
        }
        if current_entry_unrunnable {
            candidate_mask |= observed_mask & !current_entry_bit;
        }
        candidate_mask &= static_controller_ready_mask;
        let hinted_mask = candidate_mask & ready_arm_mask;
        let hinted_count = hinted_mask.count_ones() as usize;
        let hint_filter_mask = if hinted_count == 1 { hinted_mask } else { 0 };
        let hint_filter = observed_entries.first_entry_idx(hint_filter_mask);
        let candidate_mask = if hint_filter_mask != 0 {
            hinted_mask
        } else {
            candidate_mask
        };
        let candidate_controller_mask = candidate_mask & controller_mask;
        let candidate_dynamic_controller_mask = candidate_controller_mask & dynamic_controller_mask;
        let candidate_count = candidate_mask.count_ones() as usize;
        let controller_count = candidate_controller_mask.count_ones() as usize;
        let dynamic_controller_count = candidate_dynamic_controller_mask.count_ones() as usize;
        let candidate_idx = observed_entries.first_entry_idx(candidate_mask);
        let controller_idx = observed_entries.first_entry_idx(candidate_controller_mask);
        let dynamic_controller_idx =
            observed_entries.first_entry_idx(candidate_dynamic_controller_mask);
        current_has_evidence |= current_frontier_state.has_progress_evidence;
        let suppress_current_passive_without_evidence =
            should_suppress_current_passive_without_evidence(
                current_frontier,
                current_is_controller,
                current_has_evidence,
                controller_progress_sibling_exists,
            );
        let current_matches_filtered = current_entry_matches_after_filter(
            current_matches_candidate && !suppress_current_passive_without_evidence,
            current_has_offer_lanes,
            current_idx,
            hint_filter,
        );
        let current_is_candidate = current_entry_is_candidate(
            current_matches_filtered,
            current_is_controller,
            current_has_evidence,
            candidate_count,
            progress_sibling_exists,
        );
        let selection = match choose_offer_priority(
            current_is_candidate,
            dynamic_controller_count,
            controller_count,
            candidate_count,
        ) {
            Some(OfferSelectPriority::CurrentOfferEntry) => {
                Some((OfferSelectPriority::CurrentOfferEntry, current_idx))
            }
            Some(OfferSelectPriority::DynamicControllerUnique) => dynamic_controller_idx
                .map(|idx| (OfferSelectPriority::DynamicControllerUnique, idx)),
            Some(OfferSelectPriority::ControllerUnique) => {
                controller_idx.map(|idx| (OfferSelectPriority::ControllerUnique, idx))
            }
            Some(OfferSelectPriority::CandidateUnique) => {
                candidate_idx.map(|idx| (OfferSelectPriority::CandidateUnique, idx))
            }
            None => None,
        };
        if let Some((_priority, entry_idx)) = selection {
            if entry_idx != self.endpoint.cursor.index() {
                self.endpoint.set_cursor_index(entry_idx);
                self.endpoint.sync_lane_offer_state();
                return self.align_cursor_to_selected_scope();
            }
            return Ok(());
        }
        if self.endpoint.current_route_arm_authorized()?.is_some() {
            return Ok(());
        }
        if current_is_route_entry && current_has_offer_lanes {
            return Ok(());
        }
        if !current_is_route_entry {
            if let Some(entry_idx) = reentry_ready_entry_idx.or(reentry_any_entry_idx) {
                if entry_idx != self.endpoint.cursor.index() {
                    self.endpoint.set_cursor_index(entry_idx);
                    self.endpoint.sync_lane_offer_state();
                    return self.align_cursor_to_selected_scope();
                }
                return Ok(());
            }
        }
        Err(RecvError::PhaseInvariant)
    }
    fn await_transport_payload_for_offer_lane(
        &mut self,
        offer_lane: u8,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
        transport_payload: &mut Option<Payload<'r>>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        let lane_idx = offer_lane as usize;
        let port = self.endpoint.port_for_lane(lane_idx);
        let payload = match lane_port::poll_recv(&mut self.pending_recv, port, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(payload)) => payload,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
        };
        if *transport_payload_len == 0 && !payload.as_bytes().is_empty() {
            *transport_payload_len = payload.as_bytes().len();
            *transport_payload_lane = offer_lane;
            *transport_payload = Some(payload);
        }
        Poll::Ready(Ok(()))
    }
    fn await_static_passive_progress(
        &mut self,
        selection: OfferScopeSelection,
        selected_arm: Option<u8>,
        binding_evidence: &mut Option<LaneIngressEvidence>,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
        transport_payload: &mut Option<Payload<'r>>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        let materialization_meta = self.endpoint.selection_materialization_meta(selection);
        if let Some(arm) = selected_arm
            && selection.at_route_offer_entry
            && let Some(entry) = materialization_meta.passive_arm_entry(arm)
        {
            if !self.endpoint.cursor.is_recv_at(state_index_to_usize(entry)) {
                return Poll::Ready(Ok(()));
            }
        }
        if binding_evidence.is_none()
            && let Some((lane_idx, evidence)) = {
                let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
                self.endpoint.poll_binding_for_offer(
                    selection.scope_id,
                    selection.offer_lane_idx as usize,
                    frame_label_meta,
                    materialization_meta,
                )
            }
        {
            *binding_evidence = Some(LaneIngressEvidence::new(lane_idx, evidence));
            return Poll::Ready(Ok(()));
        }
        if *transport_payload_len == 0 {
            return self.await_transport_payload_for_offer_lane(
                selection.offer_lane,
                transport_payload_len,
                transport_payload_lane,
                transport_payload,
                cx,
            );
        }
        Poll::Ready(Ok(()))
    }
    fn try_poll_route_decision_immediate(
        &self,
        scope_id: ScopeId,
        offer_lanes: LaneSetView,
        cx: &mut core::task::Context<'_>,
    ) -> Option<Arm> {
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        let mut arm = None;
        while let Some(lane_idx) = next {
            let lane = lane_idx as u8;
            let port = self.endpoint.port_for_lane(lane as usize);
            if let Poll::Ready(route_arm) = port.poll_route_decision(scope_id, ROLE, cx) {
                arm = Some(route_arm);
                break;
            }
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        let arm = arm?;
        Arm::new(arm)
    }
    fn try_poll_route_decision_for_offer(
        &self,
        scope_id: ScopeId,
        offer_lanes: LaneSetView,
        cx: &mut core::task::Context<'_>,
    ) -> Option<Arm> {
        self.try_poll_route_decision_immediate(scope_id, offer_lanes, cx)
            .or_else(|| self.endpoint.poll_arm_from_ready_mask(scope_id))
    }
    fn poll_resolve_pending_action(
        &mut self,
        state: &mut OfferResolveState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        let Some(action) = state.pending_action else {
            return Poll::Ready(Err(RecvError::PhaseInvariant));
        };
        match action {
            ResolvePendingAction::YieldRestart => {
                if !state.yield_armed {
                    state.yield_armed = true;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                state.pending_action = None;
                state.yield_armed = false;
                Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier))
            }
            ResolvePendingAction::StaticPassiveProgress { selected_arm } => {
                match self.await_static_passive_progress(
                    state.selection,
                    Some(selected_arm),
                    &mut state.binding_evidence,
                    &mut state.transport_payload_len,
                    &mut state.transport_payload_lane,
                    &mut state.transport_payload,
                    cx,
                ) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(())) => {
                        state.pending_action = None;
                        Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier))
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                }
            }
        }
    }
    fn resolve_token(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        if state.pending_action.is_some() {
            return self.poll_resolve_pending_action(state, cx);
        }
        let selection = state.selection;
        let is_route_controller = state.facts.is_route_controller;
        let is_dynamic_route_scope = state.facts.is_dynamic_route_scope;
        let binding_evidence = &mut state.binding_evidence;
        let transport_payload_len = &mut state.transport_payload_len;
        let transport_payload_lane = &mut state.transport_payload_lane;
        let transport_payload = &mut state.transport_payload;
        let scope_id = selection.scope_id;
        let frontier_parallel_root = selection.frontier_parallel_root;
        let offer_lane = selection.offer_lane;
        let offer_lane_idx = selection.offer_lane_idx as usize;
        let at_route_offer_entry = selection.at_route_offer_entry;
        let offer_lanes = self.endpoint.offer_lane_set_for_scope(scope_id);

        let mut resolved_hint_frame_label = self.endpoint.peek_scope_frame_hint(scope_id);
        if *transport_payload_len != 0
            && let Some(frame_label) = resolved_hint_frame_label
        {
            let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
            self.endpoint.mark_scope_ready_arm_from_frame_label(
                scope_id,
                offer_lane,
                frame_label,
                frame_label_meta,
            );
        }

        let liveness = &mut state.liveness;
        let mut liveness_exhausted = false;

        let mut route_token = self.endpoint.peek_scope_ack(scope_id);
        if route_token.is_none() && is_route_controller && is_dynamic_route_scope {
            loop {
                let route_signals = self
                    .endpoint
                    .policy_signals_for_slot(PolicySlot::Route)
                    .into_owned();
                let resolver_step = self
                    .endpoint
                    .prepare_route_decision_from_resolver(scope_id, &route_signals)?;
                match resolver_step {
                    RouteResolveStep::Resolved(resolver_arm) => {
                        route_token = Some(RouteDecisionToken::from_resolver(resolver_arm));
                        break;
                    }
                    RouteResolveStep::Abort(reason) => {
                        return Poll::Ready(Err(RecvError::PolicyAbort { reason }));
                    }
                    RouteResolveStep::Deferred { retry_hint, source } => {
                        match self.on_frontier_defer(
                            liveness,
                            scope_id,
                            frontier_parallel_root,
                            source,
                            DeferReason::Unsupported,
                            retry_hint,
                            offer_lane,
                            binding_evidence.is_some(),
                            None,
                            frontier_visited,
                        ) {
                            FrontierDeferOutcome::Continue => {}
                            FrontierDeferOutcome::Yielded => {
                                return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
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
                    transport_payload.is_some() && *transport_payload_lane == offer_lane;
                if !staged_payload_for_offer_lane {
                    let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
                    let materialization_meta =
                        self.endpoint.selection_materialization_meta(selection);
                    self.endpoint.cache_binding_evidence_for_offer(
                        scope_id,
                        offer_lane_idx,
                        frame_label_meta,
                        materialization_meta,
                        binding_evidence,
                    );

                    self.endpoint.ingest_scope_evidence_for_offer_lanes(
                        scope_id,
                        offer_lane_idx,
                        offer_lanes,
                        is_dynamic_route_scope,
                        frame_label_meta,
                    );
                    if let Some(evidence) = binding_evidence.as_ref() {
                        self.endpoint.ingest_binding_scope_evidence(
                            scope_id,
                            evidence.lane(),
                            evidence.frame_label(),
                            is_dynamic_route_scope,
                            frame_label_meta,
                        );
                    }
                    if self.endpoint.scope_evidence_conflicted(scope_id)
                        && !self.endpoint.recover_scope_evidence_conflict(
                            scope_id,
                            is_dynamic_route_scope,
                            is_route_controller,
                        )
                    {
                        return Poll::Ready(Err(RecvError::PhaseInvariant));
                    }

                    if let Some(frame_label) = self.endpoint.peek_scope_frame_hint(scope_id) {
                        resolved_hint_frame_label = Some(frame_label);
                    }
                }
                if let Some(token) = self.endpoint.peek_scope_ack(scope_id) {
                    route_token = Some(token);
                    break;
                }

                if *transport_payload_len != 0 {
                    break;
                }

                if resolved_hint_frame_label.is_some() && passive_waited_for_wire {
                    break;
                }

                if self.endpoint.scope_has_ready_arm_evidence(scope_id) {
                    let needs_wire_turn_for_materialization = !passive_waited_for_wire
                        && *transport_payload_len == 0
                        && binding_evidence.is_none();
                    if !needs_wire_turn_for_materialization {
                        break;
                    }
                }

                if !passive_waited_for_wire {
                    let recv_lane_idx = offer_lane as usize;
                    let recv_lane = recv_lane_idx as u8;
                    let port = self.endpoint.port_for_lane(recv_lane_idx);
                    if let Poll::Ready(payload) =
                        lane_port::poll_recv(&mut self.pending_recv, port, cx)
                    {
                        let payload = payload.map_err(RecvError::Transport)?;
                        if *transport_payload_len == 0 && !payload.as_bytes().is_empty() {
                            *transport_payload_len = payload.as_bytes().len();
                            *transport_payload_lane = recv_lane;
                            *transport_payload = Some(payload);
                        }
                    }
                    passive_waited_for_wire = true;
                    continue;
                }

                match self.on_frontier_defer(
                    liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_evidence.is_some(),
                    None,
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        break;
                    }
                    FrontierDeferOutcome::Yielded => {
                        return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
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
            && self.endpoint.binding.policy_signals_provider().is_some()
        {
            let resolver_step = {
                let route_signals = self
                    .endpoint
                    .policy_signals_for_slot(PolicySlot::Route)
                    .into_owned();
                self.endpoint
                    .prepare_route_decision_from_resolver(scope_id, &route_signals)?
            };
            match resolver_step {
                RouteResolveStep::Resolved(resolver_arm) => {
                    route_token = Some(RouteDecisionToken::from_resolver(resolver_arm));
                }
                RouteResolveStep::Abort(reason) => {
                    if reason != 0 {
                        return Poll::Ready(Err(RecvError::PolicyAbort { reason }));
                    }
                }
                RouteResolveStep::Deferred { retry_hint, source } => {
                    match self.on_frontier_defer(
                        liveness,
                        scope_id,
                        frontier_parallel_root,
                        source,
                        DeferReason::Unsupported,
                        retry_hint,
                        offer_lane,
                        binding_evidence.is_some(),
                        None,
                        frontier_visited,
                    ) {
                        FrontierDeferOutcome::Continue => {}
                        FrontierDeferOutcome::Yielded => {
                            state.pending_action = Some(ResolvePendingAction::YieldRestart);
                            return self.poll_resolve_pending_action(state, cx);
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
            && binding_evidence.is_none()
            && resolved_hint_frame_label.is_none()
            && !liveness_exhausted
        {
            match self.on_frontier_defer(
                liveness,
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
                    state.pending_action = Some(ResolvePendingAction::YieldRestart);
                    return self.poll_resolve_pending_action(state, cx);
                }
                FrontierDeferOutcome::Yielded => {
                    state.pending_action = Some(ResolvePendingAction::YieldRestart);
                    return self.poll_resolve_pending_action(state, cx);
                }
                FrontierDeferOutcome::Exhausted => {
                    liveness_exhausted = true;
                }
            }
        }

        if route_token.is_none() && liveness_exhausted {
            while route_token.is_none() && liveness.can_force_poll() {
                liveness.mark_forced_poll();
                if let Some(poll_arm) =
                    self.try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
                {
                    route_token = Some(RouteDecisionToken::from_poll(poll_arm));
                    break;
                }
            }
            if route_token.is_none() {
                return Poll::Ready(Err(RecvError::PolicyAbort {
                    reason: liveness.exhaust_reason(),
                }));
            }
        }

        if route_token.is_none() {
            if !is_route_controller
                && *transport_payload_len != 0
                && *transport_payload_lane != offer_lane
            {
                return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
            }
            if let Some(poll_arm) =
                self.try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
            {
                route_token = Some(RouteDecisionToken::from_poll(poll_arm));
            } else {
                match self.on_frontier_defer(
                    liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_evidence.is_some(),
                    None,
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        state.pending_action = Some(ResolvePendingAction::YieldRestart);
                        return self.poll_resolve_pending_action(state, cx);
                    }
                    FrontierDeferOutcome::Yielded => {
                        state.pending_action = Some(ResolvePendingAction::YieldRestart);
                        return self.poll_resolve_pending_action(state, cx);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        while route_token.is_none() && liveness.can_force_poll() {
                            liveness.mark_forced_poll();
                            if let Some(poll_arm) =
                                self.try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
                            {
                                route_token = Some(RouteDecisionToken::from_poll(poll_arm));
                                break;
                            }
                        }
                        if route_token.is_none() {
                            return Poll::Ready(Err(RecvError::PolicyAbort {
                                reason: liveness.exhaust_reason(),
                            }));
                        }
                    }
                }
            }
        }

        let mut route_token = match route_token {
            Some(route_token) => route_token,
            None => return Poll::Ready(Err(RecvError::PhaseInvariant)),
        };
        if let Some(evidence) = binding_evidence.as_ref()
            && let Some(binding_arm) = {
                let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_to_arm(
                    frame_label_meta,
                    evidence.frame_label(),
                )
            }
            && binding_arm == route_token.arm().as_u8()
        {
            // Binding evidence is demux-only. Once Ack/Resolver/Poll has
            // fixed the arm, matching evidence may still contribute
            // readiness for branch materialization.
            self.endpoint.mark_scope_ready_arm(scope_id, binding_arm);
        }
        if *transport_payload_len != 0 && *transport_payload_lane == offer_lane {
            if !is_route_controller
                && is_dynamic_route_scope
                && matches!(route_token.source(), RouteDecisionSource::Ack)
            {
                self.endpoint
                    .mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
            } else if is_route_controller
                && is_dynamic_route_scope
                && matches!(
                    route_token.source(),
                    RouteDecisionSource::Resolver | RouteDecisionSource::Poll
                )
            {
                self.endpoint
                    .mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
            }
        }

        let selected_arm = loop {
            let selected_arm = route_token.arm().as_u8();
            if self
                .endpoint
                .selection_arm_requires_materialization_ready_evidence(
                    selection,
                    is_route_controller,
                    selected_arm,
                )
                && !self.endpoint.scope_has_ready_arm(scope_id, selected_arm)
            {
                if matches!(route_token.source(), RouteDecisionSource::Resolver)
                    && let Some(poll_arm) =
                        self.try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
                {
                    route_token = RouteDecisionToken::from_poll(poll_arm);
                    continue;
                }
                if *transport_payload_len != 0 {
                    let port = self
                        .endpoint
                        .port_for_lane(*transport_payload_lane as usize);
                    lane_port::requeue_recv(port);
                }
                if matches!(route_token.source(), RouteDecisionSource::Resolver) {
                    let _ = self.endpoint.take_scope_ack(scope_id);
                }
                let keep_current_scope = is_route_controller
                    && is_dynamic_route_scope
                    && !at_route_offer_entry
                    && matches!(route_token.source(), RouteDecisionSource::Resolver);
                if keep_current_scope {
                    state.pending_action = Some(ResolvePendingAction::YieldRestart);
                    return self.poll_resolve_pending_action(state, cx);
                }
                match self.on_frontier_defer(
                    liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_evidence.is_some(),
                    Some(route_token.arm().as_u8()),
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        if !is_route_controller && !is_dynamic_route_scope {
                            state.pending_action =
                                Some(ResolvePendingAction::StaticPassiveProgress {
                                    selected_arm: route_token.arm().as_u8(),
                                });
                            return self.poll_resolve_pending_action(state, cx);
                        }
                        state.pending_action = Some(ResolvePendingAction::YieldRestart);
                        return self.poll_resolve_pending_action(state, cx);
                    }
                    FrontierDeferOutcome::Yielded => {
                        state.pending_action = Some(ResolvePendingAction::YieldRestart);
                        return self.poll_resolve_pending_action(state, cx);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        while liveness.can_force_poll() {
                            liveness.mark_forced_poll();
                            if self
                                .try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
                                .is_some()
                            {
                                return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
                            }
                        }
                        return Poll::Ready(Err(RecvError::PolicyAbort {
                            reason: liveness.exhaust_reason(),
                        }));
                    }
                }
            }
            break selected_arm;
        };
        Poll::Ready(Ok(ResolveTokenOutcome::Resolved(ResolvedRouteDecision {
            route_token,
            selected_arm,
            resolved_hint_frame_label,
        })))
    }
    pub(super) fn materialize_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
        is_route_controller: bool,
        mut binding_evidence: Option<LaneIngressEvidence>,
        transport_payload_len: usize,
        transport_payload_lane: u8,
        transport_payload: Option<Payload<'r>>,
    ) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let scope_id = selection.scope_id;
        let route_token = resolved.route_token;
        let selected_arm = resolved.selected_arm;
        let resolved_hint_frame_label = resolved.resolved_hint_frame_label;
        let preview_meta = self.endpoint.preview_selected_arm_meta(
            selection,
            selected_arm,
            resolved_hint_frame_label,
        )?;
        let (_cursor_index, meta) = match preview_meta.recv_meta() {
            Some(meta) => meta,
            None => return Err(RecvError::PhaseInvariant),
        };

        let lane_wire = meta.lane;

        // Determine BranchKind before late binding resolution so wire-bound
        // branches can decide whether to wait for one additional ingress turn.
        let passive_linger_loop_label = !is_route_controller
            && self.endpoint.is_linger_route(scope_id)
            && self.endpoint.control_semantic_kind(meta.semantic).is_loop();
        let branch_kind = if self.endpoint.cursor.is_recv() {
            if passive_linger_loop_label
                || (!is_route_controller
                    && self.endpoint.control_semantic_kind(meta.semantic).is_loop()
                    && self.endpoint.selection_non_wire_loop_control_recv(
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
        } else if self.endpoint.cursor.is_send() {
            BranchKind::ArmSendHint
        } else if self.endpoint.cursor.is_local_action() || self.endpoint.cursor.is_jump() {
            BranchKind::LocalControl
        } else {
            BranchKind::EmptyArmTerminal
        };

        // Late binding channel resolution: for wire recv branches, prefer
        // binding ingress even when transport payload bytes were staged earlier.
        let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
        let binding_evidence = if matches!(branch_kind, BranchKind::WireRecv) {
            let mut selected_evidence = None;
            let lane_idx = meta.lane as usize;
            if let Some(carried) = binding_evidence.as_ref()
                && carried.lane_idx != lane_idx
            {
                let (carried_lane, carried_evidence) =
                    binding_evidence.take().unwrap().into_parts();
                self.endpoint
                    .put_back_binding_for_lane(carried_lane, carried_evidence);
            }
            if let Some(expected_frame_label) =
                frame_label_meta.preferred_binding_frame_label(Some(selected_arm))
            {
                if let Some(carried) = binding_evidence.take() {
                    let (carried_lane, carried) = carried.into_parts();
                    if carried.frame_label.raw() == expected_frame_label
                        && carried.frame_label.raw() == meta.frame_label
                    {
                        selected_evidence = Some(carried);
                    } else {
                        self.endpoint
                            .put_back_binding_for_lane(carried_lane, carried);
                    }
                }
                if selected_evidence.is_none() && expected_frame_label == meta.frame_label {
                    selected_evidence = self
                        .endpoint
                        .take_matching_binding_for_lane(lane_idx, expected_frame_label);
                }
            } else {
                if let Some(carried) = binding_evidence.take() {
                    let (carried_lane, carried) = carried.into_parts();
                    if carried.frame_label.raw() == meta.frame_label {
                        selected_evidence = Some(carried);
                    } else {
                        self.endpoint
                            .put_back_binding_for_lane(carried_lane, carried);
                    }
                }
                if selected_evidence.is_none() {
                    selected_evidence = self
                        .endpoint
                        .take_matching_binding_for_lane(lane_idx, meta.frame_label);
                }
            }
            selected_evidence.map(|evidence| LaneIngressEvidence::new(lane_idx, evidence))
        } else {
            if let Some(lane_evidence) = binding_evidence {
                let (lane_idx, evidence) = lane_evidence.into_parts();
                self.endpoint.put_back_binding_for_lane(lane_idx, evidence);
            }
            None
        };
        let binding_staged_payload = binding_evidence.and_then(|lane_evidence| {
            let (lane_idx, evidence) = lane_evidence.into_parts();
            self.endpoint
                .take_restored_binding_payload(lane_idx, evidence)
                .map(|payload| (lane_idx as u8, payload))
        });
        let transport_payload_for_branch = if transport_payload_len != 0
            && (!matches!(branch_kind, BranchKind::WireRecv) || binding_evidence.is_some())
        {
            let port = self.endpoint.port_for_lane(transport_payload_lane as usize);
            lane_port::requeue_recv(port);
            None
        } else {
            transport_payload
        };
        let branch_progress_eff = self
            .endpoint
            .cursor
            .scope_lane_last_eff_for_arm(scope_id, selected_arm, lane_wire)
            .or_else(|| {
                self.endpoint
                    .cursor
                    .scope_lane_last_eff(scope_id, lane_wire)
            })
            .unwrap_or(meta.eff_index);
        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            eff_index: branch_progress_eff,
            frame_label: meta.frame_label,
            kind: branch_kind,
            route_source: route_token.source(),
        };
        self.endpoint
            .set_cursor_index(state_index_to_usize(preview_meta.cursor_index));
        Ok(RouteBranch {
            label: meta.label,
            binding_evidence: PackedIngressEvidence::from_option(
                binding_evidence.map(|lane_evidence| lane_evidence.evidence),
            ),
            binding_evidence_lane: binding_evidence
                .map(|lane_evidence| lane_evidence.lane_idx as u8)
                .unwrap_or(u8::MAX),
            staged_payload: binding_staged_payload
                .map(|(lane, payload)| StagedPayload::Binding { lane, payload })
                .or_else(|| {
                    transport_payload_for_branch.map(|payload| StagedPayload::Transport {
                        lane: transport_payload_lane,
                        payload,
                    })
                }),
            branch_meta,
            _cfg: core::marker::PhantomData,
        })
    }

    pub(in crate::endpoint::kernel) fn preflight_route_branch_commit(
        &self,
        preview: BranchPreviewView,
    ) -> RecvResult<BranchCommitPlan> {
        let scope_id = preview.branch_meta.scope_id;
        let selected_arm = preview.branch_meta.selected_arm;
        let lane_wire = preview.branch_meta.lane_wire;
        let lane_idx = lane_wire as usize;
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let clear_other_lanes =
            self.endpoint.selected_arm_for_scope(scope_id) != Some(selected_arm);
        let route_arm_proof = if clear_other_lanes {
            self.endpoint
                .preflight_route_arm_commit_after_clearing_other_lanes(
                    lane_wire,
                    scope_id,
                    selected_arm,
                )
        } else {
            self.endpoint
                .preflight_route_arm_commit(lane_wire, scope_id, selected_arm)
        };
        if scope_id.kind() == ScopeKind::Route && route_arm_proof.is_none() {
            return Err(RecvError::PhaseInvariant);
        }
        if preview.branch_meta.route_source == RouteDecisionSource::Poll
            && preview.branch_meta.kind == BranchKind::WireRecv
        {
            let Some((arm, _)) = self.endpoint.cursor.first_recv_target_for_lane_frame_label(
                scope_id,
                lane_wire,
                preview.branch_meta.frame_label,
            ) else {
                return Err(RecvError::PhaseInvariant);
            };
            let arm = if arm == ARM_SHARED { 0 } else { arm };
            if arm != selected_arm {
                return Err(RecvError::PhaseInvariant);
            }
        }

        let meta = if preview.branch_meta.kind == BranchKind::WireRecv {
            let mut meta = if let Some(meta) = self.endpoint.cursor.try_recv_meta() {
                meta
            } else {
                return Err(RecvError::PhaseInvariant);
            };
            if meta.route_arm.is_none() {
                meta.route_arm = Some(selected_arm);
            }
            if meta.label != preview.label {
                meta.label = preview.label;
            }
            Some(meta)
        } else {
            None
        };

        Ok(BranchCommitPlan {
            preview,
            meta,
            route_arm_proof,
            clear_other_lanes,
        })
    }

    pub(in crate::endpoint::kernel) fn publish_route_branch_commit_plan(
        &mut self,
        plan: BranchCommitPlan,
    ) -> Option<RecvMeta> {
        let preview = plan.preview;
        let scope_id = preview.branch_meta.scope_id;
        let selected_arm = preview.branch_meta.selected_arm;
        let lane_wire = preview.branch_meta.lane_wire;
        let is_route_controller = self.endpoint.cursor.is_route_controller(scope_id);

        if plan.clear_other_lanes {
            self.endpoint
                .clear_scope_route_state_for_other_lanes(scope_id, lane_wire);
        }
        if let Some(proof) = plan.route_arm_proof {
            self.endpoint.commit_route_arm_after_preflight(proof);
        }
        self.endpoint
            .skip_unselected_arm_lanes(scope_id, selected_arm, lane_wire);

        if !is_route_controller {
            if let Some(plan) = self
                .endpoint
                .build_recvless_parent_route_decision_plan(scope_id)
            {
                self.endpoint.publish_recvless_parent_route_decision(plan);
            }
        }

        match preview.branch_meta.route_source {
            RouteDecisionSource::Ack if is_route_controller => {
                if matches!(preview.branch_meta.kind, BranchKind::ArmSendHint) {
                    let lane = lane_wire;
                    self.endpoint.record_route_decision_for_lane(
                        lane as usize,
                        scope_id,
                        selected_arm,
                    );
                    self.endpoint.emit_route_decision(
                        scope_id,
                        selected_arm,
                        RouteDecisionSource::Ack,
                        lane,
                    );
                } else {
                    let offer_lanes = self.endpoint.offer_lane_set_for_scope(scope_id);
                    if offer_lanes.is_empty() {
                        let lane = lane_wire;
                        self.endpoint.record_route_decision_for_lane(
                            lane as usize,
                            scope_id,
                            selected_arm,
                        );
                        self.endpoint.emit_route_decision(
                            scope_id,
                            selected_arm,
                            RouteDecisionSource::Ack,
                            lane,
                        );
                    } else {
                        let lane_limit = self.endpoint.cursor.logical_lane_count();
                        let mut next = offer_lanes.first_set(lane_limit);
                        while let Some(lane_idx) = next {
                            let lane = lane_idx as u8;
                            self.endpoint.record_route_decision_for_lane(
                                lane as usize,
                                scope_id,
                                selected_arm,
                            );
                            self.endpoint.emit_route_decision(
                                scope_id,
                                selected_arm,
                                RouteDecisionSource::Ack,
                                lane,
                            );
                            next =
                                offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
                        }
                    }
                }
            }
            RouteDecisionSource::Poll => {
                self.endpoint.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Poll,
                    self.endpoint.offer_lane_for_scope(scope_id),
                );
            }
            _ => {}
        }

        if self.endpoint.arm_has_recv(scope_id, selected_arm) {
            self.endpoint
                .consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.endpoint.clear_scope_evidence(scope_id);
        self.endpoint
            .port_for_lane(lane_wire as usize)
            .clear_route_hints();

        plan.meta
    }

    fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        let frame_hint_matches_scope =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::frame_hint_matches_scope(
                frame_label_meta,
                frame_label,
                false,
            );
        let exact_static_passive_arm = self
            .endpoint
            .static_passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        if !frame_hint_matches_scope && exact_static_passive_arm.is_none() {
            return;
        }
        if suppress_hint || !frame_hint_matches_scope {
            self.endpoint.mark_scope_ready_arm_from_binding_frame_label(
                scope_id,
                lane,
                frame_label,
                frame_label_meta,
            );
            return;
        }
        self.endpoint.record_scope_frame_hint(scope_id, frame_label);
        self.endpoint.mark_scope_ready_arm_from_binding_frame_label(
            scope_id,
            lane,
            frame_label,
            frame_label_meta,
        );
    }

    fn ingest_scope_evidence_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
        drain_transport_hints: bool,
    ) {
        if suppress_hint {
            if let Some(frame_label) = self.endpoint.take_frame_hint_for_lane(
                lane_idx,
                false,
                frame_label_meta,
                drain_transport_hints,
            ) {
                self.endpoint
                    .record_dynamic_scope_frame_hint(scope_id, frame_label);
                self.endpoint.mark_scope_ready_arm_from_frame_label(
                    scope_id,
                    lane_idx as u8,
                    frame_label,
                    frame_label_meta,
                );
            }

            if let Some(arm) = self
                .endpoint
                .ack_route_decision_for_lane(lane_idx, scope_id, ROLE)
            {
                if let Some(arm) = Arm::new(arm) {
                    self.endpoint
                        .record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
                }
            }
            return;
        }
        if let Some(arm) = self
            .endpoint
            .ack_route_decision_for_lane(lane_idx, scope_id, ROLE)
        {
            if let Some(arm) = Arm::new(arm) {
                self.endpoint
                    .record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
            }
        }
        if let Some(frame_label) = self.endpoint.take_frame_hint_for_lane(
            lane_idx,
            suppress_hint,
            frame_label_meta,
            drain_transport_hints,
        ) {
            self.endpoint.record_scope_frame_hint(scope_id, frame_label);
        }
    }

    fn ingest_scope_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: crate::global::role_program::LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        if offer_lanes.is_empty() {
            return;
        }
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let drain_transport_hints = {
                let port = self.endpoint.port_for_lane(lane_idx);
                !self.pending_recv.parks_port(port)
            };
            if self
                .endpoint
                .pending_scope_ack_lane_mask(summary_lane_idx, scope_id, lane_idx)
                || self.endpoint.pending_scope_frame_hint_lane_mask(
                    summary_lane_idx,
                    lane_idx,
                    frame_label_meta,
                    drain_transport_hints,
                )
            {
                self.ingest_scope_evidence_for_lane(
                    lane_idx,
                    scope_id,
                    suppress_hint,
                    frame_label_meta,
                    drain_transport_hints,
                );
            }
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
    }

    fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        is_dynamic_scope: bool,
        is_route_controller: bool,
    ) -> bool {
        if self.endpoint.scope_ack_conflicted(scope_id) {
            return false;
        }
        if !(is_dynamic_scope || !is_route_controller) {
            return false;
        }
        if self.endpoint.scope_frame_hint_conflicted(scope_id) {
            self.endpoint.clear_scope_frame_hint_conflict(scope_id);
            return true;
        }
        false
    }

    pub(in crate::endpoint::kernel) fn cache_binding_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        binding_evidence: &mut Option<LaneIngressEvidence>,
    ) {
        if binding_evidence.is_some() {
            return;
        }
        if let Some((lane_idx, evidence)) = self.endpoint.poll_binding_for_offer(
            scope_id,
            offer_lane_idx,
            frame_label_meta,
            materialization_meta,
        ) {
            if binding_evidence.is_none() {
                *binding_evidence = Some(LaneIngressEvidence::new(lane_idx, evidence));
            } else {
                self.endpoint.put_back_binding_for_lane(lane_idx, evidence);
            }
        }
    }

    fn refresh_cached_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let Some(slot_idx) = active_entries.slot_for_entry(entry_idx) else {
            return false;
        };
        let Some(_entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let compare_len = observation_key.len();
        let mut compare_idx = 0usize;
        while compare_idx < compare_len {
            if compare_idx != slot_idx
                && cached_key.slot(compare_idx) != observation_key.slot(compare_idx)
            {
                return false;
            }
            compare_idx += 1;
        }
        if cached_key.slot(slot_idx) == observation_key.slot(slot_idx) {
            return true;
        }
        if !self
            .endpoint
            .recompute_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
            )
        {
            return false;
        }
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    #[inline]
    fn working_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let port = self.endpoint.port_for_lane(self.endpoint.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.endpoint.cursor.frontier_scratch_layout();
        let frontier_entry_capacity = self.endpoint.cursor.max_frontier_entries();
        let mut key = frontier_working_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        key.copy_from(cached_key);
        let mut observed = frontier_observed_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        observed.copy_from(cached_observed_entries);
        (key, observed)
    }

    fn refresh_shifted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let Some((old_slot_idx, new_slot_idx)) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_move(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        if !cached_observed_entries.move_entry_slot(entry_idx, new_slot_idx) {
            return false;
        }
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::move_slot_in_array(
            &mut cached_key.slots,
            active_entries.len(),
            old_slot_idx,
            new_slot_idx,
        );
        if !cached_key.entries_equal(&observation_key) {
            return false;
        }
        if cached_key.slot(new_slot_idx) != observation_key.slot(new_slot_idx) {
            let Some(observed) = self
                .endpoint
                .offer_entry_observed_state_cached(entry_idx)
                .or_else(|| {
                    self.endpoint
                        .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                })
            else {
                return false;
            };
            if !self
                .endpoint
                .replace_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                    observed,
                )
            {
                return false;
            }
        }
        *cached_key.slot_mut(new_slot_idx) = observation_key.slot(new_slot_idx);
        if cached_key.slots != observation_key.slots {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_inserted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(insert_slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_insert(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let active_offer_lanes = self.endpoint.route_state.active_offer_lanes();
        let mut changed_external_lane = false;
        let mut check_lane = |lane_idx: usize| {
            let entry_owns_lane = active_offer_lanes.contains(lane_idx)
                && state_index_to_usize(self.endpoint.route_state.lane_offer_state(lane_idx).entry)
                    == entry_idx;
            if !entry_owns_lane
                && (cached_key.offer_lanes().contains(lane_idx)
                    != observation_key.offer_lanes().contains(lane_idx)
                    || cached_key.binding_nonempty_lanes().contains(lane_idx)
                        != observation_key.binding_nonempty_lanes().contains(lane_idx))
            {
                changed_external_lane = true;
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(observation_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        if changed_external_lane {
            return false;
        }
        let len = cached_observed_entries.len();
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::insert_slot_in_array(
            &mut cached_key.slots,
            len,
            insert_slot_idx,
            FrontierObservationSlot {
                entry,
                meta: observation_key.slot(insert_slot_idx),
            },
        );
        cached_key.set_offer_lanes(observation_key.offer_lanes());
        cached_key.set_binding_nonempty_lanes(observation_key.binding_nonempty_lanes());
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let Some(observed) = self
            .endpoint
            .offer_entry_observed_state_cached(entry_idx)
            .or_else(|| {
                self.endpoint
                    .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            })
        else {
            return false;
        };
        if !cached_observed_entries.insert_observation_at_slot_with_frontier_mask(
            entry_idx,
            insert_slot_idx,
            FrontierObservationSlot {
                entry,
                meta: observation_key.slot(insert_slot_idx),
            },
            observed,
            self.endpoint
                .offer_entry_frontier_mask(entry_idx, entry_state),
        ) {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_removed_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(removed_slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_remove(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let slot_masks = Self::frontier_observation_offer_lane_entry_slot_masks(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let mut changed_slotted_lane = false;
        let mut check_lane = |lane_idx: usize| {
            if (cached_key.offer_lanes().contains(lane_idx)
                != observation_key.offer_lanes().contains(lane_idx)
                || cached_key.binding_nonempty_lanes().contains(lane_idx)
                    != observation_key.binding_nonempty_lanes().contains(lane_idx))
                && slot_masks[lane_idx] != 0
            {
                changed_slotted_lane = true;
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(observation_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        if changed_slotted_lane {
            return false;
        }
        if !cached_observed_entries.remove_observation(entry_idx) {
            return false;
        }
        let cached_len = cached_key.len();
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::remove_slot_from_array(
            &mut cached_key.slots,
            cached_len,
            removed_slot_idx,
            FrontierObservationSlot::EMPTY,
        );
        cached_key.set_offer_lanes(observation_key.offer_lanes());
        cached_key.set_binding_nonempty_lanes(observation_key.binding_nonempty_lanes());
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_replaced_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let Some((slot_idx, old_entry_idx, new_entry_idx)) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_replace(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let Some(observed) = self
            .endpoint
            .offer_entry_observed_state_cached(new_entry_idx)
            .or_else(|| {
                self.endpoint
                    .recompute_offer_entry_observed_state_non_consuming(new_entry_idx)
            })
        else {
            return false;
        };
        let Some(new_entry_state) = self.endpoint.offer_entry_state_snapshot(new_entry_idx) else {
            return false;
        };
        if !cached_observed_entries.replace_entry_at_slot_with_frontier_mask(
            old_entry_idx,
            new_entry_idx,
            FrontierObservationSlot {
                entry: observation_key.entry_state(slot_idx),
                meta: observation_key.slot(slot_idx),
            },
            observed,
            self.endpoint
                .offer_entry_frontier_mask(new_entry_idx, new_entry_state),
        ) {
            return false;
        }
        cached_key.slots[slot_idx].entry = observation_key.entry_state(slot_idx);
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_cached_frontier_observation_scope_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        scope_id: ScopeId,
    ) {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if !cached_key.lane_sets_equal(&observation_key) {
            return;
        }
        let scope_generation = self.endpoint.scope_evidence_generation_for_scope(scope_id);
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || self.endpoint.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            if cached_key.slot(slot_idx).scope_generation == scope_generation {
                continue;
            }
            let summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
            {
                return;
            }
            let Some(lane_idx) = self
                .endpoint
                .offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                return;
            };
            let route_change_epoch = self
                .endpoint
                .ports
                .get(lane_idx)
                .and_then(Option::as_ref)
                .map(|port| port.route_change_epoch())
                .unwrap_or(0);
            if cached_key.slot(slot_idx).route_change_epoch != route_change_epoch {
                return;
            }
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                )
            {
                return;
            }
            cached_key.slot_mut(slot_idx).scope_generation = scope_generation;
            patched = true;
        }
        if !patched {
            return;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    fn refresh_cached_frontier_observation_binding_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        if previous_nonempty
            == self
                .endpoint
                .binding_inbox
                .nonempty_lanes()
                .contains(lane_idx)
        {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        if !observation_key.offer_lanes().contains(lane_idx) {
            return;
        }
        if !cached_key
            .offer_lanes()
            .equals_until(observation_key.offer_lanes(), lane_limit)
            || !cached_key
                .binding_nonempty_lanes()
                .equals_until_except_lane(
                    observation_key.binding_nonempty_lanes(),
                    lane_limit,
                    lane_idx,
                )
        {
            return;
        }
        let mut affected_slot_mask = Self::frontier_observation_offer_lane_entry_slot_masks(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        )[lane_idx];
        if affected_slot_mask == 0 {
            return;
        }
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut affected_slot_mask,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return;
            };
            let summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || cached_key.slot(slot_idx).entry_summary_fingerprint
                    != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self.endpoint.scope_evidence_generation_for_scope(
                        self.endpoint.offer_entry_scope_id(entry_idx, entry_state),
                    )
            {
                return;
            }
            let Some(representative_lane) = self
                .endpoint
                .offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                return;
            };
            let route_change_epoch = self
                .endpoint
                .ports
                .get(representative_lane)
                .and_then(Option::as_ref)
                .map(|port| port.route_change_epoch())
                .unwrap_or(0);
            if cached_key.slot(slot_idx).route_change_epoch != route_change_epoch {
                return;
            }
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                )
            {
                return;
            }
        }
        cached_key.set_binding_nonempty_lanes(observation_key.binding_nonempty_lanes());
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    fn refresh_cached_frontier_observation_route_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        let route_change_epoch = self
            .endpoint
            .ports
            .get(lane_idx)
            .and_then(Option::as_ref)
            .map(|port| port.route_change_epoch())
            .unwrap_or(0);
        if route_change_epoch == previous_change_epoch {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if !cached_key.lane_sets_equal(&observation_key) {
            return;
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || self
                    .endpoint
                    .offer_entry_representative_lane_idx(entry_idx, entry_state)
                    != Some(lane_idx)
            {
                continue;
            }
            let summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self.endpoint.scope_evidence_generation_for_scope(
                        self.endpoint.offer_entry_scope_id(entry_idx, entry_state),
                    )
            {
                return;
            }
            if cached_key.slot(slot_idx).route_change_epoch == route_change_epoch {
                continue;
            }
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                )
            {
                return;
            }
            cached_key.slot_mut(slot_idx).route_change_epoch = route_change_epoch;
            patched = true;
        }
        if !patched {
            return;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    fn refresh_frontier_observation_cache_for_scope(&mut self, scope_id: ScopeId) {
        let global_active_entries = self.endpoint.global_active_entries();
        let mut active_entries = global_active_entries.occupancy_mask();
        let mut frontier_scratch = self.endpoint.frontier_scratch_view();
        let roots = frontier_scratch.root_scopes_mut();
        roots.fill(ScopeId::none());
        let mut root_len = 0usize;
        let mut matches_scope = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut active_entries,
            )
        {
            let Some(entry_idx) = global_active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || self.endpoint.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            matches_scope = true;
            let Some(parallel_root) = self
                .endpoint
                .offer_entry_parallel_root_from_state(entry_idx, entry_state)
            else {
                continue;
            };
            let mut seen_root = false;
            let mut idx = 0usize;
            while idx < root_len {
                if roots[idx] == parallel_root {
                    seen_root = true;
                    break;
                }
                idx += 1;
            }
            if !seen_root && root_len < roots.len() {
                roots[root_len] = parallel_root;
                root_len += 1;
            }
        }
        if !matches_scope {
            return;
        }
        self.refresh_cached_frontier_observation_scope_entries(ScopeId::none(), false, scope_id);
        let mut idx = 0usize;
        while idx < root_len {
            self.refresh_cached_frontier_observation_scope_entries(roots[idx], true, scope_id);
            idx += 1;
        }
    }

    fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        self.refresh_cached_frontier_observation_binding_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_nonempty,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.endpoint.frontier_state.root_frontier_len() {
            let root = self.endpoint.frontier_state.root_frontier_state[slot_idx].root;
            if Self::frontier_observation_offer_lane_entry_slot_masks(self.endpoint, root, true)
                [lane_idx]
                != 0
            {
                self.refresh_cached_frontier_observation_binding_lane_entries(
                    root,
                    true,
                    lane_idx,
                    previous_nonempty,
                );
            }
            slot_idx += 1;
        }
    }

    fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        self.refresh_cached_frontier_observation_route_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_change_epoch,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.endpoint.frontier_state.root_frontier_len() {
            let root = self.endpoint.frontier_state.root_frontier_state[slot_idx].root;
            self.refresh_cached_frontier_observation_route_lane_entries(
                root,
                true,
                lane_idx,
                previous_change_epoch,
            );
            slot_idx += 1;
        }
    }

    fn cached_frontier_changed_entry_slot_mask(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.entries_equal(&observation_key)
        {
            return None;
        }
        let mut changed_slot_mask = 0u8;
        let slot_len = observation_key.len();
        let mut slot_idx = 0usize;
        while slot_idx < slot_len {
            if cached_key.slot(slot_idx) != observation_key.slot(slot_idx) {
                changed_slot_mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        let slot_masks = Self::frontier_observation_offer_lane_entry_slot_masks(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let mut mark_changed_lane = |lane_idx: usize| {
            if cached_key.offer_lanes().contains(lane_idx)
                != observation_key.offer_lanes().contains(lane_idx)
                || cached_key.binding_nonempty_lanes().contains(lane_idx)
                    != observation_key.binding_nonempty_lanes().contains(lane_idx)
            {
                changed_slot_mask |= slot_masks[lane_idx];
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut mark_changed_lane);
        Self::for_each_set_lane(
            observation_key.offer_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Some(changed_slot_mask)
    }

    fn refresh_frontier_observed_entries_from_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        let mut changed_slot_mask = self.cached_frontier_changed_entry_slot_mask(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            cached_key,
        )?;
        if changed_slot_mask == 0 {
            return Some(cached_observed_entries);
        }
        let mut refreshed = self.endpoint.empty_observed_entries_scratch();
        refreshed.copy_from(cached_observed_entries);
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut changed_slot_mask,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(&mut refreshed, entry_idx)
            {
                return None;
            }
        }
        Some(refreshed)
    }

    fn compose_frontier_observed_entries(
        &mut self,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        let mut composed = self.endpoint.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
                continue;
            }
            let observed = Self::cached_offer_entry_observed_state_for_rebuild(
                self.endpoint,
                entry_idx,
                &entry_state,
                observation_key,
                cached_key,
                cached_observed_entries,
            )
            .or_else(|| self.endpoint.offer_entry_observed_state_cached(entry_idx))
            .or_else(|| {
                self.endpoint
                    .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            });
            let Some(observed) = observed else {
                continue;
            };
            let Some((observed_bit, _)) = composed.insert_entry(entry_idx) else {
                continue;
            };
            composed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.endpoint
                    .offer_entry_frontier_mask(entry_idx, entry_state),
            );
        }
        composed
    }

    fn refresh_frontier_observed_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        if let Some(refreshed) = self.refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) {
            return refreshed;
        }
        self.compose_frontier_observed_entries(
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    fn refresh_frontier_observation_cache_from_cached_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let Some(observed_entries) = self.refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) else {
            return false;
        };
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            observed_entries,
        );
        true
    }

    fn refresh_frontier_observation_cache_impl(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if self.refresh_structural_frontier_observation_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            cached_key,
        ) {
            return;
        }
        let observed_entries = self.refresh_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        );
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            observed_entries,
        );
    }

    fn refresh_structural_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let active_len = active_entries.len();
        let cached_len = cached_key.len();
        if active_len == cached_len {
            if let Some(entry_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_replaced_entry_idx(
                    active_entries,
                    cached_key,
                )
                && self.refresh_replaced_frontier_observation_entry(
                    current_parallel_root,
                    use_root_observed_entries,
                    entry_idx,
                )
            {
                return true;
            }
            if CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_shifted_entry_idx(
                active_entries,
                cached_key,
            )
            .is_some()
            {
                let mut remaining_slots = active_entries.occupancy_mask();
                while let Some(slot_idx) =
                    CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                        &mut remaining_slots,
                    )
                {
                    let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                        continue;
                    };
                    if active_entries.entry_state(slot_idx) == cached_key.entry_state(slot_idx) {
                        continue;
                    }
                    if self.refresh_shifted_frontier_observation_entry(
                        current_parallel_root,
                        use_root_observed_entries,
                        entry_idx,
                    ) {
                        return true;
                    }
                }
            }
            if CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::same_active_entry_set(
                active_entries,
                cached_key,
            ) && self.refresh_permuted_frontier_observation_entries(
                current_parallel_root,
                use_root_observed_entries,
                active_entries,
            ) {
                return true;
            }
            if self.refresh_multi_replaced_frontier_observation_entries(
                current_parallel_root,
                use_root_observed_entries,
                active_entries,
            ) {
                return true;
            }
            return false;
        }
        if active_len + 1 == cached_len
            && let Some(entry_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_removed_entry_idx(
                    active_entries,
                    cached_key,
                )
            && self.refresh_removed_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        if active_len == cached_len + 1
            && let Some(entry_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_inserted_entry_idx(
                    active_entries,
                    cached_key,
                )
            && self.refresh_inserted_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        false
    }

    fn refresh_permuted_frontier_observation_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if cached_key == FrontierObservationKey::EMPTY
            || !CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::same_active_entry_set(
                active_entries,
                cached_key,
            )
        {
            return false;
        }
        let mut refreshed = self.endpoint.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
                return false;
            }
            let observed = Self::cached_offer_entry_observed_state_for_rebuild(
                self.endpoint,
                entry_idx,
                &entry_state,
                observation_key,
                cached_key,
                cached_observed_entries,
            )
            .or_else(|| self.endpoint.offer_entry_observed_state_cached(entry_idx))
            .or_else(|| {
                self.endpoint
                    .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            });
            let Some(observed) = observed else {
                return false;
            };
            let Some((observed_bit, _)) = refreshed.insert_entry(entry_idx) else {
                return false;
            };
            refreshed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.endpoint
                    .offer_entry_frontier_mask(entry_idx, entry_state),
            );
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            refreshed,
        );
        true
    }

    fn refresh_multi_replaced_frontier_observation_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let active_len = active_entries.len();
        if active_len == 0
            || active_len != cached_key.len()
            || CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::same_active_entry_set(
                active_entries,
                cached_key,
            )
        {
            return false;
        }
        let mut refreshed = self.endpoint.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut reused_cached = false;
        let mut recomputed = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
                return false;
            }
            let observed = if let Some(observed) =
                Self::cached_offer_entry_observed_state_for_rebuild(
                    self.endpoint,
                    entry_idx,
                    &entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                ) {
                reused_cached = true;
                observed
            } else if let Some(observed) =
                self.endpoint.offer_entry_observed_state_cached(entry_idx)
            {
                reused_cached = true;
                observed
            } else {
                recomputed = true;
                let Some(observed) = self
                    .endpoint
                    .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                else {
                    return false;
                };
                observed
            };
            let Some((observed_bit, _)) = refreshed.insert_entry(entry_idx) else {
                return false;
            };
            refreshed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.endpoint
                    .offer_entry_frontier_mask(entry_idx, entry_state),
            );
        }
        if !reused_cached || !recomputed {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            refreshed,
        );
        true
    }

    fn refresh_frontier_observation_cache_for_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) {
        let (cached_key, _) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if cached_key == FrontierObservationKey::EMPTY {
            self.refresh_frontier_observation_cache_impl(
                current_parallel_root,
                use_root_observed_entries,
            );
            return;
        }
        if self.refresh_cached_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_frontier_observation_cache_from_cached_entries(
            current_parallel_root,
            use_root_observed_entries,
        ) || self.refresh_replaced_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_removed_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_inserted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_shifted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) {
            return;
        }
        self.refresh_frontier_observation_cache_impl(
            current_parallel_root,
            use_root_observed_entries,
        );
    }

    fn refresh_frontier_observation_caches_for_entry(
        &mut self,
        entry_idx: usize,
        previous_root: ScopeId,
        current_root: ScopeId,
    ) {
        self.refresh_frontier_observation_cache_for_entry(ScopeId::none(), false, entry_idx);
        if !previous_root.is_none() {
            self.refresh_frontier_observation_cache_for_entry(previous_root, true, entry_idx);
        }
        if !current_root.is_none() && current_root != previous_root {
            self.refresh_frontier_observation_cache_for_entry(current_root, true, entry_idx);
        }
    }

    fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return;
        };
        let previous_root = self
            .endpoint
            .offer_entry_parallel_root_from_state(entry_idx, entry_state)
            .unwrap_or(ScopeId::none());
        if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
            #[cfg(test)]
            self.endpoint
                .frontier_state
                .clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        self.endpoint
            .detach_offer_entry_from_root_frontier(entry_idx, previous_root);
        Self::ensure_global_frontier_scratch_initialized(self.endpoint);
        let mut global_active_entries = self.endpoint.global_active_entries();
        global_active_entries.remove_entry(entry_idx);
        let Some(lane_idx) = self
            .endpoint
            .offer_entry_representative_lane_idx(entry_idx, entry_state)
        else {
            #[cfg(test)]
            self.endpoint
                .frontier_state
                .clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        };
        let info = self.endpoint.route_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            #[cfg(test)]
            self.endpoint
                .frontier_state
                .clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        #[cfg(test)]
        let selection_meta = self.endpoint.compute_offer_entry_selection_meta(
            info.scope,
            info,
            !self
                .endpoint
                .offer_lane_set_for_scope(info.scope)
                .is_empty(),
        );
        #[cfg(test)]
        let loop_meta = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_loop_meta_at(
            &self.endpoint.cursor,
            &self.endpoint.control_semantics(),
            info.scope,
            entry_idx,
        );
        #[cfg(test)]
        let frame_label_meta =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_meta_at(
                &self.endpoint.cursor,
                &self.endpoint.control_semantics(),
                info.scope,
                loop_meta,
                entry_idx,
            );
        #[cfg(test)]
        let materialization_meta = self
            .endpoint
            .compute_scope_arm_materialization_meta(info.scope);
        #[cfg(test)]
        let test_summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
        #[cfg(test)]
        if let Some(state) = self
            .endpoint
            .frontier_state
            .offer_entry_state_mut(entry_idx)
        {
            state.lane_idx = lane_idx as u8;
            state.parallel_root = info.parallel_root;
            state.frontier = info.frontier;
            state.scope_id = info.scope;
            state.selection_meta = selection_meta;
            state.frame_label_meta = frame_label_meta;
            state.materialization_meta = materialization_meta;
            state.summary = test_summary;
        }
        Self::ensure_global_frontier_scratch_initialized(self.endpoint);
        let mut global_active_entries = self.endpoint.global_active_entries();
        global_active_entries.insert_entry(entry_idx, lane_idx as u8);
        self.endpoint.attach_offer_entry_to_root_frontier(
            entry_idx,
            info.parallel_root,
            lane_idx as u8,
        );
        #[cfg(test)]
        let observed = self
            .endpoint
            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            .unwrap_or_else(|| {
                unreachable!("test observed state must recompute for active offer entry")
            });
        #[cfg(test)]
        self.endpoint
            .frontier_state
            .set_offer_entry_observed(entry_idx, observed);
        #[cfg(not(test))]
        let _ = self
            .endpoint
            .recompute_offer_entry_observed_state_non_consuming(entry_idx);
        self.refresh_frontier_observation_caches_for_entry(
            entry_idx,
            previous_root,
            info.parallel_root,
        );
    }

    fn detach_lane_from_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let remaining_active_lanes = self
            .endpoint
            .offer_entry_has_other_active_lanes(entry_idx, lane_idx);
        if !remaining_active_lanes {
            let parallel_root = if info.parallel_root.is_none() {
                self.endpoint
                    .offer_entry_state_snapshot(entry_idx)
                    .and_then(|entry_state| {
                        self.endpoint
                            .offer_entry_parallel_root_from_state(entry_idx, entry_state)
                    })
                    .unwrap_or(ScopeId::none())
            } else {
                info.parallel_root
            };
            self.endpoint
                .detach_offer_entry_from_root_frontier(entry_idx, parallel_root);
            Self::ensure_global_frontier_scratch_initialized(self.endpoint);
            let mut global_active_entries = self.endpoint.global_active_entries();
            global_active_entries.remove_entry(entry_idx);
            #[cfg(test)]
            self.endpoint
                .frontier_state
                .clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                parallel_root,
                ScopeId::none(),
            );
            return;
        }
        let Some(_entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return;
        };
        self.refresh_offer_entry_state(entry_idx);
    }

    fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        #[cfg(not(test))]
        let was_inactive = !self.endpoint.offer_entry_has_active_lanes(entry_idx);
        #[cfg(test)]
        let was_inactive = self
            .endpoint
            .global_active_entries()
            .slot_for_entry(entry_idx)
            .is_none();
        if was_inactive {
            Self::ensure_global_frontier_scratch_initialized(self.endpoint);
            let mut global_active_entries = self.endpoint.global_active_entries();
            global_active_entries.insert_entry(entry_idx, lane_idx as u8);
            self.endpoint.attach_offer_entry_to_root_frontier(
                entry_idx,
                info.parallel_root,
                lane_idx as u8,
            );
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        let old = self.endpoint.route_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.endpoint.detach_lane_from_root_frontier(lane_idx, old);
    }

    fn sync_lane_offer_state(&mut self) {
        let logical_lane_count = self.endpoint.cursor.logical_lane_count();
        let active_offer_lanes = self.endpoint.route_state.active_offer_lanes();
        Self::for_each_set_lane(active_offer_lanes, logical_lane_count, |lane_idx| {
            let needs_refresh = Self::offer_refresh_mask(self.endpoint, lane_idx);
            if !needs_refresh {
                self.clear_lane_offer_state(lane_idx);
            }
        });
        let current_phase_lanes = self.endpoint.cursor.current_phase_lane_set();
        Self::for_each_set_lane(current_phase_lanes, logical_lane_count, |lane_idx| {
            self.refresh_lane_offer_state(lane_idx);
        });
        let lane_linger_lanes = self.endpoint.route_state.lane_linger_lanes();
        Self::for_each_set_lane(lane_linger_lanes, logical_lane_count, |lane_idx| {
            self.refresh_lane_offer_state(lane_idx);
        });
        let lane_offer_linger_lanes = self.endpoint.route_state.lane_offer_linger_lanes();
        Self::for_each_set_lane(lane_offer_linger_lanes, logical_lane_count, |lane_idx| {
            self.refresh_lane_offer_state(lane_idx);
        });
    }

    fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        let old = self.endpoint.route_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.endpoint.detach_lane_from_root_frontier(lane_idx, old);
        if let Some(info) = self.endpoint.compute_lane_offer_state(lane_idx) {
            let is_linger = self.endpoint.is_linger_route(info.scope);
            self.endpoint
                .route_state
                .set_lane_offer_state(lane_idx, info, is_linger);
            self.endpoint.attach_lane_to_root_frontier(lane_idx, info);
            self.attach_lane_to_offer_entry(lane_idx, info);
        }
    }

    fn prepare_frontier_facts(
        &mut self,
        selection: OfferScopeSelection,
        frontier_visited: &mut FrontierVisitSet,
    ) -> RecvResult<OfferFrontierFacts> {
        let scope_id = selection.scope_id;
        frontier_visited.record(scope_id);
        let offer_lane = selection.offer_lane;
        let offer_lane_idx = selection.offer_lane_idx as usize;
        let at_route_offer_entry = selection.at_route_offer_entry;
        let loop_meta = self
            .endpoint
            .selection_frame_label_meta(selection)
            .loop_meta();

        let cursor_is_not_recv = !self.endpoint.cursor.is_recv();
        let is_route_controller = self.endpoint.cursor.is_route_controller(scope_id);
        let controller_selected_recv_step = is_route_controller
            && !at_route_offer_entry
            && self
                .endpoint
                .cursor
                .try_recv_meta()
                .map(|recv_meta| recv_meta.peer != ROLE)
                .unwrap_or(false);

        let is_dynamic_route_scope = self
            .endpoint
            .cursor
            .route_scope_controller_policy(scope_id)
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        let suppress_scope_frame_hint = is_dynamic_route_scope;
        let offer_lanes = self.endpoint.offer_lane_set_for_scope(scope_id);
        {
            let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
            self.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lane_idx,
                offer_lanes,
                suppress_scope_frame_hint,
                frame_label_meta,
            );
        }
        let preview_route_decision = self.endpoint.preview_scope_ack_token_non_consuming(
            scope_id,
            offer_lane_idx,
            offer_lanes,
        );
        let preview_ready_arm_evidence = self.endpoint.scope_has_ready_arm_evidence(scope_id);
        let recvless_loop_control_scope = !is_route_controller
            && !is_dynamic_route_scope
            && loop_meta.control_scope()
            && !loop_meta.arm_has_recv(0)
            && !loop_meta.arm_has_recv(1);

        let is_self_send_controller = cursor_is_not_recv
            && is_route_controller
            && !CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_has_controller_arm_entry(
                &self.endpoint.cursor,
                scope_id,
            );
        let controller_non_entry_cursor_ready = cursor_is_not_recv
            && is_route_controller
            && self.endpoint.controller_arm_at_cursor(scope_id).is_none();
        let early_route_decision = if is_route_controller {
            preview_route_decision
        } else {
            preview_route_decision
                .filter(|token| !self.endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
        };
        let early_decision_arm_has_no_recv = early_route_decision
            .map(|token| !self.endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
            .unwrap_or(false);
        let controller_pending_materialization = is_route_controller
            && self
                .endpoint
                .selected_arm_for_scope(scope_id)
                .map(|arm| {
                    self.endpoint
                        .arm_requires_materialization_ready_evidence(scope_id, arm)
                        && !self.endpoint.scope_has_ready_arm(scope_id, arm)
                })
                .unwrap_or(false);
        let controller_can_skip_recv = is_route_controller
            && !controller_pending_materialization
            && ((at_route_offer_entry
                && (is_dynamic_route_scope
                    || controller_non_entry_cursor_ready
                    || is_self_send_controller
                    || early_route_decision.is_some()))
                || (!at_route_offer_entry && cursor_is_not_recv));
        let passive_dynamic_scope_has_recv =
            self.endpoint.arm_has_recv(scope_id, 0) || self.endpoint.arm_has_recv(scope_id, 1);
        let passive_ack_is_materializable = self
            .endpoint
            .preview_scope_ack_token_non_consuming(scope_id, offer_lane_idx, offer_lanes)
            .map(|token| {
                let arm = token.arm().as_u8();
                self.endpoint.scope_has_ready_arm(scope_id, arm)
                    || !self.endpoint.arm_has_recv(scope_id, arm)
            })
            .unwrap_or(false);
        let passive_dynamic_can_skip_recv = !is_route_controller
            && is_dynamic_route_scope
            && (!passive_dynamic_scope_has_recv
                || preview_ready_arm_evidence
                || passive_ack_is_materializable);
        let skip_recv_loop = passive_dynamic_can_skip_recv
            || controller_can_skip_recv
            || early_decision_arm_has_no_recv;

        Ok(OfferFrontierFacts {
            selection,
            scope_id,
            offer_lane,
            offer_lane_idx,
            offer_lanes,
            suppress_scope_frame_hint,
            is_route_controller,
            is_dynamic_route_scope,
            recvless_loop_control_scope,
            controller_selected_recv_step,
            skip_recv_loop,
        })
    }

    fn poll_collect_offer_evidence(
        &mut self,
        state: &mut OfferCollectState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        let facts = state.facts;
        if state.binding_evidence.is_none() && state.transport_payload_len == 0 {
            let payload_view = if facts.skip_recv_loop {
                None
            } else {
                'offer_recv: loop {
                    if !facts.is_route_controller || facts.controller_selected_recv_step {
                        let frame_label_meta =
                            self.endpoint.selection_frame_label_meta(facts.selection);
                        let materialization_meta = self
                            .endpoint
                            .selection_materialization_meta(facts.selection);
                        if let Some((lane_idx, evidence)) = self.endpoint.poll_binding_for_offer(
                            facts.scope_id,
                            facts.offer_lane_idx,
                            frame_label_meta,
                            materialization_meta,
                        ) {
                            state.binding_evidence =
                                Some(LaneIngressEvidence::new(lane_idx, evidence));
                            break 'offer_recv None;
                        }
                        if facts.recvless_loop_control_scope
                            && let Some((lane_idx, evidence)) = self
                                .endpoint
                                .poll_binding_any_for_offer(facts.offer_lane_idx, facts.offer_lanes)
                        {
                            state.binding_evidence =
                                Some(LaneIngressEvidence::new(lane_idx, evidence));
                            break 'offer_recv None;
                        }
                    }

                    let payload = {
                        let port = self.endpoint.port_for_lane(facts.offer_lane_idx);
                        match lane_port::poll_recv(&mut self.pending_recv, port, cx) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Ok(payload)) => payload,
                            Poll::Ready(Err(err)) => {
                                return Poll::Ready(Err(RecvError::Transport(err)));
                            }
                        }
                    };

                    if !facts.is_route_controller || facts.controller_selected_recv_step {
                        let frame_label_meta =
                            self.endpoint.selection_frame_label_meta(facts.selection);
                        let materialization_meta = self
                            .endpoint
                            .selection_materialization_meta(facts.selection);
                        if let Some((lane_idx, evidence)) = self.endpoint.poll_binding_for_offer(
                            facts.scope_id,
                            facts.offer_lane_idx,
                            frame_label_meta,
                            materialization_meta,
                        ) {
                            state.binding_evidence =
                                Some(LaneIngressEvidence::new(lane_idx, evidence));
                            break 'offer_recv None;
                        }
                        if facts.recvless_loop_control_scope
                            && let Some((lane_idx, evidence)) = self
                                .endpoint
                                .poll_binding_any_for_offer(facts.offer_lane_idx, facts.offer_lanes)
                        {
                            state.binding_evidence =
                                Some(LaneIngressEvidence::new(lane_idx, evidence));
                            break 'offer_recv None;
                        }
                    }

                    break 'offer_recv Some(payload);
                }
            };
            if let Some(payload) = payload_view
                && !payload.as_bytes().is_empty()
            {
                state.transport_payload_len = payload.as_bytes().len();
                state.transport_payload_lane = facts.offer_lane;
                state.transport_payload = Some(payload);
            }
        }

        Poll::Ready(Ok(()))
    }

    fn poll_run(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>> {
        if self.frontier_visited.is_none() {
            let mut frontier_scratch = self.endpoint.frontier_scratch_view();
            self.frontier_visited = Some(super::frontier::frontier_visit_set_from_scratch(
                &mut frontier_scratch,
            ));
        }
        loop {
            if let Some(stage) = self.run_stage.take() {
                match stage {
                    OfferRunStage::CollectEvidence(mut stage) => {
                        match self.poll_collect_offer_evidence(&mut stage, cx) {
                            Poll::Pending => {
                                self.run_stage = Some(OfferRunStage::CollectEvidence(stage));
                                return Poll::Pending;
                            }
                            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                            Poll::Ready(Ok(())) => {
                                let scope_id = stage.facts.scope_id;
                                let offer_lane_idx = stage.facts.offer_lane_idx;
                                let suppress_scope_frame_hint =
                                    stage.facts.suppress_scope_frame_hint;
                                let is_route_controller = stage.facts.is_route_controller;
                                let is_dynamic_route_scope = stage.facts.is_dynamic_route_scope;
                                if let Some(evidence) = stage.binding_evidence.as_ref() {
                                    let frame_label_meta =
                                        self.endpoint.selection_frame_label_meta(stage.selection);
                                    self.ingest_binding_scope_evidence(
                                        scope_id,
                                        evidence.lane(),
                                        evidence.frame_label(),
                                        suppress_scope_frame_hint,
                                        frame_label_meta,
                                    );
                                }
                                {
                                    let frame_label_meta =
                                        self.endpoint.selection_frame_label_meta(stage.selection);
                                    self.ingest_scope_evidence_for_offer(
                                        scope_id,
                                        offer_lane_idx,
                                        self.endpoint.offer_lane_set_for_scope(scope_id),
                                        suppress_scope_frame_hint,
                                        frame_label_meta,
                                    );
                                }
                                if self.endpoint.scope_evidence_conflicted(scope_id)
                                    && !self.recover_scope_evidence_conflict(
                                        scope_id,
                                        is_dynamic_route_scope,
                                        is_route_controller,
                                    )
                                {
                                    return Poll::Ready(Err(RecvError::PhaseInvariant));
                                }
                                self.run_stage =
                                    Some(OfferRunStage::ResolveToken(OfferResolveState {
                                        selection: stage.selection,
                                        facts: stage.facts,
                                        binding_evidence: stage.binding_evidence,
                                        transport_payload_len: stage.transport_payload_len,
                                        transport_payload_lane: stage.transport_payload_lane,
                                        transport_payload: stage.transport_payload,
                                        liveness: OfferLivenessState::new(
                                            self.endpoint.liveness_policy,
                                        ),
                                        pending_action: None,
                                        yield_armed: false,
                                    }));
                                continue;
                            }
                        }
                    }
                    OfferRunStage::ResolveToken(mut stage) => {
                        let mut frontier_visited = self
                            .frontier_visited
                            .take()
                            .expect("offer frontier state must be initialized before polling");
                        let resolved =
                            match self.resolve_token(&mut stage, &mut frontier_visited, cx) {
                                Poll::Pending => {
                                    self.frontier_visited = Some(frontier_visited);
                                    self.run_stage = Some(OfferRunStage::ResolveToken(stage));
                                    return Poll::Pending;
                                }
                                Poll::Ready(Err(err)) => {
                                    self.frontier_visited = Some(frontier_visited);
                                    return Poll::Ready(Err(err));
                                }
                                Poll::Ready(Ok(resolved)) => {
                                    self.frontier_visited = Some(frontier_visited);
                                    resolved
                                }
                            };
                        match resolved {
                            ResolveTokenOutcome::RestartFrontier => {
                                self.carried_binding_evidence = stage.binding_evidence;
                                self.carried_transport_payload =
                                    stage.transport_payload.map(|payload| {
                                        (
                                            stage.transport_payload_len,
                                            stage.transport_payload_lane,
                                            payload,
                                        )
                                    });
                                continue;
                            }
                            ResolveTokenOutcome::Resolved(resolved) => {
                                if !stage.facts.is_route_controller {
                                    match self
                                        .endpoint
                                        .descend_selected_passive_route(stage.selection, resolved)
                                    {
                                        Ok(true) => {
                                            self.carried_binding_evidence = stage.binding_evidence;
                                            self.carried_transport_payload =
                                                stage.transport_payload.map(|payload| {
                                                    (
                                                        stage.transport_payload_len,
                                                        stage.transport_payload_lane,
                                                        payload,
                                                    )
                                                });
                                            continue;
                                        }
                                        Ok(false) => {}
                                        Err(err) => return Poll::Ready(Err(err)),
                                    }
                                }
                                return Poll::Ready(self.materialize_branch(
                                    stage.selection,
                                    resolved,
                                    stage.facts.is_route_controller,
                                    stage.binding_evidence,
                                    stage.transport_payload_len,
                                    stage.transport_payload_lane,
                                    stage.transport_payload,
                                ));
                            }
                        }
                    }
                }
            }

            let selection = match self.select_scope() {
                Ok(selection) => selection,
                Err(err) => return Poll::Ready(Err(err)),
            };
            let facts = {
                let mut frontier_visited = self
                    .frontier_visited
                    .take()
                    .expect("offer frontier state must be initialized before selection");
                let facts = match self.prepare_frontier_facts(selection, &mut frontier_visited) {
                    Ok(facts) => facts,
                    Err(err) => {
                        self.frontier_visited = Some(frontier_visited);
                        return Poll::Ready(Err(err));
                    }
                };
                self.frontier_visited = Some(frontier_visited);
                facts
            };
            let (transport_payload_len, transport_payload_lane, transport_payload) = self
                .carried_transport_payload
                .take()
                .unwrap_or((0, facts.offer_lane, Payload::new(&[])));
            self.run_stage = Some(OfferRunStage::CollectEvidence(OfferCollectState {
                selection,
                facts,
                binding_evidence: self.carried_binding_evidence.take(),
                transport_payload_len,
                transport_payload_lane,
                transport_payload: (transport_payload_len != 0).then_some(transport_payload),
            }));
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(crate) fn poll_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<crate::endpoint::kernel::MaterializedRouteBranch<'r>>> {
        let mut machine = RouteFrontierMachine {
            endpoint: self,
            frontier_visited: state.frontier_visited.take(),
            carried_binding_evidence: state.carried_binding_evidence.take(),
            carried_transport_payload: state.carried_transport_payload.take(),
            run_stage: state.run_stage.take(),
            pending_recv: core::mem::replace(
                &mut state.pending_recv,
                lane_port::PendingRecv::new(),
            ),
        };
        let poll = machine.poll_run(cx).map(|result| result.map(Into::into));
        state.frontier_visited = machine.frontier_visited.take();
        state.carried_binding_evidence = machine.carried_binding_evidence.take();
        state.carried_transport_payload = machine.carried_transport_payload.take();
        state.run_stage = machine.run_stage.take();
        state.pending_recv = machine.pending_recv;
        poll
    }

    pub(in crate::endpoint::kernel) fn preflight_branch_preview_commit_plan(
        &mut self,
        branch: BranchPreviewView,
    ) -> RecvResult<BranchCommitPlan> {
        RouteFrontierMachine::new(self).preflight_route_branch_commit(branch)
    }

    pub(in crate::endpoint::kernel) fn publish_branch_preview_commit_plan(
        &mut self,
        plan: BranchCommitPlan,
    ) -> Option<RecvMeta> {
        RouteFrontierMachine::new(self).publish_route_branch_commit_plan(plan)
    }

    pub(in crate::endpoint::kernel) fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).ingest_binding_scope_evidence(
            scope_id,
            lane,
            frame_label,
            suppress_hint,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_offer_lanes(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).ingest_scope_evidence_for_offer(
            scope_id,
            summary_lane_idx,
            offer_lanes,
            suppress_hint,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        is_dynamic_scope: bool,
        is_route_controller: bool,
    ) -> bool {
        RouteFrontierMachine::new(self).recover_scope_evidence_conflict(
            scope_id,
            is_dynamic_scope,
            is_route_controller,
        )
    }

    pub(in crate::endpoint::kernel) fn cache_binding_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        binding_evidence: &mut Option<LaneIngressEvidence>,
    ) {
        RouteFrontierMachine::new(self).cache_binding_evidence_for_offer(
            scope_id,
            offer_lane_idx,
            frame_label_meta,
            materialization_meta,
            binding_evidence,
        )
    }

    pub(in crate::endpoint::kernel) fn record_scope_ack(
        &mut self,
        scope_id: ScopeId,
        token: RouteDecisionToken,
    ) {
        RouteFrontierMachine::new(self).record_scope_ack(scope_id, token)
    }

    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        RouteFrontierMachine::new(self).mark_scope_ready_arm(scope_id, arm)
    }

    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).mark_scope_ready_arm_from_frame_label(
            scope_id,
            lane,
            frame_label,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_binding_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).mark_scope_ready_arm_from_binding_frame_label(
            scope_id,
            lane,
            frame_label,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_scope(
        &mut self,
        scope_id: ScopeId,
    ) {
        RouteFrontierMachine::new(self).refresh_frontier_observation_cache_for_scope(scope_id)
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        RouteFrontierMachine::new(self)
            .refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty)
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        RouteFrontierMachine::new(self)
            .refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch)
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn cached_frontier_changed_entry_slot_mask(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        RouteFrontierMachine::new(self).cached_frontier_changed_entry_slot_mask(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            cached_key,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn refresh_frontier_observed_entries_from_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        RouteFrontierMachine::new(self).refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn refresh_cached_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        RouteFrontierMachine::new(self).refresh_cached_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        )
    }

    #[cfg(test)]
    pub(super) fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        RouteFrontierMachine::new(self).clear_lane_offer_state(lane_idx)
    }

    pub(crate) fn sync_lane_offer_state(&mut self) {
        RouteFrontierMachine::new(self).sync_lane_offer_state()
    }

    pub(super) fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        RouteFrontierMachine::new(self).refresh_lane_offer_state(lane_idx)
    }
}
