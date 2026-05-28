//! Offer-path helpers for scope selection and branch materialization.

mod cache;
mod cache_rebuild;
mod cache_refresh;
mod commit;
mod commit_types;
mod facts;
mod first_recv_dispatch;
mod frontier_types;
mod ingress;
mod ingress_types;
mod materialization;
mod passive;
mod profile;
mod resolve;
mod resolve_materialization;
mod resolve_types;
mod select;
mod select_alignment;
mod state;
use core::{
    ops::ControlFlow,
    task::{Poll, ready},
};

use super::authority::{
    Arm, DeferReason, DeferSource, RouteDecisionSource, RouteDecisionToken, RouteResolveStep,
};
use super::core::{CursorEndpoint, MaterializedRouteBranch};
use super::evidence::ScopeFrameLabelMeta;
use super::frontier::{
    ActiveEntrySet, FrontierDeferOutcome, FrontierObservationKey, FrontierObservationSlot,
    FrontierVisitSet, LaneOfferState, ObservedEntrySet, OfferEntryObservedState, OfferEntryState,
    OfferEvidenceOutcome, OfferLaneEntrySlotMasks, OfferProgressState, OfferSelectPriority,
    checked_state_index, frontier_observation_key_view_from_storage,
    frontier_observed_entries_view_from_storage,
    frontier_offer_lane_entry_slot_masks_view_from_storage, frontier_snapshot_from_scratch,
    frontier_working_observation_key_view_from_storage,
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

pub(in crate::endpoint::kernel) use self::commit_types::{
    BranchCommitPlan, BranchKind, BranchMeta,
};
pub(in crate::endpoint::kernel) use self::frontier_types::{
    CachedRecvMeta, CurrentFrontierSelectionState, CurrentScopeSelectionMeta,
    FrontierObservationDomain, FrontierStaticFacts, ScopeArmMaterializationMeta,
};
use self::ingress::{OfferFrontierFacts, OfferIngressTurn};
pub(in crate::endpoint::kernel) use self::ingress_types::{
    LaneIngressEvidence, OfferScopeSelection, ResolvedFrameHint,
};
use self::profile::OfferAuthorityPath;
pub(in crate::endpoint::kernel) use self::profile::OfferScopeProfile;
use self::resolve_types::ResolvePendingState;
pub(in crate::endpoint::kernel) use self::resolve_types::{
    ResolveTokenOutcome, ResolvedRouteDecision, RouteDecisionCommitEvidence,
};
pub(in crate::endpoint::kernel) use self::state::OfferState;
use self::state::{OfferCollectState, OfferExecution, OfferResolveState, OfferStagedIngress};

struct HintDrainAuthority<'a> {
    pending_recv: &'a lane_port::PendingRecv,
}

