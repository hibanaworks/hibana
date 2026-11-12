use crate::endpoint::{RecvError, RecvResult};
use crate::global::typestate::state_index_to_usize;
use crate::transport::Transport;

use super::super::authority::{Arm, RouteArmToken};
use super::super::core::{
    CommittedCommitDelta, OperationKernel,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
};
use super::{BranchCommitPlan, BranchKind, BranchMeta};
impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn preflight_branch_preview_commit_plan(
        &mut self,
        branch: BranchMeta,
    ) -> RecvResult<BranchCommitPlan> {
        let lane_wire = branch.lane_wire;
        let lane_idx = lane_wire as usize;
        if lane_idx >= self.progress.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let Some(choice) = branch.choice else {
            if branch.kind != BranchKind::WireRecv {
                return Err(RecvError::PhaseInvariant);
            }
            let meta = self
                .progress
                .cursor
                .try_recv_meta_at(state_index_to_usize(branch.cursor_index))
                .ok_or(RecvError::PhaseInvariant)?;
            return Ok(BranchCommitPlan::new(
                Some(meta),
                super::super::SelectedRouteCommitRowsRef::EMPTY,
            ));
        };
        let route_seed_rows = {
            let crate::endpoint::kernel::core::ActorProgress {
                cursor,
                decision_state,
                route_commit_rows,
                ..
            } = &mut *self.progress;
            let event_idx = state_index_to_usize(branch.cursor_index);
            let mut rows = route_commit_rows.begin();
            prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
                decision_state,
                cursor,
                lane_wire,
                event_idx,
                &mut rows,
            )?;
            rows.finish_for_lane(lane_wire)?
        };
        let scope_id = choice.scope_id;
        let selected_arm = choice.selected_arm;
        if choice.route_token.is_committed()
            && self.committed_offer_arm(scope_id, branch.cursor_index.as_usize())
                != Some(Arm::from_raw(selected_arm))
        {
            return Err(RecvError::PhaseInvariant);
        }
        if route_seed_rows.is_empty() {
            return Err(RecvError::PhaseInvariant);
        }
        let branch_scope_arm = route_seed_rows.arm_for_scope(&self.progress.cursor, scope_id);
        if branch_scope_arm != Some(selected_arm) {
            return Err(RecvError::PhaseInvariant);
        }
        if choice.route_token.is_resolver()
            && self
                .progress
                .cursor
                .route_scope_resolver(scope_id)
                .is_none()
        {
            return Err(RecvError::PhaseInvariant);
        }
        if choice.route_token.is_poll() && branch.kind == BranchKind::WireRecv {
            if choice.profile.poll_wire_commit_requires_event() {
                if !choice
                    .route_arm_selection_commit_evidence
                    .emits_route_arm_selection_event()
                {
                    return Err(RecvError::PhaseInvariant);
                }
            } else if choice
                .profile
                .poll_wire_commit_requires_intrinsic_observation()
            {
                let Some(arm) = self
                    .progress
                    .cursor
                    .route_arm_for_index(scope_id, state_index_to_usize(branch.cursor_index))
                else {
                    return Err(RecvError::PhaseInvariant);
                };
                if arm != selected_arm {
                    return Err(RecvError::PhaseInvariant);
                }
            }
        }

        let meta = if branch.kind == BranchKind::WireRecv {
            let mut meta = if let Some(meta) = self
                .progress
                .cursor
                .try_recv_meta_at(state_index_to_usize(branch.cursor_index))
            {
                meta
            } else {
                return Err(RecvError::PhaseInvariant);
            };
            if meta.route_arm.is_none() {
                meta.route_arm = Some(selected_arm);
            }
            Some(meta)
        } else {
            None
        };

        Ok(BranchCommitPlan::new(meta, route_seed_rows))
    }

    pub(in crate::endpoint::kernel) fn publish_branch_preview_commit_plan(
        &mut self,
        branch: BranchMeta,
        committed: &CommittedCommitDelta,
    ) {
        let routes = committed.selected_routes();
        let mut branch_route_fresh = false;
        let mut route_idx = 0usize;
        while route_idx < routes.len() {
            let Some(row) = routes.get(&self.progress.cursor, route_idx) else {
                crate::invariant();
            };
            if committed.route_is_fresh(route_idx)
                && branch
                    .choice
                    .is_some_and(|choice| row.scope() == choice.scope_id)
            {
                branch_route_fresh = true;
            }
            if committed.route_is_fresh(route_idx)
                && self.is_route_controller(row.scope())
                && self
                    .progress
                    .cursor
                    .route_scope_resolver(row.scope())
                    .is_some()
            {
                self.emit_route_arm_selection(
                    row.scope(),
                    RouteArmToken::from_resolver(Arm::from_raw(row.selected_arm())),
                );
            }
            route_idx += 1;
        }
        let Some(choice) = branch.choice else {
            return;
        };
        let scope_id = choice.scope_id;
        let selected_arm = choice.selected_arm;
        let route_token = choice.route_token;

        if route_token.is_committed() {
            if branch_route_fresh || self.selected_arm_for_scope(scope_id) != Some(selected_arm) {
                crate::invariant();
            }
        } else if route_token.is_resolver() {
            // Another independent fragment may already have committed this
            // preview's choice. It must have selected the same arm.
            if !branch_route_fresh && self.selected_arm_for_scope(scope_id) != Some(selected_arm) {
                crate::invariant();
            }
            if self
                .progress
                .cursor
                .route_scope_resolver(scope_id)
                .is_none()
            {
                crate::invariant();
            }
        } else if route_token.is_ack() && choice.profile.publishes_controller_ack_decision() {
            let arm = Arm::from_raw(selected_arm);
            let token = RouteArmToken::from_ack(arm);
            self.emit_route_arm_selection(scope_id, token);
        } else if route_token.is_poll() {
            self.emit_route_arm_selection(scope_id, route_token);
        }

        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
    }
}
