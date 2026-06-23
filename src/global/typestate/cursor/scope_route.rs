mod event_progress;
mod navigation;
mod roll;
mod row_completion;
mod send_preview;
mod send_preview_route;
mod send_preview_start;

use super::super::facts::{LocalConflict, PackedEventConflict, PassiveArmChildFact};
use super::{
    CursorInvariantError, EffIndex, EventCursor, EventSemanticKind, LaneSetView, LocalAction,
    LocalDependency, RecvMeta, RelocatableResidentLaneStep, ResidentLaneStep,
    RouteOfferCursorState, RouteResolver, ScopeId, SendMeta, StateIndex, state_index_to_usize,
};
use crate::global::role_program::PackedLaneRange;

pub(crate) enum CurrentRouteArmAuthorization {
    NotAuthorized,
    Authorized,
}

impl CurrentRouteArmAuthorization {
    #[inline]
    pub(crate) const fn authorizes_current_arm(self) -> bool {
        matches!(self, Self::Authorized)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RouteOfferEntryCursorPosition {
    AtEntry,
    AfterEntry,
}

impl RouteOfferEntryCursorPosition {
    #[inline]
    pub(crate) const fn is_at_entry(self) -> bool {
        matches!(self, Self::AtEntry)
    }
}

impl EventCursor {
    #[inline]
    pub(crate) fn event_lane_head_allows(
        &self,
        progress_step: RelocatableResidentLaneStep,
        preview_conflict: PackedEventConflict,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let target = progress_step.0;
        if self.relocatable_step_done(progress_step) {
            if !self.has_reentry_scopes() {
                return false;
            }
            let Some(idx) = self.node_index_for_relocatable_step(progress_step) else {
                return false;
            };
            if !self.roll_reentry_event_allows_index(idx, target.lane, selected_arm_for_scope) {
                return false;
            }
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
                    && self.event_conflict_row_allows_with_preview(
                        self.machine().event_conflict_for_index(pending_idx),
                        preview_conflict,
                        selected_arm_for_scope,
                    )
                {
                    return false;
                }
            }
            step_idx += 1;
        }
        true
    }

    #[inline(never)]
    pub(crate) fn selected_enclosing_route_scope_end_at(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut selected = None;
        let mut selected_len = usize::MAX;
        let mut slot = 0usize;
        while let Some(region) = self.machine().route_scope_rows_by_slot(slot) {
            if idx >= region.start() && idx < region.end() {
                let scope = region.scope();
                if let Some(arm) = selected_arm_for_scope(scope)
                    && self.selected_route_arm_completes_scope(
                        scope,
                        arm,
                        &mut selected_arm_for_scope,
                    )
                {
                    let len = region.end() - region.start();
                    if len < selected_len {
                        selected = Some(region.end());
                        selected_len = len;
                    }
                }
            }
            slot += 1;
        }
        selected
    }

    pub(crate) fn visit_branch_recv_reentry_route_rows(
        &self,
        meta_scope: ScopeId,
        branch_scope: ScopeId,
        mut visit: impl FnMut(ScopeId, Option<u8>) -> bool,
    ) -> bool {
        let mut reentry_scope = meta_scope;
        let mut depth = 0usize;
        let depth_bound = self.local_steps_len() + PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            if reentry_scope == branch_scope {
                return true;
            }
            if reentry_scope != branch_scope && self.route_scope_reentry(reentry_scope) {
                let selected =
                    match self.route_scope_conflict_arm_for_scope(branch_scope, reentry_scope) {
                        Some(arm) => Some(arm),
                        None => self.route_scope_conflict_arm_for_scope(meta_scope, reentry_scope),
                    };
                if !visit(reentry_scope, selected) {
                    return false;
                }
            }
            let Some((parent, _arm)) = self.route_conflict_parent_arm(reentry_scope) else {
                return true;
            };
            reentry_scope = parent;
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
        if initial_scope == root_scope || self.route_scope_rows(initial_scope).is_none() {
            return None;
        }
        if !visit(root_scope, selected_arm) {
            return None;
        }
        let mut target_scope = initial_scope;
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            if let Some(arm) = preview_arm_for_scope(target_scope) {
                if !visit(target_scope, arm) {
                    return None;
                }
                if let Some(child_scope) = self.passive_child_scope(target_scope, arm)
                    && self.route_scope_rows(child_scope).is_some()
                {
                    target_scope = child_scope;
                    depth += 1;
                    continue;
                }
                return self
                    .route_scope_arm_recv_index(target_scope, arm)
                    .or_else(|| self.passive_observer_arm_entry_index(target_scope, arm));
            }
            return self.route_scope_materialization_index(target_scope);
        }
        crate::invariant();
    }

