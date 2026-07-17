use core::task::Poll;

use super::passive::{
    PassiveRouteEvidenceContext, PassiveRouteEvidenceInput, PassiveRouteEvidenceOutcome,
};
use super::{
    Arm, CursorEndpoint, FrameEvidenceResolution, FrontierDeferOutcome, FrontierDeferRequest,
    FrontierScratchWorkspace, FrontierVisitSet, IngressEvidenceState, OfferAuthorityPath,
    OfferResolveState, RecvError, RecvResult, ResolveTokenOutcome, ResolvedRouteArm,
    RouteArmCommitEvidence, RouteArmToken, RouteResolveStep, Transport,
};
pub(super) struct RouteAuthorityResolution {
    pub(super) route_token: RouteArmToken,
    pub(super) commit_evidence: RouteArmCommitEvidence,
}

enum RouteAuthorityOutcome {
    Resolved(RouteAuthorityResolution),
    RestartFrontier,
}

pub(super) enum MaterializationReadyOutcome {
    Ready(ResolvedRouteArm),
    RestartFrontier,
}

enum RouteResolveOutcome {
    Token(RouteArmToken),
    RestartFrontier,
}

enum PassiveRouteAuthorityOutcome {
    EvidenceOnly(FrameEvidenceResolution),
    RestartFrontier,
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    pub(super) fn resolve_token(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        if !state.pending.is_ready() {
            return self.poll_resolve_pending_state(state, pending_recv, cx);
        }

        let authority = match self.collect_route_authority(
            state,
            pending_recv,
            frontier_visited,
            cx,
            scratch,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier)) => {
                return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
            }
            Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(authority))) => authority,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        let resolved = match self.ensure_materialization_ready(
            state,
            pending_recv,
            authority,
            frontier_visited,
            cx,
            scratch,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(MaterializationReadyOutcome::RestartFrontier)) => {
                return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
            }
            Poll::Ready(Ok(MaterializationReadyOutcome::Ready(resolved))) => resolved,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        Poll::Ready(Ok(ResolveTokenOutcome::Resolved(resolved)))
    }

    #[inline(never)]
    fn collect_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        let selection = state.selection();
        let profile = state.facts.profile;
        let scope_id = selection.scope_id;

        let frame_evidence = FrameEvidenceResolution::unresolved();
        if profile.frame_evidence_is_branch_authority() {
            match self.staged_transport_passive_route_token(state, scope_id) {
                Ok(Some(route_token)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                        RouteAuthorityResolution {
                            route_token,
                            commit_evidence: RouteArmCommitEvidence::PollFrame,
                        },
                    )));
                }
                Ok(None) => {}
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
        match profile.authority_path() {
            OfferAuthorityPath::ControllerResolver => {
                match self.controller_resolver_authority(state, frontier_visited) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(RouteResolveOutcome::Token(route_token))) => Poll::Ready(Ok(
                        RouteAuthorityOutcome::Resolved(RouteAuthorityResolution {
                            route_token,
                            commit_evidence: RouteArmCommitEvidence::Resolver,
                        }),
                    )),
                    Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                        Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                }
            }
            OfferAuthorityPath::PassiveEvidence => self.collect_passive_route_authority(
                state,
                pending_recv,
                frontier_visited,
                cx,
                frame_evidence,
                scratch,
            ),
            OfferAuthorityPath::LocalSources => self.poll_route_authority_after_local_sources_miss(
                state,
                pending_recv,
                frontier_visited,
                cx,
                frame_evidence,
                scratch,
            ),
        }
    }

    fn staged_transport_passive_route_token(
        &mut self,
        state: &mut OfferResolveState<'r>,
        scope_id: crate::global::const_dsl::ScopeId,
    ) -> RecvResult<Option<RouteArmToken>> {
        let Some(key) = state.ingress.transport_frame_key() else {
            return Ok(None);
        };
        let (arm, marks_descendant) = if let Some(target_idx) =
            state.selection().observed_target_index()
        {
            let mut selected = None;
            self.cursor
                .visit_route_arms_for_index(target_idx, |candidate_scope, candidate_arm| {
                    if candidate_scope == scope_id {
                        selected = Some(candidate_arm);
                    }
                });
            let Some(selected) = selected else {
                return Ok(None);
            };
            (Arm::from_raw(selected), false)
        } else {
            let Some(selected) = self
                .cursor
                .passive_descendant_dispatch_arm_for_key(scope_id, key)
            else {
                return Ok(None);
            };
            (Arm::from_raw(selected), true)
        };
        if marks_descendant {
            self.mark_intrinsic_passive_descendant_path_ready(scope_id, key);
        }
        self.mark_scope_ready_arm_from_exact_passive_arm(scope_id, arm);
        Ok(Some(RouteArmToken::from_poll(arm)))
    }

    fn collect_passive_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_evidence: FrameEvidenceResolution,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        match self.passive_evidence_authority(
            state,
            pending_recv,
            frontier_visited,
            cx,
            frame_evidence,
            scratch,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(PassiveRouteAuthorityOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(PassiveRouteAuthorityOutcome::EvidenceOnly(frame_evidence))) => self
                .collect_route_authority_after_passive_evidence_only(
                    state,
                    pending_recv,
                    frontier_visited,
                    cx,
                    frame_evidence,
                    scratch,
                ),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn collect_route_authority_after_passive_evidence_only(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_evidence: FrameEvidenceResolution,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        let scope_id = state.selection().scope_id;
        if state.facts.profile.frame_evidence_is_branch_authority() {
            match self.staged_transport_passive_route_token(state, scope_id) {
                Ok(Some(route_token)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                        RouteAuthorityResolution {
                            route_token,
                            commit_evidence: RouteArmCommitEvidence::PollFrame,
                        },
                    )));
                }
                Ok(None) => {}
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
        self.poll_route_authority_after_local_sources_miss(
            state,
            pending_recv,
            frontier_visited,
            cx,
            frame_evidence,
            scratch,
        )
    }

    #[inline(never)]
    fn poll_route_authority_after_local_sources_miss(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_evidence: FrameEvidenceResolution,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        if state.facts.profile.is_passive()
            && !state.ingress.has_transport()
            && !frame_evidence.is_resolved()
        {
            match self.defer_missing_route_authority(
                state,
                pending_recv,
                frontier_visited,
                cx,
                IngressEvidenceState::Absent,
                scratch,
            ) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(_)) => return Poll::Ready(Err(RecvError::PhaseInvariant)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        self.poll_or_defer_route_authority(state, pending_recv, frontier_visited, cx, scratch)
    }

    fn controller_resolver_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        _frontier_visited: &mut FrontierVisitSet,
    ) -> Poll<RecvResult<RouteResolveOutcome>> {
        let selection = state.selection();
        let scope_id = selection.scope_id;
        match self.prepare_route_arm_selection_from_resolver(scope_id) {
            Ok(RouteResolveStep::Resolved(resolver_arm)) => Poll::Ready(Ok(
                RouteResolveOutcome::Token(RouteArmToken::from_resolver(resolver_arm)),
            )),
            Ok(RouteResolveStep::Reject(resolver_id)) => {
                Poll::Ready(Err(RecvError::ResolverReject { resolver_id }))
            }
            Err(err) => Poll::Ready(Err(err)),
        }
    }

    fn passive_evidence_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_evidence: FrameEvidenceResolution,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<PassiveRouteAuthorityOutcome>> {
        let selection = state.selection();
        match self.poll_passive_route_evidence(
            PassiveRouteEvidenceInput {
                selection,
                frame_evidence,
            },
            PassiveRouteEvidenceContext::new(
                &mut state.ingress,
                &mut state.progress,
                frontier_visited,
            ),
            pending_recv,
            cx,
            scratch,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(PassiveRouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::EvidenceOnly { frame_evidence })) => {
                Poll::Ready(Ok(PassiveRouteAuthorityOutcome::EvidenceOnly(
                    frame_evidence,
                )))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    #[inline(never)]
    fn poll_or_defer_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        let selection = state.selection();
        if state.facts.profile.is_passive()
            && state
                .ingress
                .transport_lane_wire()
                .is_some_and(|lane| lane != selection.offer_lane)
        {
            return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
        }
        if let Some(authority) = self.poll_route_authority(selection.scope_id) {
            return Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(authority)));
        }
        match self.defer_missing_route_authority(
            state,
            pending_recv,
            frontier_visited,
            cx,
            state.ingress.evidence_state(),
            scratch,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(RouteResolveOutcome::Token(_))) => {
                Poll::Ready(Err(RecvError::PhaseInvariant))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    pub(super) fn poll_route_authority(
        &self,
        scope_id: crate::global::const_dsl::ScopeId,
    ) -> Option<RouteAuthorityResolution> {
        let is_dynamic_route_scope = self.cursor.route_scope_resolver(scope_id).is_some();
        if is_dynamic_route_scope {
            return None;
        }
        self.poll_arm_from_ready_mask(scope_id)
            .map(|arm| RouteAuthorityResolution {
                route_token: RouteArmToken::from_poll(arm),
                commit_evidence: RouteArmCommitEvidence::PollFrame,
            })
    }

    fn defer_missing_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        ingress: IngressEvidenceState,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> Poll<RecvResult<RouteResolveOutcome>> {
        let selection = state.selection();
        match self.on_frontier_defer(
            &mut state.progress,
            FrontierDeferRequest {
                scope_id: selection.scope_id,
                current_parallel: selection.frontier_parallel_root,
                ingress,
            },
            frontier_visited,
            scratch,
        ) {
            FrontierDeferOutcome::Continue | FrontierDeferOutcome::Yielded => {
                state.pending.arm_yield_restart();
                self.poll_resolve_pending_as(
                    state,
                    pending_recv,
                    cx,
                    RouteResolveOutcome::RestartFrontier,
                )
            }
            FrontierDeferOutcome::Pending => Poll::Pending,
        }
    }

    pub(super) fn poll_resolve_pending_as<Outcome>(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
        restart: Outcome,
    ) -> Poll<RecvResult<Outcome>> {
        match self.poll_resolve_pending_state(state, pending_recv, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier)) => Poll::Ready(Ok(restart)),
            Poll::Ready(Ok(ResolveTokenOutcome::Resolved(_))) => {
                Poll::Ready(Err(RecvError::PhaseInvariant))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}
