//! Offer-path helpers for scope selection and branch materialization.

mod commit;
mod commit_types;
mod facts;
mod first_recv_dispatch;
mod frontier_types;
#[cfg(all(test, hibana_repo_tests))]
mod handoff_tests;
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
mod select_candidates;
mod select_observed;
mod select_scope_selection;
mod state;
use core::task::Poll;

use super::authority::RouteResolveStep;
pub(in crate::endpoint::kernel) use super::authority::{Arm, RouteArmToken};
use super::core::OperationKernel;
use super::frontier::FrontierVisitSet;
use super::lane_port;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{LANE_SET_VIEW_WORDS, LaneWord, lane_word_index};
use crate::global::typestate::{InboundFrameKey, state_index_to_usize};
use crate::transport::Transport;

pub(in crate::endpoint::kernel) use self::commit_types::{
    BranchCommitPlan, BranchKind, BranchMeta,
};
pub(in crate::endpoint::kernel) use self::frontier_types::{
    CachedRecvMeta, ScopeArmMaterializationMeta,
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
    AdmittedOfferEvent, ResolveTokenOutcome, ResolvedRouteArm, RouteArmCommitEvidence,
};
use self::select::FrontierDeferRequest;
pub(in crate::endpoint::kernel) use self::state::OfferState;
use self::state::{
    OfferCollectState, OfferExecution, OfferExecutionKind, OfferResolveState, OfferStagedIngress,
};
pub(in crate::endpoint::kernel) use super::core::IngressEvidenceState;

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(crate) fn poll_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<u8>> {
        // Poll each transport direction at most once in this call. Pending
        // registers a wakeup; this mask is discarded before the next poll.
        let mut inspected_lanes = [0; LANE_SET_VIEW_WORDS];
        loop {
            let step = match state.execution.kind() {
                OfferExecutionKind::Uninitialized => self.poll_offer_uninitialized(state),
                OfferExecutionKind::Selecting => {
                    self.poll_offer_selecting(state, cx, &mut inspected_lanes)
                }
                OfferExecutionKind::Collecting => {
                    self.poll_offer_collecting(state, cx, &mut inspected_lanes)
                }
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
        if !self.progress.cursor.has_route_scopes() {
            return Poll::Ready(Err(RecvError::PhaseInvariant));
        }
        state.execution = OfferExecution::Selecting {
            frontier_visited: self.progress.frontier_state.empty_frontier_visit_set(),
        };
        Poll::Ready(Ok(None))
    }

    #[inline(never)]
    fn poll_offer_selecting(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
        inspected_lanes: &mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> Poll<RecvResult<Option<u8>>> {
        let projected = if state.carried_transport_lane_wire().is_none() {
            let OfferExecution::Selecting { frontier_visited } = &state.execution else {
                crate::invariant();
            };
            let frontier = self.read_projected_offer_frontier(frontier_visited, None)?;
            match self.poll_any_active_offer_transport_frame(
                &mut state.pending_recv,
                cx,
                inspected_lanes,
                &frontier.incoming,
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
            Some(frontier)
        } else {
            None
        };
        let carried_lane = state.carried_transport_lane_wire();
        let carried_key = state.carried_transport_frame_key();
        let carried_observation =
            state.carried_transport_observation(self.context.sid.raw(), self.public_header.role());
        let selection_result = {
            let OfferExecution::Selecting { frontier_visited } = &mut state.execution else {
                crate::invariant();
            };
            self.select_scope(
                carried_lane,
                carried_key,
                carried_observation,
                frontier_visited,
                projected,
            )
        };
        let selection = match selection_result {
            Ok(None) => {
                // Lookup visits belong to one search. No frame was selected,
                // so the next external wake must be able to search afresh.
                state.execution = OfferExecution::Selecting {
                    frontier_visited: self.progress.frontier_state.empty_frontier_visit_set(),
                };
                return Poll::Pending;
            }
            Ok(Some(ObservedIngressSelection::Branch(selection))) => selection,
            Ok(Some(ObservedIngressSelection::OrdinaryReceive(lane))) => {
                let mut ingress = state.take_carried_ingress();
                let payload = crate::invariant_some(ingress.take_transport());
                if let Err(error) = self.requeue_offer_transport_payload(payload) {
                    state.discard_terminal();
                    return Poll::Ready(Err(error));
                }
                // Ordinary input remains on its transport direction until
                // recv consumes it. Do not inspect it twice in this poll.
                let (word, bit) = lane_word_index(usize::from(lane));
                inspected_lanes[word] |= bit;
                return Poll::Ready(Ok(None));
            }
            Ok(Some(ObservedIngressSelection::PendingReceive(lane))) => {
                let mut ingress = state.take_carried_ingress();
                let payload = crate::invariant_some(ingress.take_transport());
                if let Err(error) = self.requeue_offer_transport_payload(payload) {
                    state.discard_terminal();
                    return Poll::Ready(Err(error));
                }
                let (word, bit) = lane_word_index(usize::from(lane));
                inspected_lanes[word] |= bit;
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
        inspected_lanes: &mut [LaneWord; LANE_SET_VIEW_WORDS],
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
                self.collect_offer_ingress(
                    &mut state.pending_recv,
                    stage.facts,
                    cx,
                    inspected_lanes,
                )
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
                    let incoming = self.read_projected_offer_incoming(inspected_lanes);
                    match self.poll_any_active_offer_transport_frame(
                        &mut state.pending_recv,
                        cx,
                        inspected_lanes,
                        &incoming,
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
                        frontier_visited.take(),
                        core::mem::replace(&mut stage.ingress, OfferStagedIngress::empty()),
                    ));
                }
                ResolveTokenOutcome::Resolved(resolved) => {
                    if stage.facts.profile.is_controller()
                        && let Some(child) = self
                            .progress
                            .cursor
                            .passive_child_scope(stage.selection().scope_id, resolved.selected_arm)
                        && resolved.outbound.is_none()
                    {
                        // With no independent send head, the child owns the next
                        // visible event. Carry the affine preview
                        // through every immediate decision; only event commit publishes
                        // the selected ancestry recorded by the resident descriptor.
                        let entry = self
                            .progress
                            .cursor
                            .route_scope_materialization_index(child)
                            .ok_or(RecvError::PhaseInvariant)?;
                        let selection = self.offer_scope_selection_for_scope_lane(
                            child,
                            entry,
                            self.offer_lane_for_scope(child),
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
                            &resolved,
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
                        match self.prepare_offer_event(
                            selection,
                            resolved,
                            stage.facts.profile,
                            &mut stage.ingress,
                        ) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Ok(Some(plan))) => {
                                branch_label = Some(self.produce_branch(plan));
                            }
                            Poll::Ready(Ok(None)) => {
                                restart = Some((
                                    frontier_visited.take(),
                                    core::mem::replace(
                                        &mut stage.ingress,
                                        OfferStagedIngress::empty(),
                                    ),
                                ));
                            }
                            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
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
