use core::task::Poll;

use super::passive::{
    PassiveRouteEvidenceContext, PassiveRouteEvidenceInput, PassiveRouteEvidenceOutcome,
};
use super::{
    Clock, CursorEndpoint, DeferReason, FrameHintResolution, FrontierDeferOutcome,
    FrontierDeferRequest, FrontierVisitSet, OfferAuthorityPath, OfferResolveState, RecvError,
    RecvResult, ResolveTokenOutcome, ResolvedRouteArm, RouteArmCommitEvidence, RouteArmToken,
    RouteResolveStep, Transport,
};
pub(super) struct RouteAuthorityResolution {
    pub(super) route_token: RouteArmToken,
    pub(super) frame_hint: FrameHintResolution,
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
    NoAuthority,
    RestartFrontier,
}

enum PassiveRouteResolutionOutcome {
    Authority(RouteArmToken, FrameHintResolution),
    EvidenceOnly(FrameHintResolution),
    RestartFrontier,
}

impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: Clock,
{
    pub(super) fn resolve_token(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        if !state.pending.is_ready() {
            return self.poll_resolve_pending_state(state, pending_recv, cx);
        }

        let authority =
            match self.collect_route_authority(state, pending_recv, frontier_visited, cx) {
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

    fn collect_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        let selection = state.selection();
        let profile = state.facts.profile;
        let scope_id = selection.scope_id;

        let frame_hint = if self.peek_scope_frame_hint_with_lane(scope_id).is_some() {
            FrameHintResolution::resolved()
        } else {
            FrameHintResolution::unresolved()
        };
        if let Some(route_token) = self.peek_scope_ack(scope_id) {
            return Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                RouteAuthorityResolution {
                    route_token,
                    frame_hint,
                    commit_evidence: RouteArmCommitEvidence::CachedOrDemux,
                },
            )));
        }

        match profile.authority_path_after_ack_miss() {
            OfferAuthorityPath::ControllerResolver => {
                match self.controller_resolver_authority(state, frontier_visited) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(RouteResolveOutcome::Token(route_token))) => Poll::Ready(Ok(
                        RouteAuthorityOutcome::Resolved(RouteAuthorityResolution {
                            route_token,
                            frame_hint,
                            commit_evidence: RouteArmCommitEvidence::CachedOrDemux,
                        }),
                    )),
                    Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                        Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
                    }
                    Poll::Ready(Ok(RouteResolveOutcome::NoAuthority)) => {
                        Poll::Ready(Err(RecvError::PhaseInvariant))
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                }
            }
            OfferAuthorityPath::PassiveEvidence => self
                .collect_passive_route_authority_after_ack_miss(
                    state,
                    pending_recv,
                    frontier_visited,
                    cx,
                    frame_hint,
                ),
            OfferAuthorityPath::LocalSources => self.poll_route_authority_after_local_sources_miss(
                state,
                pending_recv,
                frontier_visited,
                cx,
                frame_hint,
            ),
        }
    }

    fn collect_passive_route_authority_after_ack_miss(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_hint: FrameHintResolution,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        match self.passive_evidence_authority(state, pending_recv, frontier_visited, cx, frame_hint)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(PassiveRouteResolutionOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(PassiveRouteResolutionOutcome::Authority(route_token, frame_hint))) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                    RouteAuthorityResolution {
                        route_token,
                        frame_hint,
                        commit_evidence: RouteArmCommitEvidence::CachedOrDemux,
                    },
                )))
            }
            Poll::Ready(Ok(PassiveRouteResolutionOutcome::EvidenceOnly(frame_hint))) => self
                .collect_route_authority_after_passive_evidence_only(
                    state,
                    pending_recv,
                    frontier_visited,
                    cx,
                    frame_hint,
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
        frame_hint: FrameHintResolution,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        if !state.facts.profile.is_dynamic() {
            return self.poll_route_authority_after_local_sources_miss(
                state,
                pending_recv,
                frontier_visited,
                cx,
                frame_hint,
            );
        }

        match self.passive_dynamic_resolver_authority(state, pending_recv, frontier_visited, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(RouteResolveOutcome::Token(route_token))) => Poll::Ready(Ok(
                RouteAuthorityOutcome::Resolved(RouteAuthorityResolution {
                    route_token,
                    frame_hint,
                    commit_evidence: RouteArmCommitEvidence::CachedOrDemux,
                }),
            )),
            Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(RouteResolveOutcome::NoAuthority)) => self
                .poll_route_authority_after_local_sources_miss(
                    state,
                    pending_recv,
                    frontier_visited,
                    cx,
                    frame_hint,
                ),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn poll_route_authority_after_local_sources_miss(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_hint: FrameHintResolution,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        if state.facts.profile.is_passive()
            && !state.ingress.has_transport()
            && !frame_hint.is_resolved()
        {
            match self.defer_missing_route_authority(
                state,
                pending_recv,
                frontier_visited,
                cx,
                false,
            ) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(_)) => return Poll::Ready(Err(RecvError::PhaseInvariant)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        match self.poll_or_defer_route_authority(state, pending_recv, frontier_visited, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(RouteResolveOutcome::Token(route_token))) => Poll::Ready(Ok(
                RouteAuthorityOutcome::Resolved(RouteAuthorityResolution {
                    route_token,
                    frame_hint,
                    commit_evidence: RouteArmCommitEvidence::PollFrame,
                }),
            )),
            Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(RouteResolveOutcome::NoAuthority)) => {
                Poll::Ready(Err(RecvError::PhaseInvariant))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn controller_resolver_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
    ) -> Poll<RecvResult<RouteResolveOutcome>> {
        let selection = state.selection();
        let scope_id = selection.scope_id;
        loop {
            let resolver_step = match self.prepare_route_arm_selection_from_resolver(scope_id) {
                Ok(step) => step,
                Err(err) => return Poll::Ready(Err(err)),
            };
            match resolver_step {
                RouteResolveStep::Resolved(resolver_arm) => {
                    return Poll::Ready(Ok(RouteResolveOutcome::Token(
                        RouteArmToken::from_resolver(resolver_arm),
                    )));
                }
                RouteResolveStep::Reject(resolver_id) => {
                    return Poll::Ready(Err(RecvError::ResolverReject { resolver_id }));
                }
                RouteResolveStep::NoAuthority => {
                    return Poll::Ready(Ok(RouteResolveOutcome::NoAuthority));
                }
                RouteResolveStep::Deferred => {
                    match self.on_frontier_defer(
                        &mut state.progress,
                        FrontierDeferRequest {
                            scope_id,
                            current_parallel: selection.frontier_parallel_root,
                            reason: DeferReason::Unsupported,
                            offer_lane: selection.offer_lane,
                            ingress_ready: state.ingress.has_transport(),
                            selected_arm: None,
                        },
                        frontier_visited,
                    ) {
                        FrontierDeferOutcome::Continue => continue,
                        FrontierDeferOutcome::Yielded => {
                            return Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier));
                        }
                        FrontierDeferOutcome::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }

    fn passive_evidence_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        frame_hint: FrameHintResolution,
    ) -> Poll<RecvResult<PassiveRouteResolutionOutcome>> {
        let selection = state.selection();
        let offer_lanes = self.offer_lane_set_for_scope(selection.scope_id);
        match self.poll_passive_route_evidence(
            PassiveRouteEvidenceInput {
                selection,
                offer_lanes,
                profile: state.facts.profile,
                frame_hint,
            },
            PassiveRouteEvidenceContext::new(
                &mut state.ingress,
                &mut state.progress,
                frontier_visited,
            ),
            pending_recv,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(PassiveRouteResolutionOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::Authority {
                route_token,
                frame_hint,
            })) => Poll::Ready(Ok(PassiveRouteResolutionOutcome::Authority(
                route_token,
                frame_hint,
            ))),
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::EvidenceOnly { frame_hint })) => {
                Poll::Ready(Ok(PassiveRouteResolutionOutcome::EvidenceOnly(frame_hint)))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn passive_dynamic_resolver_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteResolveOutcome>> {
        let selection = state.selection();
        let scope_id = selection.scope_id;
        let resolver_step = match self.prepare_route_arm_selection_from_resolver(scope_id) {
            Ok(step) => step,
            Err(err) => return Poll::Ready(Err(err)),
        };
        match resolver_step {
            RouteResolveStep::Resolved(resolver_arm) => Poll::Ready(Ok(
                RouteResolveOutcome::Token(RouteArmToken::from_resolver(resolver_arm)),
            )),
            RouteResolveStep::Reject(resolver_id) => {
                Poll::Ready(Err(RecvError::ResolverReject { resolver_id }))
            }
            RouteResolveStep::NoAuthority => Poll::Ready(Ok(RouteResolveOutcome::NoAuthority)),
            RouteResolveStep::Deferred => match self.on_frontier_defer(
                &mut state.progress,
                FrontierDeferRequest {
                    scope_id,
                    current_parallel: selection.frontier_parallel_root,
                    reason: DeferReason::Unsupported,
                    offer_lane: selection.offer_lane,
                    ingress_ready: state.ingress.has_transport(),
                    selected_arm: None,
                },
                frontier_visited,
            ) {
                FrontierDeferOutcome::Continue => Poll::Ready(Ok(RouteResolveOutcome::NoAuthority)),
                FrontierDeferOutcome::Yielded => {
                    state.pending.arm_yield_restart();
                    self.poll_resolve_pending_as(
                        state,
                        pending_recv,
                        cx,
                        RouteResolveOutcome::RestartFrontier,
                    )
                }
                FrontierDeferOutcome::Pending => Poll::Pending,
            },
        }
    }

    fn poll_or_defer_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteResolveOutcome>> {
        let selection = state.selection();
        if state.facts.profile.is_passive()
            && state
                .ingress
                .transport_lane_wire()
                .is_some_and(|lane| lane != selection.offer_lane)
        {
            return Poll::Ready(Ok(RouteResolveOutcome::RestartFrontier));
        }
        let offer_lanes = self.offer_lane_set_for_scope(selection.scope_id);
        if let Some(poll_arm) =
            self.try_poll_route_arm_selection_for_offer(selection.scope_id, offer_lanes, cx)
        {
            Poll::Ready(Ok(RouteResolveOutcome::Token(RouteArmToken::from_poll(
                poll_arm,
            ))))
        } else {
            self.defer_missing_route_authority(
                state,
                pending_recv,
                frontier_visited,
                cx,
                state.ingress.has_transport(),
            )
        }
    }

    fn defer_missing_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        has_ingress: bool,
    ) -> Poll<RecvResult<RouteResolveOutcome>> {
        let selection = state.selection();
        match self.on_frontier_defer(
            &mut state.progress,
            FrontierDeferRequest {
                scope_id: selection.scope_id,
                current_parallel: selection.frontier_parallel_root,
                reason: DeferReason::NoEvidence,
                offer_lane: selection.offer_lane,
                ingress_ready: has_ingress,
                selected_arm: None,
            },
            frontier_visited,
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
