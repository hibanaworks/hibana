//! Offer-path helpers for scope selection and branch materialization.

use super::authority::{Arm, RouteDecisionSource, RouteDecisionToken};
use super::core::{CursorEndpoint, RouteBranch};
use super::evidence::ScopeLabelMeta;
use super::frontier::{
    ActiveEntrySet, FrontierKind, FrontierObservationKey, FrontierObservationSlot, LaneOfferState,
    ObservedEntrySet, checked_state_index,
};
#[cfg(test)]
use super::frontier::{FrontierCandidate, OfferEntryState};
use super::lane_port;
use crate::binding::BindingSlot;
use crate::control::cap::mint::{CapShot, EpochTable, MintConfigMarker};
use crate::eff::EffIndex;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::{PolicyMode, ScopeId};
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{
    ARM_SHARED, MAX_FIRST_RECV_DISPATCH, RecvMeta, StateIndex, state_index_to_usize,
};
use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::transport::Transport;

#[derive(Clone, Copy)]
pub(super) struct OfferScopeSelection {
    pub(super) scope_id: ScopeId,
    pub(super) frontier_parallel_root: Option<ScopeId>,
    pub(super) offer_lanes: [u8; MAX_LANES],
    pub(super) offer_lane_mask: u8,
    pub(super) offer_lanes_len: u8,
    pub(super) offer_lane: u8,
    pub(super) offer_lane_idx: u8,
    pub(super) at_route_offer_entry: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CachedRecvMeta {
    pub(super) cursor_index: StateIndex,
    pub(super) eff_index: EffIndex,
    pub(super) peer: u8,
    pub(super) label: u8,
    pub(super) resource: Option<u8>,
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
        resource: None,
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
                resource: self.resource,
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
    pub(super) arm_count: u8,
    pub(super) controller_arm_entry: [StateIndex; 2],
    pub(super) controller_arm_label: [u8; 2],
    pub(super) controller_recv_mask: u8,
    pub(super) controller_cross_role_recv_mask: u8,
    pub(super) recv_entry: [StateIndex; 2],
    pub(super) passive_arm_entry: [StateIndex; 2],
    pub(super) passive_arm_scope: [ScopeId; 2],
    pub(super) binding_demux_lane_mask: [u8; 2],
    pub(super) first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub(super) first_recv_len: u8,
}

impl ScopeArmMaterializationMeta {
    pub(super) const EMPTY: Self = Self {
        arm_count: 0,
        controller_arm_entry: [StateIndex::MAX; 2],
        controller_arm_label: [0; 2],
        controller_recv_mask: 0,
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::MAX; 2],
        passive_arm_entry: [StateIndex::MAX; 2],
        passive_arm_scope: [ScopeId::none(); 2],
        binding_demux_lane_mask: [0; 2],
        first_recv_dispatch: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
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
    pub(super) fn record_binding_demux_lane(&mut self, arm: u8, lane: u8) {
        let bit = 1u8 << (lane as usize);
        if arm == ARM_SHARED {
            self.binding_demux_lane_mask[0] |= bit;
            self.binding_demux_lane_mask[1] |= bit;
            return;
        }
        let arm = arm as usize;
        if arm < self.binding_demux_lane_mask.len() {
            self.binding_demux_lane_mask[arm] |= bit;
        }
    }

    #[inline]
    pub(super) fn binding_demux_lane_mask(&self, preferred_arm: Option<u8>) -> u8 {
        preferred_arm
            .and_then(|arm| self.binding_demux_lane_mask.get(arm as usize).copied())
            .unwrap_or(self.binding_demux_lane_mask[0] | self.binding_demux_lane_mask[1])
    }

