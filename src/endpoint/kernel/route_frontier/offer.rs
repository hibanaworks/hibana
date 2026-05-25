//! Offer-path helpers for scope selection and branch materialization.

#[path = "offer/cache.rs"]
mod cache;
#[path = "offer/cache_rebuild.rs"]
mod cache_rebuild;
#[path = "offer/cache_refresh.rs"]
mod cache_refresh;
#[path = "offer/commit.rs"]
mod commit;
#[path = "offer/endpoint_bridge.rs"]
mod endpoint_bridge;
#[path = "offer/facts.rs"]
mod facts;
#[path = "offer/ingress.rs"]
mod ingress;
#[path = "offer/materialization.rs"]
mod materialization;
#[path = "offer/passive.rs"]
mod passive;
#[path = "offer/resolve.rs"]
mod resolve;
#[path = "offer/select.rs"]
mod select;
#[path = "offer/select_alignment.rs"]
mod select_alignment;
#[path = "offer/state.rs"]
mod state;
#[path = "offer/types.rs"]
mod types;
use core::{
    ops::ControlFlow,
    task::{Poll, ready},
};

use super::authority::{
    Arm, DeferReason, DeferSource, RouteDecisionSource, RouteDecisionToken, RouteResolveStep,
};
use super::core::{BranchPreviewView, CursorEndpoint, RouteBranch};
use super::evidence::ScopeFrameLabelMeta;
use super::frontier::{
    ActiveEntrySet, FrontierDeferOutcome, FrontierObservationKey, FrontierObservationSlot,
    FrontierVisitSet, LaneOfferState, ObservedEntrySet, OfferEntryObservedState, OfferEntryState,
    OfferEvidenceOutcome, OfferLaneEntrySlotMasks, OfferProgressState, OfferSelectPriority,
    checked_state_index, choose_offer_priority, current_entry_is_candidate,
    current_entry_matches_after_filter, frontier_observation_key_view_from_storage,
    frontier_observed_entries_view_from_storage,
    frontier_offer_lane_entry_slot_masks_view_from_storage, frontier_snapshot_from_scratch,
    frontier_working_observation_key_view_from_storage,
    should_suppress_current_passive_without_evidence,
};
use super::lane_port;
use crate::binding::BindingSlot;
use crate::control::cap::mint::{EpochTable, MintConfigMarker};
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::{ScopeId, ScopeKind};
use crate::global::role_program::LaneSetView;
use crate::global::typestate::state_index_to_usize;
use crate::policy_runtime::PolicySlot;
use crate::rendezvous::port::Port;
use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::transport::Transport;

use self::ingress::{OfferFrontierFacts, OfferIngressMode, OfferIngressTurn};
pub(in crate::endpoint::kernel) use self::state::OfferState;
use self::state::{OfferCollectState, OfferResolveState, OfferRunStage, OfferStagedIngress};
use self::types::ResolvePendingState;
pub(in crate::endpoint::kernel) use self::types::{
    BranchCommitPlan, CachedRecvMeta, FrontierObservationDomain, FrontierStaticFacts,
    LaneIngressEvidence, ScopeArmMaterializationMeta,
};
pub(in crate::endpoint::kernel) use self::types::{
    BranchKind, BranchMeta, CurrentFrontierSelectionState, CurrentScopeSelectionMeta,
    OfferScopeSelection, ResolveTokenOutcome, ResolvedFrameHint, ResolvedRouteDecision,
    RouteDecisionCommitEvidence,
};

