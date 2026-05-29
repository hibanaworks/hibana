use core::task::Poll;

use super::passive::{
    PassiveRouteEvidenceContext, PassiveRouteEvidenceInput, PassiveRouteEvidenceOutcome,
};
use super::{
    Clock, CursorEndpoint, DeferReason, DeferSource, EndpointSlot, EpochTable,
    FrontierDeferOutcome, FrontierVisitSet, LabelUniverse, MintConfigMarker, OfferAuthorityPath,
    OfferResolveState, PolicySlot, RecvError, RecvResult, ResolveTokenOutcome, ResolvedFrameHint,
    ResolvedRouteDecision, RouteDecisionCommitEvidence, RouteDecisionToken, RouteResolveStep,
    Transport,
};
pub(super) struct RouteAuthorityResolution {
    pub(super) route_token: RouteDecisionToken,
    pub(super) resolved_hint_frame: Option<ResolvedFrameHint>,
    pub(super) commit_evidence: RouteDecisionCommitEvidence,
}

enum RouteAuthorityOutcome {
    Resolved(RouteAuthorityResolution),
    RestartFrontier,
}

pub(super) enum MaterializationReadyOutcome {
    Ready(ResolvedRouteDecision),
    RestartFrontier,
}

enum RouteAuthoritySourceOutcome {
    Token(RouteDecisionToken),
    NoAuthority,
    RestartFrontier,
}

enum PassiveRouteAuthorityOutcome {
    Authority(RouteDecisionToken, Option<ResolvedFrameHint>),
    EvidenceOnly(Option<ResolvedFrameHint>),
    RestartFrontier,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
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
        let offer_lane = selection.offer_lane;

        let resolved_hint_frame = self
            .peek_scope_frame_hint_with_lane(scope_id)
            .map(|(lane, frame_label)| ResolvedFrameHint { lane, frame_label });
        if state.ingress.has_transport()
            && let Some(frame_hint) = resolved_hint_frame
        {
            let frame_label_meta = self.selection_frame_label_meta(selection);
            self.mark_scope_ready_arm_from_frame_label(
                scope_id,
                offer_lane,
                frame_hint.frame_label,
                frame_label_meta,
            );
        }

