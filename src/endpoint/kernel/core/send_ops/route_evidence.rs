use super::super::{
    Arm, CommittedCommitDelta, OperationKernel, RouteArmToken, SelectedRouteCommitRowsRef,
    SendError, SendMeta, SendResult, Transport,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
};

impl<'r, T> OperationKernel<'_, 'r, T>
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
        let crate::endpoint::kernel::core::ActorProgress {
            cursor,
            decision_state,
            route_commit_rows,
            ..
        } = &mut *self.progress;
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
    pub(super) fn commit_send_route_evidence_delta(&mut self, delta: &CommittedCommitDelta) {
        let routes = delta.selected_routes();
        if routes.len() == 0 {
            return;
        }
        let route_lane = crate::invariant_some(delta.selected_route_lane());
        let mut idx = 0usize;
        while idx < routes.len() {
            let route_row = crate::invariant_some(routes.get(&self.progress.cursor, idx));
            let scope_id = route_row.scope();
            let selected_arm = route_row.selected_arm();
            if delta.route_is_fresh(idx)
                && self.is_route_controller(scope_id)
                && self
                    .progress
                    .cursor
                    .route_scope_resolver(scope_id)
                    .is_some()
            {
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_resolver(Arm::from_raw(selected_arm)),
                    route_lane,
                );
            }
            if self.arm_has_recv(scope_id, selected_arm) {
                self.consume_scope_ready_arm(scope_id, selected_arm);
            }
            self.clear_scope_evidence(scope_id);
            idx += 1;
        }
    }
}
