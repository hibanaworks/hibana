use super::super::{
    Arm, CommittedCommitDelta, CursorEndpoint, RouteArmToken, SelectedRouteCommitRow,
    SelectedRouteCommitRowsRef, SendError, SendMeta, SendResult, Transport,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
};

#[derive(Clone, Copy)]
enum SendRouteEvidencePhase {
    Begin,
    Continue,
}

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
        rows.finish_for_lane(meta.lane)
            .map_err(|_| SendError::PhaseInvariant)
    }

    #[inline(never)]
    fn commit_send_route_row_evidence(
        &mut self,
        route_row: SelectedRouteCommitRow,
        lane_wire: u8,
        phase: SendRouteEvidencePhase,
    ) {
        let scope_id = route_row.scope();
        let selected_arm = route_row.selected_arm();
        let route_token = self.peek_live_scope_ack(scope_id);
        if matches!(phase, SendRouteEvidencePhase::Begin) {
            let arm = match route_token {
                Some(RouteArmToken::Resolver(arm)) => arm,
                None if self.cursor.route_scope_resolver(scope_id).is_some() => {
                    Arm::from_raw(selected_arm)
                }
                Some(RouteArmToken::Ack(_) | RouteArmToken::Poll(_)) | None => crate::invariant(),
            };
            self.emit_route_arm_selection(scope_id, RouteArmToken::from_resolver(arm), lane_wire);
            if self.arm_has_recv(scope_id, selected_arm) {
                self.consume_scope_ready_arm(scope_id, selected_arm);
            }
            self.clear_scope_evidence(scope_id);
            return;
        }
        match route_token {
            Some(RouteArmToken::Ack(_)) => {
                let arm = Arm::from_raw(selected_arm);
                self.emit_route_arm_selection(scope_id, RouteArmToken::from_ack(arm), lane_wire);
            }
            Some(RouteArmToken::Poll(_)) => {
                let arm = Arm::from_raw(selected_arm);
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_poll(arm),
                    self.offer_lane_for_scope(scope_id),
                );
            }
            Some(RouteArmToken::Resolver(_)) => crate::invariant(),
            None if self.cursor.route_scope_resolver(scope_id).is_some() => {
                let arm = Arm::from_raw(selected_arm);
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
    pub(super) fn commit_send_route_evidence_delta(
        &mut self,
        delta: &CommittedCommitDelta,
        begin_route_start: Option<usize>,
    ) {
        let routes = delta.selected_routes();
        let Some(route_lane) = delta.selected_route_lane() else {
            return;
        };
        let mut idx = 0usize;
        while idx < routes.len() {
            if let Some(route_row) = routes.get(&self.cursor, idx) {
                let phase = if begin_route_start.is_some_and(|start| idx >= start) {
                    if !delta.route_is_fresh(idx) {
                        crate::invariant();
                    }
                    SendRouteEvidencePhase::Begin
                } else {
                    SendRouteEvidencePhase::Continue
                };
                self.commit_send_route_row_evidence(route_row, route_lane, phase);
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
            if let Some(resolver) = self.cursor.route_scope_resolver(scope_id) {
                let resolver_id = resolver.resolver_id();
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
