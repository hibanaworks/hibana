use super::passive::*;
use super::*;
struct RouteAuthorityResolution {
    route_token: RouteDecisionToken,
    resolved_hint_frame: Option<ResolvedFrameHint>,
    commit_evidence: RouteDecisionCommitEvidence,
}

enum RouteAuthorityOutcome {
    Resolved(RouteAuthorityResolution),
    RestartFrontier,
}

enum MaterializationReadyOutcome {
    Ready(u8),
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
    pub(super) fn resolve_token(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        if !state.pending.is_ready() {
            return self.poll_resolve_pending_state(state, cx);
        }

        let mut authority = match self.collect_route_authority(state, frontier_visited, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier)) => {
                return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
            }
            Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(authority))) => authority,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        let selected_arm =
            match self.ensure_materialization_ready(state, &mut authority, frontier_visited, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(MaterializationReadyOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(MaterializationReadyOutcome::Ready(selected_arm))) => selected_arm,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            };

        Poll::Ready(Ok(ResolveTokenOutcome::Resolved(ResolvedRouteDecision {
            route_token: authority.route_token,
            selected_arm,
            resolved_hint_frame_label: authority.resolved_hint_frame.map(|frame| frame.frame_label),
            route_decision_commit_evidence: authority.commit_evidence,
        })))
    }

    fn collect_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteAuthorityOutcome>> {
        let selection = state.selection;
        let is_route_controller = state.facts.is_route_controller;
        let is_dynamic_route_scope = state.facts.is_dynamic_route_scope;
        let scope_id = selection.scope_id;
        let offer_lane = selection.offer_lane;

        let mut resolved_hint_frame = self
            .endpoint
            .peek_scope_frame_hint_with_lane(scope_id)
            .map(|(lane, frame_label)| ResolvedFrameHint { lane, frame_label });
        let mut commit_evidence = RouteDecisionCommitEvidence::CachedOrDemux;
        if state.ingress.has_transport()
            && let Some(frame_hint) = resolved_hint_frame
        {
            let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
            self.endpoint.mark_scope_ready_arm_from_frame_label(
                scope_id,
                offer_lane,
                frame_hint.frame_label,
                frame_label_meta,
            );
        }

        let mut route_token = self.endpoint.peek_scope_ack(scope_id);
        if route_token.is_none() && is_route_controller && is_dynamic_route_scope {
            match self.controller_resolver_authority(state, frontier_visited) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(token))) => {
                    route_token = Some(token);
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority)) => {
                    return Poll::Ready(Err(RecvError::PhaseInvariant));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        if route_token.is_none() && !is_route_controller {
            match self.passive_evidence_authority(state, frontier_visited, cx, resolved_hint_frame)
            {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(PassiveRouteAuthorityOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(PassiveRouteAuthorityOutcome::Authority(
                    token,
                    passive_resolved_hint_frame,
                ))) => {
                    route_token = Some(token);
                    resolved_hint_frame = passive_resolved_hint_frame;
                }
                Poll::Ready(Ok(PassiveRouteAuthorityOutcome::EvidenceOnly(
                    passive_resolved_hint_frame,
                ))) => {
                    resolved_hint_frame = passive_resolved_hint_frame;
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        if route_token.is_none() && !is_route_controller && is_dynamic_route_scope {
            match self.passive_dynamic_resolver_authority(state, frontier_visited, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(token))) => {
                    route_token = Some(token);
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority)) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        if route_token.is_none()
            && !is_route_controller
            && !state.ingress.has_transport()
            && !state.ingress.has_binding()
            && resolved_hint_frame.is_none()
        {
            match self.defer_missing_route_authority(state, frontier_visited, cx, false) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(_)) => return Poll::Ready(Err(RecvError::PhaseInvariant)),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        if route_token.is_none() {
            match self.poll_or_defer_route_authority(state, frontier_visited, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(token))) => {
                    route_token = Some(token);
                    commit_evidence = RouteDecisionCommitEvidence::PollFrame;
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier)) => {
                    return Poll::Ready(Ok(RouteAuthorityOutcome::RestartFrontier));
                }
                Poll::Ready(Ok(RouteAuthoritySourceOutcome::NoAuthority)) => {
                    return Poll::Ready(Err(RecvError::PhaseInvariant));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        let Some(route_token) = route_token else {
            return Poll::Ready(Err(RecvError::PhaseInvariant));
        };
        Poll::Ready(Ok(RouteAuthorityOutcome::Resolved(
            RouteAuthorityResolution {
                route_token,
                resolved_hint_frame,
                commit_evidence,
            },
        )))
    }

    fn controller_resolver_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection;
        let scope_id = selection.scope_id;
        loop {
            let route_signals = self
                .endpoint
                .policy_signals_for_slot(PolicySlot::Route)
                .into_owned();
            let resolver_step = match self
                .endpoint
                .prepare_route_decision_from_resolver(scope_id, &route_signals)
            {
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
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        resolved_hint_frame: Option<ResolvedFrameHint>,
    ) -> Poll<RecvResult<PassiveRouteAuthorityOutcome>> {
        let selection = state.selection;
        let offer_lanes = self.endpoint.offer_lane_set_for_scope(selection.scope_id);
        let is_dynamic_route_scope = state.facts.is_dynamic_route_scope;
        match self.poll_passive_route_evidence(
            PassiveRouteEvidenceInput {
                selection,
                offer_lanes,
                is_dynamic_route_scope,
                resolved_hint_frame,
            },
            PassiveRouteEvidenceContext::new(
                &mut state.ingress,
                &mut state.progress,
                frontier_visited,
            ),
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
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection;
        let scope_id = selection.scope_id;
        let route_signals = self
            .endpoint
            .policy_signals_for_slot(PolicySlot::Route)
            .into_owned();
        let resolver_step = match self
            .endpoint
            .prepare_route_decision_from_resolver(scope_id, &route_signals)
        {
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
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection;
        if !state.facts.is_route_controller
            && state
                .ingress
                .transport_lane_wire()
                .map(|lane| lane != selection.offer_lane)
                .unwrap_or(false)
        {
            return Poll::Ready(Ok(RouteAuthoritySourceOutcome::RestartFrontier));
        }
        let offer_lanes = self.endpoint.offer_lane_set_for_scope(selection.scope_id);
        if let Some(poll_arm) =
            self.try_poll_route_decision_for_offer(selection.scope_id, offer_lanes, cx)
        {
            Poll::Ready(Ok(RouteAuthoritySourceOutcome::Token(
                RouteDecisionToken::from_poll(poll_arm),
            )))
        } else {
            let has_binding = state.ingress.has_binding();
            self.defer_missing_route_authority(state, frontier_visited, cx, has_binding)
        }
    }

    fn defer_missing_route_authority(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        has_binding: bool,
    ) -> Poll<RecvResult<RouteAuthoritySourceOutcome>> {
        let selection = state.selection;
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
                    cx,
                    RouteAuthoritySourceOutcome::RestartFrontier,
                )
            }
            FrontierDeferOutcome::Pending => Poll::Pending,
        }
    }

    fn ensure_materialization_ready(
        &mut self,
        state: &mut OfferResolveState<'r>,
        authority: &mut RouteAuthorityResolution,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MaterializationReadyOutcome>> {
        self.mark_materialization_ready_from_ingress(state, authority.route_token);

        let mut route_token = authority.route_token;
        let mut commit_evidence = authority.commit_evidence;

        let selected_arm = loop {
            let selected_arm = route_token.arm().as_u8();
            if !self.selected_arm_missing_materialization_evidence(state, selected_arm) {
                break selected_arm;
            }
            if let Some(poll_token) = self.poll_unready_resolver_authority(state, route_token, cx) {
                route_token = poll_token;
                commit_evidence = RouteDecisionCommitEvidence::PollFrame;
                continue;
            }
            return self.rollback_and_defer_unready_materialization(
                state,
                frontier_visited,
                cx,
                route_token,
            );
        };
        authority.route_token = route_token;
        authority.commit_evidence = commit_evidence;
        Poll::Ready(Ok(MaterializationReadyOutcome::Ready(selected_arm)))
    }

    fn mark_materialization_ready_from_ingress(
        &mut self,
        state: &OfferResolveState<'r>,
        route_token: RouteDecisionToken,
    ) {
        let selection = state.selection;
        let scope_id = selection.scope_id;
        if let Some(evidence) = state.ingress.binding()
            && let Some(binding_arm) = {
                let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_to_arm(
                    frame_label_meta,
                    evidence.frame_label(),
                )
            }
            && binding_arm == route_token.arm().as_u8()
        {
            self.endpoint.mark_scope_ready_arm(scope_id, binding_arm);
        }
        let transport_ready_source = state.facts.is_dynamic_route_scope
            && ((!state.facts.is_route_controller
                && matches!(
                    route_token.source(),
                    RouteDecisionSource::Ack | RouteDecisionSource::Poll
                ))
                || (state.facts.is_route_controller
                    && matches!(
                        route_token.source(),
                        RouteDecisionSource::Resolver | RouteDecisionSource::Poll
                    )));
        if state.ingress.transport_lane_wire() == Some(selection.offer_lane)
            && transport_ready_source
        {
            self.endpoint
                .mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
        }
    }

    fn selected_arm_missing_materialization_evidence(
        &self,
        state: &OfferResolveState<'r>,
        selected_arm: u8,
    ) -> bool {
        self.endpoint
            .selection_arm_requires_materialization_ready_evidence(
                state.selection,
                state.facts.is_route_controller,
                selected_arm,
            )
            && !self
                .endpoint
                .scope_has_ready_arm(state.selection.scope_id, selected_arm)
    }

    fn poll_unready_resolver_authority(
        &mut self,
        state: &OfferResolveState<'r>,
        route_token: RouteDecisionToken,
        cx: &mut core::task::Context<'_>,
    ) -> Option<RouteDecisionToken> {
        if !matches!(route_token.source(), RouteDecisionSource::Resolver) {
            return None;
        }
        let scope_id = state.selection.scope_id;
        let offer_lanes = self.endpoint.offer_lane_set_for_scope(scope_id);
        self.try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
            .map(RouteDecisionToken::from_poll)
    }

    fn rollback_and_defer_unready_materialization(
        &mut self,
        state: &mut OfferResolveState<'r>,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        route_token: RouteDecisionToken,
    ) -> Poll<RecvResult<MaterializationReadyOutcome>> {
        let selection = state.selection;
        if let Some(payload) = state.ingress.take_transport() {
            self.requeue_offer_transport_payload(payload);
        }
        if matches!(route_token.source(), RouteDecisionSource::Resolver) {
            let _ = self.endpoint.take_scope_ack(selection.scope_id);
        }
        let keep_current_scope = state.facts.is_route_controller
            && state.facts.is_dynamic_route_scope
            && !selection.at_route_offer_entry
            && matches!(route_token.source(), RouteDecisionSource::Resolver);
        if keep_current_scope {
            state.pending.arm_yield_restart();
            return self.poll_resolve_pending_as(
                state,
                cx,
                MaterializationReadyOutcome::RestartFrontier,
            );
        }
        match self.on_frontier_defer(
            &mut state.progress,
            selection.scope_id,
            selection.frontier_parallel_root,
            DeferSource::Resolver,
            DeferReason::NoEvidence,
            selection.offer_lane,
            state.ingress.has_binding(),
            Some(route_token.arm().as_u8()),
            frontier_visited,
        ) {
            FrontierDeferOutcome::Continue => {
                if !state.facts.is_route_controller && !state.facts.is_dynamic_route_scope {
                    state
                        .pending
                        .arm_static_passive_progress(route_token.arm().as_u8());
                } else {
                    state.pending.arm_yield_restart();
                }
                self.poll_resolve_pending_as(
                    state,
                    cx,
                    MaterializationReadyOutcome::RestartFrontier,
                )
            }
            FrontierDeferOutcome::Yielded => {
                state.pending.arm_yield_restart();
                self.poll_resolve_pending_as(
                    state,
                    cx,
                    MaterializationReadyOutcome::RestartFrontier,
                )
            }
            FrontierDeferOutcome::Pending => Poll::Pending,
        }
    }

    fn poll_resolve_pending_as<Outcome>(
        &mut self,
        state: &mut OfferResolveState<'r>,
        cx: &mut core::task::Context<'_>,
        restart: Outcome,
    ) -> Poll<RecvResult<Outcome>> {
        match self.poll_resolve_pending_state(state, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier)) => Poll::Ready(Ok(restart)),
            Poll::Ready(Ok(ResolveTokenOutcome::Resolved(_))) => {
                Poll::Ready(Err(RecvError::PhaseInvariant))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}
