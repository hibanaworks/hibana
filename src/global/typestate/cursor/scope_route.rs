mod event_flow;
mod navigation;

use super::super::facts::{LocalConflict, PackedEventConflict};
use super::{
    ControlSemanticKind, EffIndex, EventCursor, FirstRecvDispatchSpec, JumpReason, LaneSetView,
    LocalAction, LocalDependency, LoopControlMeaning, LoopMetadata, LoopRole,
    MAX_FIRST_RECV_DISPATCH, PolicyMode, RecvMeta, RelocatableResidentLaneStep, ResidentLaneStep,
    ResidentLaneStepError, RouteOfferCursorState, ScopeId, StateIndex, as_state_index,
    state_index_to_usize,
};

impl EventCursor {
    #[inline(always)]
    pub(crate) fn pending_event_progress_step(
        &self,
        idx: usize,
        lane: u8,
    ) -> Result<Option<RelocatableResidentLaneStep>, ResidentLaneStepError> {
        let progress_step = self.relocatable_resident_lane_step_at_index(idx, lane as usize)?;
        if self.relocatable_step_done(progress_step) {
            Ok(None)
        } else {
            Ok(Some(progress_step))
        }
    }

    #[inline(always)]
    pub(crate) fn event_lane_head_allows(
        &self,
        progress_step: RelocatableResidentLaneStep,
        preview_scope: ScopeId,
        preview_arm: Option<u8>,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let target = progress_step.0;
        if self.relocatable_step_done(progress_step) {
            return false;
        }
        let mut step_idx = 0usize;
        while step_idx < target.step_idx as usize {
            if self.machine().event_program().local_step_lane(step_idx) == Some(target.lane) {
                let candidate = RelocatableResidentLaneStep(ResidentLaneStep {
                    step_idx: step_idx as u16,
                    lane: target.lane,
                });
                if !self.relocatable_step_done(candidate)
                    && let Some(pending_idx) = self
                        .machine()
                        .state_for_step_index(step_idx)
                        .map(state_index_to_usize)
                    && self.event_conflict_allows(
                        pending_idx,
                        preview_scope,
                        preview_arm,
                        |scope| selected_arm_for_scope(scope),
                    )
                {
                    return false;
                }
            }
            step_idx = step_idx.saturating_add(1);
        }
        true
    }

    #[inline(always)]
    pub(crate) fn selected_route_scope_end_at(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let region = self.route_scope_region_at(idx)?;
        let arm = selected_arm_for_scope(region.scope())?;
        if !self.route_arm_events_done(region.scope(), arm, |scope| selected_arm_for_scope(scope)) {
            return None;
        }
        Some(region.end())
    }

    pub(crate) fn route_arm_events_done(
        &self,
        scope_id: ScopeId,
        arm: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut idx = 0usize;
        let limit = self.local_steps_len();
        while idx < limit {
            if self.event_route_arm_for_scope(idx, scope_id) != Some(arm) {
                idx += 1;
                continue;
            }
            let Some(row) = self.machine().event_program().event_row_at(idx) else {
                idx += 1;
                continue;
            };
            if self.node_conflict_allows(idx, |scope| selected_arm_for_scope(scope)) {
                let Ok(progress_step) =
                    self.relocatable_resident_lane_step_at_index(idx, row.lane() as usize)
                else {
                    return false;
                };
                if !self.relocatable_step_done(progress_step) {
                    return false;
                }
            }
            idx += 1;
        }
        true
    }