    #[inline]
    pub(super) fn binding_demux_lane_mask_for_label_mask(
        &self,
        label_meta: ScopeLabelMeta,
        label_mask: u128,
    ) -> u8 {
        if label_mask == 0 {
            return 0;
        }
        let mut lane_mask = 0u8;
        let mut arm = 0u8;
        while arm <= 1 {
            if (label_meta.binding_demux_label_mask_for_arm(arm) & label_mask) != 0 {
                lane_mask |= self.binding_demux_lane_mask(Some(arm));
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        if lane_mask != 0 {
            lane_mask
        } else {
            self.binding_demux_lane_mask(None)
        }
    }

    #[inline]
    pub(super) fn first_recv_target(&self, label: u8) -> Option<(u8, StateIndex)> {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (entry_label, arm, target) = self.first_recv_dispatch[idx];
            if entry_label == label && !target.is_max() {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn arm_has_first_recv_dispatch(&self, arm: u8) -> bool {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (_label, dispatch_arm, target) = self.first_recv_dispatch[idx];
            if !target.is_max() && (dispatch_arm == arm || dispatch_arm == ARM_SHARED) {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(super) fn controller_arm_is_recv(&self, arm: u8) -> bool {
        arm < 2 && (self.controller_recv_mask & (1u8 << arm)) != 0
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
    pub(super) resolved_label_hint: Option<u8>,
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
    /// Classification of the branch for decode() dispatch.
    pub(crate) kind: BranchKind,
    /// Route decision source used when commit emits route-decision events.
    pub(crate) route_source: RouteDecisionSource,
}

/// Classification of branch types for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/binding.
    WireRecv,
    /// Synthetic local control: CanonicalControl self-send that doesn't go on wire.
    /// Decode from zero buffer; scope settlement uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should continue on the same borrowed endpoint with `flow().send()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
}

struct RouteFrontierMachine<
    'endpoint,
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
    C: Clock,
    Mint: MintConfigMarker,
{
    endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
}

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    const fn new(
        endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> Self {
        Self { endpoint }
    }

    #[inline]
    fn take_pending_branch_preview(
        &mut self,
    ) -> Option<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        self.endpoint.take_pending_branch_preview()
    }

    #[inline]
    fn select_scope(&mut self) -> RecvResult<OfferScopeSelection> {
        self.endpoint.select_scope()
    }

    #[inline]
    async fn resolve_token(
        &mut self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        is_dynamic_route_scope: bool,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
        frontier_visited: &mut super::frontier::FrontierVisitSet,
    ) -> RecvResult<ResolveTokenOutcome> {
        self.endpoint
            .resolve_token(
                selection,
                is_route_controller,
                is_dynamic_route_scope,
                binding_classification,
                transport_payload_len,
                transport_payload_lane,
                frontier_visited,
            )
            .await
    }

    #[inline]
    fn materialize_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
        is_route_controller: bool,
        binding_classification: Option<crate::binding::IncomingClassification>,
        transport_payload_len: usize,
        transport_payload_lane: u8,
    ) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        self.endpoint.materialize_branch(
            selection,
            resolved,
            is_route_controller,
            binding_classification,
            transport_payload_len,
            transport_payload_lane,
        )
    }

    fn commit_pending_branch_preview(
        &mut self,
        preview: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<Option<RecvMeta>> {
        let scope_id = preview.branch_meta.scope_id;
        let selected_arm = preview.branch_meta.selected_arm;
        let lane_wire = preview.branch_meta.lane_wire;
        let is_route_controller = self.endpoint.cursor.is_route_controller(scope_id);
        if !is_route_controller {
            self.endpoint
                .propagate_recvless_parent_route_decision(scope_id, selected_arm);
        }

        match preview.branch_meta.route_source {
            RouteDecisionSource::Ack if is_route_controller => {
                let (offer_lanes, offer_lanes_len) = self.endpoint.offer_lanes_for_scope(scope_id);
                if offer_lanes_len == 0 {
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
                    let mut lane_idx = 0usize;
                    while lane_idx < offer_lanes_len {
                        let lane = offer_lanes[lane_idx];
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
                        lane_idx += 1;
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

        self.endpoint
            .skip_unselected_arm_lanes(scope_id, selected_arm, lane_wire);
        self.endpoint
            .set_route_arm(lane_wire, scope_id, selected_arm)?;
        self.endpoint
            .set_cursor_index(state_index_to_usize(preview.cursor_index));

        let meta = if preview.branch_meta.kind == BranchKind::WireRecv {
            let mut meta = self
                .endpoint
                .cursor
                .try_recv_meta()
                .ok_or(RecvError::PhaseInvariant)?;
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

        if self.endpoint.arm_has_recv(scope_id, selected_arm) {
            self.endpoint
                .consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.endpoint.clear_scope_evidence(scope_id);
        if lane_wire == 5 {
            self.endpoint
                .port_for_lane(lane_wire as usize)
                .clear_route_hints();
        }

        Ok(meta)
    }

    #[inline]
    fn commit_branch_preview(
        &mut self,
        branch: &RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<Option<RecvMeta>> {
        self.commit_pending_branch_preview(branch.clone())
    }

    fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        let hint_matches_scope =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::hint_matches_scope(
                label_meta, label, false,
            );
        let exact_static_passive_arm = self
            .endpoint
            .static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        if !hint_matches_scope && exact_static_passive_arm.is_none() {
            return;
        }
        if suppress_hint || !hint_matches_scope {
            self.endpoint
                .mark_scope_ready_arm_from_binding_label(scope_id, label, label_meta);
            return;
        }
        self.endpoint.record_scope_hint(scope_id, label);
        self.endpoint
            .mark_scope_ready_arm_from_binding_label(scope_id, label, label_meta);
    }

    fn ingest_scope_evidence_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        if suppress_hint {
            if let Some(label) = self
                .endpoint
                .take_hint_for_lane(lane_idx, false, label_meta)
            {
                self.endpoint.record_scope_hint_dynamic(scope_id, label);
                self.endpoint
                    .mark_scope_ready_arm_from_label(scope_id, label, label_meta);
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
        if let Some(label) = self
            .endpoint
            .take_hint_for_lane(lane_idx, suppress_hint, label_meta)
        {
            self.endpoint.record_scope_hint(scope_id, label);
        }
    }

    fn ingest_scope_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lane_mask: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        if offer_lane_mask == 0 {
            return;
        }
        let pending_ack_mask =
            self.endpoint
                .pending_scope_ack_lane_mask(summary_lane_idx, scope_id, offer_lane_mask);
        let pending_hint_mask = self.endpoint.pending_scope_hint_lane_mask(
            summary_lane_idx,
            offer_lane_mask,
            label_meta,
        );
        let mut pending_evidence_mask = pending_ack_mask | pending_hint_mask;
        if pending_evidence_mask == 0 {
            return;
        }
        while let Some(lane_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut pending_evidence_mask,
            )
        {
            self.ingest_scope_evidence_for_lane(lane_idx, scope_id, suppress_hint, label_meta);
        }
    }

    fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        is_dynamic_scope: bool,
        is_route_controller: bool,
    ) -> bool {
        if is_dynamic_scope {
            self.endpoint.clear_scope_evidence(scope_id);
            return true;
        }
        if is_route_controller {
            return false;
        }
        self.endpoint.clear_scope_evidence(scope_id);
        true
    }

    pub(in crate::endpoint::kernel) fn cache_binding_classification_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
    ) {
        if binding_classification.is_some() {
            return;
        }
        if let Some((lane_idx, classification)) = self.endpoint.poll_binding_for_offer(
            scope_id,
            offer_lane_idx,
            offer_lane_mask,
            label_meta,
            materialization_meta,
        ) {
            if binding_classification.is_none() {
                *binding_classification = Some(classification);
            } else {
                self.endpoint
                    .put_back_binding_for_lane(lane_idx, classification);
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
        let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .endpoint
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
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
        self.endpoint.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .endpoint
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
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
        self.endpoint.store_frontier_observation(
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
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .endpoint
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
        if ((cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            & !self
                .endpoint
                .offer_entry_offer_lane_mask(entry_idx, entry_state))
            != 0
            || ((cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask)
                & !self
                    .endpoint
                    .offer_entry_offer_lane_mask(entry_idx, entry_state))
                != 0
        {
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
        cached_key.offer_lane_mask = observation_key.offer_lane_mask;
        cached_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
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
        self.endpoint.store_frontier_observation(
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .endpoint
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
        let changed_lane_mask = (cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            | (cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask);
        if changed_lane_mask != 0 {
            let slot_masks = self
                .endpoint
                .frontier_observation_offer_lane_entry_slot_masks(
                    current_parallel_root,
                    use_root_observed_entries,
                );
            let mut remaining_lanes = changed_lane_mask;
            while let Some(lane_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                    &mut remaining_lanes,
                )
            {
                if slot_masks[lane_idx] != 0 {
                    return false;
                }
            }
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
        cached_key.offer_lane_mask = observation_key.offer_lane_mask;
        cached_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        self.endpoint.store_frontier_observation(
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .endpoint
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
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
        self.endpoint.store_frontier_observation(
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
            .endpoint
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let offer_lane_mask = self
            .endpoint
            .frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask
            || cached_key.binding_nonempty_mask
                != (self.endpoint.binding_inbox.nonempty_mask & offer_lane_mask)
        {
            return;
        }
        let scope_generation = self.endpoint.scope_evidence_generation_for_scope(scope_id);
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0
                || self.endpoint.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            if cached_key.slot(slot_idx).scope_generation == scope_generation {
                continue;
            }
            let summary = self
                .endpoint
                .compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
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
        self.endpoint.store_frontier_observation(
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
        previous_nonempty_mask: u8,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_bit = 1u8 << lane_idx;
        if ((previous_nonempty_mask ^ self.endpoint.binding_inbox.nonempty_mask) & lane_bit) == 0 {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .endpoint
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let offer_lane_mask = self
            .endpoint
            .frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask || (offer_lane_mask & lane_bit) == 0 {
            return;
        }
        let binding_nonempty_mask = self.endpoint.binding_inbox.nonempty_mask & offer_lane_mask;
        if ((cached_key.binding_nonempty_mask ^ binding_nonempty_mask) & !lane_bit) != 0 {
            return;
        }
        let mut affected_slot_mask = self
            .endpoint
            .frontier_observation_offer_lane_entry_slot_masks(
                current_parallel_root,
                use_root_observed_entries,
            )[lane_idx];
        if affected_slot_mask == 0 {
            return;
        }
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut affected_slot_mask,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return;
            };
            let summary = self
                .endpoint
                .compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
            if entry_state.active_mask == 0
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
        cached_key.binding_nonempty_mask = binding_nonempty_mask;
        let _ = self.endpoint.next_frontier_observation_epoch();
        self.endpoint.store_frontier_observation(
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
        if lane_idx >= MAX_LANES {
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
            .endpoint
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let offer_lane_mask = self
            .endpoint
            .frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask
            || cached_key.binding_nonempty_mask
                != (self.endpoint.binding_inbox.nonempty_mask & offer_lane_mask)
        {
            return;
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0
                || self
                    .endpoint
                    .offer_entry_representative_lane_idx(entry_idx, entry_state)
                    != Some(lane_idx)
            {
                continue;
            }
            let summary = self
                .endpoint
                .compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
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
        self.endpoint.store_frontier_observation(
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
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut active_entries,
            )
        {
            let Some(entry_idx) = global_active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0
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
        previous_nonempty_mask: u8,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        self.refresh_cached_frontier_observation_binding_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_nonempty_mask,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.endpoint.frontier_state.root_frontier_len() {
            let root = self.endpoint.frontier_state.root_frontier_state[slot_idx].root;
            if self
                .endpoint
                .frontier_observation_offer_lane_entry_slot_masks(root, true)[lane_idx]
                != 0
            {
                self.refresh_cached_frontier_observation_binding_lane_entries(
                    root,
                    true,
                    lane_idx,
                    previous_nonempty_mask,
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
        if lane_idx >= MAX_LANES {
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
        let mut slot_idx = 0usize;
        while slot_idx < MAX_LANES {
            if observation_key.entry_state(slot_idx).is_max() {
                break;
            }
            if cached_key.slot(slot_idx) != observation_key.slot(slot_idx) {
                changed_slot_mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        let mut changed_lane_mask = cached_key.offer_lane_mask ^ observation_key.offer_lane_mask;
        changed_lane_mask |=
            cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask;
        if changed_lane_mask != 0 {
            let slot_masks = self
                .endpoint
                .frontier_observation_offer_lane_entry_slot_masks(
                    current_parallel_root,
                    use_root_observed_entries,
                );
            let mut remaining_lanes = changed_lane_mask;
            while let Some(lane_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                    &mut remaining_lanes,
                )
            {
                changed_slot_mask |= slot_masks[lane_idx];
            }
        }
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
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
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
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0 {
                continue;
            }
            let observed = self
                .endpoint
                .cached_offer_entry_observed_state_for_rebuild(
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) = self
            .endpoint
            .frontier_observation_cache(current_parallel_root, use_root_observed_entries);
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
        self.endpoint.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            observed_entries,
        );
        true
    }

    fn refresh_frontier_observation_cache(
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) = self
            .endpoint
            .frontier_observation_cache(current_parallel_root, use_root_observed_entries);
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
        self.endpoint.store_frontier_observation(
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
                    CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) = self
            .endpoint
            .frontier_observation_cache(current_parallel_root, use_root_observed_entries);
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
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = self
                .endpoint
                .cached_offer_entry_observed_state_for_rebuild(
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
        self.endpoint.store_frontier_observation(
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
        let observation_key = self
            .endpoint
            .frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) = self
            .endpoint
            .frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
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
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_lane_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = if let Some(observed) =
                self.endpoint.cached_offer_entry_observed_state_for_rebuild(
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
        self.endpoint.store_frontier_observation(
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
        let (cached_key, _) = self
            .endpoint
            .frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            self.refresh_frontier_observation_cache(
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
        self.refresh_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
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
        let active_mask = entry_state.active_mask;
        if active_mask == 0 {
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
        self.endpoint.ensure_global_frontier_scratch_initialized();
        let mut global_active_entries = self.endpoint.global_active_entries();
        global_active_entries.remove_entry(entry_idx);
        let lane_idx = active_mask.trailing_zeros() as usize;
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
        let (_, offer_lanes_len) = self.endpoint.offer_lanes_for_scope(info.scope);
        #[cfg(test)]
        let selection_meta = self.endpoint.compute_offer_entry_selection_meta(
            info.scope,
            info,
            offer_lanes_len != 0,
        );
        #[cfg(test)]
        let loop_meta = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_loop_meta_at(
            &self.endpoint.cursor,
            &self.endpoint.control_semantics(),
            info.scope,
            entry_idx,
        );
        #[cfg(test)]
        let label_meta = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_label_meta_at(
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
        let offer_lane_mask = self.endpoint.offer_lane_mask_for_scope_id(info.scope);
        #[cfg(test)]
        let test_summary = self
            .endpoint
            .compute_offer_entry_static_summary(active_mask, entry_idx);
        #[cfg(test)]
        {
            let Some(state) = self
                .endpoint
                .frontier_state
                .offer_entry_state_mut(entry_idx)
            else {
                return;
            };
            state.lane_idx = lane_idx as u8;
            state.parallel_root = info.parallel_root;
            state.frontier = info.frontier;
            state.scope_id = info.scope;
            state.offer_lane_mask = offer_lane_mask;
            state.selection_meta = selection_meta;
            state.label_meta = label_meta;
            state.materialization_meta = materialization_meta;
            state.summary = test_summary;
        }
        self.endpoint.ensure_global_frontier_scratch_initialized();
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
        let bit = 1u8 << lane_idx;
        let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return;
        };
        let parallel_root = self
            .endpoint
            .offer_entry_parallel_root_from_state(entry_idx, entry_state)
            .unwrap_or(ScopeId::none());
        let active_mask = entry_state.active_mask & !bit;
        if active_mask == 0 {
            self.endpoint
                .detach_offer_entry_from_root_frontier(entry_idx, parallel_root);
            self.endpoint.ensure_global_frontier_scratch_initialized();
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
        #[cfg(test)]
        self.endpoint
            .frontier_state
            .set_offer_entry_active_mask(entry_idx, active_mask);
        self.refresh_offer_entry_state(entry_idx);
    }

    fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let bit = 1u8 << lane_idx;
        #[cfg(not(test))]
        let was_inactive = (self
            .endpoint
            .offer_entry_active_mask_from_route_state(entry_idx)
            & !bit)
            == 0;
        #[cfg(test)]
        let was_inactive = if let Some(state) = self
            .endpoint
            .frontier_state
            .offer_entry_state_mut(entry_idx)
        {
            let was_inactive = state.active_mask == 0;
            state.active_mask |= bit;
            was_inactive
        } else {
            let mut state = OfferEntryState::EMPTY;
            state.active_mask = bit;
            self.endpoint
                .frontier_state
                .set_offer_entry_state(entry_idx, state);
            true
        };
        if was_inactive {
            self.endpoint.ensure_global_frontier_scratch_initialized();
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
        let refresh_mask = self.endpoint.offer_refresh_mask();
        let mut stale_mask = self.endpoint.route_state.active_offer_mask & !refresh_mask;
        while stale_mask != 0 {
            let lane_idx = stale_mask.trailing_zeros() as usize;
            stale_mask &= !(1u8 << lane_idx);
            self.clear_lane_offer_state(lane_idx);
        }
        let mut lane_mask = refresh_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
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

    async fn run(&mut self) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        if let Some(branch) = self.take_pending_branch_preview() {
            return Ok(branch);
        }
        let mut frontier_scratch = self.endpoint.frontier_scratch_view();
        let mut frontier_visited =
            super::frontier::frontier_visit_set_from_scratch(&mut frontier_scratch);
        let mut carried_binding_classification = None;
        let mut carried_transport_payload = None;
        'offer_frontier: loop {
            let selection = self.select_scope()?;
            let scope_id = selection.scope_id;
            frontier_visited.record(scope_id);
            let offer_lane_mask = selection.offer_lane_mask;
            let offer_lane = selection.offer_lane;
            let offer_lane_idx = selection.offer_lane_idx as usize;
            let at_route_offer_entry = selection.at_route_offer_entry;
            let loop_meta = self.endpoint.selection_label_meta(selection).loop_meta();

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

            let route_policy_is_dynamic = self
                .endpoint
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false);
            let is_dynamic_route_scope = route_policy_is_dynamic;
            let suppress_scope_hint = is_dynamic_route_scope;
            {
                let label_meta = self.endpoint.selection_label_meta(selection);
                self.ingest_scope_evidence_for_offer(
                    scope_id,
                    offer_lane_idx,
                    selection.offer_lane_mask,
                    suppress_scope_hint,
                    label_meta,
                );
            }
            let preview_route_decision = self.endpoint.preview_scope_ack_token_non_consuming(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
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
            let early_hint_resolves_recvless = false;
            let controller_static_entry_ready = false;
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
                        || early_route_decision.is_some()
                        || controller_static_entry_ready))
                    || (!at_route_offer_entry && cursor_is_not_recv));
            let passive_dynamic_scope_has_recv =
                self.endpoint.arm_has_recv(scope_id, 0) || self.endpoint.arm_has_recv(scope_id, 1);
            let passive_ack_is_materializable = self
                .endpoint
                .preview_scope_ack_token_non_consuming(scope_id, offer_lane_idx, offer_lane_mask)
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
                || early_decision_arm_has_no_recv
                || early_hint_resolves_recvless;
            let mut binding_classification = carried_binding_classification.take();
            let (mut transport_payload_len, mut transport_payload_lane) =
                carried_transport_payload.take().unwrap_or((0, offer_lane));
            if binding_classification.is_none() && transport_payload_len == 0 {
                let payload_view = if skip_recv_loop {
                    0usize
                } else {
                    'offer_recv: loop {
                        if !is_route_controller || controller_selected_recv_step {
                            let label_meta = self.endpoint.selection_label_meta(selection);
                            let materialization_meta =
                                self.endpoint.selection_materialization_meta(selection);
                            if let Some((_, classification)) = self.endpoint.poll_binding_for_offer(
                                scope_id,
                                offer_lane_idx,
                                offer_lane_mask,
                                label_meta,
                                materialization_meta,
                            ) {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                            if recvless_loop_control_scope
                                && let Some((_, classification)) = self
                                    .endpoint
                                    .poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
                            {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                        }

                        let payload_len = {
                            let port = self.endpoint.port_for_lane(offer_lane_idx);
                            let payload = lane_port::recv_future(port)
                                .await
                                .map_err(RecvError::Transport)?;
                            lane_port::copy_payload_into_scratch(port, &payload)
                                .map_err(|_| RecvError::PhaseInvariant)?
                        };

                        if !is_route_controller || controller_selected_recv_step {
                            let label_meta = self.endpoint.selection_label_meta(selection);
                            let materialization_meta =
                                self.endpoint.selection_materialization_meta(selection);
                            if let Some((_, classification)) = self.endpoint.poll_binding_for_offer(
                                scope_id,
                                offer_lane_idx,
                                offer_lane_mask,
                                label_meta,
                                materialization_meta,
                            ) {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                            if recvless_loop_control_scope
                                && let Some((_, classification)) = self
                                    .endpoint
                                    .poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
                            {
                                binding_classification = Some(classification);
                                break 'offer_recv 0usize;
                            }
                        }

                        break 'offer_recv payload_len;
                    }
                };
                if payload_view != 0 {
                    transport_payload_len = payload_view;
                    transport_payload_lane = offer_lane;
                }
            }
            if let Some(classification) = binding_classification.as_ref() {
                let label_meta = self.endpoint.selection_label_meta(selection);
                self.ingest_binding_scope_evidence(
                    scope_id,
                    classification.label,
                    suppress_scope_hint,
                    label_meta,
                );
            }
            {
                let label_meta = self.endpoint.selection_label_meta(selection);
                self.ingest_scope_evidence_for_offer(
                    scope_id,
                    offer_lane_idx,
                    selection.offer_lane_mask,
                    suppress_scope_hint,
                    label_meta,
                );
            }
            if self.endpoint.scope_evidence_conflicted(scope_id)
                && !self.recover_scope_evidence_conflict(
                    scope_id,
                    is_dynamic_route_scope,
                    is_route_controller,
                )
            {
                return Err(RecvError::PhaseInvariant);
            }

            let resolved = match self
                .resolve_token(
                    selection,
                    is_route_controller,
                    is_dynamic_route_scope,
                    &mut binding_classification,
                    &mut transport_payload_len,
                    &mut transport_payload_lane,
                    &mut frontier_visited,
                )
                .await
            {
                Ok(resolved) => match resolved {
                    ResolveTokenOutcome::RestartFrontier => {
                        carried_binding_classification = binding_classification;
                        carried_transport_payload = (transport_payload_len != 0)
                            .then_some((transport_payload_len, transport_payload_lane));
                        continue 'offer_frontier;
                    }
                    ResolveTokenOutcome::Resolved(resolved) => resolved,
                },
                Err(err) => return Err(err),
            };
            if !is_route_controller {
                match self
                    .endpoint
                    .descend_selected_passive_route(selection, resolved)
                {
                    Ok(true) => {
                        carried_binding_classification = binding_classification;
                        carried_transport_payload = (transport_payload_len != 0)
                            .then_some((transport_payload_len, transport_payload_lane));
                        continue 'offer_frontier;
                    }
                    Ok(false) => {}
                    Err(err) => return Err(err),
                }
            }
            return self.materialize_branch(
                selection,
                resolved,
                is_route_controller,
                binding_classification,
                transport_payload_len,
                transport_payload_lane,
            );
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
    B: BindingSlot,
{
    /// Observe an inbound route branch.
    ///
    /// Route hints are drained once per call and consumed only when they match
    /// the current route scope.
    /// Loop control evidence that resolves a recv-less branch is treated as
    /// EmptyArmTerminal and skip decode.
    pub async fn offer(
        &mut self,
    ) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        RouteFrontierMachine::new(self).run().await
    }

    pub(super) fn commit_pending_branch_preview(
        &mut self,
        preview: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<Option<RecvMeta>> {
        RouteFrontierMachine::new(self).commit_pending_branch_preview(preview)
    }

    pub(in crate::endpoint::kernel) fn commit_branch_preview(
        &mut self,
        branch: &RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<Option<RecvMeta>> {
        RouteFrontierMachine::new(self).commit_branch_preview(branch)
    }

    pub(in crate::endpoint::kernel) fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        RouteFrontierMachine::new(self).ingest_binding_scope_evidence(
            scope_id,
            label,
            suppress_hint,
            label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lane_mask: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        RouteFrontierMachine::new(self).ingest_scope_evidence_for_offer(
            scope_id,
            summary_lane_idx,
            offer_lane_mask,
            suppress_hint,
            label_meta,
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

    pub(in crate::endpoint::kernel) fn cache_binding_classification_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
    ) {
        RouteFrontierMachine::new(self).cache_binding_classification_for_offer(
            scope_id,
            offer_lane_idx,
            offer_lane_mask,
            label_meta,
            materialization_meta,
            binding_classification,
        )
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) {
        RouteFrontierMachine::new(self)
            .refresh_frontier_observation_cache(current_parallel_root, use_root_observed_entries)
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
        previous_nonempty_mask: u8,
    ) {
        RouteFrontierMachine::new(self)
            .refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask)
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
    pub(in crate::endpoint::kernel) fn refresh_structural_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        RouteFrontierMachine::new(self).refresh_structural_frontier_observation_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            cached_key,
        )
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
    pub(in crate::endpoint::kernel) fn refresh_frontier_observed_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        RouteFrontierMachine::new(self).refresh_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn patch_frontier_observed_entries_from_cached_structure(
        &mut self,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        Some(
            RouteFrontierMachine::new(self).compose_frontier_observed_entries(
                active_entries,
                observation_key,
                cached_key,
                cached_observed_entries,
            ),
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
    pub(in crate::endpoint::kernel) fn refresh_inserted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        RouteFrontierMachine::new(self).refresh_inserted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn refresh_removed_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        RouteFrontierMachine::new(self).refresh_removed_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn refresh_replaced_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        RouteFrontierMachine::new(self).refresh_replaced_frontier_observation_entry(
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
