use crate::control::cap::mint::{EpochTable, MintConfigMarker};
use crate::endpoint::{RecvError, RecvResult};
use crate::global::typestate::{ARM_SHARED, state_index_to_usize};
use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::transport::Transport;

use super::super::authority::{Arm, RouteArmToken};
use super::super::core::{
    BranchPreviewView, CursorEndpoint, event_selected_route_scope_from_event_rows,
    prepare_event_selected_route_commit_row_from_event_rows, scope_slot_for_route_from_cursor,
};
use super::{BranchCommitPlan, BranchKind};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(in crate::endpoint::kernel) fn preflight_branch_preview_commit_plan(
        &self,
        preview: BranchPreviewView,
    ) -> RecvResult<BranchCommitPlan> {
        let scope_id = preview.branch_meta.scope_id;
        let selected_arm = preview.branch_meta.selected_arm;
        let lane_wire = preview.branch_meta.lane_wire;
        let lane_idx = lane_wire as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let route_row = prepare_event_selected_route_commit_row_from_event_rows(
            &self.decision_state,
            &self.cursor,
            lane_wire,
            state_index_to_usize(preview.branch_meta.cursor_index),
            selected_arm,
        );
        if route_row.is_none() {
            let route_scope = event_selected_route_scope_from_event_rows(
                &self.cursor,
                state_index_to_usize(preview.branch_meta.cursor_index),
                selected_arm,
            )
            .ok_or(RecvError::PhaseInvariant)?;
            if scope_slot_for_route_from_cursor(&self.cursor, route_scope).is_some()
                && self.selected_arm_for_scope(route_scope) != Some(selected_arm)
            {
                return Err(RecvError::PhaseInvariant);
            }
        }
        if preview.branch_meta.route_token.is_poll()
            && preview.branch_meta.kind == BranchKind::WireRecv
        {
            if preview
                .branch_meta
                .profile
                .poll_wire_commit_requires_event()
            {
                if !preview
                    .branch_meta
                    .route_arm_selection_commit_evidence
                    .emits_route_arm_selection_event()
                {
                    return Err(RecvError::PhaseInvariant);
                }
            } else if preview
                .branch_meta
                .profile
                .poll_wire_commit_requires_static_observation()
            {
                let Some((arm, _)) = self.cursor.observed_recv_target_for_lane_frame_label(
                    scope_id,
                    lane_wire,
                    preview.branch_meta.frame_label,
                ) else {
                    return Err(RecvError::PhaseInvariant);
                };
                let arm = if arm == ARM_SHARED { 0 } else { arm };
                if arm != selected_arm {
                    return Err(RecvError::PhaseInvariant);
                }
            }
        }

        let meta = if preview.branch_meta.kind == BranchKind::WireRecv {
            let mut meta = if let Some(meta) = self
                .cursor
                .try_recv_meta_at(state_index_to_usize(preview.branch_meta.cursor_index))
            {
                meta
            } else {
                return Err(RecvError::PhaseInvariant);
            };
            if meta.route_arm.is_none() {
                meta.route_arm = Some(selected_arm);
            }
            if meta.label != preview.label {
                meta.label = preview.label;
            }
            Some(meta)
        } else {
            None
        };

        Ok(BranchCommitPlan {
            preview,
            meta,
            route_row,
        })
    }

    pub(in crate::endpoint::kernel) fn publish_branch_preview_commit_plan(
        &mut self,
        plan: BranchCommitPlan,
    ) {
        let preview = plan.preview;
        let scope_id = preview.branch_meta.scope_id;
        let selected_arm = preview.branch_meta.selected_arm;
        let lane_wire = preview.branch_meta.lane_wire;

        if preview
            .branch_meta
            .profile
            .publishes_recvless_parent_route_arm_selection()
        {
            if let Some(plan) = self.build_recvless_parent_route_arm_selection_plan(scope_id) {
                self.publish_recvless_parent_route_arm_selection(plan);
            }
        }

        if preview.branch_meta.route_token.is_ack()
            && preview
                .branch_meta
                .profile
                .publishes_controller_ack_decision()
        {
            let Some(arm) = Arm::new(selected_arm) else {
                return;
            };
            let token = RouteArmToken::from_ack(arm);
            if matches!(preview.branch_meta.kind, BranchKind::ArmSendHint) {
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
                        next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
                    }
                }
            }
        } else if preview.branch_meta.route_token.is_poll() {
            self.emit_route_arm_selection(
                scope_id,
                preview.branch_meta.route_token,
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
