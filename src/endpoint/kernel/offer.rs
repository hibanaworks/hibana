//! Offer-path helpers for scope selection and branch materialization.

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
mod select_observed;
mod select_scope_selection;
mod state;
use core::{
    ops::ControlFlow,
    task::{Poll, ready},
};

use super::authority::RouteResolveStep;
pub(in crate::endpoint::kernel) use super::authority::{Arm, RouteArmToken};
use super::core::CursorEndpoint;
use super::evidence::{ScopeFrameLabelScratch, ScopeFrameLabelView};
use super::frontier::{
    ActiveEntrySet, FrontierDeferOutcome, FrontierVisitSet, LaneOfferState, ObservedEntrySet,
    OfferEvidenceOutcome, OfferProgressState, OfferSelectPriority, frontier_snapshot_from_scratch,
};
use super::lane_port;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::{ReentryMark, ScopeId};
use crate::global::role_program::LaneSetView;
use crate::global::typestate::state_index_to_usize;
use crate::transport::Transport;

pub(in crate::endpoint::kernel) use self::commit_types::{
    BranchCommitPlan, BranchKind, BranchMeta,
};
pub(in crate::endpoint::kernel) use self::frontier_types::{
    CachedRecvMeta, CachedRouteArm, CurrentFrontierSelectionState,
    CurrentReentryControllerEvidence, CurrentScopeSelectionMeta, FrontierFacts, FrontierReadiness,
    ScopeArmMaterializationMeta,
};
use self::ingress::OfferFrontierFacts;
pub(in crate::endpoint::kernel) use self::ingress_types::{
    FrameHintResolution, OfferScopeSelection,
};
use self::profile::OfferAuthorityPath;
pub(in crate::endpoint::kernel) use self::profile::{OfferEntryPosition, OfferScopeProfile};
use self::resolve_types::ResolvePendingState;
pub(in crate::endpoint::kernel) use self::resolve_types::{
    ResolveTokenOutcome, ResolvedRouteArm, RouteArmCommitEvidence,
};
use self::select::FrontierDeferRequest;
pub(in crate::endpoint::kernel) use self::state::OfferState;
use self::state::{
    OfferCollectState, OfferExecution, OfferExecutionKind, OfferResolveState, OfferStagedIngress,
};
pub(in crate::endpoint::kernel) use super::core::IngressEvidenceState;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::endpoint::kernel) enum FrameHintIngestion {
    Scope,
    Dynamic,
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn ingest_scope_evidence_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        frame_hint_ingestion: FrameHintIngestion,
        frame_label_meta: &ScopeFrameLabelView<'_>,
    ) {
        match frame_hint_ingestion {
            FrameHintIngestion::Dynamic => {
                if let Some(frame_label) = self.take_frame_hint_for_lane(lane_idx, frame_label_meta)
                {
                    self.record_dynamic_scope_frame_hint(scope_id, lane_idx as u8, frame_label);
                    self.mark_scope_ready_arm_from_frame_label(
                        scope_id,
                        lane_idx as u8,
                        frame_label,
                        frame_label_meta,
                    );
                }
            }
            FrameHintIngestion::Scope => {
                if let Some(frame_label) = self.take_frame_hint_for_lane(lane_idx, frame_label_meta)
                {
                    self.record_scope_frame_hint(scope_id, lane_idx as u8, frame_label);
                    self.mark_scope_ready_arm_from_frame_label(
                        scope_id,
                        lane_idx as u8,
                        frame_label,
                        frame_label_meta,
                    );
                }
            }
        }
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lanes: crate::global::role_program::LaneSetView,
        frame_hint_ingestion: FrameHintIngestion,
        frame_label_meta: &ScopeFrameLabelView<'_>,
    ) {
        self.ingest_scope_evidence_for_offer_impl(
            scope_id,
            offer_lanes,
            frame_hint_ingestion,
            frame_label_meta,
        )
    }

    fn ingest_scope_evidence_for_offer_impl(
        &mut self,
        scope_id: ScopeId,
        offer_lanes: crate::global::role_program::LaneSetView,
        frame_hint_ingestion: FrameHintIngestion,
        frame_label_meta: &ScopeFrameLabelView<'_>,
    ) {
        if offer_lanes.is_empty() {
            return;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let has_ack = self.pending_scope_ack_lane_mask(lane_idx, scope_id, lane_idx);
            let has_frame_hint = self.pending_scope_frame_hint_on_lane(lane_idx, frame_label_meta);
            if has_ack || has_frame_hint {
                self.ingest_scope_evidence_for_lane(
                    lane_idx,
                    scope_id,
                    frame_hint_ingestion,
                    frame_label_meta,
                );
            }
            next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
    }

    fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        if !self.offer_entry_has_active_lanes(entry_idx) {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            return;
        }
        let detached_root = match self
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
            .map(|state| state.parallel_root)
        {
            Some(root) => root,
            None => ScopeId::none(),
        };
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
            scan_idx = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        let Some(lane_idx) = representative_lane_idx else {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            return;
        };
        self.detach_offer_entry_from_root_frontier(entry_idx, detached_root);
        Self::ensure_global_frontier_scratch_ready(self);
        let mut global_active_entries = self.global_active_entries();
        global_active_entries.remove_entry(entry_idx);
        let info = self.decision_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            return;
        }
        let selection_meta = self.compute_offer_entry_selection_meta(
            info.scope,
            info,
            !self.offer_lane_set_for_scope(info.scope).is_empty(),
        );
        let entry_summary = self.compute_offer_entry_summary(entry_idx);
        if let Some(state) = self.frontier_state.offer_entry_state_mut(entry_idx) {
            state.lane_idx = lane_idx as u8;
            state.parallel_root = info.parallel_root;
            state.frontier = info.frontier;
            state.scope_id = info.scope;
            state.selection_meta = selection_meta;
            state.summary = entry_summary;
        }
        Self::ensure_global_frontier_scratch_ready(self);
        let mut global_active_entries = self.global_active_entries();
        global_active_entries.insert_entry(entry_idx, lane_idx as u8);
        self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
    }

    fn detach_lane_from_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_absent() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let remaining_active_lanes = self.offer_entry_has_other_active_lanes(entry_idx, lane_idx);
        if !remaining_active_lanes {
            let parallel_root = if info.parallel_root.is_none() {
                match self
                    .offer_entry_state_snapshot(entry_idx)
                    .and_then(|_| self.offer_entry_parallel_root(entry_idx))
                {
                    Some(root) => root,
                    None => ScopeId::none(),
                }
            } else {
                info.parallel_root
            };
            self.detach_offer_entry_from_root_frontier(entry_idx, parallel_root);
            Self::ensure_global_frontier_scratch_ready(self);
            let mut global_active_entries = self.global_active_entries();
            global_active_entries.remove_entry(entry_idx);
            self.frontier_state.clear_offer_entry_state(entry_idx);
            return;
        }
        if self.offer_entry_state_snapshot(entry_idx).is_none() {
            return;
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_absent() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let was_inactive = self
            .global_active_entries()
            .slot_for_entry(entry_idx)
            .is_none();
        if was_inactive {
            Self::ensure_global_frontier_scratch_ready(self);
            let mut global_active_entries = self.global_active_entries();
            global_active_entries.insert_entry(entry_idx, lane_idx as u8);
            self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    pub(in crate::endpoint::kernel) fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        let detached = self.decision_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, detached);
        self.detach_lane_from_root_frontier(detached);
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
            start = lane_idx + 1;
        }

        let mut lane_idx = 0usize;
        while lane_idx < logical_lane_count {
            if self.cursor.lane_has_pending_step(lane_idx) {
                self.refresh_lane_offer_state(lane_idx);
            }
            lane_idx += 1;
        }

        start = 0;
        while let Some(lane_idx) = {
            self.decision_state
                .lane_reentry_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            self.refresh_lane_offer_state(lane_idx);
            start = lane_idx + 1;
        }

        start = 0;
        while let Some(lane_idx) = {
            self.decision_state
                .lane_offer_reentry_lanes()
                .next_set_from(start, logical_lane_count)
        } {
            self.refresh_lane_offer_state(lane_idx);
            start = lane_idx + 1;
        }
    }

    pub(in crate::endpoint::kernel) fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        let detached = self.decision_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, detached);
        self.detach_lane_from_root_frontier(detached);
        if let Some(info) = self.compute_lane_offer_state(lane_idx) {
            let reentry = if self.is_reentry_route(info.scope) {
                ReentryMark::Reentrant
            } else {
                ReentryMark::SinglePass
            };
            self.decision_state
                .set_lane_offer_state(lane_idx, info, reentry);
            self.attach_lane_to_root_frontier(info);
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
            state.ingress.stage_transport(ingress);
        }
        Poll::Ready(Ok(()))
    }

    pub(crate) fn poll_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<u8>> {
        loop {
            let step = match state.execution.kind() {
                OfferExecutionKind::Uninitialized => self.poll_offer_uninitialized(state),
                OfferExecutionKind::Selecting => self.poll_offer_selecting(state, cx),
                OfferExecutionKind::Collecting => self.poll_offer_collecting(state, cx),
                OfferExecutionKind::Resolving => self.poll_offer_resolving(state, cx),
            };
            match step {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(None)) => {}
                Poll::Ready(Ok(Some(branch))) => return Poll::Ready(Ok(branch)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }
    }

    #[inline(never)]
    fn poll_offer_uninitialized(
        &mut self,
        state: &mut OfferState<'r>,
    ) -> Poll<RecvResult<Option<u8>>> {
        let mut frontier_scratch = self.frontier_scratch_view();
        state.execution = OfferExecution::Selecting {
            frontier_visited: super::frontier::frontier_visit_set_from_scratch(
                &mut frontier_scratch,
            ),
        };
        Poll::Ready(Ok(None))
    }

    #[inline(never)]
    fn poll_offer_selecting(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Option<u8>>> {
        if state.carried_transport_lane_wire().is_none() {
            match self.poll_any_active_offer_transport_frame(&mut state.pending_recv, cx) {
                Poll::Pending => {}
                Poll::Ready(Ok(Some(frame))) => {
                    let mut ingress = OfferStagedIngress::empty();
                    ingress.stage_transport(frame);
                    state.carry_ingress(ingress);
                }
                Poll::Ready(Ok(None)) => {}
                Poll::Ready(Err(err)) => {
                    state.discard_terminal();
                    return Poll::Ready(Err(err));
                }
            }
        }
        let selection = match self.select_scope(
            state.carried_transport_lane_wire(),
            state.carried_transport_frame_label_raw(),
        ) {
            Ok(selection) => selection,
            Err(err) => {
                state.discard_terminal();
                return Poll::Ready(Err(err));
            }
        };
        let (frontier_visited, facts) = {
            let OfferExecution::Selecting { frontier_visited } = &mut state.execution else {
                crate::invariant();
            };
            let facts = self.prepare_frontier_facts(selection, frontier_visited);
            (*frontier_visited, facts)
        };
        state.execution = OfferExecution::Collecting {
            frontier_visited,
            stage: OfferCollectState {
                facts,
                ingress: state.take_carried_ingress(),
            },
        };
        Poll::Ready(Ok(None))
    }

    #[inline(never)]
    fn poll_offer_collecting(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Option<u8>>> {
        let (frontier_visited, resolve_stage) = {
            let OfferExecution::Collecting {
                frontier_visited,
                stage,
            } = &mut state.execution
            else {
                crate::invariant();
            };
            match self.poll_collect_offer_evidence(&mut state.pending_recv, stage, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => {
                    stage.discard_terminal();
                    return Poll::Ready(Err(err));
                }
                Poll::Ready(Ok(())) => {
                    let scope_id = stage.facts.scope_id();
                    let frame_hint_ingestion = stage.facts.profile.frame_hint_ingestion();
                    let mut frame_label_scratch = ScopeFrameLabelScratch::EMPTY;
                    self.write_selection_frame_label_meta(
                        stage.facts.selection,
                        &mut frame_label_scratch,
                    );
                    self.ingest_scope_evidence_for_offer(
                        scope_id,
                        self.offer_lane_set_for_scope(scope_id),
                        frame_hint_ingestion,
                        &frame_label_scratch.view(),
                    );
                    if self.scope_evidence_conflicted(scope_id) {
                        stage.discard_terminal();
                        return Poll::Ready(Err(RecvError::PhaseInvariant));
                    }
                    let ingress =
                        core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty());
                    (
                        *frontier_visited,
                        OfferResolveState {
                            facts: stage.facts,
                            ingress,
                            progress: OfferProgressState::new(),
                            pending: ResolvePendingState::ready(),
                        },
                    )
                }
            }
        };
        state.execution = OfferExecution::Resolving {
            frontier_visited,
            stage: resolve_stage,
        };
        Poll::Ready(Ok(None))
    }

    #[inline(never)]
    fn poll_offer_resolving(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Option<u8>>> {
        let mut restart = None;
        let mut branch_label = None;
        {
            let OfferExecution::Resolving {
                frontier_visited,
                stage,
            } = &mut state.execution
            else {
                crate::invariant();
            };
            let resolved =
                match self.resolve_token(stage, &mut state.pending_recv, frontier_visited, cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(err)) => {
                        stage.discard_terminal();
                        return Poll::Ready(Err(err));
                    }
                    Poll::Ready(Ok(resolved)) => resolved,
                };
            match resolved {
                ResolveTokenOutcome::RestartFrontier => {
                    restart = Some((
                        *frontier_visited,
                        core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty()),
                    ));
                }
                ResolveTokenOutcome::Resolved(resolved) => {
                    if stage.facts.profile.is_passive() {
                        let descended = match self.descend_selected_passive_route(
                            stage.selection(),
                            resolved,
                            stage
                                .ingress
                                .transport_frame_label_raw()
                                .map(|label| (stage.selection().offer_lane, label)),
                        ) {
                            Ok(descended) => descended,
                            Err(err) => {
                                stage.discard_terminal();
                                return Poll::Ready(Err(err));
                            }
                        };
                        if descended {
                            restart = Some((
                                *frontier_visited,
                                core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty()),
                            ));
                        }
                    }
                    if restart.is_none() {
                        let selection = stage.selection();
                        match self.produce_branch(
                            selection,
                            resolved,
                            stage.facts.profile,
                            &mut stage.ingress,
                        ) {
                            Ok(label) => branch_label = Some(label),
                            Err(err) => return Poll::Ready(Err(err)),
                        }
                    }
                }
            }
        }
        if let Some((frontier_visited, ingress)) = restart {
            state.carry_ingress(ingress);
            state.execution = OfferExecution::Selecting { frontier_visited };
            return Poll::Ready(Ok(None));
        }
        let label = crate::invariant_some(branch_label);
        Poll::Ready(Ok(Some(label)))
    }
}