    pub(crate) fn visit_decode_linger_route_rows(
        &self,
        meta_scope: ScopeId,
        branch_scope: ScopeId,
        mut visit: impl FnMut(ScopeId, Option<u8>) -> bool,
    ) -> bool {
        let mut linger_scope = meta_scope;
        let mut depth = 0usize;
        let depth_bound = self
            .local_steps_len()
            .saturating_add(PackedEventConflict::MAX_CHAIN_DEPTH);
        while depth < depth_bound {
            if linger_scope == branch_scope {
                return true;
            }
            if linger_scope != branch_scope && self.route_scope_linger(linger_scope) {
                let selected = self
                    .route_scope_conflict_arm_for_scope(branch_scope, linger_scope)
                    .or_else(|| self.route_scope_conflict_arm_for_scope(meta_scope, linger_scope));
                if !visit(linger_scope, selected) {
                    return false;
                }
            }
            let Some((parent, _arm)) = self.route_conflict_parent_arm(linger_scope) else {
                return true;
            };
            linger_scope = parent;
            depth += 1;
        }
        false
    }

    pub(crate) fn visit_passive_route_materialization_rows(
        &self,
        root_scope: ScopeId,
        initial_scope: ScopeId,
        selected_arm: u8,
        mut preview_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut visit: impl FnMut(ScopeId, u8) -> bool,
    ) -> Option<usize> {
        if initial_scope == root_scope || self.route_scope_region_by_id(initial_scope).is_none() {
            return None;
        }
        if !visit(root_scope, selected_arm) {
            return None;
        }
        let mut target_scope = initial_scope;
        let mut depth = 0usize;
        let depth_bound = self
            .local_steps_len()
            .saturating_add(PackedEventConflict::MAX_CHAIN_DEPTH);
        while depth < depth_bound {
            if let Some(arm) = preview_arm_for_scope(target_scope) {
                if !visit(target_scope, arm) {
                    return None;
                }
                if let Some(child_scope) = self.passive_arm_scope_by_arm(target_scope, arm)
                    && child_scope != target_scope
                    && self.route_scope_region_by_id(child_scope).is_some()
                {
                    target_scope = child_scope;
                    depth += 1;
                    continue;
                }
            }
            return self.route_scope_materialization_index(target_scope);
        }
        None
    }

    pub(crate) fn route_scope_materialization_index(&self, scope_id: ScopeId) -> Option<usize> {
        if let Some(offer_entry) = self.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
        {
            return Some(state_index_to_usize(offer_entry));
        }
        self.route_scope_region_by_id(scope_id)
            .map(|region| region.start())
    }

    pub(crate) fn decode_linger_cursor_step(
        &self,
        meta: RecvMeta,
        next_index: StateIndex,
        mut authorized_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<RelocatableResidentLaneStep> {
        let mut linger_scope = meta.scope;
        loop {
            if self.route_scope_linger(linger_scope)
                && let Some(arm) = authorized_arm_for_scope(linger_scope)
                && arm == 0
                && let Some(last_eff) = self.route_arm_lane_last_eff(linger_scope, arm, meta.lane)
                && last_eff == meta.eff_index
                && let Some(first_step) =
                    self.route_arm_lane_first_step(linger_scope, arm, meta.lane)
            {
                return Some(first_step);
            }
            let Some((parent, _arm)) = self.route_conflict_parent_arm(linger_scope) else {
                break;
            };
            linger_scope = parent;
        }

        let next_usize = state_index_to_usize(next_index);
        if let Some(region) = self.route_scope_region_at(next_usize)
            && region.linger()
        {
            let at_scope_start = next_usize == region.start();
            let at_passive_branch = self.jump_reason_at(next_usize)
                == Some(JumpReason::PassiveObserverBranch)
                && self.node_scope_matches(next_usize, region.scope());
            if (at_scope_start || at_passive_branch)
                && let Some(arm) = authorized_arm_for_scope(region.scope())
                && arm == 0
                && let Some(first_step) =
                    self.route_arm_lane_first_step(region.scope(), arm, meta.lane)
            {
                return Some(first_step);
            }
        }
        None
    }

    fn dependency_events_done(
        &self,
        dependency: LocalDependency,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut idx = dependency.start();
        let end = dependency.end().min(self.local_steps_len());
        while idx < end {
            let Some(row) = self.machine().event_program().event_row_at(idx) else {
                idx += 1;
                continue;
            };
            if self.node_conflict_allows(idx, |scope| selected_arm_for_scope(scope)) {
                let Ok(progress_step) =
                    self.relocatable_resident_lane_step_at_index(idx, row.lane() as usize)
                else {
                    return false;
                };
                if !self.relocatable_step_done(progress_step) {
                    return false;
                }
            }
            idx += 1;
        }
        true
    }

    fn dependency_applies(
        dependency: LocalDependency,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        match dependency.conflict() {
            LocalConflict::Unconditional => true,
            LocalConflict::SharedRoute => false,
            LocalConflict::RouteArm { scope, arm } => selected_arm_for_scope(scope) == Some(arm),
        }
    }

    #[inline]
    pub(crate) fn enclosing_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().enclosing_loop(scope_id)
    }

