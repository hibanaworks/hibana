//! Route-decision materialization readiness for public offer resolution.

use core::task::Poll;

use super::resolve::{MaterializationReadyOutcome, RouteAuthorityResolution};
use super::{
    BindingSlot, Clock, CursorEndpoint, DeferReason, DeferSource, EpochTable, FrontierDeferOutcome,
    FrontierVisitSet, LabelUniverse, MintConfigMarker, OfferResolveState, RecvResult,
    ResolvedRouteDecision, RouteDecisionCommitEvidence, RouteDecisionSource, RouteDecisionToken,
    Transport,
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(super) fn ensure_materialization_ready(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        authority: RouteAuthorityResolution,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<MaterializationReadyOutcome>> {
        let RouteAuthorityResolution {
            mut route_token,
            resolved_hint_frame,
            mut commit_evidence,
        } = authority;
        self.mark_materialization_ready_from_ingress(state, route_token);

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
                pending_recv,
                frontier_visited,
                cx,
                route_token,
            );
        };
        Poll::Ready(Ok(MaterializationReadyOutcome::Ready(
            ResolvedRouteDecision {
                route_token,
                selected_arm,
                resolved_hint_frame_label: resolved_hint_frame.map(|frame| frame.frame_label),
                route_decision_commit_evidence: commit_evidence,
            },
        )))
    }

    fn mark_materialization_ready_from_ingress(
        &mut self,
        state: &OfferResolveState<'r>,
        route_token: RouteDecisionToken,
    ) {
        let selection = state.selection();
        let scope_id = selection.scope_id;
        if let Some(evidence) = state.ingress.binding()
            && let Some(binding_arm) = {
                let frame_label_meta = self.selection_frame_label_meta(selection);
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_frame_label_to_arm(
                    frame_label_meta,
                    evidence.frame_label(),
                )
            }
            && binding_arm == route_token.arm().as_u8()
        {
            self.mark_scope_ready_arm(scope_id, binding_arm);
        }
        if state.ingress.transport_lane_wire() == Some(selection.offer_lane)
            && state
                .facts
                .profile
                .transport_marks_ready_from_source(route_token.source())
        {
            self.mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
        }
    }

    fn selected_arm_missing_materialization_evidence(
        &self,
        state: &OfferResolveState<'r>,
        selected_arm: u8,
    ) -> bool {
        self.selection_arm_requires_materialization_ready_evidence(
            state.selection(),
            state.facts.profile.is_controller(),
            selected_arm,
        ) && !self.scope_has_ready_arm(state.selection().scope_id, selected_arm)
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
        let scope_id = state.selection().scope_id;
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        self.try_poll_route_decision_for_offer(scope_id, offer_lanes, cx)
            .map(RouteDecisionToken::from_poll)
    }

    fn rollback_and_defer_unready_materialization(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        route_token: RouteDecisionToken,
    ) -> Poll<RecvResult<MaterializationReadyOutcome>> {
        let selection = state.selection();
        if let Some(payload) = state.ingress.take_transport() {
            self.requeue_offer_transport_payload(payload);
        }
        if matches!(route_token.source(), RouteDecisionSource::Resolver) {
            let _ = self.take_scope_ack(selection.scope_id);
        }
        if state
            .facts
            .profile
            .keeps_current_scope_for_unready_resolver(selection, route_token.source())
        {
            state.pending.arm_yield_restart();
            return self.poll_resolve_pending_as(
                state,
                pending_recv,
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
                if state.facts.profile.static_passive_progress_after_defer() {
                    state
                        .pending
                        .arm_static_passive_progress(route_token.arm().as_u8());
                } else {
                    state.pending.arm_yield_restart();
                }
                self.poll_resolve_pending_as(
                    state,
                    pending_recv,
                    cx,
                    MaterializationReadyOutcome::RestartFrontier,
                )
            }
            FrontierDeferOutcome::Yielded => {
                state.pending.arm_yield_restart();
                self.poll_resolve_pending_as(
                    state,
                    pending_recv,
                    cx,
                    MaterializationReadyOutcome::RestartFrontier,
                )
            }
            FrontierDeferOutcome::Pending => Poll::Pending,
        }
    }
}
