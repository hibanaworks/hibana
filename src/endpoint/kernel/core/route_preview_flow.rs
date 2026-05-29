use super::{
    Arm, CursorEndpoint, EndpointSlot, EpochTable, JumpReason, LabelUniverse, MintConfigMarker,
    PassiveArmNavigation, ScopeId, ScopeKind, SendError, SendMeta, SendResult, Transport,
    checked_state_index, state_index_to_usize,
};
#[cfg(test)]
use crate::global::{MessageSpec, SendableLabel};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    #[inline]
    fn preview_scope_region_at(&self, idx: usize) -> Option<crate::global::typestate::ScopeRegion> {
        let scope_id = self.cursor.node_scope_id_at(idx);
        if scope_id.is_none() {
            None
        } else {
            self.cursor.scope_region_by_id(scope_id)
        }
    }

    #[inline]
    fn preview_is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: usize) -> bool {
        let mut arm = 0u8;
        while arm <= 1 {
            if self
                .cursor
                .controller_arm_entry_by_arm(scope_id, arm)
                .map(|(entry, _)| state_index_to_usize(entry) == idx)
                .unwrap_or(false)
            {
                return true;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        false
    }

    fn preview_follow_jumps_from(&self, mut idx: usize) -> SendResult<usize> {
        let mut flow_iter = 0u32;
        let step_bound = self.typestate_step_bound();
        while self.cursor.is_jump_at(idx) {
            if self.cursor.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch) {
                break;
            }
            idx = state_index_to_usize(self.cursor.typestate_node(idx).next());
            flow_iter += 1;
            if flow_iter > step_bound {
                return Err(SendError::PhaseInvariant);
            }
        }
        Ok(idx)
    }

    fn preview_find_arm_for_send_label_in_scope(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> Option<u8> {
        let mut arm = 0u8;
        while arm <= 1 {
            let Some(PassiveArmNavigation::WithinArm { entry }) = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope_id, arm)
            else {
                if arm == 1 {
                    break;
                }
                arm += 1;
                continue;
            };
            let entry_idx = state_index_to_usize(entry);
            let matches = self
                .cursor
                .try_send_meta_at(entry_idx)
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
                || self
                    .cursor
                    .try_local_meta_at(entry_idx)
                    .map(|meta| meta.label == target_label)
                    .unwrap_or(false);
            if matches {
                return Some(arm);
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    fn preview_follow_passive_observer_for_label(
        &self,
        idx: usize,
        target_label: u8,
    ) -> Option<usize> {
        let scope_id = self.cursor.node_scope_id_at(idx);
        let target_arm = self.preview_find_arm_for_send_label_in_scope(scope_id, target_label)?;
        match self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, target_arm)?
        {
            PassiveArmNavigation::WithinArm { entry } => Some(state_index_to_usize(entry)),
        }
    }

    #[inline]
    fn preview_route_arm_for(
        &self,
        lane: u8,
        scope: ScopeId,
        preview_route_arm: Option<(u8, ScopeId, u8)>,
    ) -> Option<u8> {
        if let Some((preview_lane, preview_scope, preview_arm)) = preview_route_arm
            && preview_lane == lane
            && preview_scope == scope
        {
            return Some(preview_arm);
        }
        self.route_arm_for(lane, scope)
    }

    fn preview_selected_arm_for_scope_with_route(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<(u8, ScopeId, u8)>,
    ) -> Option<u8> {
        if scope_id.is_none() {
            return None;
        }
        if let Some((preview_lane, preview_scope, _)) = preview_route_arm
            && preview_scope == scope_id
            && (preview_lane as usize) < self.cursor.logical_lane_count()
        {
            return self.preview_route_arm_for(preview_lane, scope_id, preview_route_arm);
        }
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let Some(summary_lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) else {
            return None;
        };
        self.preview_scope_ack_token_non_consuming(scope_id, summary_lane_idx, offer_lanes)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    #[inline]
    fn preview_can_advance_route_scope(
        &self,
        scope_id: ScopeId,
        target_label: u8,
        preview_route_arm: Option<(u8, ScopeId, u8)>,
    ) -> bool {
        let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
        self.preview_route_arm_for(lane_wire, scope_id, preview_route_arm)
            .is_some()
    }

    #[inline]
    fn preview_flow_start_index(&self, target_label: u8) -> usize {
        if self
            .cursor
            .try_recv_meta()
            .map(|meta| meta.label == target_label)
            .unwrap_or(false)
            || self
                .cursor
                .try_send_meta()
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
            || self
                .cursor
                .try_local_meta()
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
        {
            return self.cursor.index();
        }
        if let Some(region) = self.cursor.scope_region()
            && region.kind == ScopeKind::Route
            && self.cursor.is_route_controller(region.scope_id)
            && self
                .cursor
                .controller_arm_entry_for_label(region.scope_id, target_label)
                .is_some()
        {
            return self.cursor.index();
        }
        if let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label)
            && let Some(idx) = self.cursor.index_for_lane_step(lane_idx)
        {
            return idx;
        }
        self.cursor.index()
    }

    /// Preview the current send transition without mutating endpoint state.
    pub(crate) fn preview_flow_meta(
        &mut self,
        target_label: u8,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        if let Some(kind) = self.session_fault() {
            return Err(SendError::SessionFault(kind));
        }
        let mut idx = self.preview_flow_start_index(target_label);
        let mut preview_route_arm: Option<(u8, ScopeId, u8)> = None;

        if let Some(region) = self.preview_scope_region_at(idx) {
            if region.kind == ScopeKind::Route {
                let scope_id = region.scope_id;
                let at_route_start = idx == region.start;
                let unlabeled = !self.cursor.is_send_at(idx)
                    && !self.cursor.is_recv_at(idx)
                    && !self.cursor.is_local_action_at(idx);
                let at_decision = at_route_start || unlabeled || self.cursor.is_jump_at(idx);

                if region.linger && self.cursor.is_jump_at(idx) {
                    idx = self.preview_follow_jumps_from(idx)?;
                }

                if self.cursor.is_route_controller(scope_id) {
                    let at_arm_entry = self.preview_is_at_controller_arm_entry(scope_id, idx);
                    let at_decision = at_arm_entry || at_decision;
                    if at_decision {
                        if let Some(entry_idx) = self
                            .cursor
                            .controller_arm_entry_for_label(scope_id, target_label)
                        {
                            idx = state_index_to_usize(entry_idx);
                        }
                    }
                } else if at_decision {
                    let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
                    let offer_lanes = self.offer_lane_set_for_scope(scope_id);
                    let preview_arm = offer_lanes
                        .first_set(self.cursor.logical_lane_count())
                        .and_then(|summary_lane_idx| {
                            self.preview_scope_ack_token_non_consuming(
                                scope_id,
                                summary_lane_idx,
                                offer_lanes,
                            )
                            .map(|token| token.arm().as_u8())
                        });
                    let selected_arm = preview_arm
                        .or_else(|| {
                            self.preview_selected_arm_for_scope_with_route(
                                scope_id,
                                preview_route_arm,
                            )
                        })
                        .or_else(|| {
                            self.preview_route_arm_for(lane_wire, scope_id, preview_route_arm)
                        });
                    if let Some(selected_arm) = selected_arm {
                        preview_route_arm = Some((lane_wire, scope_id, selected_arm));
                        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
                            .cursor
                            .follow_passive_observer_arm_for_scope(scope_id, selected_arm)
                        {
                            idx = state_index_to_usize(entry);
                        }
                    }
                }
            }
        }

        let mut flow_iter = 0u32;
        let step_bound = self.typestate_step_bound();
        loop {
            flow_iter += 1;
            debug_assert!(
                flow_iter <= step_bound,
                "flow(): exceeded compiled typestate step bound - CFG cycle bug"
            );
            if flow_iter > step_bound {
                return Err(SendError::PhaseInvariant);
            }

            idx = self.preview_follow_jumps_from(idx)?;

            if self.cursor.is_jump_at(idx)
                && self.cursor.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch)
                && let Some(next_idx) =
                    self.preview_follow_passive_observer_for_label(idx, target_label)
            {
                idx = next_idx;
                continue;
            }

            if !self.cursor.is_send_at(idx) && !self.cursor.is_local_action_at(idx) {
                if let Some(region) = self.preview_scope_region_at(idx)
                    && region.kind == ScopeKind::Route
                    && self.preview_can_advance_route_scope(
                        region.scope_id,
                        target_label,
                        preview_route_arm,
                    )
                {
                    idx = region.end;
                    continue;
                }
                return Err(SendError::PhaseInvariant);
            }

            let current_meta = if self.cursor.is_local_action_at(idx) {
                let local = self
                    .cursor
                    .try_local_meta_at(idx)
                    .ok_or(SendError::PhaseInvariant)?;
                SendMeta::new(
                    local.eff_index,
                    ROLE,
                    local.label,
                    local.frame_label,
                    local.resource,
                    local.semantic,
                    local.is_control,
                    local.next,
                    local.scope,
                    local.route_arm,
                    local.shot,
                    local.policy,
                    local.lane,
                )
            } else {
                self.cursor
                    .try_send_meta_at(idx)
                    .ok_or(SendError::PhaseInvariant)?
            };

            if current_meta.label == target_label {
                return Ok(crate::endpoint::kernel::SendPreview::new(
                    current_meta,
                    checked_state_index(idx).ok_or(SendError::PhaseInvariant)?,
                ));
            }

            if let Some(region) = self.preview_scope_region_at(idx)
                && region.kind == ScopeKind::Route
                && self.preview_can_advance_route_scope(
                    region.scope_id,
                    target_label,
                    preview_route_arm,
                )
            {
                idx = region.end;
                continue;
            }

            return Err(SendError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn preview_flow<M>(&mut self) -> SendResult<crate::endpoint::kernel::SendPreview>
    where
        M: MessageSpec + SendableLabel,
        T: Transport + 'r,
        U: LabelUniverse,
        C: crate::runtime::config::Clock,
        E: EpochTable,
        Mint: MintConfigMarker,
    {
        self.preview_flow_meta(<M as MessageSpec>::LOGICAL_LABEL)
    }
}
