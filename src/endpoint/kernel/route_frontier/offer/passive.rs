use super::*;

pub(super) struct PassiveRouteEvidenceInput<'a> {
    pub(super) selection: OfferScopeSelection,
    pub(super) offer_lanes: crate::global::role_program::LaneSetView<'a>,
    pub(super) is_dynamic_route_scope: bool,
    pub(super) resolved_hint_frame: Option<(u8, u8)>,
}

pub(super) struct PassiveRouteEvidenceContext<'a, 'r> {
    ingress: &'a mut OfferStagedIngress<'r>,
    progress: &'a mut OfferProgressState,
    frontier_visited: &'a mut FrontierVisitSet,
}

pub(super) enum PassiveRouteEvidenceOutcome {
    Authority {
        authority: PassiveRouteAuthority,
        resolved_hint_frame: Option<(u8, u8)>,
    },
    EvidenceOnly {
        resolved_hint_frame: Option<(u8, u8)>,
    },
    RestartFrontier,
}

pub(super) enum PassiveRouteAuthority {
    Ack(RouteDecisionToken),
    StaticPoll(RouteDecisionToken),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PassiveWireTurn {
    Unpolled,
    Polled,
}

impl PassiveWireTurn {
    #[inline]
    const fn has_polled(self) -> bool {
        matches!(self, Self::Polled)
    }
}

impl<'a, 'r> PassiveRouteEvidenceContext<'a, 'r> {
    #[inline]
    pub(super) fn new(
        ingress: &'a mut OfferStagedIngress<'r>,
        progress: &'a mut OfferProgressState,
        frontier_visited: &'a mut FrontierVisitSet,
    ) -> Self {
        Self {
            ingress,
            progress,
            frontier_visited,
        }
    }

    #[inline]
    fn has_binding(&self) -> bool {
        self.ingress.has_binding()
    }

    #[inline]
    fn has_transport(&self) -> bool {
        self.ingress.has_transport()
    }

    #[inline]
    fn transport_lane_wire(&self) -> Option<u8> {
        self.ingress.transport_lane_wire()
    }

    #[inline]
    fn stage_transport(&mut self, frame: lane_port::ReceivedFrame<'r>) {
        self.ingress.stage_transport(frame);
    }

    #[inline]
    fn stage_binding(&mut self, evidence: LaneIngressEvidence) {
        self.ingress.stage_binding(evidence);
    }