    pub(crate) fn route_scope_materialization_index(&self, scope_id: ScopeId) -> Option<usize> {
        if let Some(offer_entry) = self.route_scope_offer_entry(scope_id)
            && !offer_entry.is_absent()
        {
            return Some(state_index_to_usize(offer_entry));
        }
        self.route_scope_rows(scope_id).map(|region| region.start())
    }

    fn event_reentry_cursor_step(
        &self,
        scope: ScopeId,
        lane: u8,
        eff_index: EffIndex,
        next_index: StateIndex,
        mut authorized_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<RelocatableResidentLaneStep> {
        let mut reentry_scope = scope;
        loop {
            if self.route_scope_reentry(reentry_scope)
                && let Some(arm) = authorized_arm_for_scope(reentry_scope)
                && let Some(last_eff) = self.route_arm_lane_last_eff(reentry_scope, arm, lane)
                && last_eff == eff_index
                && let Some(first_step) = self.route_arm_lane_first_step(reentry_scope, arm, lane)
            {
                return Some(first_step);
            }
            let Some((parent, _arm)) = self.route_conflict_parent_arm(reentry_scope) else {
                break;
            };
            reentry_scope = parent;
        }

        let next_usize = state_index_to_usize(next_index);
        if let Some(region) = self.route_scope_rows_at(next_usize)
            && region.reentry()
            && next_usize == region.start()
            && let Some(arm) = authorized_arm_for_scope(region.scope())
            && let Some(first_step) = self.route_arm_lane_first_step(region.scope(), arm, lane)
        {
            return Some(first_step);
        }
        None
    }

    pub(crate) fn recv_reentry_cursor_step(
        &self,
        meta: RecvMeta,
        next_index: StateIndex,
        authorized_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<RelocatableResidentLaneStep> {
        self.event_reentry_cursor_step(
            meta.scope,
            meta.lane,
            meta.eff_index,
            next_index,
            authorized_arm_for_scope,
        )
    }

    pub(crate) fn send_reentry_cursor_step(
        &self,
        meta: SendMeta,
        next_index: StateIndex,
        authorized_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<RelocatableResidentLaneStep> {
        let scope = if meta.route_scope.is_none() {
            meta.scope
        } else {
            meta.route_scope
        };
        self.event_reentry_cursor_step(
            scope,
            meta.lane,
            meta.eff_index,
            next_index,
            authorized_arm_for_scope,
        )
    }

    #[inline]
    pub(crate) fn node_event_done_for_lane(&self, idx: usize, lane: u8) -> bool {
        let Ok(step) = self.relocatable_resident_lane_step_at_index(idx, lane as usize) else {
            crate::invariant();
        };
        self.relocatable_step_done(step)
    }

    #[inline]
    pub(crate) fn clear_node_event_done_for_lane(&mut self, idx: usize, lane: u8) {
        let Ok(step) = self.relocatable_resident_lane_step_at_index(idx, lane as usize) else {
            crate::invariant();
        };
        self.clear_local_event_done(step.0.step_idx as usize);
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
    pub(crate) fn enclosing_roll_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().enclosing_roll(scope_id)
    }

    #[inline]
    pub(crate) fn node_roll_scope(&self, index: usize) -> Option<ScopeId> {
        let scope = self.typestate_node(index).scope();
        if scope.is_none() {
            None
        } else {
            self.enclosing_roll_scope(scope)
        }
    }

    pub(super) fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.machine().passive_arm_entry(scope_id, arm)
    }

    fn route_recv_state(&self, scope_id: ScopeId, target_arm: u8) -> Option<StateIndex> {
        self.machine().route_recv_state(scope_id, target_arm)
    }

    fn route_arm_count_inner(&self, scope_id: ScopeId) -> Option<u8> {
        self.route_scope_rows(scope_id).map(|_| 2)
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
        Some(self.machine().route_scope_offer_entry_by_slot(slot))
    }

    fn route_scope_slot_inner(&self, scope_id: ScopeId) -> Option<usize> {
        self.machine().route_scope_dense_ordinal(scope_id)
    }

    fn visit_first_recv_dispatch_inner(
        &self,
        scope_id: ScopeId,
        visitor: impl FnMut(u8, StateIndex),
    ) -> Option<()> {
        self.machine().visit_first_recv_dispatch(scope_id, visitor)
    }

    fn first_recv_dispatch_arm_mask_inner(&self, scope_id: ScopeId) -> Option<u8> {
        self.machine().first_recv_dispatch_arm_mask(scope_id)
    }

    fn route_arm_lane_first_step_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<RelocatableResidentLaneStep> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        let step = self
            .machine()
            .event_program()
            .route_arm_lane_first_step_by_slot(slot, arm, lane)? as usize;
        match self.relocatable_resident_lane_step_at_index(step, lane as usize) {
            Ok(step) => Some(step),
            Err(CursorInvariantError::INVARIANT) => crate::invariant(),
        }
    }

    fn route_arm_lane_last_eff_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        let step = self
            .machine()
            .event_program()
            .route_arm_lane_last_step_by_slot(slot, arm, lane)? as usize;
        match self.machine().node(step).action() {
            LocalAction::Send { eff_index, .. }
            | LocalAction::Recv { eff_index, .. }
            | LocalAction::Local { eff_index, .. } => Some(eff_index),
            LocalAction::Terminate => None,
        }
    }

