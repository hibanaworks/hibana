//! Offer frontier fact derivation.

use super::*;

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
    pub(super) fn prepare_frontier_facts(
        &mut self,
        selection: OfferScopeSelection,
        frontier_visited: &mut FrontierVisitSet,
    ) -> RecvResult<OfferFrontierFacts> {
        let scope_id = selection.scope_id;
        frontier_visited.record(scope_id);
        let offer_lane_idx = selection.offer_lane_idx as usize;
        let at_route_offer_entry = selection.at_route_offer_entry;
        let loop_meta = self
            .endpoint
            .selection_frame_label_meta(selection)
            .loop_meta();

        let cursor_is_not_recv = !self.endpoint.cursor.is_recv();
        let is_route_controller = self.endpoint.cursor.is_route_controller(scope_id);
        let controller_selected_recv_step = is_route_controller
            && !at_route_offer_entry
            && self
                .endpoint
                .cursor
                .try_recv_meta()
                .map(|recv_meta| recv_meta.peer != ROLE)
                .unwrap_or(false);

        let is_dynamic_route_scope = self
            .endpoint
            .cursor
            .route_scope_controller_policy(scope_id)
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        let suppress_scope_frame_hint = is_dynamic_route_scope;
        let offer_lanes = self.endpoint.offer_lane_set_for_scope(scope_id);
        {
            let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
            self.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lane_idx,
                offer_lanes,
                suppress_scope_frame_hint,
                frame_label_meta,
            );
        }
        let preview_route_decision = self.endpoint.preview_scope_ack_token_non_consuming(
            scope_id,
            offer_lane_idx,
            offer_lanes,
        );
        let preview_ready_arm_evidence = self.endpoint.scope_has_ready_arm_evidence(scope_id);
        let preview_frame_hint_evidence = self.endpoint.peek_scope_frame_hint(scope_id).is_some();
        let recvless_loop_control_scope = !is_route_controller
            && !is_dynamic_route_scope
            && loop_meta.control_scope()
            && !loop_meta.arm_has_recv(0)
            && !loop_meta.arm_has_recv(1);

        let is_self_send_controller = cursor_is_not_recv
            && is_route_controller
            && !CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_has_controller_arm_entry(
                &self.endpoint.cursor,
                scope_id,
            );
        let controller_non_entry_cursor_ready = cursor_is_not_recv
            && is_route_controller
            && self.endpoint.controller_arm_at_cursor(scope_id).is_none();
        let early_route_decision = if is_route_controller {
            preview_route_decision
        } else {
            preview_route_decision
                .filter(|token| !self.endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
        };
        let early_decision_arm_has_no_recv = early_route_decision
            .map(|token| !self.endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
            .unwrap_or(false);
        let controller_pending_materialization = is_route_controller
            && self
                .endpoint
                .selected_arm_for_scope(scope_id)
                .map(|arm| {
                    self.endpoint
                        .arm_requires_materialization_ready_evidence(scope_id, arm)
                        && !self.endpoint.scope_has_ready_arm(scope_id, arm)
                })
                .unwrap_or(false);
        let controller_can_skip_recv = is_route_controller
            && !controller_pending_materialization
            && ((at_route_offer_entry
                && (is_dynamic_route_scope
                    || controller_non_entry_cursor_ready
                    || is_self_send_controller
                    || early_route_decision.is_some()))
                || (!at_route_offer_entry && cursor_is_not_recv));
        let passive_dynamic_scope_has_recv =
            self.endpoint.arm_has_recv(scope_id, 0) || self.endpoint.arm_has_recv(scope_id, 1);
        let passive_ack_is_materializable = self
            .endpoint
            .preview_scope_ack_token_non_consuming(scope_id, offer_lane_idx, offer_lanes)
            .map(|token| {
                let arm = token.arm().as_u8();
                self.endpoint.scope_has_ready_arm(scope_id, arm)
                    || !self.endpoint.arm_has_recv(scope_id, arm)
            })
            .unwrap_or(false);
        let passive_dynamic_can_skip_recv = !is_route_controller
            && is_dynamic_route_scope
            && (!passive_dynamic_scope_has_recv
                || preview_ready_arm_evidence
                || passive_ack_is_materializable);
        let passive_evidence_can_skip_recv =
            !is_route_controller && (preview_ready_arm_evidence || preview_frame_hint_evidence);
        let skip_recv_loop = passive_evidence_can_skip_recv
            || passive_dynamic_can_skip_recv
            || controller_can_skip_recv
            || early_decision_arm_has_no_recv;
        let ingress_mode = if skip_recv_loop {
            OfferIngressMode::Skip
        } else if !is_route_controller || controller_selected_recv_step {
            if recvless_loop_control_scope {
                OfferIngressMode::ProbeSelectedAndRecvlessLoopBinding
            } else {
                OfferIngressMode::ProbeSelectedBinding
            }
        } else {
            OfferIngressMode::TransportOnly
        };

        Ok(OfferFrontierFacts {
            selection,
            scope_id,
            offer_lane_idx,
            suppress_scope_frame_hint,
            is_route_controller,
            is_dynamic_route_scope,
            ingress_mode,
        })
    }
}
