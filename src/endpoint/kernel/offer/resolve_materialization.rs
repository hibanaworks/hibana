//! Route-decision materialization readiness for public offer resolution.

use core::task::Poll;

use super::resolve::{MaterializationReadyOutcome, RouteAuthorityResolution};
use super::{
    CursorEndpoint, DeferReason, FrontierDeferOutcome, FrontierDeferRequest, FrontierVisitSet,
    OfferResolveState, RecvResult, ResolvedRouteArm, RouteArmCommitEvidence, RouteArmToken,
    Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
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
            frame_hint,
            mut commit_evidence,
        } = authority;

        let selected_arm = loop {
            let selected_arm = route_token.arm().as_u8();
            if !self.selected_arm_missing_materialization_evidence(state, selected_arm, route_token)
            {
                break selected_arm;
            }
            if let Some(poll_token) = self.poll_unready_resolver_authority(state, route_token, cx) {
                route_token = poll_token;
                commit_evidence = RouteArmCommitEvidence::PollFrame;
                continue;
            }
            return self.requeue_and_defer_unready_materialization(
                state,
                pending_recv,
                frontier_visited,
                cx,
                route_token,
            );
        };
        Poll::Ready(Ok(MaterializationReadyOutcome::Ready(ResolvedRouteArm {
            route_token,
            selected_arm,
            frame_hint,
            route_arm_selection_commit_evidence: commit_evidence,
        })))
    }

    fn selected_arm_missing_materialization_evidence(
        &self,
        state: &OfferResolveState<'r>,
        selected_arm: u8,
        token: RouteArmToken,
    ) -> bool {
        let requires = self.selection_arm_requires_materialization_ready_evidence(
            state.selection(),
            state.facts.profile.is_controller(),
            selected_arm,
        );
        if !requires || self.scope_has_ready_arm(state.selection().scope_id, selected_arm) {
            return false;
        }
        !self.staged_transport_can_materialize_selected_arm(state, selected_arm, token)
    }

    fn staged_transport_can_materialize_selected_arm(
        &self,
        state: &OfferResolveState<'r>,
        selected_arm: u8,
        token: RouteArmToken,
    ) -> bool {
        if !state.facts.profile.transport_marks_ready_from_source(token) {
            return false;
        }
        let selection = state.selection();
        let Some(lane) = state.ingress.transport_lane_wire() else {
            return false;
        };
        self.route_scope_arm_lane_set_for_scope(selection.scope_id, selected_arm)
            .is_some_and(|lanes| lanes.contains(lane as usize))
    }

    fn poll_unready_resolver_authority(
        &mut self,
        state: &OfferResolveState<'r>,
        route_token: RouteArmToken,
        cx: &mut core::task::Context<'_>,
    ) -> Option<RouteArmToken> {
        if !route_token.is_resolver() {
            return None;
        }
        let scope_id = state.selection().scope_id;
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        self.try_poll_route_arm_selection_for_offer(scope_id, offer_lanes, cx)
            .map(RouteArmToken::from_poll)
    }

    fn requeue_and_defer_unready_materialization(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        frontier_visited: &mut FrontierVisitSet,
        cx: &mut core::task::Context<'_>,
        route_token: RouteArmToken,
    ) -> Poll<RecvResult<MaterializationReadyOutcome>> {
        let selection = state.selection();
        if let Some(payload) = state.ingress.take_transport()
            && let Err(err) = self.requeue_offer_transport_payload(payload)
        {
            return Poll::Ready(Err(err));
        }
        if route_token.is_resolver() {
            self.clear_scope_ack(selection.scope_id);
        }
        if state
            .facts
            .profile
            .keeps_current_scope_for_unready_resolver(selection, route_token)
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
            FrontierDeferRequest {
                scope_id: selection.scope_id,
                current_parallel: selection.frontier_parallel_root,
                reason: DeferReason::EvidenceAbsent,
                offer_lane: selection.offer_lane,
                ingress: state.ingress.evidence_state(),
                selected_arm: Some(route_token.arm().as_u8()),
            },
            frontier_visited,
        ) {
            FrontierDeferOutcome::Continue => {
                if state.facts.profile.intrinsic_passive_progress_after_defer() {
                    state
                        .pending
                        .arm_intrinsic_passive_progress(route_token.arm().as_u8());
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