        if let Some(route_token) = self.peek_scope_ack(scope_id) {
            return Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                RouteAuthorityResolution {
                    route_token,
                    resolved_hint_frame,
                    commit_evidence: RouteDecisionCommitEvidence::CachedOrDemux,
                },
            )));
        }

        match profile.authority_path_after_ack_miss() {
            OfferAuthorityPath::ControllerResolver => match self
                .controller_resolver_authority(state, frontier_visited)
            {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(route_token))) => Poll::Ready(
                    Ok(RouteAuthorityOutcome::Resolved(RouteAuthorityResolution {
                        route_token,
                        resolved_hint_frame,
                        commit_evidence: RouteDecisionCommitEvidence::CachedOrDemux,
                    })),
                ),
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority)) => {
                    Poll::Ready(Err(RecvError::PhaseInvariant))
                }
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            },
            OfferAuthorityPath::PassiveEvidence => self
                .collect_passive_route_authority_after_ack_miss(
                    state,
                    pending_recv,
                    frontier_visited,
                    cx,
                    resolved_hint_frame,
                ),
            OfferAuthorityPath::LocalSources => self.poll_route_authority_after_local_sources_miss(
                state,
                pending_recv,
                frontier_visited,
                cx,
                resolved_hint_frame,
            ),
        }
    }

    fn collect_passive_route_authority_after_ack_miss(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        resolved_hint_frame: Option<ResolvedFrameHint>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        match self.passive_evidence_authority(
            state,
            pending_recv,
            frontier_visited,
            cx,
            resolved_hint_frame,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(PassiveRouteAuthorityOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(PassiveRouteAuthorityOutcome::Authority(
                route_token,
                resolved_hint_frame,
            ))) => Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                RouteAuthorityResolution {
                    route_token,
                    resolved_hint_frame,
                    commit_evidence: RouteDecisionCommitEvidence::CachedOrDemux,
                },
            ))),
            Poll::Ready(Ok(PassiveRouteAuthorityOutcome::EvidenceOnly(resolved_hint_frame))) => {
                self.collect_route_authority_after_passive_evidence_only(
                    state,
                    pending_recv,
                    frontier_visited,
                    cx,
                    resolved_hint_frame,
                )
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn collect_route_authority_after_passive_evidence_only(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        resolved_hint_frame: Option<ResolvedFrameHint>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        if state.facts.profile.is_dynamic() {
            match self.passive_dynamic_resolver_authority(state, pending_recv, frontier_visited, cx)
            {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(route_token))) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
                        RouteAuthorityResolution {
                            route_token,
                            resolved_hint_frame,
                            commit_evidence: RouteDecisionCommitEvidence::CachedOrDemux,
                        },
                    )));
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority)) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        self.poll_route_authority_after_local_sources_miss(
            state,
            pending_recv,
            frontier_visited,
            cx,
            resolved_hint_frame,
        )
    }

    fn poll_route_authority_after_local_sources_miss(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        resolved_hint_frame: Option<ResolvedFrameHint>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        if state.facts.profile.is_passive()
            && !state.ingress.has_transport()
            && !state.ingress.has_binding()
            && resolved_hint_frame.is_none()
        {
            match self.defer_missing_route_authority(
                state,
                pending_recv,
                frontier_visited,
                cx,
                false,
            ) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(_)) => return Poll::Ready(Err(RecvError::PhaseInvariant)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        match self.poll_or_defer_route_authority(state, pending_recv, frontier_visited, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(route_token))) => Poll::Ready(Ok(
                RouteAuthorityOutcome::Resolved(RouteAuthorityResolution {
                    route_token,
                    resolved_hint_frame,
                    commit_evidence: RouteDecisionCommitEvidence::PollFrame,
                }),
            )),
            Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority)) => {
                Poll::Ready(Err(RecvError::PhaseInvariant))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn controller_resolver_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection();
        let scope_id = selection.scope_id;
        loop {
            let decision_signals = self.policy_signals_for_slot(PolicySlot::Decision);
            let resolver_step =
                match self.prepare_route_decision_from_resolver(scope_id, &decision_signals) {
                    Ok(step) => step,
                    Err(err) => return Poll::Ready(Err(err)),
                };
            match resolver_step {
                RouteResolveStep::Resolved(resolver_arm) => {
                    return Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(
                        RouteDecisionToken::from_resolver(resolver_arm),
                    )));
                }
                RouteResolveStep::Abort(reason) => {
                    return Poll::Ready(Err(RecvError::PolicyAbort { reason }));
                }
                RouteResolveStep::Deferred { source } => {
                    match self.on_frontier_defer(
                        &mut state.progress,
                        scope_id,
                        selection.frontier_parallel_root,
                        source,
                        DeferReason::Unsupported,
                        selection.offer_lane,
                        state.ingress.has_binding(),
                        None,
                        frontier_visited,
                    ) {
                        FrontierDeferOutcome::Continue => {}
                        FrontierDeferOutcome::Yielded => {
                            return Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier));
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
        resolved_hint_frame: Option<ResolvedFrameHint>,
    ) -> Poll<RecvResult<PassiveRouteAuthorityOutcome>> {
        let selection = state.selection();
        let offer_lanes = self.offer_lane_set_for_scope(selection.scope_id);
        match self.poll_passive_route_evidence(
            PassiveRouteEvidenceInput {
                selection,
                offer_lanes,
                profile: state.facts.profile,
                resolved_hint_frame,
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
                Poll::Ready(Ok(PassiveRouteAuthorityOutcome::RestartFrontier))
            }
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::Authority {
                authority,
                resolved_hint_frame,
            })) => Poll::Ready(Ok(PassiveRouteAuthorityOutcome::Authority(
                authority.into_route_token(),
                resolved_hint_frame,
            ))),
            Poll::Ready(Ok(PassiveRouteEvidenceOutcome::EvidenceOnly {
                resolved_hint_frame,
            })) => Poll::Ready(Ok(PassiveRouteAuthorityOutcome::EvidenceOnly(
                resolved_hint_frame,
            ))),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    fn passive_dynamic_resolver_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection();
        let scope_id = selection.scope_id;
        let decision_signals = self.policy_signals_for_slot(PolicySlot::Decision);
        let resolver_step =
            match self.prepare_route_decision_from_resolver(scope_id, &decision_signals) {
                Ok(step) => step,
                Err(err) => return Poll::Ready(Err(err)),
            };
        match resolver_step {
            RouteResolveStep::Resolved(resolver_arm) => Poll::Ready(Ok(
                RouteAuthoritySourceOutcome::Token(RouteDecisionToken::from_resolver(resolver_arm)),
            )),
            RouteResolveStep::Abort(reason) => {
                if reason != 0 {
                    Poll::Ready(Err(RecvError::PolicyAbort { reason }))
                } else {
                    Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority))
                }
            }
            RouteResolveStep::Deferred { source } => match self.on_frontier_defer(
                &mut state.progress,
                scope_id,
                selection.frontier_parallel_root,
                source,
                DeferReason::Unsupported,
                selection.offer_lane,
                state.ingress.has_binding(),
                None,
                frontier_visited,
            ) {
                FrontierDeferOutcome::Continue => {
                    Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority))
                }
                FrontierDeferOutcome::Yielded => {
                    state.pending.arm_yield_restart();
                    self.poll_resolve_pending_as(
                        state,
                        pending_recv,
                        cx,
                        RouteAuthoritySourceOutcome::RestartFrontier,
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
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection();
        if state.facts.profile.is_passive()
            && state
                .ingress
                .transport_lane_wire()
                .map(|lane| lane != selection.offer_lane)
                .unwrap_or(false)
        {
            return Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier));
        }
        let offer_lanes = self.offer_lane_set_for_scope(selection.scope_id);
        if let Some(poll_arm) =
            self.try_poll_route_decision_for_offer(selection.scope_id, offer_lanes, cx)
        {
            Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(
                RouteDecisionToken::from_poll(poll_arm),
            )))
        } else {
            let has_binding = state.ingress.has_binding();
            self.defer_missing_route_authority(
                state,
                pending_recv,
                frontier_visited,
                cx,
                has_binding,
            )
        }
    }

    fn defer_missing_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        has_binding: bool,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection();
        match self.on_frontier_defer(
            &mut state.progress,
            selection.scope_id,
            selection.frontier_parallel_root,
            DeferSource::Resolver,
            DeferReason::NoEvidence,
            selection.offer_lane,
            has_binding,
            None,
            frontier_visited,
        ) {
            FrontierDeferOutcome::Continue | FrontierDeferOutcome::Yielded => {
                state.pending.arm_yield_restart();
                self.poll_resolve_pending_as(
                    state,
                    pending_recv,
                    cx,
                    RouteAuthoritySourceOutcome::RestartFrontier,
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