impl HintDrainAuthority<'_> {
    #[inline]
    fn should_drain_transport_hints<'r, T, E>(&self, port: &Port<'r, T, E>) -> bool
    where
        T: Transport + 'r,
        E: EpochTable + 'r,
    {
        !self.pending_recv.parks_port(port)
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
    #[inline]
    fn discard_terminal_ingress(&mut self, state: &mut OfferState<'r>) {
        state.discard_terminal();
    }

    pub(in crate::endpoint::kernel) fn ingest_binding_scope_evidence(
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
            self.passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        if !frame_hint_matches_scope && exact_passive_arm.is_none() {
            return;
        }
        if suppress_hint || !frame_hint_matches_scope {
            self.mark_scope_ready_arm_from_binding_frame_label(
                scope_id,
                lane,
                frame_label,
                frame_label_meta,
            );
            return;
        }
        self.record_scope_frame_hint(scope_id, lane, frame_label);
        self.mark_scope_ready_arm_from_binding_frame_label(
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
            if let Some(frame_label) = self.take_frame_hint_for_lane(
                lane_idx,
                false,
                frame_label_meta,
                drain_transport_hints,
            ) {
                self.record_dynamic_scope_frame_hint(scope_id, lane_idx as u8, frame_label);
                self.mark_scope_ready_arm_from_frame_label(
                    scope_id,
                    lane_idx as u8,
                    frame_label,
                    frame_label_meta,
                );
            }

            if let Some(arm) = self.ack_route_decision_for_lane(lane_idx, scope_id, ROLE) {
                if let Some(arm) = Arm::new(arm) {
                    self.record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
                }
            }
            return;
        }
        if let Some(arm) = self.ack_route_decision_for_lane(lane_idx, scope_id, ROLE) {
            if let Some(arm) = Arm::new(arm) {
                self.record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
            }
        }
        if let Some(frame_label) = self.take_frame_hint_for_lane(
            lane_idx,
            suppress_hint,
            frame_label_meta,
            drain_transport_hints,
        ) {
            self.record_scope_frame_hint(scope_id, lane_idx as u8, frame_label);
        }
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_offer(
        &mut self,
        pending_recv: &lane_port::PendingRecv,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: crate::global::role_program::LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        self.ingest_scope_evidence_for_offer_with_drain(
            HintDrainAuthority { pending_recv },
            scope_id,
            summary_lane_idx,
            offer_lanes,
            suppress_hint,
            frame_label_meta,
        )
    }

    fn ingest_scope_evidence_for_offer_with_drain(
        &mut self,
        drain: HintDrainAuthority<'_>,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: crate::global::role_program::LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        if offer_lanes.is_empty() {
            return;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let drain_transport_hints = {
                let port = self.port_for_lane(lane_idx);
                drain.should_drain_transport_hints(port)
            };
            let has_ack = self.pending_scope_ack_lane_mask(summary_lane_idx, scope_id, lane_idx);
            let has_frame_hint = self.pending_scope_frame_hint_on_lane(
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

    pub(in crate::endpoint::kernel) fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        profile: OfferScopeProfile,
    ) -> bool {
        if self.scope_ack_conflicted(scope_id) {
            return false;
        }
        if !profile.recovers_frame_hint_conflict() {
            return false;
        }
        if self.scope_frame_hint_conflicted(scope_id) {
            self.clear_scope_frame_hint_conflict(scope_id);
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
        if let Some((lane_idx, evidence)) = self.poll_binding_for_offer(
            scope_id,
            offer_lane_idx,
            frame_label_meta,
            materialization_meta,
        ) {
            if binding_evidence.is_none() {
                *binding_evidence = Some(LaneIngressEvidence::new(lane_idx, evidence));
            } else {
                self.put_back_binding_for_lane(lane_idx, evidence);
            }
        }
    }

    fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        if !self.offer_entry_has_active_lanes(entry_idx) {
            let previous_root = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
                .map(|state| state.parallel_root)
                .unwrap_or(ScopeId::none());
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        let previous_root = self
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
            .map(|state| state.parallel_root)
            .unwrap_or(ScopeId::none());
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut scan_idx = active_offer_lanes.first_set(lane_limit);
        let mut representative_lane_idx = None;
        while let Some(lane_idx) = scan_idx {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                representative_lane_idx = Some(lane_idx);
                break;
            }
            scan_idx = active_offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        let Some(lane_idx) = representative_lane_idx else {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        };
        self.detach_offer_entry_from_root_frontier(entry_idx, previous_root);
        Self::ensure_global_frontier_scratch_initialized(self);
        let mut global_active_entries = self.global_active_entries();
        global_active_entries.remove_entry(entry_idx);
        let info = self.decision_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        let selection_meta = self.compute_offer_entry_selection_meta(
            info.scope,
            info,
            !self.offer_lane_set_for_scope(info.scope).is_empty(),
        );
        let test_summary = self.compute_offer_entry_static_summary(entry_idx);
        if let Some(state) = self.frontier_state.offer_entry_state_mut(entry_idx) {
            state.lane_idx = lane_idx as u8;
            state.parallel_root = info.parallel_root;
            state.frontier = info.frontier;
            state.scope_id = info.scope;
            state.selection_meta = selection_meta;
            state.summary = test_summary;
        }
        Self::ensure_global_frontier_scratch_initialized(self);
        let mut global_active_entries = self.global_active_entries();
        global_active_entries.insert_entry(entry_idx, lane_idx as u8);
        self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
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
        let remaining_active_lanes = self.offer_entry_has_other_active_lanes(entry_idx, lane_idx);
        if !remaining_active_lanes {
            let parallel_root = if info.parallel_root.is_none() {
                self.offer_entry_state_snapshot(entry_idx)
                    .and_then(|entry_state| {
                        self.offer_entry_parallel_root_from_state(entry_idx, entry_state)
                    })
                    .unwrap_or(ScopeId::none())
            } else {
                info.parallel_root
            };
            self.detach_offer_entry_from_root_frontier(entry_idx, parallel_root);
            Self::ensure_global_frontier_scratch_initialized(self);
            let mut global_active_entries = self.global_active_entries();
            global_active_entries.remove_entry(entry_idx);
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                parallel_root,
                ScopeId::none(),
            );
            return;
        }
        if self.offer_entry_state_snapshot(entry_idx).is_none() {
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
            .global_active_entries()
            .slot_for_entry(entry_idx)
            .is_none();
        if was_inactive {
            Self::ensure_global_frontier_scratch_initialized(self);
            let mut global_active_entries = self.global_active_entries();
            global_active_entries.insert_entry(entry_idx, lane_idx as u8);
            self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    pub(in crate::endpoint::kernel) fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        let old = self.decision_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.detach_lane_from_root_frontier(lane_idx, old);
    }

    pub(crate) fn sync_lane_offer_state(&mut self) {
        let logical_lane_count = self.cursor.logical_lane_count();
        let mut start = 0usize;
        while let Some(lane_idx) = {
            self.decision_state
                .active_offer_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            let needs_refresh = Self::offer_refresh_mask(self, lane_idx);
            if !needs_refresh {
                self.clear_lane_offer_state(lane_idx);
            }
            start = lane_idx.saturating_add(1);
        }

        let current_phase_lanes = self.cursor.current_phase_lane_set();
        Self::for_each_set_lane(current_phase_lanes, logical_lane_count, |lane_idx| {
            self.refresh_lane_offer_state(lane_idx);
        });

        start = 0;
        while let Some(lane_idx) = {
            self.decision_state
                .lane_linger_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            self.refresh_lane_offer_state(lane_idx);
            start = lane_idx.saturating_add(1);
        }

        start = 0;
        while let Some(lane_idx) = {
            self.decision_state
                .lane_offer_linger_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            self.refresh_lane_offer_state(lane_idx);
            start = lane_idx.saturating_add(1);
        }
    }

    pub(in crate::endpoint::kernel) fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        let old = self.decision_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.detach_lane_from_root_frontier(lane_idx, old);
        if let Some(info) = self.compute_lane_offer_state(lane_idx) {
            let is_linger = self.is_linger_route(info.scope);
            self.decision_state
                .set_lane_offer_state(lane_idx, info, is_linger);
            self.attach_lane_to_root_frontier(lane_idx, info);
            self.attach_lane_to_offer_entry(lane_idx, info);
        }
    }

    fn poll_collect_offer_evidence(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        state: &mut OfferCollectState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        if state.ingress.is_empty()
            && let Some(ingress) =
                ready!(self.collect_offer_ingress(pending_recv, state.facts, cx))?
        {
            match ingress {
                OfferIngressTurn::Binding(evidence) => state.ingress.stage_binding(evidence),
                OfferIngressTurn::Transport(payload) => state.ingress.stage_transport(payload),
            }
        }
        Poll::Ready(Ok(()))
    }

    pub(crate) fn poll_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MaterializedRouteBranch<'r>>> {
        loop {
            match core::mem::replace(&mut state.execution, OfferExecution::Uninitialized) {
                OfferExecution::Uninitialized => {
                    let mut frontier_scratch = self.frontier_scratch_view();
                    state.execution = OfferExecution::Selecting {
                        frontier_visited: super::frontier::frontier_visit_set_from_scratch(
                            &mut frontier_scratch,
                        ),
                    };
                }
                OfferExecution::Selecting {
                    mut frontier_visited,
                } => {
                    let selection = match self.select_scope() {
                        Ok(selection) => selection,
                        Err(err) => {
                            self.discard_terminal_ingress(state);
                            return Poll::Ready(Err(err));
                        }
                    };
                    let facts = match self.prepare_frontier_facts(
                        &state.pending_recv,
                        selection,
                        &mut frontier_visited,
                    ) {
                        Ok(facts) => facts,
                        Err(err) => {
                            state.execution = OfferExecution::Selecting { frontier_visited };
                            self.discard_terminal_ingress(state);
                            return Poll::Ready(Err(err));
                        }
                    };
                    state.execution = OfferExecution::Collecting {
                        frontier_visited,
                        stage: OfferCollectState {
                            facts,
                            ingress: state.take_carried_ingress(),
                        },
                    };
                }
                OfferExecution::Collecting {
                    frontier_visited,
                    mut stage,
                } => {
                    match self.poll_collect_offer_evidence(&mut state.pending_recv, &mut stage, cx)
                    {
                        Poll::Pending => {
                            state.execution = OfferExecution::Collecting {
                                frontier_visited,
                                stage,
                            };
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(err)) => {
                            stage.discard_terminal();
                            return Poll::Ready(Err(err));
                        }
                        Poll::Ready(Ok(())) => {
                            let scope_id = stage.facts.scope_id();
                            let offer_lane_idx = stage.facts.offer_lane_idx();
                            let suppress_scope_frame_hint =
                                stage.facts.profile.suppresses_scope_frame_hint();
                            let frame_label_meta =
                                self.selection_frame_label_meta(stage.facts.selection);
                            if let Some(evidence) = stage.ingress.binding() {
                                self.ingest_binding_scope_evidence(
                                    scope_id,
                                    evidence.lane(),
                                    evidence.frame_label(),
                                    suppress_scope_frame_hint,
                                    frame_label_meta,
                                );
                            }
                            self.ingest_scope_evidence_for_offer(
                                &state.pending_recv,
                                scope_id,
                                offer_lane_idx,
                                self.offer_lane_set_for_scope(scope_id),
                                suppress_scope_frame_hint,
                                frame_label_meta,
                            );
                            if self.scope_evidence_conflicted(scope_id)
                                && !self
                                    .recover_scope_evidence_conflict(scope_id, stage.facts.profile)
                            {
                                stage.discard_terminal();
                                return Poll::Ready(Err(RecvError::PhaseInvariant));
                            }
                            state.execution = OfferExecution::Resolving {
                                frontier_visited,
                                stage: OfferResolveState {
                                    facts: stage.facts,
                                    ingress: stage.ingress,
                                    progress: OfferProgressState::new(self.offer_progress_policy),
                                    pending: ResolvePendingState::ready(),
                                },
                            };
                        }
                    }
                }
                OfferExecution::Resolving {
                    mut frontier_visited,
                    mut stage,
                } => {
                    let resolved = match self.resolve_token(
                        &mut stage,
                        &mut state.pending_recv,
                        &mut frontier_visited,
                        cx,
                    ) {
                        Poll::Pending => {
                            state.execution = OfferExecution::Resolving {
                                frontier_visited,
                                stage,
                            };
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(err)) => {
                            stage.discard_terminal();
                            return Poll::Ready(Err(err));
                        }
                        Poll::Ready(Ok(resolved)) => resolved,
                    };
                    match resolved {
                        ResolveTokenOutcome::RestartFrontier => {
                            state.carry_ingress(stage.ingress);
                            state.execution = OfferExecution::Selecting { frontier_visited };
                        }
                        ResolveTokenOutcome::Resolved(resolved) => {
                            if stage.facts.profile.is_passive() {
                                match self
                                    .descend_selected_passive_route(stage.selection(), resolved)
                                {
                                    Ok(true) => {
                                        state.carry_ingress(stage.ingress);
                                        state.execution =
                                            OfferExecution::Selecting { frontier_visited };
                                        continue;
                                    }
                                    Ok(false) => {}
                                    Err(err) => {
                                        stage.discard_terminal();
                                        return Poll::Ready(Err(err));
                                    }
                                }
                            }
                            let selection = stage.selection();
                            let (binding_evidence, transport_payload) = stage.ingress.into_parts();
                            return Poll::Ready(
                                self.produce_branch(
                                    selection,
                                    resolved,
                                    stage.facts.profile,
                                    binding_evidence,
                                    transport_payload,
                                )
                                .map(Into::into),
                            );
                        }
                    }
                }
            }
        }
    }
}
