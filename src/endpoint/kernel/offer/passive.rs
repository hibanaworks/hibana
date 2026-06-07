use super::{
    Clock, CursorEndpoint, DeferReason, DeferSource, EpochTable, FrontierDeferOutcome,
    FrontierVisitSet, LabelUniverse, MintConfigMarker, OfferProgressState, OfferResolveState,
    OfferScopeProfile, OfferScopeSelection, OfferStagedIngress, Poll, RecvError, RecvResult,
    ResolvePendingState, ResolveTokenOutcome, ResolvedFrameHint, RouteDecisionToken, Transport,
    lane_port,
};
pub(super) struct PassiveRouteEvidenceInput<'a> {
    pub(super) selection: OfferScopeSelection,
    pub(super) offer_lanes: crate::global::role_program::LaneSetView<'a>,
    pub(super) profile: OfferScopeProfile,
    pub(super) resolved_hint_frame: Option<ResolvedFrameHint>,
}

pub(super) struct PassiveRouteEvidenceContext<'a, 'r> {
    ingress: &'a mut OfferStagedIngress<'r>,
    progress: &'a mut OfferProgressState,
    frontier_visited: &'a mut FrontierVisitSet,
}

pub(super) enum PassiveRouteEvidenceOutcome {
    Authority {
        authority: PassiveRouteAuthority,
        resolved_hint_frame: Option<ResolvedFrameHint>,
    },
    EvidenceOnly {
        resolved_hint_frame: Option<ResolvedFrameHint>,
    },
    RestartFrontier,
}

pub(super) enum PassiveRouteAuthority {
    Ack(RouteDecisionToken),
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
    fn has_transport(&self) -> bool {
        self.ingress.has_transport()
    }

    #[inline]
    fn transport_lane_wire(&self) -> Option<u8> {
        self.ingress.transport_lane_wire()
    }

    #[inline]
    fn transport_frame_label_raw(&self) -> Option<u8> {
        self.ingress.transport_frame_label_raw()
    }

    #[inline]
    fn stage_transport(&mut self, frame: lane_port::PreambleFrame<'r>) {
        self.ingress.stage_transport(frame);
    }
}

impl PassiveRouteAuthority {
    #[inline]
    pub(super) fn into_route_token(self) -> RouteDecisionToken {
        match self {
            Self::Ack(token) => token,
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(super) fn poll_resolve_pending_state(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<ResolveTokenOutcome>> {
        match state.pending {
            ResolvePendingState::Ready => Poll::Ready(Err(RecvError::PhaseInvariant)),
            ResolvePendingState::YieldRestartUnarmed => {
                state.pending.complete_yield_turn();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            ResolvePendingState::YieldRestartArmed => {
                state.pending.clear();
                Poll::Ready(Ok(ResolveTokenOutcome::RestartFrontier))
            }
            ResolvePendingState::StaticPassiveProgress { selected_arm } => {
                match self.await_static_passive_progress(
                    pending_recv,
                    state.selection(),
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
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<PassiveRouteEvidenceOutcome>> {
        let PassiveRouteEvidenceInput {
            selection,
            offer_lanes,
            profile,
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
                profile,
                pending_recv,
                &mut state,
            )? {
                resolved_hint_frame = Some(frame_hint);
            }
            if let Some(token) = self.peek_scope_ack(scope_id) {
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

            if self.scope_has_ready_arm_evidence(scope_id) {
                let needs_wire_turn_for_materialization =
                    !wire_turn.has_polled() && !state.has_transport();
                if !needs_wire_turn_for_materialization {
                    break;
                }
            }

            if !wire_turn.has_polled() {
                if let Some(frame_hint) =
                    self.poll_passive_wire_turn(selection, pending_recv, &mut state, cx)?
                {
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
                state.has_transport(),
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
        profile: OfferScopeProfile,
        pending_recv: &lane_port::PendingRecv,
        state: &mut PassiveRouteEvidenceContext<'_, 'r>,
    ) -> RecvResult<Option<ResolvedFrameHint>> {
        let scope_id = selection.scope_id;
        if let Some(frame_lane) = state.transport_lane_wire() {
            let Some(frame_label) = state.transport_frame_label_raw() else {
                return Ok(None);
            };
            let frame_label_meta = self.selection_frame_label_meta(selection);
            if frame_label_meta
                .frame_hint_mask()
                .contains_frame_label(frame_label)
            {
                self.mark_scope_ready_arm_from_frame_label(
                    scope_id,
                    frame_lane,
                    frame_label,
                    frame_label_meta,
                );
                return Ok(Some(ResolvedFrameHint::staged_transport()));
            }
            return Ok(None);
        }

        let frame_label_meta = self.selection_frame_label_meta(selection);
        self.ingest_scope_evidence_for_offer(
            pending_recv,
            scope_id,
            selection.offer_lane as usize,
            offer_lanes,
            profile.suppresses_scope_frame_hint(),
            frame_label_meta,
        );
        if self.scope_evidence_conflicted(scope_id) {
            return Err(RecvError::PhaseInvariant);
        }

        Ok(self
            .peek_scope_frame_hint_with_lane(scope_id)
            .map(|_| ResolvedFrameHint::scope_evidence()))
    }

    fn poll_passive_wire_turn(
        &mut self,
        selection: OfferScopeSelection,
        pending_recv: &mut lane_port::PendingRecv,
        state: &mut PassiveRouteEvidenceContext<'_, 'r>,
        cx: &mut core::task::Context<'_>,
    ) -> RecvResult<Option<ResolvedFrameHint>> {
        let recv_lane_idx = selection.offer_lane as usize;
        let recv_lane = recv_lane_idx as u8;
        let frame = match self.poll_received_transport_frame_for_lane(
            pending_recv,
            recv_lane_idx,
            recv_lane,
            cx,
        ) {
            Poll::Pending => return Ok(None),
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Err(err),
        };
        let observed_frame_label = frame.observed_frame_label_raw();
        state.stage_transport(frame);
        let frame_label_meta = self.selection_frame_label_meta(selection);
        if let Some(frame_label) = observed_frame_label {
            if frame_label_meta
                .frame_hint_mask()
                .contains_frame_label(frame_label)
            {
                return Ok(Some(ResolvedFrameHint::staged_transport()));
            }
        }
        Ok(self
            .peek_scope_frame_hint_with_lane(selection.scope_id)
            .map(|_| ResolvedFrameHint::scope_evidence()))
    }
}
