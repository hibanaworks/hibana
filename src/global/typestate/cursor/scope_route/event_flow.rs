use super::super::super::facts::LocalDependencyState;
use super::super::{
    EffIndex, EnabledEventCommit, EventCursor, FlowPreviewError, JumpReason, LocalDependency,
    RecvlessParentRouteDecision, RelocatableResidentLaneStep, ResidentLaneStepError, ScopeId,
    SendMeta, StateIndex, state_index_to_usize,
};

#[derive(Clone, Copy)]
struct FlowPreviewRouteArm {
    lane: u8,
    scope: ScopeId,
    arm: u8,
}

impl EventCursor {
    #[inline(always)]
    pub(crate) fn recvless_parent_route_decision(
        &self,
        child_scope: ScopeId,
        mut arm_requires_wire_recv: impl FnMut(ScopeId, u8) -> bool,
    ) -> Option<RecvlessParentRouteDecision> {
        let parent_scope = self.route_parent_scope(child_scope)?;
        let parent_region = self.scope_region_by_id(parent_scope)?;
        if !parent_region.linger {
            return None;
        }
        if self.is_route_controller(parent_scope) {
            return None;
        }
        if self
            .route_scope_controller_policy(parent_scope)
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false)
        {
            return None;
        }
        let mut arm = 0u8;
        while arm <= 1 {
            if arm_requires_wire_recv(parent_scope, arm) {
                return None;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        let parent_arm = self.route_parent_arm(child_scope)?;
        RecvlessParentRouteDecision::new(parent_scope, parent_arm)
    }

    #[inline(always)]
    pub(crate) fn is_non_wire_loop_control_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        role: u8,
    ) -> bool {
        let Some(entry_idx) = self.passive_observer_arm_entry_index(scope_id, arm) else {
            return false;
        };
        let Some(recv_meta) = self.try_recv_meta_at(entry_idx) else {
            return false;
        };
        recv_meta.is_control
            && recv_meta.route_arm == Some(arm)
            && (recv_meta.peer == role
                || (!self.is_route_controller(scope_id) && recv_meta.semantic.is_loop()))
    }