    #[inline]
    pub(crate) fn node_loop_scope(&self, index: usize) -> Option<ScopeId> {
        let scope = self.typestate_node(index).scope();
        if scope.is_none() {
            None
        } else {
            self.enclosing_loop_scope(scope)
        }
    }

    // =========================================================================
    // Label Seeking
    // =========================================================================

    /// Find cursor at node with given label.
    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn seek_label_index(&self, label: u8) -> Option<usize> {
        for i in 0..self.machine().node_len() {
            let node = self.machine().node(i);
            let node_label = match node.action() {
                LocalAction::Send { label: l, .. }
                | LocalAction::Recv { label: l, .. }
                | LocalAction::Local { label: l, .. } => Some(l),
                LocalAction::Terminate => None,
            };
            if node_label == Some(label) {
                return Some(i);
            }
        }
        None
    }

    fn try_index_for_loop_control(&self, meaning: LoopControlMeaning) -> Option<usize> {
        for i in 0..self.machine().node_len() {
            let node = self.machine().node(i);
            let semantic = match node.action() {
                LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Local { .. } => {
                    node.control_semantic()
                }
                LocalAction::Terminate => continue,
            };
            if LoopControlMeaning::from_semantic(semantic) == Some(meaning) {
                return Some(i);
            }
        }
        None
    }

    fn successor_index_for_loop_control(&self, meaning: LoopControlMeaning) -> usize {
        let index = self
            .try_index_for_loop_control(meaning)
            .expect("loop control not found in typestate");
        state_index_to_usize(self.machine().node(index).next())
    }

    pub(super) fn passive_arm_jump(&self, _scope_id: ScopeId, _arm: u8) -> Option<StateIndex> {
        None
    }

