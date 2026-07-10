use super::super::{
    Arm, CommittedCommitDelta, CursorEndpoint, RouteArmToken, SelectedRouteCommitRow,
    SelectedRouteCommitRowsRef, SendError, SendMeta, SendResult, Transport,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    pub(super) fn build_send_selected_route_rows(
        &mut self,
        event_idx: usize,
        meta: SendMeta,
    ) -> SendResult<SelectedRouteCommitRowsRef> {
        if meta.selected_route_arm.is_none() {
            return Ok(SelectedRouteCommitRowsRef::EMPTY);
        }
        let Self {
            cursor,
            decision_state,
            route_commit_rows,
            ..
        } = self;
        let mut rows = route_commit_rows.begin();
        prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
            decision_state,
            cursor,
            meta.lane,
            event_idx,
            &mut rows,
        )
        .map_err(|_| SendError::PhaseInvariant)?;
        Ok(rows.as_commit_rows(meta.lane))
    }

    #[inline(never)]
    fn publish_send_route_row_evidence(
        &mut self,
        route_row: SelectedRouteCommitRow,
        lane_wire: u8,
    ) {
        let scope_id = route_row.scope();
        let selected_arm = route_row.selected_arm();
        let route_token = self.peek_live_scope_ack(scope_id);
        match route_token {
            Some(RouteArmToken::Ack(_)) => {
                let arm = Arm::from_raw(selected_arm);
                self.record_route_arm_selection_for_lane(
                    lane_wire as usize,
                    scope_id,
                    selected_arm,
                );
                self.emit_route_arm_selection(scope_id, RouteArmToken::from_ack(arm), lane_wire);
            }
            Some(RouteArmToken::Poll(_)) => {
                let arm = Arm::from_raw(selected_arm);
                self.record_route_arm_selection_for_scope_lanes(scope_id, selected_arm, lane_wire);
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_poll(arm),
                    self.offer_lane_for_scope(scope_id),
                );
            }
            Some(RouteArmToken::Resolver(arm)) => {
                self.record_route_arm_selection_for_scope_lanes(scope_id, selected_arm, lane_wire);
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_resolver(arm),
                    lane_wire,
                );
                if self.arm_has_recv(scope_id, selected_arm) {
                    self.consume_scope_ready_arm(scope_id, selected_arm);
                }
                self.clear_scope_evidence(scope_id);
                return;
            }
            None if self
                .cursor
                .route_scope_controller_resolver(scope_id)
                .is_some_and(|(resolver, _)| resolver.is_dynamic()) =>
            {
                let arm = Arm::from_raw(selected_arm);
                self.record_route_arm_selection_for_scope_lanes(scope_id, selected_arm, lane_wire);
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_resolver(arm),
                    lane_wire,
                );
                if self.arm_has_recv(scope_id, selected_arm) {
                    self.consume_scope_ready_arm(scope_id, selected_arm);
                }
                self.clear_scope_evidence(scope_id);
                return;
            }
            None => {
                if self.arm_has_recv(scope_id, selected_arm) {
                    self.consume_scope_ready_arm(scope_id, selected_arm);
                }
                self.clear_scope_evidence(scope_id);
                return;
            }
        }

        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
    }

    #[inline(never)]
    pub(super) fn publish_send_route_evidence_delta(&mut self, delta: &CommittedCommitDelta) {
        let routes = delta.selected_routes();
        let Some(route_lane) = delta.selected_route_lane() else {
            return;
        };
        let mut idx = 0usize;
        while idx < routes.len() {
            if let Some(route_row) = routes.get(&self.cursor, idx) {
                self.publish_send_route_row_evidence(route_row, route_lane);
            }
            idx += 1;
        }
    }

    #[inline(never)]
    pub(super) fn publish_send_resolver_success_audits_from(
        &self,
        delta: &CommittedCommitDelta,
        start: u16,
    ) {
        let routes = delta.selected_routes();
        let Some(route_lane) = delta.selected_route_lane() else {
            return;
        };
        let mut idx = start as usize;
        while idx < routes.len() {
            let Some(route_row) = routes.get(&self.cursor, idx) else {
                crate::invariant();
            };
            let scope_id = route_row.scope();
            if let Some((crate::global::const_dsl::RouteResolver::Dynamic { resolver_id, .. }, _)) =
                self.cursor.route_scope_controller_resolver(scope_id)
            {
                let arm = match route_row.selected_arm() {
                    0 => crate::session::cluster::core::DecisionArm::Left,
                    1 => crate::session::cluster::core::DecisionArm::Right,
                    _ => crate::invariant(),
                };
                self.emit_dynamic_resolver_success_audit(route_lane, scope_id, resolver_id, arm);
            }
            idx += 1;
        }
    }
}