    #[inline]
    pub(crate) fn parallel_scope_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().parallel_root(scope_id)
    }

    #[inline(always)]
    pub(crate) fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        self.machine().dependency_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) fn dependency_state_for_index(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> LocalDependencyState {
        let Some(dependency) = self.dependency_for_index(idx) else {
            return LocalDependencyState::Satisfied;
        };
        if !Self::dependency_applies(dependency, |scope| selected_arm_for_scope(scope)) {
            return LocalDependencyState::InactiveByConflict;
        }
        if self.dependency_events_done(dependency, |scope| selected_arm_for_scope(scope)) {
            LocalDependencyState::Satisfied
        } else {
            LocalDependencyState::Blocked
        }
    }

    pub(crate) fn event_row_matches_commit(
        &self,
        idx: usize,
        eff_index: EffIndex,
        label: u8,
        is_control: bool,
        scope: ScopeId,
        route_arm: Option<u8>,
        lane: u8,
    ) -> bool {
        self.machine()
            .event_program()
            .event_row_at(idx)
            .is_some_and(|row| {
                row.matches_commit(eff_index, label, is_control, scope, route_arm, lane)
            })
    }

    pub(crate) fn enabled_event_allows_commit(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        lane: u8,
        cursor_after: StateIndex,
        preview_scope: ScopeId,
        preview_arm: Option<u8>,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Result<(), ResidentLaneStepError> {
        if self
            .try_next_index_past_jumps_from(StateIndex::from_usize(idx))
            .map_err(|_| ResidentLaneStepError)?
            != cursor_after
        {
            return Err(ResidentLaneStepError);
        }
        if !self
            .dependency_state_for_index(idx, |scope| selected_arm_for_scope(scope))
            .allows_event()
        {
            return Err(ResidentLaneStepError);
        }
        if !self.node_conflict_allows(idx, |scope| {
            if scope == preview_scope && preview_arm.is_some() {
                preview_arm
            } else {
                selected_arm_for_scope(scope)
            }
        }) {
            return Err(ResidentLaneStepError);
        }
        let resident_step = self.relocatable_resident_lane_step_at_index(idx, lane as usize)?;
        if resident_step != progress_step || self.relocatable_step_done(progress_step) {
            return Err(ResidentLaneStepError);
        }
        if !self.event_lane_head_allows(progress_step, preview_scope, preview_arm, |scope| {
            selected_arm_for_scope(scope)
        }) {
            return Err(ResidentLaneStepError);
        }
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn enabled_event_commit(
        &self,
        idx: usize,
        eff_index: EffIndex,
        label: u8,
        is_control: bool,
        scope: ScopeId,
        route_arm: Option<u8>,
        lane: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Result<EnabledEventCommit, ResidentLaneStepError> {
        if !self.event_row_matches_commit(idx, eff_index, label, is_control, scope, route_arm, lane)
        {
            return Err(ResidentLaneStepError);
        }
        if let Some(arm) = route_arm
            && let Some(selected) = selected_arm_for_scope(scope)
            && selected != arm
        {
            return Err(ResidentLaneStepError);
        }
        let progress_step = self.relocatable_resident_lane_step_at_index(idx, lane as usize)?;
        let cursor_after = self
            .try_next_index_past_jumps_from(StateIndex::from_usize(idx))
            .map_err(|_| ResidentLaneStepError)?;
        self.enabled_event_allows_commit(
            idx,
            progress_step,
            lane,
            cursor_after,
            scope,
            route_arm,
            |scope| selected_arm_for_scope(scope),
        )?;
        Ok(EnabledEventCommit::new(progress_step, cursor_after))
    }

    #[inline(always)]
    pub(crate) fn event_dependency_allows(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.dependency_state_for_index(idx, |scope| selected_arm_for_scope(scope))
            .allows_event()
    }

    #[inline(always)]
    pub(crate) fn event_conflict_allows(
        &self,
        idx: usize,
        preview_scope: ScopeId,
        preview_arm: Option<u8>,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.node_conflict_allows(idx, |scope| {
            if scope == preview_scope && preview_arm.is_some() {
                preview_arm
            } else {
                selected_arm_for_scope(scope)
            }
        })
    }

    pub(crate) fn recv_start_index_for_label(
        &self,
        target_label: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> usize {
        let current = self.index();
        if let Some(meta) = self.try_recv_meta_at(current)
            && meta.label == target_label
        {
            return current;
        }
        if let Some(meta) = self.try_send_meta_at(current)
            && meta.label == target_label
        {
            return current;
        }
        if let Some(meta) = self.try_local_meta_at(current)
            && meta.label == target_label
        {
            return current;
        }
        if let Some(region) = self.route_scope_region_at(current)
            && self.is_route_controller(region.scope())
            && self
                .controller_arm_entry_for_label(region.scope(), target_label)
                .is_some()
        {
            return current;
        }
        if let Some(region) = self.route_scope_region_at(current)
            && let Some(selected_arm) = selected_arm_for_scope(region.scope())
            && let Some(idx) = self.selected_route_label_index(
                region.scope(),
                target_label,
                selected_arm,
                |scope| selected_arm_for_scope(scope),
            )
        {
            return idx;
        }
        if let Some((lane_idx, _)) = self.pending_step_for_label(target_label)
            && let Some(idx) = self.index_for_lane_step(lane_idx)
        {
            return idx;
        }
        current
    }

    pub(crate) fn recv_descriptor_index_for_label(
        &self,
        target_label: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut idx =
            self.recv_start_index_for_label(target_label, |scope| selected_arm_for_scope(scope));
        let mut iter_count = 0usize;
        let descriptor_bound = self
            .local_steps_len()
            .saturating_add(self.route_scope_count())
            .saturating_add(1);
        while iter_count <= descriptor_bound {
            iter_count += 1;
            if let Some(region) = self.route_scope_region_at(idx)
                && idx == region.start()
            {
                let scope_id = region.scope();
                let arm = selected_arm_for_scope(scope_id)?;
                if let Some(entry_idx) = self.selected_route_arm_recv_entry_index(scope_id, arm) {
                    idx = entry_idx;
                    continue;
                }
                if region.end() != idx {
                    idx = region.end();
                    continue;
                }
            }
            if !self.event_conflict_allows(idx, ScopeId::none(), None, |scope| {
                selected_arm_for_scope(scope)
            }) {
                idx = state_index_to_usize(self.node_next_index_at(idx));
                continue;
            }
            if let Some(meta) = self.try_recv_meta_at(idx) {
                if let Some(arm) = meta.route_arm {
                    match selected_arm_for_scope(meta.scope) {
                        Some(selected) if selected == arm => {}
                        Some(_) => {
                            idx = state_index_to_usize(self.node_next_index_at(idx));
                            continue;
                        }
                        None if meta.label != target_label => {
                            idx = state_index_to_usize(self.node_next_index_at(idx));
                            continue;
                        }
                        None => {}
                    }
                }
                return Some(idx);
            }
            if let Some(end) =
                self.selected_route_scope_end_at(idx, |scope| selected_arm_for_scope(scope))
                && end != idx
            {
                idx = end;
                continue;
            }
            return None;
        }
        None
    }

    #[inline]
    fn flow_is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: usize) -> bool {
        let mut arm = 0u8;
        while arm <= 1 {
            if self
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

    #[inline]
    fn flow_controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> Option<(u8, usize)> {
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some((entry, label)) = self.controller_arm_entry_by_arm(scope_id, arm)
                && label == target_label
            {
                return Some((arm, state_index_to_usize(entry)));
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    #[inline]
    fn flow_send_lane_at(&self, idx: usize) -> Option<u8> {
        self.try_send_meta_at(idx)
            .map(|meta| meta.lane)
            .or_else(|| self.try_local_meta_at(idx).map(|meta| meta.lane))
    }

    fn flow_follow_jumps_from(&self, mut idx: usize) -> Result<usize, FlowPreviewError> {
        let mut iter = 0usize;
        let bound = self
            .local_steps_len()
            .saturating_add(self.route_scope_count())
            .saturating_add(1);
        while self.is_jump_at(idx) {
            if self.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch) {
                break;
            }
            idx = state_index_to_usize(self.node_next_index_at(idx));
            iter = iter.saturating_add(1);
            if iter > bound {
                return Err(FlowPreviewError::Invariant);
            }
        }
        Ok(idx)
    }

    fn flow_find_arm_for_send_label_in_scope(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> Option<u8> {
        let mut arm = 0u8;
        while arm <= 1 {
            let Some(entry_idx) = self.passive_observer_arm_entry_index(scope_id, arm) else {
                if arm == 1 {
                    break;
                }
                arm += 1;
                continue;
            };
            let matches = self
                .try_send_meta_at(entry_idx)
                .map(|meta| meta.label == target_label)
                .unwrap_or(false)
                || self
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

    fn flow_follow_passive_observer_for_label(
        &self,
        idx: usize,
        target_label: u8,
    ) -> Option<usize> {
        let scope_id = self.node_scope_id_at(idx);
        let target_arm = self.flow_find_arm_for_send_label_in_scope(scope_id, target_label)?;
        self.passive_observer_arm_entry_index(scope_id, target_arm)
    }

    #[inline]
    fn flow_selected_arm_for_scope_with_route<F>(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<FlowPreviewRouteArm>,
        arm_for_scope: &mut F,
    ) -> Option<u8>
    where
        F: FnMut(ScopeId) -> Option<u8>,
    {
        if scope_id.is_none() {
            return None;
        }
        if let Some(preview) = preview_route_arm
            && preview.scope == scope_id
            && (preview.lane as usize) < self.logical_lane_count()
        {
            return Some(preview.arm);
        }
        arm_for_scope(scope_id)
    }

    fn flow_route_scope_end_if_complete<F>(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<FlowPreviewRouteArm>,
        arm_for_scope: &mut F,
    ) -> Option<usize>
    where
        F: FnMut(ScopeId) -> Option<u8>,
    {
        self.flow_selected_arm_for_scope_with_route(scope_id, preview_route_arm, arm_for_scope)?;
        if !self.scope_events_done(scope_id, |scope| {
            self.flow_selected_arm_for_scope_with_route(scope, preview_route_arm, arm_for_scope)
        }) {
            return None;
        }
        self.route_scope_end_by_id(scope_id)
    }

    #[inline]
    fn flow_pending_label_index(&self, target_label: u8) -> Option<usize> {
        let idx = self.index();
        if let Some(meta) = self.try_recv_meta_at(idx)
            && meta.label == target_label
            && self.index_for_lane_step(meta.lane as usize) == Some(idx)
        {
            return Some(idx);
        }
        if let Some(meta) = self.try_send_meta_at(idx)
            && meta.label == target_label
            && self.index_for_lane_step(meta.lane as usize) == Some(idx)
        {
            return Some(idx);
        }
        if let Some(meta) = self.try_local_meta_at(idx)
            && meta.label == target_label
            && self.index_for_lane_step(meta.lane as usize) == Some(idx)
        {
            return Some(idx);
        }
        None
    }

    #[inline]
    fn flow_start_index_for_label(&self, target_label: u8) -> usize {
        if let Some(idx) = self.flow_pending_label_index(target_label) {
            return idx;
        }
        if let Some((lane_idx, _)) = self.pending_step_for_label(target_label)
            && let Some(idx) = self.index_for_lane_step(lane_idx)
        {
            return idx;
        }
        if let Some(region) = self.route_scope_region_at(self.index())
            && self.is_route_controller(region.scope())
            && self
                .controller_arm_entry_for_label(region.scope(), target_label)
                .is_some()
        {
            return self.index();
        }
        self.first_pending_step_index(usize::MAX)
            .unwrap_or_else(|| self.index())
    }

    pub(crate) fn flow_preview_send_meta_for_label<const ROLE: u8>(
        &self,
        target_label: u8,
        mut committed_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut inferred_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut lane_for_label_or_offer: impl FnMut(ScopeId, u8) -> u8,
    ) -> Result<(SendMeta, StateIndex), FlowPreviewError> {
        let mut idx = self.flow_start_index_for_label(target_label);
        let mut preview_route_arm: Option<FlowPreviewRouteArm> = None;

        if let Some(region) = self.route_scope_region_at(idx) {
            let scope_id = region.scope();
            let at_route_start = idx == region.start();
            let unlabeled =
                !self.is_send_at(idx) && !self.is_recv_at(idx) && !self.is_local_action_at(idx);
            let at_decision = at_route_start || unlabeled || self.is_jump_at(idx);

            if region.linger() && self.is_jump_at(idx) {
                idx = self.flow_follow_jumps_from(idx)?;
            }

            if self.is_route_controller(scope_id) {
                let at_arm_entry = self.flow_is_at_controller_arm_entry(scope_id, idx);
                let at_decision = at_arm_entry || at_decision;
                if at_decision {
                    if let Some((arm, entry_idx)) =
                        self.flow_controller_arm_entry_for_label(scope_id, target_label)
                    {
                        if let Some(selected) = committed_arm_for_scope(scope_id)
                            && selected != arm
                        {
                            return Err(FlowPreviewError::Invariant);
                        }
                        idx = entry_idx;
                        if let Some(lane) = self.flow_send_lane_at(idx) {
                            preview_route_arm = Some(FlowPreviewRouteArm {
                                lane,
                                scope: scope_id,
                                arm,
                            });
                        }
                    }
                }
            } else if at_decision {
                let lane_wire = lane_for_label_or_offer(scope_id, target_label);
                let selected_arm = inferred_arm_for_scope(scope_id).or_else(|| {
                    preview_route_arm.and_then(|preview| {
                        (preview.lane == lane_wire && preview.scope == scope_id)
                            .then_some(preview.arm)
                    })
                });
                if let Some(selected_arm) = selected_arm {
                    preview_route_arm = Some(FlowPreviewRouteArm {
                        lane: lane_wire,
                        scope: scope_id,
                        arm: selected_arm,
                    });
                    if let Some(entry_idx) =
                        self.passive_observer_arm_entry_index(scope_id, selected_arm)
                    {
                        idx = entry_idx;
                    }
                }
            }
        }

        let mut iter = 0usize;
        let bound = self
            .local_steps_len()
            .saturating_add(self.route_scope_count())
            .saturating_add(1);
        loop {
            iter = iter.saturating_add(1);
            if iter > bound {
                return Err(FlowPreviewError::Invariant);
            }

            idx = self.flow_follow_jumps_from(idx)?;

            let (preview_scope, preview_arm) = preview_route_arm
                .map(|preview| (preview.scope, Some(preview.arm)))
                .unwrap_or((ScopeId::none(), None));
            if !self.event_conflict_allows(idx, preview_scope, preview_arm, |scope| {
                self.flow_selected_arm_for_scope_with_route(
                    scope,
                    preview_route_arm,
                    &mut inferred_arm_for_scope,
                )
            }) {
                idx = state_index_to_usize(self.node_next_index_at(idx));
                continue;
            }

            if self.is_jump_at(idx)
                && self.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch)
                && let Some(next_idx) =
                    self.flow_follow_passive_observer_for_label(idx, target_label)
            {
                idx = next_idx;
                continue;
            }

            if !self.is_send_at(idx) && !self.is_local_action_at(idx) {
                if let Some(recv_meta) = self.try_recv_meta_at(idx)
                    && let Ok(progress_step) =
                        self.relocatable_resident_lane_step_at_index(idx, recv_meta.lane as usize)
                    && self.relocatable_step_done(progress_step)
                {
                    idx = state_index_to_usize(self.node_next_index_at(idx));
                    continue;
                }
                if let Some(region) = self.route_scope_region_at(idx)
                    && let Some(end) = self.flow_route_scope_end_if_complete(
                        region.scope(),
                        preview_route_arm,
                        &mut inferred_arm_for_scope,
                    )
                {
                    idx = end;
                    continue;
                }
                return Err(FlowPreviewError::Invariant);
            }

            let current_meta = if self.is_local_action_at(idx) {
                let local = self
                    .try_local_meta_at(idx)
                    .ok_or(FlowPreviewError::Invariant)?;
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
                self.try_send_meta_at(idx)
                    .ok_or(FlowPreviewError::Invariant)?
            };

            let Some(progress_step) = self
                .pending_event_progress_step(idx, current_meta.lane)
                .map_err(|_| FlowPreviewError::Invariant)?
            else {
                idx = state_index_to_usize(self.node_next_index_at(idx));
                continue;
            };
            if !self.event_lane_head_allows(progress_step, preview_scope, preview_arm, |scope| {
                self.flow_selected_arm_for_scope_with_route(
                    scope,
                    preview_route_arm,
                    &mut inferred_arm_for_scope,
                )
            }) {
                return Err(FlowPreviewError::Invariant);
            }

            if current_meta.label == target_label {
                if !self.event_dependency_allows(idx, |scope| {
                    self.flow_selected_arm_for_scope_with_route(
                        scope,
                        preview_route_arm,
                        &mut inferred_arm_for_scope,
                    )
                }) {
                    return Err(FlowPreviewError::Invariant);
                }
                return Ok((current_meta, StateIndex::from_usize(idx)));
            }

            if let Some(region) = self.route_scope_region_at(idx)
                && let Some(end) = self.flow_route_scope_end_if_complete(
                    region.scope(),
                    preview_route_arm,
                    &mut inferred_arm_for_scope,
                )
            {
                idx = end;
                continue;
            }

            return Err(FlowPreviewError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            });
        }
    }
}