    pub(super) fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.machine().passive_arm_entry(scope_id, arm)
    }

    fn route_recv_state(&self, scope_id: ScopeId, target_arm: u8) -> Option<StateIndex> {
        self.machine().route_recv_state(scope_id, target_arm)
    }

    fn route_arm_count_inner(&self, scope_id: ScopeId) -> Option<u8> {
        self.scope_region_by_id(scope_id).map(|_| 2)
    }

    fn route_scope_offer_lane_set_inner(&self, scope_id: ScopeId) -> Option<LaneSetView<'static>> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine()
            .event_program()
            .route_scope_offer_lane_set_by_slot(slot)
    }

    fn route_scope_arm_lane_set_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine()
            .event_program()
            .route_scope_arm_lane_set_by_slot(slot, arm)
    }

    fn route_scope_offer_entry_inner(&self, scope_id: ScopeId) -> Option<StateIndex> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine().route_scope_offer_entry_by_slot(slot)
    }

    fn route_scope_slot_inner(&self, scope_id: ScopeId) -> Option<usize> {
        self.machine().route_scope_dense_ordinal(scope_id)
    }

    pub(super) fn first_recv_dispatch_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.machine()
            .first_recv_dispatch_target_for_lane_frame_label(scope_id, lane, frame_label)
    }

    #[cfg(test)]
    fn first_recv_dispatch_entry_inner(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<FirstRecvDispatchSpec> {
        let len = self.first_recv_dispatch_table_inner(scope_id)?.1 as usize;
        if idx >= len {
            return None;
        }
        let table = self.first_recv_dispatch_table_inner(scope_id)?.0;
        Some(table[idx])
    }

    fn first_recv_dispatch_table_inner(
        &self,
        scope_id: ScopeId,
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        self.machine().first_recv_dispatch_table(scope_id)
    }

    fn route_arm_lane_first_step_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<RelocatableResidentLaneStep> {
        let mut idx = 0usize;
        let limit = self.local_steps_len();
        while idx < limit {
            if self.event_route_arm_for_scope(idx, scope_id) != Some(arm) {
                idx += 1;
                continue;
            }
            match self.machine().node(idx).action() {
                LocalAction::Send { lane: l, .. }
                | LocalAction::Recv { lane: l, .. }
                | LocalAction::Local { lane: l, .. }
                    if l == lane =>
                {
                    return self
                        .relocatable_resident_lane_step_at_index(idx, lane as usize)
                        .ok();
                }
                _ => {}
            }
            idx += 1;
        }
        None
    }

    fn route_arm_lane_last_eff_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        let mut found = None;
        let mut idx = 0usize;
        let limit = self.local_steps_len();
        while idx < limit {
            let node = self.machine().node(idx);
            if self.event_route_arm_for_scope(idx, scope_id) == Some(arm) {
                match node.action() {
                    LocalAction::Send {
                        eff_index, lane: l, ..
                    }
                    | LocalAction::Recv {
                        eff_index, lane: l, ..
                    }
                    | LocalAction::Local {
                        eff_index, lane: l, ..
                    } if l == lane => found = Some(eff_index),
                    _ => {}
                }
            }
            idx += 1;
        }
        found
    }

    fn controller_arm_entry_for_label_inner(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        self.machine()
            .controller_arm_entry_for_label(scope_id, label)
    }

    fn controller_arm_entry_by_arm_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.machine().controller_arm_entry_by_arm(scope_id, arm)
    }

    fn passive_arm_scope_inner(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        self.route_scope_for_selected_child_arm(scope_id, arm)
    }

    #[inline(always)]
    pub(crate) fn route_scope_conflict_arm_for_scope(
        &self,
        mut child_scope: ScopeId,
        target_scope: ScopeId,
    ) -> Option<u8> {
        if child_scope.is_none() || target_scope.is_none() {
            return None;
        }
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            let (scope, arm) = self.route_conflict_parent_arm(child_scope)?;
            if scope == target_scope {
                return Some(arm);
            }
            child_scope = scope;
            depth += 1;
        }
        None
    }

    // =========================================================================
    // Route Scope Methods
    // =========================================================================

    /// Get recv node index for a route arm.
    pub(crate) fn route_scope_arm_recv_index(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> Option<usize> {
        self.route_recv_state(scope_id, target_arm)
            .map(state_index_to_usize)
    }

    /// Get arm count for a route scope.
    pub(crate) fn route_scope_arm_count(&self, scope_id: ScopeId) -> Option<u8> {
        self.route_arm_count_inner(scope_id)
    }

    /// Get the compiled offer-lane mask for a route scope.
    pub(crate) fn route_scope_offer_lane_set(
        &self,
        scope_id: ScopeId,
    ) -> Option<LaneSetView<'static>> {
        self.route_scope_offer_lane_set_inner(scope_id)
    }

    /// Get the compiled lane mask for one arm of a route scope.
    pub(crate) fn route_scope_arm_lane_set(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.route_scope_arm_lane_set_inner(scope_id, arm)
    }

    /// Get offer entry index for a route scope.
    /// u16::MAX indicates the entry check is disabled (e.g., linger routes).
    pub(crate) fn route_scope_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        self.route_scope_offer_entry_inner(scope_id)
    }

    #[inline]
    pub(crate) fn has_route_scope(&self, scope_id: ScopeId) -> bool {
        self.route_scope_region_by_id(scope_id).is_some()
    }

    #[inline]
    pub(crate) fn route_scope_for_offer_node(&self, node_scope: ScopeId) -> Option<ScopeId> {
        self.route_scope_region_by_id(node_scope)
            .map(|region| region.scope())
    }

    #[inline]
    pub(crate) fn route_offer_entry_allows_current(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        selected_arm: Option<u8>,
    ) -> bool {
        if let Some(offer_entry) = self.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
            && current_idx != state_index_to_usize(offer_entry)
        {
            return selected_arm.is_some() && self.current_route_arm() == selected_arm;
        }
        true
    }

    #[inline]
    pub(crate) fn route_offer_entry_matches_current(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
    ) -> Option<bool> {
        let offer_entry = self.route_scope_offer_entry(scope_id)?;
        Some(offer_entry.is_max() || current_idx == state_index_to_usize(offer_entry))
    }

    #[inline]
    pub(crate) fn route_scope_present_for_entry(
        &self,
        entry_idx: usize,
        entry_scope: Option<ScopeId>,
    ) -> bool {
        if let Some(scope_id) = entry_scope
            && self.has_route_scope(scope_id)
        {
            return true;
        }
        self.route_scope_region_at(entry_idx).is_some()
    }

    pub(crate) fn current_route_arm_authorization(
        &self,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut preview_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Result<Option<bool>, ResidentLaneStepError> {
        let Some(region) = self.route_scope_region_at(self.index()) else {
            return Ok(None);
        };
        let scope = region.scope();
        let Some(current_arm) = self.node_route_arm_at(self.index()) else {
            return Ok(None);
        };
        if self.index() == region.start() && self.is_route_controller(scope) {
            return Ok(None);
        }
        if let Some(selected_arm) = selected_arm_for_scope(scope) {
            return Ok((selected_arm == current_arm).then_some(false));
        }
        if let Some(preview_arm) = preview_arm_for_scope(scope) {
            return Ok((preview_arm == current_arm).then_some(false));
        }
        if !self.is_route_controller(scope) {
            return Ok(None);
        }
        Err(ResidentLaneStepError)
    }

    pub(crate) fn normalize_lane_offer_entry(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
        selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        linger_offer: impl FnMut() -> Option<(ScopeId, StateIndex)>,
    ) -> Option<RouteOfferCursorState> {
        self.normalize_lane_offer_entry_inner(
            scope_id,
            entry_idx,
            selected_arm_for_scope,
            linger_offer,
        )
    }

    fn normalize_lane_offer_entry_inner(
        &self,
        mut scope_id: ScopeId,
        mut entry_idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut linger_offer: impl FnMut() -> Option<(ScopeId, StateIndex)>,
    ) -> Option<RouteOfferCursorState> {
        let mut region = self.route_scope_region_by_id(scope_id)?;
        let mut entry = self
            .route_scope_offer_entry(region.scope())
            .unwrap_or(StateIndex::MAX);
        if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
            let canonical_entry = state_index_to_usize(entry);
            if canonical_entry >= region.start() && canonical_entry < region.end() {
                let selected_arm = selected_arm_for_scope(scope_id);
                if region.linger() || selected_arm.is_none() {
                    entry_idx = canonical_entry;
                } else {
                    let (linger_scope, linger_entry) = linger_offer()?;
                    scope_id = linger_scope;
                    entry_idx = state_index_to_usize(linger_entry);
                    region = self.route_scope_region_by_id(scope_id)?;
                    entry = self
                        .route_scope_offer_entry(region.scope())
                        .unwrap_or(StateIndex::MAX);
                    if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
                        return None;
                    }
                }
            } else {
                let (linger_scope, linger_entry) = linger_offer()?;
                scope_id = linger_scope;
                entry_idx = state_index_to_usize(linger_entry);
                region = self.route_scope_region_by_id(scope_id)?;
                entry = self
                    .route_scope_offer_entry(region.scope())
                    .unwrap_or(StateIndex::MAX);
                if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
                    return None;
                }
            }
        }
        let entry_idx = if entry.is_max() {
            entry_idx
        } else {
            state_index_to_usize(entry)
        };
        self.contains_node_index(entry_idx)
            .then_some(RouteOfferCursorState::new(region.scope(), entry_idx))
    }

    pub(crate) fn active_linger_offer_entry(
        &self,
        scope_id: ScopeId,
    ) -> Option<(ScopeId, StateIndex)> {
        let region = self.route_scope_region_by_id(scope_id)?;
        Some((scope_id, StateIndex::from_usize(region.start())))
    }

    #[inline]
    pub(crate) fn passive_descendant_child_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<ScopeId> {
        self.passive_arm_scope_by_arm(scope_id, arm)
    }

    #[inline]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_slot_inner(scope_id)
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<FirstRecvDispatchSpec> {
        self.first_recv_dispatch_entry_inner(scope_id, idx)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        self.first_recv_dispatch_table_inner(scope_id)
    }

    pub(crate) fn route_arm_lane_first_step(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<RelocatableResidentLaneStep> {
        self.route_arm_lane_first_step_inner(scope_id, arm, lane)
    }

    pub(crate) fn route_arm_lane_last_eff(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        self.route_arm_lane_last_eff_inner(scope_id, arm, lane)
    }

    /// Get the controller arm entry index for a given label.
    /// Returns the StateIndex of the arm whose label matches, used by flow() for O(1) lookup.
    pub(crate) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        if !self.is_route_controller(scope_id) {
            return None;
        }
        self.controller_arm_entry_for_label_inner(scope_id, label)
    }

    /// Get the controller arm entry (index, label) for a given arm number.
    /// Used by offer() to navigate to the selected arm's entry point.
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        if !self.is_route_controller(scope_id) {
            return None;
        }
        self.controller_arm_entry_by_arm_inner(scope_id, arm)
    }

    #[inline]
    pub(crate) fn shared_controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.controller_arm_entry_by_arm_inner(scope_id, arm)
    }

    #[inline]
    pub(crate) fn control_semantic_at(&self, idx: usize) -> ControlSemanticKind {
        self.machine().node(idx).control_semantic()
    }

    #[inline]
    pub(crate) fn passive_arm_scope_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        self.passive_arm_scope_inner(scope_id, arm)
    }

    /// Get route controller policy metadata.
    ///
    /// The tuple `(PolicyMode, EffIndex, u8, ControlOp)` corresponds to the
    /// controller-provided
    /// policy mode, the effect index of the send action that declared it, and the
    /// control descriptor metadata embedded in the DSL. Route policies are tracked
    /// for both generic route decisions and loop-based routing.
    pub(crate) fn route_scope_controller_policy(
        &self,
        scope_id: ScopeId,
    ) -> Option<(
        PolicyMode,
        EffIndex,
        u8,
        crate::control::cap::mint::ControlOp,
    )> {
        self.machine().route_controller(scope_id)
    }

    // =========================================================================
    // Loop Metadata
    // =========================================================================

    /// Get loop metadata for current scope.
    pub(crate) fn loop_metadata_inner(&self) -> Option<LoopMetadata> {
        let node = self.machine().node(self.idx_usize());
        let action = node.action();
        let role = self.machine().role();
        let (eff_index, controller, target, role_kind) = match action {
            LocalAction::Send {
                eff_index, peer, ..
            } => (eff_index, role, peer, LoopRole::Controller),
            LocalAction::Recv {
                eff_index, peer, ..
            } => (eff_index, peer, role, LoopRole::Target),
            _ => return None,
        };
        if LoopControlMeaning::from_semantic(node.control_semantic())
            != Some(LoopControlMeaning::Continue)
        {
            return None;
        }
        let scope = self.node_loop_scope(self.idx_usize())?;
        let continue_index = self.successor_index_for_loop_control(LoopControlMeaning::Continue);
        let break_index = self.successor_index_for_loop_control(LoopControlMeaning::Break);
        Some(LoopMetadata {
            scope,
            controller,
            target,
            role: role_kind,
            eff_index,
            decision_index: as_state_index(self.idx_usize()),
            continue_index: as_state_index(continue_index),
            break_index: as_state_index(break_index),
        })
    }
}