    #[inline]
    fn binding(&self) -> Option<&LaneIngressEvidence> {
        self.ingress.binding()
    }
}

impl PassiveRouteAuthority {
    #[inline]
    pub(super) fn into_route_token(self) -> RouteDecisionToken {
        match self {
            Self::Ack(token) | Self::StaticPoll(token) => token,
        }
    }
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
    pub(super) fn poll_resolve_pending_state(
        &mut self,
        state: &mut OfferResolveState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        match state.pending {
            ResolvePendingState::Ready => Poll::Ready(Err(RecvError::PhaseInvariant)),
            ResolvePendingState::YieldRestart { armed } => {
                if !armed {
                    state.pending.complete_yield_turn();
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                state.pending.clear();
                Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier))
            }
            ResolvePendingState::StaticPassiveProgress { selected_arm } => {
                match self.await_static_passive_progress(
                    state.selection,
                    Some(selected_arm),
                    &mut state.ingress,
                    cx,
                ) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(())) => {
                        state.pending.clear();
                        Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier))
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                }
            }
        }
    }

    pub(super) fn poll_passive_route_evidence(
        &mut self,
        input: PassiveRouteEvidenceInput<'_>,
        mut state: PassiveRouteEvidenceContext<'_, 'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<PassiveRouteEvidenceOutcome>> {
        let PassiveRouteEvidenceInput {
            selection,
            offer_lanes,
            is_dynamic_route_scope,
            mut resolved_hint_frame,
        } = input;
        let scope_id = selection.scope_id;
        let frontier_parallel_root = selection.frontier_parallel_root;
        let offer_lane = selection.offer_lane;
        let mut wire_turn = PassiveWireTurn::Unpolled;
        loop {
            if let Some(frame_hint) = self.refresh_passive_scope_evidence(
                selection,
                offer_lanes,
                is_dynamic_route_scope,
                &mut state,
            )? {
                resolved_hint_frame = Some(frame_hint);
            }
            if let Some(frame) = resolved_hint_frame
                && let Some(derived) = self.passive_authority_from_frame_hint(
                    selection,
                    is_dynamic_route_scope,
                    &state,
                    frame,
                )
            {
                return Poll::Ready(Ok(PassiveRouteEvidenceOutcome::Authority {
                    authority: derived,
                    resolved_hint_frame,
                }));
            }
            if let Some(token) = self.endpoint.peek_scope_ack(scope_id) {
                return Poll::Ready(Ok(PassiveRouteEvidenceOutcome::Authority {
                    authority: PassiveRouteAuthority::Ack(token),
                    resolved_hint_frame,
                }));
            }

            if state.has_transport() {
                break;
            }

            if resolved_hint_frame.is_some() && wire_turn.has_polled() {
                break;
            }

            if self.endpoint.scope_has_ready_arm_evidence(scope_id) {
                let needs_wire_turn_for_materialization =
                    !wire_turn.has_polled() && !state.has_transport() && !state.has_binding();
                if !needs_wire_turn_for_materialization {
                    break;
                }
            }

            if !wire_turn.has_polled() {
                if let Some(frame_hint) = self.poll_passive_wire_turn(selection, &mut state, cx)? {
                    resolved_hint_frame = Some(frame_hint);
                }
                wire_turn = PassiveWireTurn::Polled;
                continue;
            }

            match self.on_frontier_defer(
                state.progress,
                scope_id,
                frontier_parallel_root,
                DeferSource::Resolver,
                DeferReason::NoEvidence,
                offer_lane,
                state.has_binding(),
                None,
                state.frontier_visited,
            ) {
                FrontierDeferOutcome::Continue => break,
                FrontierDeferOutcome::Yielded => {
                    return Poll::Ready(Ok(PassiveRouteEvidenceOutcome::RestartFrontier));
                }
                FrontierDeferOutcome::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(PassiveRouteEvidenceOutcome::EvidenceOnly {
            resolved_hint_frame,
        }))
    }

    fn refresh_passive_scope_evidence(
        &mut self,
        selection: OfferScopeSelection,
        offer_lanes: crate::global::role_program::LaneSetView<'_>,
        is_dynamic_route_scope: bool,
        state: &mut PassiveRouteEvidenceContext<'_, 'r>,
    ) -> RecvResult<Option<(u8, u8)>> {
        let scope_id = selection.scope_id;
        let staged_payload_for_offer_lane =
            state.transport_lane_wire() == Some(selection.offer_lane);
        if staged_payload_for_offer_lane {
            return Ok(None);
        }

        let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
        let materialization_meta = self.endpoint.selection_materialization_meta(selection);
        if !state.has_binding()
            && let Some((lane_idx, evidence)) = self.endpoint.poll_binding_for_offer(
                scope_id,
                selection.offer_lane_idx as usize,
                frame_label_meta,
                materialization_meta,
            )
        {
            state.stage_binding(LaneIngressEvidence::new(lane_idx, evidence));
        }

        self.endpoint.ingest_scope_evidence_for_offer_lanes(
            scope_id,
            selection.offer_lane_idx as usize,
            offer_lanes,
            is_dynamic_route_scope,
            frame_label_meta,
        );
        if let Some(evidence) = state.binding() {
            self.endpoint.ingest_binding_scope_evidence(
                scope_id,
                evidence.lane(),
                evidence.frame_label(),
                is_dynamic_route_scope,
                frame_label_meta,
            );
        }
        if self.endpoint.scope_evidence_conflicted(scope_id)
            && !self.endpoint.recover_scope_evidence_conflict(
                scope_id,
                is_dynamic_route_scope,
                false,
            )
        {
            return Err(RecvError::PhaseInvariant);
        }

        Ok(self.endpoint.peek_scope_frame_hint_with_lane(scope_id))
    }

    fn passive_authority_from_frame_hint(
        &self,
        selection: OfferScopeSelection,
        is_dynamic_route_scope: bool,
        state: &PassiveRouteEvidenceContext<'_, 'r>,
        frame: (u8, u8),
    ) -> Option<PassiveRouteAuthority> {
        if is_dynamic_route_scope {
            return None;
        }

        let (hint_lane, frame_label) = frame;
        let route_evidence_lane = state.transport_lane_wire().unwrap_or(hint_lane);
        let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
        self.endpoint
            .static_passive_dispatch_arm_from_exact_frame_label(
                selection.scope_id,
                route_evidence_lane,
                frame_label,
            )
            .or_else(|| {
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_to_arm(
                    frame_label_meta,
                    frame_label,
                )
            })
            .and_then(Arm::new)
            .map(RouteDecisionToken::from_poll)
            .map(PassiveRouteAuthority::StaticPoll)
    }

    fn poll_passive_wire_turn(
        &mut self,
        selection: OfferScopeSelection,
        state: &mut PassiveRouteEvidenceContext<'_, 'r>,
        cx: &mut core::task::Context<'_>,
    ) -> RecvResult<Option<(u8, u8)>> {
        let recv_lane_idx = selection.offer_lane as usize;
        let recv_lane = recv_lane_idx as u8;
        let port = self.endpoint.port_for_lane(recv_lane_idx);
        let Poll::Ready(frame) =
            lane_port::poll_recv_frame(&mut self.pending_recv, recv_lane_idx, port, cx)
        else {
            return Ok(None);
        };

        let frame = frame.map_err(RecvError::Transport)?;
        state.stage_transport(frame);
        let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
        if let Some(frame_label) =
            self.endpoint
                .take_frame_hint_for_lane(recv_lane_idx, false, frame_label_meta, true)
        {
            self.endpoint.mark_scope_ready_arm_from_frame_label(
                selection.scope_id,
                recv_lane,
                frame_label,
                frame_label_meta,
            );
            return Ok(Some((recv_lane, frame_label)));
        }

        Ok(self
            .endpoint
            .peek_scope_frame_hint_with_lane(selection.scope_id))
    }
}
