use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::RouteResolver;
use crate::global::typestate::state_index_to_usize;
use crate::session::cluster::core::DecisionArm;
use crate::transport::Transport;

use super::super::authority::{Arm, RouteArmToken};
use super::super::core::{
    CursorEndpoint, prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
};
use super::{BranchCommitPlan, BranchKind, BranchMeta};
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn preflight_branch_preview_commit_plan(
        &mut self,
        branch: BranchMeta,
    ) -> RecvResult<BranchCommitPlan> {
        let scope_id = branch.scope_id;
        let selected_arm = branch.selected_arm;
        let lane_wire = branch.lane_wire;
        let lane_idx = lane_wire as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let route_seed_rows = {
            let Self {
                cursor,
                decision_state,
                route_commit_rows,
                ..
            } = self;
            let event_idx = state_index_to_usize(branch.cursor_index);
            let Some(event_meta) = cursor.try_recv_meta_at(event_idx) else {
                return Err(RecvError::PhaseInvariant);
            };
            if event_meta.route_arm.is_none() {
                return Err(RecvError::PhaseInvariant);
            }
            let mut rows = route_commit_rows.begin();
            prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
                decision_state,
                cursor,
                lane_wire,
                event_idx,
                &mut rows,
            )?;
            rows.as_commit_rows(lane_wire)
        };
        if route_seed_rows.is_empty() {
            return Err(RecvError::PhaseInvariant);
        }
        let mut branch_scope_arm = None;
        let mut row_idx = 0usize;
        while row_idx < route_seed_rows.len() {
            if let Some(row) = route_seed_rows.get(&self.cursor, row_idx)
                && row.scope() == scope_id
            {
                branch_scope_arm = Some(row.selected_arm());
                break;
            }
            row_idx += 1;
        }
        if branch_scope_arm != Some(selected_arm) {
            return Err(RecvError::PhaseInvariant);
        }
        if branch.route_token.is_resolver() {
            let Some((RouteResolver::Dynamic { scope, .. }, _)) =
                self.cursor.route_scope_controller_resolver(scope_id)
            else {
                return Err(RecvError::PhaseInvariant);
            };
            if scope != scope_id {
                return Err(RecvError::PhaseInvariant);
            }
        }
        if branch.route_token.is_poll() && branch.kind == BranchKind::WireRecv {
            if branch.profile.poll_wire_commit_requires_event() {
                if !branch
                    .route_arm_selection_commit_evidence
                    .emits_route_arm_selection_event()
                {
                    return Err(RecvError::PhaseInvariant);
                }
            } else if branch
                .profile
                .poll_wire_commit_requires_intrinsic_observation()
            {
                let Some(arm) = self
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
    ) {
        let scope_id = branch.scope_id;
        let selected_arm = branch.selected_arm;
        let lane_wire = branch.lane_wire;
        let route_token = branch.route_token;

        if route_token.is_resolver() {
            let Some((RouteResolver::Dynamic { resolver_id, .. }, _)) =
                self.cursor.route_scope_controller_resolver(scope_id)
            else {
                crate::invariant();
            };
            let decision_lane = self.offer_lane_for_scope(scope_id);
            let arm = match selected_arm {
                0 => DecisionArm::Left,
                1 => DecisionArm::Right,
                _ => crate::invariant(),
            };
            self.emit_dynamic_resolver_success_audit(decision_lane, scope_id, resolver_id, arm);
            self.record_route_arm_selection_for_scope_lanes(scope_id, selected_arm, decision_lane);
            self.record_scope_ack(scope_id, route_token);
            self.emit_route_arm_selection(scope_id, route_token, decision_lane);
        } else if route_token.is_ack() && branch.profile.publishes_controller_ack_decision() {
            let Some(arm) = Arm::new(selected_arm) else {
                return;
            };
            let token = RouteArmToken::from_ack(arm);
            if matches!(branch.kind, BranchKind::ArmSendHint) {
                let lane = lane_wire;
                self.record_route_arm_selection_for_lane(lane as usize, scope_id, selected_arm);
                self.emit_route_arm_selection(scope_id, token, lane);
            } else {
                let offer_lanes = self.offer_lane_set_for_scope(scope_id);
                if offer_lanes.is_empty() {
                    let lane = lane_wire;
                    self.record_route_arm_selection_for_lane(lane as usize, scope_id, selected_arm);
                    self.emit_route_arm_selection(scope_id, token, lane);
                } else {
                    let lane_limit = self.cursor.logical_lane_count();
                    let mut next = offer_lanes.first_set(lane_limit);
                    while let Some(lane_idx) = next {
                        let lane = lane_idx as u8;
                        self.record_route_arm_selection_for_lane(
                            lane as usize,
                            scope_id,
                            selected_arm,
                        );
                        self.emit_route_arm_selection(scope_id, token, lane);
                        next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
                    }
                }
            }
        } else if route_token.is_poll() {
            self.emit_route_arm_selection(
                scope_id,
                route_token,
                self.offer_lane_for_scope(scope_id),
            );
        }

        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
        self.port_for_lane(lane_wire as usize).clear_route_hints();
    }
}