#[path = "offer/machine.rs"]
mod machine;
pub(in crate::endpoint::kernel) use self::machine::RouteFrontierMachine;

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
        let exact_passive_arm =
            self.endpoint
                .passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        if !frame_hint_matches_scope && exact_passive_arm.is_none() {
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
        self.endpoint
            .record_scope_frame_hint(scope_id, lane, frame_label);
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
                self.endpoint.record_dynamic_scope_frame_hint(
                    scope_id,
                    lane_idx as u8,
                    frame_label,
                );
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
            self.endpoint
                .record_scope_frame_hint(scope_id, lane_idx as u8, frame_label);
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
            let has_ack =
                self.endpoint
                    .pending_scope_ack_lane_mask(summary_lane_idx, scope_id, lane_idx);
            let has_frame_hint = self.endpoint.pending_scope_frame_hint_on_lane(
                lane_idx,
                frame_label_meta,
                drain_transport_hints,
            );
            if has_ack || has_frame_hint {
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

    #[cfg(test)]
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

    fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
            let previous_root = self
                .endpoint
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
                .map(|state| state.parallel_root)
                .unwrap_or(ScopeId::none());
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
        let previous_root = self
            .endpoint
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
            .map(|state| state.parallel_root)
            .unwrap_or(ScopeId::none());
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let active_offer_lanes = self.endpoint.route_state.active_offer_lanes();
        let mut scan_idx = active_offer_lanes.first_set(lane_limit);
        let mut representative_lane_idx = None;
        while let Some(lane_idx) = scan_idx {
            let info = self.endpoint.route_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                representative_lane_idx = Some(lane_idx);
                break;
            }
            scan_idx = active_offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        let Some(lane_idx) = representative_lane_idx else {
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
        self.endpoint
            .detach_offer_entry_from_root_frontier(entry_idx, previous_root);
        Self::ensure_global_frontier_scratch_initialized(self.endpoint);
        let mut global_active_entries = self.endpoint.global_active_entries();
        global_active_entries.remove_entry(entry_idx);
        let info = self.endpoint.route_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
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
        let selection_meta = self.endpoint.compute_offer_entry_selection_meta(
            info.scope,
            info,
            !self
                .endpoint
                .offer_lane_set_for_scope(info.scope)
                .is_empty(),
        );
        let test_summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
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
        if self
            .endpoint
            .offer_entry_state_snapshot(entry_idx)
            .is_none()
        {
            return;
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
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
        let mut start = 0usize;
        while let Some(lane_idx) = {
            self.endpoint
                .route_state
                .active_offer_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            let needs_refresh = Self::offer_refresh_mask(self.endpoint, lane_idx);
            if !needs_refresh {
                self.clear_lane_offer_state(lane_idx);
            }
            start = lane_idx.saturating_add(1);
        }

        let current_phase_lanes = self.endpoint.cursor.current_phase_lane_set();
        Self::for_each_set_lane(current_phase_lanes, logical_lane_count, |lane_idx| {
            self.refresh_lane_offer_state(lane_idx);
        });

        start = 0;
        while let Some(lane_idx) = {
            self.endpoint
                .route_state
                .lane_linger_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            self.refresh_lane_offer_state(lane_idx);
            start = lane_idx.saturating_add(1);
        }

        start = 0;
        while let Some(lane_idx) = {
            self.endpoint
                .route_state
                .lane_offer_linger_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            self.refresh_lane_offer_state(lane_idx);
            start = lane_idx.saturating_add(1);
        }
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

    fn poll_collect_offer_evidence(
        &mut self,
        state: &mut OfferCollectState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        if state.ingress.is_empty()
            && let Some(ingress) = ready!(self.collect_offer_ingress(state.facts, cx))?
        {
            match ingress {
                OfferIngressTurn::Binding(evidence) => state.ingress.stage_binding(evidence),
                OfferIngressTurn::Transport(payload) => state.ingress.stage_transport(payload),
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
                            Poll::Ready(Err(err)) => {
                                stage.discard_terminal();
                                return Poll::Ready(Err(err));
                            }
                            Poll::Ready(Ok(())) => {
                                let scope_id = stage.facts.scope_id;
                                let offer_lane_idx = stage.facts.offer_lane_idx;
                                let suppress_scope_frame_hint =
                                    stage.facts.suppress_scope_frame_hint;
                                let is_route_controller = stage.facts.is_route_controller;
                                let is_dynamic_route_scope = stage.facts.is_dynamic_route_scope;
                                if let Some(evidence) = stage.ingress.binding() {
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
                                    stage.discard_terminal();
                                    return Poll::Ready(Err(RecvError::PhaseInvariant));
                                }
                                self.run_stage =
                                    Some(OfferRunStage::ResolveToken(OfferResolveState {
                                        selection: stage.selection,
                                        facts: stage.facts,
                                        ingress: stage.ingress,
                                        progress: OfferProgressState::new(
                                            self.endpoint.offer_progress_policy,
                                        ),
                                        pending: ResolvePendingState::ready(),
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
                                    stage.discard_terminal();
                                    return Poll::Ready(Err(err));
                                }
                                Poll::Ready(Ok(resolved)) => {
                                    self.frontier_visited = Some(frontier_visited);
                                    resolved
                                }
                            };
                        match resolved {
                            ResolveTokenOutcome::RestartFrontier => {
                                let (binding_evidence, transport_payload) =
                                    stage.ingress.into_parts();
                                self.carried_binding_evidence = binding_evidence;
                                self.carried_transport_payload = transport_payload;
                                continue;
                            }
                            ResolveTokenOutcome::Resolved(resolved) => {
                                if !stage.facts.is_route_controller {
                                    match self
                                        .endpoint
                                        .descend_selected_passive_route(stage.selection, resolved)
                                    {
                                        Ok(true) => {
                                            let (binding_evidence, transport_payload) =
                                                stage.ingress.into_parts();
                                            self.carried_binding_evidence = binding_evidence;
                                            self.carried_transport_payload = transport_payload;
                                            continue;
                                        }
                                        Ok(false) => {}
                                        Err(err) => {
                                            stage.discard_terminal();
                                            return Poll::Ready(Err(err));
                                        }
                                    }
                                }
                                let (binding_evidence, transport_payload) =
                                    stage.ingress.into_parts();
                                return Poll::Ready(self.produce_branch(
                                    stage.selection,
                                    resolved,
                                    stage.facts.is_route_controller,
                                    binding_evidence,
                                    transport_payload,
                                ));
                            }
                        }
                    }
                }
            }

            let selection = match self.select_scope() {
                Ok(selection) => selection,
                Err(err) => {
                    self.discard_terminal_ingress();
                    return Poll::Ready(Err(err));
                }
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
                        self.discard_terminal_ingress();
                        return Poll::Ready(Err(err));
                    }
                };
                self.frontier_visited = Some(frontier_visited);
                facts
            };
            self.run_stage = Some(OfferRunStage::CollectEvidence(OfferCollectState {
                selection,
                facts,
                ingress: OfferStagedIngress::new(
                    self.carried_binding_evidence.take(),
                    self.carried_transport_payload.take(),
                ),
            }));
        }
    }
}
