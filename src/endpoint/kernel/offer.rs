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
mod progress;
#[cfg(all(test, hibana_repo_tests))]
#[path = "offer/requeue_callback_tests.rs"]
mod requeue_callback_tests;
mod resolve;
mod resolve_materialization;
mod resolve_types;
mod select;
mod select_alignment;
mod select_observed;
mod select_scope_selection;
mod state;
use core::{ops::ControlFlow, task::Poll};

use super::authority::RouteResolveStep;
pub(in crate::endpoint::kernel) use super::authority::{Arm, RouteArmToken};
use super::core::CursorEndpoint;
use super::frontier::{
    ActiveEntrySet, FrontierProgressSelection, FrontierScratchWorkspace, FrontierVisitSet,
    LaneOfferState, ObservedEntrySet, OfferEntryKey,
};
use super::lane_port;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::{ReentryMark, ScopeId};
use crate::global::role_program::{LANE_SET_VIEW_WORDS, LaneWord, lane_word_index};
use crate::global::typestate::{InboundFrameKey, state_index_to_usize};
use crate::transport::Transport;

pub(in crate::endpoint::kernel) use self::commit_types::{
    BranchCommitPlan, BranchKind, BranchMeta,
};
pub(in crate::endpoint::kernel) use self::frontier_types::{
    CachedRecvMeta, CurrentFrontierSelectionState, CurrentReentryControllerEvidence,
    CurrentScopeSelectionMeta, FrontierFacts, FrontierReadiness, ScopeArmMaterializationMeta,
};
use self::ingress::OfferFrontierFacts;
pub(in crate::endpoint::kernel) use self::ingress_types::{
    FrameEvidenceResolution, ObservedIngressSelection, OfferScopeSelection,
};
use self::profile::OfferAuthorityPath;
pub(in crate::endpoint::kernel) use self::profile::{OfferEntryPosition, OfferScopeProfile};
pub(in crate::endpoint::kernel) use self::progress::{
    EvidenceFingerprint, FrontierDeferOutcome, OfferEvidenceOutcome, OfferProgressState,
};
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

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn detach_lane_from_offer_entry(&mut self, info: LaneOfferState) {
        if info == LaneOfferState::EMPTY {
            return;
        }
        let key = crate::invariant_some(info.key());
        let replacement = self.active_offer_entry(key);
        if replacement.is_some_and(|active| !active.accepts_lane(info)) {
            crate::invariant();
        }
        self.detach_offer_entry_from_root_frontier(key, info.parallel_root);
        if let Some(active) = replacement {
            self.attach_offer_entry_to_root_frontier(
                key,
                active.parallel_root(),
                active.representative_lane(),
            );
        }
    }

    fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let key = crate::invariant_some(info.key());
        let active = crate::invariant_some(self.active_offer_entry(key));
        if !active.accepts_lane(info) {
            crate::invariant();
        }
        if let Some(previous) = self.active_offer_entry_excluding(key, Some(lane_idx)) {
            self.detach_offer_entry_from_root_frontier(key, previous.parallel_root());
        }
        self.attach_offer_entry_to_root_frontier(
            key,
            active.parallel_root(),
            active.representative_lane(),
        );
    }

    pub(crate) fn sync_lane_offer_state(&mut self) {
        let logical_lane_count = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < logical_lane_count {
            if self.decision_state.active_offer_lanes().contains(lane_idx) {
                let detached = self
                    .decision_state
                    .take_lane_offer_state_for_rebuild(lane_idx);
                self.detach_lane_from_offer_entry(detached);
                self.detach_lane_from_root_frontier(detached);
            }
            lane_idx += 1;
        }

        lane_idx = 0;
        while lane_idx < logical_lane_count {
            if Self::offer_refresh_mask(self, lane_idx) {
                self.refresh_lane_offer_state(lane_idx);
            } else if self.decision_state.clear_lane_offer_state(lane_idx) != LaneOfferState::EMPTY
            {
                crate::invariant();
            }
            lane_idx += 1;
        }
    }

    pub(in crate::endpoint::kernel) fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= self.cursor.logical_lane_count() {
            crate::invariant();
        }
        self.clear_lane_offer_cache(lane_idx);
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

    fn clear_lane_offer_cache(&mut self, lane_idx: usize) {
        let detached = self.decision_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(detached);
        self.detach_lane_from_root_frontier(detached);
    }

    pub(crate) fn poll_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<u8>> {
        // A causally early frame is inspected at most once in this poll. The
        // transport retains it; this scan mask neither commits nor parks a lane.
        let mut deferred_lanes = [0; LANE_SET_VIEW_WORDS];
        loop {
            let step = match state.execution.kind() {
                OfferExecutionKind::Uninitialized => self.poll_offer_uninitialized(state),
                OfferExecutionKind::Selecting => {
                    self.poll_offer_selecting(state, cx, scratch, &mut deferred_lanes)
                }
                OfferExecutionKind::Collecting => {
                    self.poll_offer_collecting(state, cx, &deferred_lanes)
                }
                OfferExecutionKind::Resolving => self.poll_offer_resolving(state, cx, scratch),
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
        state.execution = OfferExecution::Selecting {
            frontier_visited: self.frontier_state.empty_frontier_visit_set(),
        };
        Poll::Ready(Ok(None))
    }

    #[inline(never)]
    fn poll_offer_selecting(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
        scratch: &mut FrontierScratchWorkspace<'_>,
        deferred_lanes: &mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> Poll<RecvResult<Option<u8>>> {
        if state.carried_transport_lane_wire().is_none() {
            match self.poll_any_active_offer_transport_frame(
                &mut state.pending_recv,
                cx,
                deferred_lanes,
            ) {
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
        let carried_lane = state.carried_transport_lane_wire();
        let carried_key = state.carried_transport_frame_key();
        let carried_observation = state.carried_transport_observation(self.sid.raw(), ROLE);
        let selection_result = {
            let OfferExecution::Selecting { frontier_visited } = &mut state.execution else {
                crate::invariant();
            };
            self.select_scope(
                carried_lane,
                carried_key,
                carried_observation,
                frontier_visited,
                scratch,
            )
        };
        let selection = match selection_result {
            Ok(ObservedIngressSelection::Branch(selection)) => selection,
            Ok(ObservedIngressSelection::OrdinaryReceive(lane)) => {
                let mut ingress = state.take_carried_ingress();
                let payload = crate::invariant_some(ingress.take_transport());
                if let Err(error) = self.requeue_offer_transport_payload(payload) {
                    state.discard_terminal();
                    return Poll::Ready(Err(error));
                }
                // This exact receive belongs to recv, not to the cached route.
                // Cursor progress will refresh the cache when recv consumes it.
                self.clear_lane_offer_cache(usize::from(lane));
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Ok(ObservedIngressSelection::PendingReceive(lane)) => {
                let mut ingress = state.take_carried_ingress();
                let payload = crate::invariant_some(ingress.take_transport());
                if let Err(error) = self.requeue_offer_transport_payload(payload) {
                    state.discard_terminal();
                    return Poll::Ready(Err(error));
                }
                let (word, bit) = lane_word_index(usize::from(lane));
                deferred_lanes[word] |= bit;
                return Poll::Ready(Ok(None));
            }
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
            (frontier_visited.take(), facts)
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
        deferred_lanes: &[LaneWord; LANE_SET_VIEW_WORDS],
    ) -> Poll<RecvResult<Option<u8>>> {
        let (frontier_visited, resolve_stage) = {
            let OfferExecution::Collecting {
                frontier_visited,
                stage,
            } = &mut state.execution
            else {
                crate::invariant();
            };
            let collected = if stage.ingress.is_empty() {
                self.collect_offer_ingress(&mut state.pending_recv, stage.facts, cx, deferred_lanes)
            } else {
                Poll::Ready(Ok(None))
            };
            match collected {
                Poll::Ready(Ok(Some(frame))) => {
                    // All newly observed frames enter exact receive matching,
                    // including frames that arrived after selection began.
                    let mut ingress = OfferStagedIngress::empty();
                    ingress.stage_transport(frame);
                    let visited = frontier_visited.take();
                    state.execution = OfferExecution::Selecting {
                        frontier_visited: visited,
                    };
                    state.carry_ingress(ingress);
                    return Poll::Ready(Ok(None));
                }
                Poll::Pending => {
                    // No selected-scope frame is owned yet. Keep independent
                    // active receive lanes armed while this scope is waiting.
                    match self.poll_any_active_offer_transport_frame(
                        &mut state.pending_recv,
                        cx,
                        deferred_lanes,
                    ) {
                        Poll::Ready(Ok(Some(frame))) => {
                            let mut ingress = OfferStagedIngress::empty();
                            ingress.stage_transport(frame);
                            let visited = frontier_visited.take();
                            state.execution = OfferExecution::Selecting {
                                frontier_visited: visited,
                            };
                            state.carry_ingress(ingress);
                            return Poll::Ready(Ok(None));
                        }
                        Poll::Ready(Err(error)) => {
                            state.discard_terminal();
                            return Poll::Ready(Err(error));
                        }
                        Poll::Ready(Ok(None)) | Poll::Pending => return Poll::Pending,
                    }
                }
                Poll::Ready(Err(err)) => {
                    stage.discard_terminal();
                    return Poll::Ready(Err(err));
                }
                Poll::Ready(Ok(None)) => {
                    let scope_id = stage.facts.scope_id();
                    if self.scope_evidence_conflicted(scope_id) {
                        stage.discard_terminal();
                        return Poll::Ready(Err(RecvError::PhaseInvariant));
                    }
                    let ingress =
                        core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty());
                    (
                        frontier_visited.take(),
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
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<Option<u8>>> {
        let mut restart = None;
        let mut nested = None;
        let mut branch_label = None;
        {
            let OfferExecution::Resolving {
                frontier_visited,
                stage,
            } = &mut state.execution
            else {
                crate::invariant();
            };
            let resolved = match self.resolve_token(
                stage,
                &mut state.pending_recv,
                frontier_visited,
                cx,
                scratch,
            ) {
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
                        frontier_visited.take(),
                        core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty()),
                    ));
                }
                ResolveTokenOutcome::Resolved(resolved) => {
                    if stage.facts.profile.is_controller()
                        && let Some(child) = self
                            .cursor
                            .passive_child_scope(stage.selection().scope_id, resolved.selected_arm)
                    {
                        // The child owns the next visible event. Carry the affine preview
                        // through every immediate decision; only event commit publishes
                        // the selected ancestry recorded by the resident descriptor.
                        let entry = self
                            .cursor
                            .route_scope_materialization_index(child)
                            .ok_or(RecvError::PhaseInvariant)?;
                        let selection = self.offer_scope_selection_for_scope_lane(
                            child,
                            entry,
                            stage.selection().offer_lane,
                        )?;
                        let facts = self.prepare_frontier_facts(selection, frontier_visited);
                        nested = Some((
                            frontier_visited.take(),
                            OfferCollectState {
                                facts,
                                ingress: core::mem::replace(
                                    &mut stage.ingress,
                                    OfferStagedIngress::empty(),
                                ),
                            },
                        ));
                    } else if stage.facts.profile.is_passive() {
                        let descended = match self.descend_selected_passive_route(
                            stage.selection(),
                            resolved,
                            stage.ingress.transport_frame_key(),
                        ) {
                            Ok(descended) => descended,
                            Err(err) => {
                                stage.discard_terminal();
                                return Poll::Ready(Err(err));
                            }
                        };
                        if descended {
                            restart = Some((
                                frontier_visited.take(),
                                core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty()),
                            ));
                        }
                    }
                    if restart.is_none() && nested.is_none() {
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
        if let Some((frontier_visited, stage)) = nested {
            state.execution = OfferExecution::Collecting {
                frontier_visited,
                stage,
            };
            return Poll::Ready(Ok(None));
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
