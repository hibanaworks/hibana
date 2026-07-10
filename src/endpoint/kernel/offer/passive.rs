use super::{
    CursorEndpoint, FrameEvidenceResolution, FrontierDeferOutcome, FrontierDeferRequest,
    FrontierVisitSet, IngressEvidenceState, OfferProgressState, OfferResolveState,
    OfferScopeSelection, OfferStagedIngress, Poll, RecvError, RecvResult, ResolvePendingState,
    ResolveTokenOutcome, RouteArmToken, ScopeFrameLabelScratch, Transport, lane_port,
};
pub(super) struct PassiveRouteEvidenceInput<'a> {
    pub(super) selection: OfferScopeSelection,
    pub(super) offer_lanes: crate::global::role_program::LaneSetView<'a>,
    pub(super) frame_evidence: FrameEvidenceResolution,
}

pub(super) struct PassiveRouteEvidenceContext<'a, 'r> {
    ingress: &'a mut OfferStagedIngress<'r>,
    progress: &'a mut OfferProgressState,
    frontier_visited: &'a mut FrontierVisitSet,
}

pub(super) enum PassiveRouteEvidenceOutcome {
    Authority {
        route_token: RouteArmToken,
    },
    EvidenceOnly {
        frame_evidence: FrameEvidenceResolution,
    },
    RestartFrontier,
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
    fn evidence_state(&self) -> IngressEvidenceState {
        self.ingress.evidence_state()
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

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
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
            ResolvePendingState::IntrinsicPassiveProgress { selected_arm } => {
                match self.await_intrinsic_passive_progress(
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
            mut frame_evidence,
        } = input;
        let scope_id = selection.scope_id;
        let frontier_parallel_root = selection.frontier_parallel_root;
        let mut wire_turn = PassiveWireTurn::Unpolled;
        loop {
            frame_evidence.record(self.refresh_passive_scope_evidence(selection, &mut state)?);
            if let Some(arm) = self.try_poll_route_arm_selection_immediate(scope_id, offer_lanes) {
                return Poll::Ready(Ok(PassiveRouteEvidenceOutcome::Authority {
                    route_token: RouteArmToken::from_ack(arm),
                }));
            }
            if let Some(token) = self.peek_live_scope_ack(scope_id) {
                return Poll::Ready(Ok(PassiveRouteEvidenceOutcome::Authority {
                    route_token: token,
                }));
            }

            if state.has_transport() {
                break;
            }

            if frame_evidence.is_resolved() && wire_turn.has_polled() {
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
                frame_evidence.record(self.poll_passive_wire_turn(
                    selection,
                    pending_recv,
                    &mut state,
                    cx,
                )?);
                wire_turn = PassiveWireTurn::Polled;
                continue;
            }

            match self.on_frontier_defer(
                state.progress,
                FrontierDeferRequest {
                    scope_id,
                    current_parallel: frontier_parallel_root,
                    ingress: state.evidence_state(),
                },
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
            frame_evidence,
        }))
    }

    fn refresh_passive_scope_evidence(
        &mut self,
        selection: OfferScopeSelection,
        state: &mut PassiveRouteEvidenceContext<'_, 'r>,
    ) -> RecvResult<FrameEvidenceResolution> {
        let scope_id = selection.scope_id;
        if let Some(frame_lane) = state.transport_lane_wire() {
            let Some(frame_label) = state.transport_frame_label_raw() else {
                return Ok(FrameEvidenceResolution::unresolved());
            };
            let mut frame_label_scratch = ScopeFrameLabelScratch::EMPTY;
            self.write_selection_frame_label_meta(selection, &mut frame_label_scratch);
            let frame_label_meta = frame_label_scratch.view();
            if frame_label_meta
                .evidence_frame_label_mask()
                .contains_frame_label(frame_label)
            {
                self.mark_scope_ready_arm_from_frame_label(
                    scope_id,
                    frame_lane,
                    frame_label,
                    &frame_label_meta,
                );
                return Ok(FrameEvidenceResolution::resolved());
            }
            return Ok(FrameEvidenceResolution::unresolved());
        }

        if self.scope_evidence_conflicted(scope_id) {
            return Err(RecvError::PhaseInvariant);
        }

        Ok(FrameEvidenceResolution::unresolved())
    }

    fn poll_passive_wire_turn(
        &mut self,
        selection: OfferScopeSelection,
        pending_recv: &mut lane_port::PendingRecv,
        state: &mut PassiveRouteEvidenceContext<'_, 'r>,
        cx: &mut core::task::Context<'_>,
    ) -> RecvResult<FrameEvidenceResolution> {
        let recv_lane_idx = selection.offer_lane as usize;
        let recv_lane = recv_lane_idx as u8;
        let frame = match self.poll_received_framed_transport_frame_for_lane(
            pending_recv,
            recv_lane_idx,
            recv_lane,
            cx,
        ) {
            Poll::Pending => return Ok(FrameEvidenceResolution::unresolved()),
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Err(err),
        };
        let observed_frame_label = frame.observed_frame_label_raw();
        let observed = frame.observed_transport_frame(self.sid.raw(), recv_lane, ROLE);
        let mut frame_label_scratch = ScopeFrameLabelScratch::EMPTY;
        self.write_selection_frame_label_meta(selection, &mut frame_label_scratch);
        let frame_label_meta = frame_label_scratch.view();
        if frame_label_meta
            .evidence_frame_label_mask()
            .contains_frame_label(observed_frame_label)
        {
            state.stage_transport(frame);
            return Ok(FrameEvidenceResolution::resolved());
        }
        self.emit_materialization_mismatch_observation(
            recv_lane_idx,
            recv_lane,
            lane_port::FrameMismatch::label_mismatch(observed),
        );
        frame.discard_uncommitted();
        Err(RecvError::PhaseInvariant)
    }
}
