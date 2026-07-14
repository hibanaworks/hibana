//! Route-decision materialization readiness for public offer resolution.

use core::task::Poll;

use super::resolve::{MaterializationReadyOutcome, RouteAuthorityResolution};
use super::{
    CursorEndpoint, FrontierDeferOutcome, FrontierDeferRequest, FrontierVisitSet,
    OfferResolveState, RecvResult, ResolvedRouteArm, RouteArmToken, Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
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
            mut commit_evidence,
        } = authority;

        let selected_arm = loop {
            let selected_arm = route_token.arm().as_u8();
            if !self.selected_arm_missing_materialization_evidence(state, selected_arm, route_token)
            {
                break selected_arm;
            }
            if let Some(authority) = self.poll_unready_resolver_authority(state, route_token) {
                route_token = authority.route_token;
                commit_evidence = authority.commit_evidence;
                continue;
            }
            match self.poll_selected_arm_materialization_frame(
                state,
                pending_recv,
                selected_arm,
                route_token,
                cx,
            ) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Ready(Ok(true)) => continue,
                Poll::Ready(Ok(false)) => {}
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
            route_arm_selection_commit_evidence: commit_evidence,
        })))
    }

    #[inline(never)]
    fn poll_selected_arm_materialization_frame(
        &mut self,
        state: &mut OfferResolveState<'r>,
        pending_recv: &mut super::lane_port::PendingRecv,
        selected_arm: u8,
        token: RouteArmToken,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<bool>> {
        if state.ingress.has_transport()
            || !state.facts.profile.transport_marks_ready_from_source(token)
        {
            return Poll::Ready(Ok(false));
        }
        let scope_id = state.selection().scope_id;
        let Some(lanes) = self.route_scope_arm_lane_set_for_scope(scope_id, selected_arm) else {
            return Poll::Ready(Ok(false));
        };
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            match self.poll_received_framed_transport_frame_for_lane(
                pending_recv,
                lane_idx,
                lane_idx as u8,
                cx,
            ) {
                Poll::Ready(Ok(frame)) => {
                    state.ingress.stage_transport(frame);
                    return Poll::Ready(Ok(true));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => {}
            }
            next = lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        Poll::Pending
    }

    #[inline(never)]
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

    #[inline(never)]
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
        let Some(frame_label) = state.ingress.transport_frame_label_raw() else {
            return false;
        };
        self.cursor
            .passive_descendant_dispatch_arm_from_exact_frame_label(
                selection.scope_id,
                lane,
                frame_label,
            )
            == Some(selected_arm)
    }

    #[inline(never)]
    fn poll_unready_resolver_authority(
        &mut self,
        state: &OfferResolveState<'r>,
        route_token: RouteArmToken,
    ) -> Option<RouteAuthorityResolution> {
        if !route_token.is_resolver() {
            return None;
        }
        let scope_id = state.selection().scope_id;
        self.poll_route_authority(scope_id)
    }

    #[inline(never)]
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
                ingress: state.ingress.evidence_state(),
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
