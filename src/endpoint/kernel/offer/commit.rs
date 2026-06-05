use crate::binding::EndpointSlot;
use crate::control::cap::mint::{EpochTable, MintConfigMarker};
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::ScopeKind;
use crate::global::typestate::{ARM_SHARED, state_index_to_usize};
use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::transport::Transport;

use super::super::authority::RouteDecisionSource;
use super::super::core::{BranchPreviewView, CursorEndpoint};
use super::{BranchCommitPlan, BranchKind};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
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
        let clear_other_lanes = self.selected_arm_for_scope(scope_id) != Some(selected_arm);
        let route_arm_proof = if clear_other_lanes {
            self.preflight_route_arm_commit_after_clearing_other_lanes(
                lane_wire,
                scope_id,
                selected_arm,
            )
        } else {
            self.preflight_route_arm_commit(lane_wire, scope_id, selected_arm)
        };
        if scope_id.kind() == ScopeKind::Route && route_arm_proof.is_none() {
            return Err(RecvError::PhaseInvariant);
        }
        if preview.branch_meta.route_source == RouteDecisionSource::Poll
            && preview.branch_meta.kind == BranchKind::WireRecv
        {
            if preview
                .branch_meta
                .profile
                .poll_wire_commit_requires_event()
            {
                if !preview
                    .branch_meta
                    .route_decision_commit_evidence
                    .emits_route_decision_event()
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
            route_arm_proof,
            clear_other_lanes,
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

        if plan.clear_other_lanes {
            self.clear_scope_route_state_for_other_lanes(scope_id, lane_wire);
        }
        if let Some(proof) = plan.route_arm_proof {
            self.commit_route_arm_after_preflight(proof);
        }
        self.skip_unselected_arm_lanes(scope_id, selected_arm, lane_wire);

        if preview
            .branch_meta
            .profile
            .publishes_recvless_parent_route_decision()
        {
            if let Some(plan) = self.build_recvless_parent_route_decision_plan(scope_id) {
                self.publish_recvless_parent_route_decision(plan);
            }
        }

        match preview.branch_meta.route_source {
            RouteDecisionSource::Ack
                if preview
                    .branch_meta
                    .profile
                    .publishes_controller_ack_decision() =>
            {
                if matches!(preview.branch_meta.kind, BranchKind::ArmSendHint) {
                    let lane = lane_wire;
                    self.record_route_decision_for_lane(lane as usize, scope_id, selected_arm);
                    self.emit_route_decision(
                        scope_id,
                        selected_arm,
                        RouteDecisionSource::Ack,
                        lane,
                    );
                } else {
                    let offer_lanes = self.offer_lane_set_for_scope(scope_id);
                    if offer_lanes.is_empty() {
                        let lane = lane_wire;
                        self.record_route_decision_for_lane(lane as usize, scope_id, selected_arm);
                        self.emit_route_decision(
                            scope_id,
                            selected_arm,
                            RouteDecisionSource::Ack,
                            lane,
                        );
                    } else {
                        let lane_limit = self.cursor.logical_lane_count();
                        let mut next = offer_lanes.first_set(lane_limit);
                        while let Some(lane_idx) = next {
                            let lane = lane_idx as u8;
                            self.record_route_decision_for_lane(
                                lane as usize,
                                scope_id,
                                selected_arm,
                            );
                            self.emit_route_decision(
                                scope_id,
                                selected_arm,
                                RouteDecisionSource::Ack,
                                lane,
                            );
                            next =
                                offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
                        }
                    }
                }
            }
            RouteDecisionSource::Poll => {
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Poll,
                    self.offer_lane_for_scope(scope_id),
                );
            }
            _ => {}
        }

        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
        self.port_for_lane(lane_wire as usize).clear_route_hints();
    }
}
