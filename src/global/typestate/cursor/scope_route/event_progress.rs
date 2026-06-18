use super::super::super::facts::LocalDependencyState;
use super::super::{
    CursorInvariantError, EnabledEventCommit, EventCursor, LocalDependency, PackedEventConflict,
    RelocatableResidentLaneStep, ScopeId, SendMeta, SendPreviewError, StateIndex,
    state_index_to_usize,
};
use crate::global::typestate::EventCommitMeta;

#[derive(Clone, Copy)]
struct SendPreviewRouteArm {
    lane: u8,
    scope: ScopeId,
    arm: u8,
}

impl EventCursor {
    #[inline]
    pub(crate) fn parallel_scope_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().parallel_root(scope_id)
    }

    #[inline(always)]
    pub(crate) fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        self.machine().dependency_for_index(current_idx)
    }

    #[inline(always)]
    fn dependency_state_for_index(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> LocalDependencyState {
        let Some(dependency) = self.dependency_for_index(idx) else {
            return LocalDependencyState::Satisfied;
        };
        if !Self::dependency_applies(dependency, &mut selected_arm_for_scope) {
            return LocalDependencyState::InactiveByConflict;
        }
        if self.dependency_row_live_events_done(dependency, selected_arm_for_scope) {
            LocalDependencyState::Satisfied
        } else {
            LocalDependencyState::Blocked
        }
    }

    pub(crate) fn event_row_matches_commit(&self, idx: usize, event: EventCommitMeta) -> bool {
        self.machine()
            .event_program()
            .event_row_at(idx)
            .is_some_and(|row| {
                row.matches_commit(
                    event.eff_index,
                    event.label,
                    event.origin,
                    event.scope,
                    event.route_arm,
                    event.lane,
                )
            })
    }

    fn validate_event_enabled_commit(
        &self,
        idx: usize,
        progress_step: RelocatableResidentLaneStep,
        cursor_after: StateIndex,
        event: EventCommitMeta,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Result<(), CursorInvariantError> {
        if self.node_next_index_at(idx) != cursor_after {
            return Err(CursorInvariantError::INVARIANT);
        }
        if !self
            .dependency_state_for_index(idx, &mut selected_arm_for_scope)
            .allows_event()
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        if !self.event_conflict_row_allows_with_preview(
            preview_conflict,
            preview_conflict,
            &mut selected_arm_for_scope,
        ) {
            return Err(CursorInvariantError::INVARIANT);
        }
        let resident_step =
            self.relocatable_resident_lane_step_at_index(idx, event.lane as usize)?;
        if resident_step != progress_step {
            return Err(CursorInvariantError::INVARIANT);
        }
        if self.relocatable_step_done(progress_step)
            && !self.roll_reentry_event_allows_index(idx, event.lane, &mut selected_arm_for_scope)
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        if !self.event_lane_head_allows(progress_step, preview_conflict, |scope| {
            selected_arm_for_scope(scope)
        }) {
            return Err(CursorInvariantError::INVARIANT);
        }
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn event_enabled(
        &self,
        idx: usize,
        event: EventCommitMeta,
        selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Result<EnabledEventCommit, CursorInvariantError> {
        if !self.event_row_matches_commit(idx, event) {
            return Err(CursorInvariantError::INVARIANT);
        }
        let progress_step =
            self.relocatable_resident_lane_step_at_index(idx, event.lane as usize)?;
        let cursor_after = self.node_next_index_at(idx);
        self.validate_event_enabled_commit(
            idx,
            progress_step,
            cursor_after,
            event,
            selected_arm_for_scope,
        )?;
        Ok(EnabledEventCommit::new(progress_step, cursor_after))
    }

    fn local_label_at(&self, idx: usize) -> Option<u8> {
        if let Some(meta) = self.try_recv_meta_at(idx) {
            return Some(meta.label);
        }
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some(meta.label);
        }
        if let Some(meta) = self.try_local_meta_at(idx) {
            return Some(meta.label);
        }
        None
    }

    fn route_arm_label_index(&self, scope_id: ScopeId, arm: u8, target_label: u8) -> Option<usize> {
        let slot = self.route_scope_slot(scope_id)?;
        let row = self
            .machine()
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)?;
        let mut idx = row.start();
        while idx < row.end() {
            if self.local_label_at(idx) == Some(target_label) {
                return Some(idx);
            }
            idx += 1;
        }
        None
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
        if self
            .send_preview_controller_scope_for_label_at(current, target_label)
            .is_some()
        {
            return current;
        }
        if let Some(region) = self.enclosing_route_scope_rows_at(current)
            && let Some(selected_arm) = selected_arm_for_scope(region.scope())
            && let Some(idx) = self.selected_route_label_index(
                region.scope(),
                target_label,
                selected_arm,
                &mut selected_arm_for_scope,
            )
        {
            return idx;
        }
        if let Some((lane_idx, _)) = self.pending_step_for_label(target_label)
            && let Some(idx) = self.index_for_lane_step(lane_idx)
        {
            return idx;
        }
        if let Some(idx) = self.roll_reentry_index_for_label(target_label, selected_arm_for_scope) {
            return idx;
        }
        current
    }

    pub(crate) fn recv_descriptor_index_for_label(
        &self,
        target_label: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut idx = self.recv_start_index_for_label(target_label, &mut selected_arm_for_scope);
        let mut iter_count = 0usize;
        let descriptor_bound = self.local_steps_len() + PackedEventConflict::MAX_CHAIN_DEPTH;
        while iter_count <= descriptor_bound {
            iter_count += 1;
            if let Some(region) = self.route_scope_rows_at(idx)
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
            let preview_conflict = self.machine().event_conflict_for_index(idx);
            if !self.event_conflict_row_allows_with_preview(
                preview_conflict,
                preview_conflict,
                &mut selected_arm_for_scope,
            ) {
                idx = state_index_to_usize(self.node_next_index_at(idx));
                continue;
            }
            if let Some(meta) = self.try_recv_meta_at(idx) {
                if let Some(arm) = meta.route_arm {
                    if let Some(selected) = selected_arm_for_scope(meta.scope) {
                        if selected != arm {
                            idx = state_index_to_usize(self.node_next_index_at(idx));
                            continue;
                        }
                    } else if meta.label != target_label {
                        idx = state_index_to_usize(self.node_next_index_at(idx));
                        continue;
                    }
                }
                return Some(idx);
            }
            if let Some(end) = self.selected_route_scope_end_at(idx, &mut selected_arm_for_scope)
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
    fn send_preview_is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: usize) -> bool {
        let mut arm = 0u8;
        while arm <= 1 {
            if self
                .controller_arm_entry_by_arm(scope_id, arm)
                .is_some_and(|(entry, _)| state_index_to_usize(entry) == idx)
                || self.route_arm_for_index(scope_id, idx) == Some(arm)
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

    fn send_preview_controller_scope_for_label_at(
        &self,
        idx: usize,
        target_label: u8,
    ) -> Option<ScopeId> {
        let mut selected = None;
        let mut selected_len = 0usize;
        let mut slot = 0usize;
        while let Some(region) = self.machine().route_scope_rows_by_slot(slot) {
            if idx >= region.start() && idx < region.end() {
                let scope = region.scope();
                let len = region.end() - region.start();
                if len >= selected_len
                    && self.is_route_controller(scope)
                    && self
                        .send_preview_controller_arm_entry_for_label(scope, target_label)
                        .is_some()
                {
                    selected = Some(scope);
                    selected_len = len;
                }
            }
            slot += 1;
        }
        selected
    }

    #[inline]
    fn send_preview_controller_arm_entry_for_label(
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
            if let Some(idx) = self.route_arm_label_index(scope_id, arm, target_label) {
                return Some((arm, idx));
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    #[inline]
    fn send_preview_lane_at(&self, idx: usize) -> Option<u8> {
        self.try_send_meta_at(idx)
            .map(|meta| meta.lane)
            .or_else(|| self.try_local_meta_at(idx).map(|meta| meta.lane))
    }

    #[inline]
    fn send_preview_selected_arm_for_scope_with_route<F>(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<SendPreviewRouteArm>,
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

    fn send_preview_route_scope_end_if_complete<F>(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<SendPreviewRouteArm>,
        arm_for_scope: &mut F,
    ) -> Option<usize>
    where
        F: FnMut(ScopeId) -> Option<u8>,
    {
        let arm = self.send_preview_selected_arm_for_scope_with_route(
            scope_id,
            preview_route_arm,
            arm_for_scope,
        )?;
        if !self.selected_route_arm_completes_scope(scope_id, arm, |scope| {
            self.send_preview_selected_arm_for_scope_with_route(
                scope,
                preview_route_arm,
                arm_for_scope,
            )
        }) {
            return None;
        }
        self.route_scope_end_by_id(scope_id)
    }

    #[inline]
    fn send_preview_pending_label_index(&self, target_label: u8) -> Option<usize> {
        let idx = self.index();
        if let Some(meta) = self.try_recv_meta_at(idx)
            && meta.label == target_label
            && self.index_for_lane_step(meta.lane as usize) == Some(idx)
        {
            return Some(idx);
        }
        if let Some(meta) = self.try_send_meta_at(idx)
            && meta.label == target_label
        {
            return Some(idx);
        }
        if let Some(meta) = self.try_local_meta_at(idx)
            && meta.label == target_label
        {
            return Some(idx);
        }
        None
    }

    #[inline]
    fn send_preview_start_index_for_label(
        &self,
        target_label: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> usize {
        if let Some(idx) =
            self.roll_reentry_index_for_label(target_label, &mut selected_arm_for_scope)
        {
            return idx;
        }
        if let Some(idx) = self.send_preview_pending_label_index(target_label) {
            return idx;
        }
        if let Some((lane_idx, _)) = self.pending_step_for_label(target_label)
            && let Some(idx) = self.index_for_lane_step(lane_idx)
        {
            return idx;
        }
        if self
            .send_preview_controller_scope_for_label_at(self.index(), target_label)
            .is_some()
        {
            return self.index();
        }
        if let Some(region) = self.enclosing_route_scope_rows_at(self.index())
            && let Some(selected_arm) = selected_arm_for_scope(region.scope())
            && let Some(idx) = self.selected_route_label_index(
                region.scope(),
                target_label,
                selected_arm,
                selected_arm_for_scope,
            )
        {
            return idx;
        }
        if self.enclosing_route_scope_rows_at(self.index()).is_some() {
            return self.index();
        }
        if let Some(idx) = self.first_pending_step_index(usize::MAX) {
            return idx;
        }
        self.index()
    }

    pub(crate) fn send_preview_meta_for_label<const ROLE: u8>(
        &self,
        target_label: u8,
        mut committed_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut lane_for_label_or_offer: impl FnMut(ScopeId, u8) -> u8,
    ) -> Result<(SendMeta, StateIndex), SendPreviewError> {
        let roll_reentry =
            self.roll_reentry_index_for_label(target_label, &mut committed_arm_for_scope);
        let mut idx = match roll_reentry {
            Some(idx) => idx,
            None => {
                self.send_preview_start_index_for_label(target_label, &mut committed_arm_for_scope)
            }
        };
        let mut preview_route_arm: Option<SendPreviewRouteArm> = None;

        if let Some(scope_id) = self
            .send_preview_controller_scope_for_label_at(idx, target_label)
            .or_else(|| {
                self.enclosing_route_scope_rows_at(idx)
                    .map(|region| region.scope())
            })
        {
            let at_route_start = self
                .route_scope_rows(scope_id)
                .is_some_and(|region| idx == region.start());
            let unlabeled =
                !self.is_send_at(idx) && !self.is_recv_at(idx) && !self.is_local_action_at(idx);
            let at_decision = at_route_start || unlabeled;

            if self.is_route_controller(scope_id) {
                let at_arm_entry = self.send_preview_is_at_controller_arm_entry(scope_id, idx);
                let at_decision = at_arm_entry || at_decision;
                if at_decision
                    && let Some((arm, entry_idx)) =
                        self.send_preview_controller_arm_entry_for_label(scope_id, target_label)
                {
                    if let Some(selected) = committed_arm_for_scope(scope_id)
                        && selected != arm
                        && !self.route_scope_reentry(scope_id)
                    {
                        return Err(SendPreviewError::Invariant);
                    }
                    idx = entry_idx;
                    if let Some(lane) = self.send_preview_lane_at(idx) {
                        preview_route_arm = Some(SendPreviewRouteArm {
                            lane,
                            scope: scope_id,
                            arm,
                        });
                    }
                }
                if at_decision
                    && preview_route_arm.is_none()
                    && let Some(arm) = self.route_arm_for_index(scope_id, idx)
                    && let Some(lane) = self.send_preview_lane_at(idx)
                {
                    preview_route_arm = Some(SendPreviewRouteArm {
                        lane,
                        scope: scope_id,
                        arm,
                    });
                }
            } else if at_decision {
                let lane_wire = lane_for_label_or_offer(scope_id, target_label);
                let selected_arm = selected_arm_for_scope(scope_id).or_else(|| {
                    preview_route_arm.and_then(|preview| {
                        (preview.lane == lane_wire && preview.scope == scope_id)
                            .then_some(preview.arm)
                    })
                });
                if let Some(selected_arm) = selected_arm {
                    preview_route_arm = Some(SendPreviewRouteArm {
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
        let bound = self.local_steps_len() + PackedEventConflict::MAX_CHAIN_DEPTH;
        loop {
            iter += 1;
            if iter > bound {
                return Err(SendPreviewError::Invariant);
            }

            let preview_conflict = self.machine().event_conflict_for_index(idx);
            if !self.event_conflict_row_allows_with_preview(
                preview_conflict,
                preview_conflict,
                |scope| {
                    self.send_preview_selected_arm_for_scope_with_route(
                        scope,
                        preview_route_arm,
                        &mut selected_arm_for_scope,
                    )
                },
            ) {
                idx = state_index_to_usize(self.node_next_index_at(idx));
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
                if let Some(region) = self.enclosing_route_scope_rows_at(idx)
                    && let Some(end) = self.send_preview_route_scope_end_if_complete(
                        region.scope(),
                        preview_route_arm,
                        &mut selected_arm_for_scope,
                    )
                {
                    idx = end;
                    continue;
                }
                return Err(SendPreviewError::Invariant);
            }

            let mut current_meta = if self.is_local_action_at(idx) {
                let local = self
                    .try_local_meta_at(idx)
                    .ok_or(SendPreviewError::Invariant)?;
                SendMeta {
                    eff_index: local.eff_index,
                    peer: ROLE,
                    label: local.label,
                    frame_label: local.frame_label,
                    semantic: local.semantic,
                    origin: local.origin,
                    next: local.next,
                    scope: local.scope,
                    route_scope: local.route_scope,
                    route_arm: local.route_arm,
                    selected_route_arm: local.route_arm,
                    lane: local.lane,
                }
            } else {
                self.try_send_meta_at(idx)
                    .ok_or(SendPreviewError::Invariant)?
            };
            if let Some(preview) = preview_route_arm {
                if current_meta.route_scope.is_none() {
                    current_meta.route_scope = preview.scope;
                }
                if current_meta.route_scope == preview.scope {
                    current_meta.selected_route_arm = Some(preview.arm);
                }
            }
            let Some(progress_step) = self
                .pending_event_progress_step(idx, current_meta.lane, |scope| {
                    self.send_preview_selected_arm_for_scope_with_route(
                        scope,
                        preview_route_arm,
                        &mut selected_arm_for_scope,
                    )
                })
                .map_err(|_| SendPreviewError::Invariant)?
            else {
                idx = state_index_to_usize(self.node_next_index_at(idx));
                continue;
            };
            if !self.event_lane_head_allows(progress_step, preview_conflict, |scope| {
                self.send_preview_selected_arm_for_scope_with_route(
                    scope,
                    preview_route_arm,
                    &mut selected_arm_for_scope,
                )
            }) {
                return Err(SendPreviewError::Invariant);
            }

            if current_meta.label == target_label {
                self.event_enabled(idx, EventCommitMeta::from(current_meta), |scope| {
                    self.send_preview_selected_arm_for_scope_with_route(
                        scope,
                        preview_route_arm,
                        &mut selected_arm_for_scope,
                    )
                })
                .map_err(|_| SendPreviewError::Invariant)?;
                return Ok((current_meta, StateIndex::from_usize(idx)));
            }

            if let Some(region) = self.enclosing_route_scope_rows_at(idx)
                && let Some(end) = self.send_preview_route_scope_end_if_complete(
                    region.scope(),
                    preview_route_arm,
                    &mut selected_arm_for_scope,
                )
            {
                idx = end;
                continue;
            }

            return Err(SendPreviewError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            });
        }
    }
}