    fn controller_arm_entry_by_arm_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.machine().controller_arm_entry_by_arm(scope_id, arm)
    }

    fn passive_child_scope_inner(&self, route_scope: ScopeId, arm: u8) -> Option<ScopeId> {
        let slot = self.route_scope_slot_inner(route_scope)?;
        let row: PassiveArmChildFact = self.machine().passive_arm_child_fact_by_slot(slot, arm)?;
        if row.route_scope() != route_scope || row.arm() != arm {
            crate::invariant();
        }
        let child_scope = row.child_route_scope()?;
        (child_scope != route_scope).then_some(child_scope)
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
    /// StateIndex::ABSENT means this route authorizes the current cursor entry.
    pub(crate) fn route_scope_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        self.route_scope_offer_entry_inner(scope_id)
    }

    #[inline]
    pub(crate) fn has_route_scope(&self, scope_id: ScopeId) -> bool {
        self.route_scope_rows(scope_id).is_some()
    }

    #[inline]
    pub(crate) fn route_scope_for_offer_node(
        &self,
        node_scope: ScopeId,
        current_idx: usize,
    ) -> Option<ScopeId> {
        if let Some(region) = self.route_scope_rows(node_scope) {
            return Some(region.scope());
        }
        self.node_route_arm_at(current_idx)?;
        self.enclosing_route_scope_rows_at(current_idx)
            .map(|region| region.scope())
    }

    #[inline]
    fn route_arm_entry_matches_current(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        arm: u8,
    ) -> bool {
        self.passive_observer_arm_entry_index(scope_id, arm) == Some(current_idx)
            || self
                .shared_controller_arm_entry_by_arm(scope_id, arm)
                .is_some_and(|(entry, _)| state_index_to_usize(entry) == current_idx)
    }

    #[inline]
    pub(crate) fn route_offer_entry_allows_current(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        selected_arm: Option<u8>,
    ) -> bool {
        let Some(position) =
            self.route_offer_entry_cursor_position(scope_id, current_idx, selected_arm)
        else {
            return false;
        };
        if position.is_at_entry() {
            return true;
        }
        selected_arm
            .or_else(|| self.route_arm_for_index(scope_id, current_idx))
            .is_some_and(|arm| {
                self.route_current_index_allows_selected_arm(scope_id, current_idx, arm)
            })
    }

    #[inline]
    pub(crate) fn route_offer_entry_cursor_position(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        selected_arm: Option<u8>,
    ) -> Option<RouteOfferEntryCursorPosition> {
        let offer_entry = self.route_scope_offer_entry(scope_id)?;
        if offer_entry.is_absent() || current_idx == state_index_to_usize(offer_entry) {
            return Some(RouteOfferEntryCursorPosition::AtEntry);
        }
        if let Some(arm) = selected_arm.or_else(|| self.route_arm_for_index(scope_id, current_idx))
            && self.route_arm_entry_matches_current(scope_id, current_idx, arm)
        {
            return Some(RouteOfferEntryCursorPosition::AtEntry);
        }
        Some(RouteOfferEntryCursorPosition::AfterEntry)
    }

    #[inline]
    fn route_current_index_allows_selected_arm(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        selected_arm: u8,
    ) -> bool {
        let Some(region) = self.route_scope_rows(scope_id) else {
            return false;
        };
        if current_idx < region.start() || current_idx >= region.end() {
            return false;
        }
        let conflict = self.machine().event_conflict_for_index(current_idx);
        let mut selected_arm_for_scope = |_scope| None;
        self.event_conflict_row_allows(
            conflict,
            scope_id,
            Some(selected_arm),
            &mut selected_arm_for_scope,
        )
    }

    #[inline]
    fn route_offer_entry_for_index(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        selected_arm: Option<u8>,
        canonical_entry: StateIndex,
    ) -> usize {
        if canonical_entry.is_absent()
            || self
                .route_offer_entry_cursor_position(scope_id, current_idx, selected_arm)
                .is_some_and(RouteOfferEntryCursorPosition::is_at_entry)
        {
            current_idx
        } else {
            state_index_to_usize(canonical_entry)
        }
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
        self.checked_route_scope_rows_at(entry_idx).is_some()
    }

    pub(crate) fn current_route_arm_authorization(
        &self,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut preview_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Result<CurrentRouteArmAuthorization, CursorInvariantError> {
        let Some(region) = self.route_scope_rows_at(self.index()) else {
            return Ok(CurrentRouteArmAuthorization::NotAuthorized);
        };
        let scope = region.scope();
        let Some(current_arm) = self.node_route_arm_at(self.index()) else {
            return Ok(CurrentRouteArmAuthorization::NotAuthorized);
        };
        if self.index() == region.start() && self.is_route_controller(scope) {
            return Ok(CurrentRouteArmAuthorization::NotAuthorized);
        }
        if let Some(selected_arm) = selected_arm_for_scope(scope) {
            if selected_arm != current_arm && self.route_scope_reentry(scope) {
                return Ok(CurrentRouteArmAuthorization::Authorized);
            }
            if selected_arm == current_arm {
                return Ok(CurrentRouteArmAuthorization::Authorized);
            }
            return Ok(CurrentRouteArmAuthorization::NotAuthorized);
        }
        if let Some(preview_arm) = preview_arm_for_scope(scope) {
            if preview_arm == current_arm {
                return Ok(CurrentRouteArmAuthorization::Authorized);
            }
            return Ok(CurrentRouteArmAuthorization::NotAuthorized);
        }
        if !self.is_route_controller(scope) {
            return Ok(CurrentRouteArmAuthorization::NotAuthorized);
        }
        Err(CursorInvariantError::INVARIANT)
    }

    pub(crate) fn normalize_lane_offer_entry(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
        selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        reentry_offer: impl FnMut() -> Option<(ScopeId, StateIndex)>,
    ) -> Option<RouteOfferCursorState> {
        self.normalize_lane_offer_entry_inner(
            scope_id,
            entry_idx,
            selected_arm_for_scope,
            reentry_offer,
        )
    }

    fn normalize_lane_offer_entry_inner(
        &self,
        mut scope_id: ScopeId,
        mut entry_idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut reentry_offer: impl FnMut() -> Option<(ScopeId, StateIndex)>,
    ) -> Option<RouteOfferCursorState> {
        let mut region = self.route_scope_rows(scope_id)?;
        let mut entry = crate::invariant_some(self.route_scope_offer_entry(region.scope()));
        if !entry.is_absent() && state_index_to_usize(entry) != entry_idx {
            let selected_arm = selected_arm_for_scope(scope_id);
            if self
                .route_offer_entry_cursor_position(scope_id, entry_idx, selected_arm)
                .is_some_and(RouteOfferEntryCursorPosition::is_at_entry)
            {
                return self
                    .contains_node_index(entry_idx)
                    .then_some(RouteOfferCursorState::new(region.scope(), entry_idx));
            }
            let canonical_entry = state_index_to_usize(entry);
            if canonical_entry >= region.start() && canonical_entry < region.end() {
                if region.reentry() || selected_arm.is_none() {
                    entry_idx = canonical_entry;
                } else {
                    let (reentry_scope, reentry_entry) = reentry_offer()?;
                    scope_id = reentry_scope;
                    entry_idx = state_index_to_usize(reentry_entry);
                    region = self.route_scope_rows(scope_id)?;
                    entry = crate::invariant_some(self.route_scope_offer_entry(region.scope()));
                    if !entry.is_absent() && state_index_to_usize(entry) != entry_idx {
                        return None;
                    }
                }
            } else {
                let (reentry_scope, reentry_entry) = reentry_offer()?;
                scope_id = reentry_scope;
                entry_idx = state_index_to_usize(reentry_entry);
                region = self.route_scope_rows(scope_id)?;
                entry = crate::invariant_some(self.route_scope_offer_entry(region.scope()));
                if !entry.is_absent() && state_index_to_usize(entry) != entry_idx {
                    return None;
                }
            }
        }
        let selected_arm = selected_arm_for_scope(scope_id);
        let entry_idx =
            self.route_offer_entry_for_index(region.scope(), entry_idx, selected_arm, entry);
        self.contains_node_index(entry_idx)
            .then_some(RouteOfferCursorState::new(region.scope(), entry_idx))
    }

    pub(crate) fn active_reentry_offer_entry(
        &self,
        scope_id: ScopeId,
    ) -> Option<(ScopeId, StateIndex)> {
        let region = self.route_scope_rows(scope_id)?;
        Some((scope_id, StateIndex::from_usize(region.start())))
    }

    #[inline]
    pub(crate) fn passive_child_scope(&self, route_scope: ScopeId, arm: u8) -> Option<ScopeId> {
        self.passive_child_scope_inner(route_scope, arm)
    }

    #[inline]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_slot_inner(scope_id)
    }

    #[inline]
    pub(crate) fn route_commit_range_for_conflict(
        &self,
        conflict: PackedEventConflict,
    ) -> Option<PackedLaneRange> {
        let LocalConflict::RouteArm { scope, arm } = conflict.to_conflict()? else {
            return None;
        };
        if scope.is_none() {
            return None;
        }
        let slot = self.route_scope_slot_inner(scope)?;
        let range = self.machine().route_commit_range_by_slot(slot, arm);
        (!range.is_absent_or_zero_len()).then_some(range)
    }

    #[inline]
    pub(crate) fn route_commit_row_at(
        &self,
        range: PackedLaneRange,
        idx: usize,
    ) -> Option<PackedEventConflict> {
        if range.is_empty() || idx >= range.len() {
            return None;
        }
        let row = self.machine().route_commit_row_at(range.start() + idx);
        row.to_conflict().is_some().then_some(row)
    }

    #[inline]
    pub(crate) fn visit_route_scope_first_recv_dispatch(
        &self,
        scope_id: ScopeId,
        visitor: impl FnMut(u8, StateIndex),
    ) -> Option<()> {
        self.visit_first_recv_dispatch_inner(scope_id, visitor)
    }

    pub(crate) fn route_scope_first_recv_dispatch_arm_mask(&self, scope_id: ScopeId) -> Option<u8> {
        self.first_recv_dispatch_arm_mask_inner(scope_id)
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
    pub(crate) fn event_semantic_at(&self, idx: usize) -> EventSemanticKind {
        self.machine().node(idx).event_semantic()
    }

    #[inline]
    /// Get route controller resolver metadata.
    ///
    /// The tuple `(RouteResolver, u8)` corresponds to the route-scope resolver
    /// binding and the decision tag baked by projection.
    pub(crate) fn route_scope_controller_resolver(
        &self,
        scope_id: ScopeId,
    ) -> Option<(RouteResolver, u8)> {
        self.machine().route_controller(scope_id)
    }
}
