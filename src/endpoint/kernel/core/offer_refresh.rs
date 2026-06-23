use super::{
    ActiveEntrySet, CursorEndpoint, CursorRefresh, LaneOfferState, OfferEntrySummary,
    ReentryScopeLiveness, ScopeId, StateIndex, Transport, state_index_to_usize,
};
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn root_frontier_active_mask(&self, root: ScopeId) -> u8 {
        self.frontier_state.root_frontier_active_mask(root)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn root_frontier_active_entries(
        &self,
        root: ScopeId,
    ) -> ActiveEntrySet {
        self.frontier_state.root_frontier_active_entries(root)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn active_frontier_entries(
        &self,
        current_parallel: Option<ScopeId>,
    ) -> ActiveEntrySet {
        match current_parallel {
            Some(root) => self.root_frontier_active_entries(root),
            None => self.global_active_entries(),
        }
    }

    pub(in crate::endpoint::kernel) fn detach_lane_from_root_frontier(
        &mut self,
        info: LaneOfferState,
    ) {
        self.frontier_state.detach_lane_from_root_frontier(info);
    }

    pub(in crate::endpoint::kernel) fn attach_lane_to_root_frontier(
        &mut self,
        info: LaneOfferState,
    ) {
        self.frontier_state.attach_lane_to_root_frontier(info);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn attach_offer_entry_to_root_frontier(
        &mut self,
        entry_idx: usize,
        root: ScopeId,
        lane_idx: u8,
    ) {
        self.frontier_state
            .attach_offer_entry_to_root_frontier(entry_idx, root, lane_idx);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn detach_offer_entry_from_root_frontier(
        &mut self,
        entry_idx: usize,
        root: ScopeId,
    ) {
        self.frontier_state
            .detach_offer_entry_from_root_frontier(entry_idx, root);
    }

    pub(in crate::endpoint::kernel) fn compute_offer_entry_summary_from_route_state(
        &self,
        entry_idx: usize,
    ) -> OfferEntrySummary {
        let mut summary = OfferEntrySummary::EMPTY;
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
                continue;
            }
            summary.observe_lane(info);
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        summary
    }

    pub(in crate::endpoint::kernel) fn compute_offer_entry_summary(
        &self,
        entry_idx: usize,
    ) -> OfferEntrySummary {
        self.compute_offer_entry_summary_from_route_state(entry_idx)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_frontier_mask(&self, entry_idx: usize) -> u8 {
        self.compute_offer_entry_summary(entry_idx).frontier_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn refresh_after_cursor_move(
        &mut self,
        refresh: CursorRefresh,
    ) {
        match refresh {
            CursorRefresh::Lane(lane) => self.refresh_lane_offer_state(lane as usize),
            CursorRefresh::AllLanes => self.sync_lane_offer_state(),
        }
    }

    pub(in crate::endpoint::kernel) fn compute_lane_offer_state(
        &self,
        lane_idx: usize,
    ) -> Option<LaneOfferState> {
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        let reentry_offer = self.active_reentry_offer_for_lane(lane_idx);
        let (entry_idx, scope_id) = if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
            let scope_id = self.cursor.node_scope_id_at(idx);
            if self.cursor.has_route_scope(scope_id) {
                (idx, scope_id)
            } else {
                let (scope_id, entry) = reentry_offer?;
                (state_index_to_usize(entry), scope_id)
            }
        } else {
            let (scope_id, entry) = reentry_offer?;
            (state_index_to_usize(entry), scope_id)
        };
        let normalized = self.cursor.normalize_lane_offer_entry(
            scope_id,
            entry_idx,
            |scope| self.selected_live_arm_for_scope(scope),
            || self.active_reentry_offer_for_lane(lane_idx),
        )?;
        let scope_id = normalized.scope_id();
        let entry_idx = normalized.entry_idx();
        let is_controller = self.cursor.is_route_controller(scope_id);
        let is_dynamic = self
            .cursor
            .route_scope_controller_resolver(scope_id)
            .is_some_and(|(resolver, _)| resolver.is_dynamic());
        let frontier_facts =
            Self::frontier_facts_at(&self.cursor, scope_id, is_controller, is_dynamic, entry_idx);
        let mut flags = 0u8;
        if is_controller {
            flags |= LaneOfferState::FLAG_CONTROLLER;
        }
        if is_dynamic {
            flags |= LaneOfferState::FLAG_DYNAMIC;
        }
        if frontier_facts.ready() {
            flags |= LaneOfferState::FLAG_INTRINSIC_READY;
        }
        let parallel_root = match Self::parallel_scope_root(&self.cursor, scope_id) {
            Some(root) => root,
            None => ScopeId::none(),
        };
        Some(LaneOfferState {
            scope: scope_id,
            entry: StateIndex::from_usize(entry_idx),
            parallel_root,
            frontier: frontier_facts.frontier,
            flags,
        })
    }

    pub(super) fn active_reentry_offer_for_lane(
        &self,
        lane_idx: usize,
    ) -> Option<(ScopeId, StateIndex)> {
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        let scope = self
            .decision_state
            .active_reentry_scope_for_lane(lane_idx, |scope| {
                if !self.is_reentry_route(scope) {
                    return ReentryScopeLiveness::NotReentry;
                }
                let Some(arm) = self.selected_arm_for_scope(scope) else {
                    return ReentryScopeLiveness::Incomplete;
                };
                if self.reentrant_selected_arm_complete(scope, arm) {
                    ReentryScopeLiveness::Complete
                } else {
                    ReentryScopeLiveness::Incomplete
                }
            })?;
        self.cursor.active_reentry_offer_entry(scope)
    }

    pub(in crate::endpoint::kernel) fn active_reentry_scope_for_observed_frame(
        &self,
        lane: u8,
        frame_label: u8,
    ) -> Option<ScopeId> {
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        self.decision_state
            .active_reentry_scope_for_lane(lane_idx, |scope| {
                if !self.is_reentry_route(scope)
                    || self
                        .cursor
                        .passive_descendant_target_index_from_exact_frame_label(
                            scope,
                            lane,
                            frame_label,
                        )
                        .is_none()
                {
                    return ReentryScopeLiveness::NotReentry;
                }
                let Some(arm) = self.selected_arm_for_scope(scope) else {
                    return ReentryScopeLiveness::Incomplete;
                };
                if self.reentrant_selected_arm_complete(scope, arm) {
                    ReentryScopeLiveness::Complete
                } else {
                    ReentryScopeLiveness::Incomplete
                }
            })
    }
}
