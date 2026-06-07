use super::{
    ActiveEntrySet, CursorEndpoint, CursorRefresh, EpochTable, LabelUniverse, LaneOfferState,
    MintConfigMarker, OfferEntryState, OfferEntryStaticSummary, ScopeId, StateIndex, Transport,
    state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
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
        current_parallel
            .map(|root| self.root_frontier_active_entries(root))
            .unwrap_or(self.global_active_entries())
    }

    pub(in crate::endpoint::kernel) fn detach_lane_from_root_frontier(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
    ) {
        self.frontier_state
            .detach_lane_from_root_frontier(lane_idx, info);
    }

    pub(in crate::endpoint::kernel) fn attach_lane_to_root_frontier(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
    ) {
        self.frontier_state
            .attach_lane_to_root_frontier(lane_idx, info);
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

    pub(in crate::endpoint::kernel) fn compute_offer_entry_static_summary_from_route_state(
        &self,
        entry_idx: usize,
    ) -> OfferEntryStaticSummary {
        let mut summary = OfferEntryStaticSummary::EMPTY;
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                next = active_offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
                continue;
            }
            summary.observe_lane(info);
            next = active_offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        summary
    }

    pub(in crate::endpoint::kernel) fn compute_offer_entry_static_summary(
        &self,
        entry_idx: usize,
    ) -> OfferEntryStaticSummary {
        let summary = self.compute_offer_entry_static_summary_from_route_state(entry_idx);
        #[cfg(test)]
        if summary.frontier_mask == 0 {
            if let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) {
                if self.offer_entry_has_active_lanes(entry_idx) {
                    return entry_state.summary;
                }
            }
        }
        summary
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_frontier_mask(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> u8 {
        let _ = entry_state;
        self.compute_offer_entry_static_summary(entry_idx)
            .frontier_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_frontier_mask_for_entry(
        &self,
        entry_idx: usize,
    ) -> Option<u8> {
        let entry_state = self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return None;
        }
        Some(self.offer_entry_frontier_mask(entry_idx, entry_state))
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
        let (entry_idx, scope_id) = if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
            (idx, self.cursor.node_scope_id_at(idx))
        } else {
            let (scope_id, entry) = self.active_linger_offer_for_lane(lane_idx)?;
            (state_index_to_usize(entry), scope_id)
        };
        let normalized = self.cursor.normalize_lane_offer_entry(
            scope_id,
            entry_idx,
            |scope| self.selected_arm_for_scope(scope),
            || self.active_linger_offer_for_lane(lane_idx),
        )?;
        let scope_id = normalized.scope_id();
        let entry_idx = normalized.entry_idx();
        let is_controller = self.cursor.is_route_controller(scope_id);
        let is_dynamic = self
            .cursor
            .route_scope_controller_policy(scope_id)
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        let static_facts = Self::frontier_static_facts_at(
            &self.cursor,
            &self.control_semantics(),
            scope_id,
            is_controller,
            is_dynamic,
            entry_idx,
        );
        let mut flags = 0u8;
        if is_controller {
            flags |= LaneOfferState::FLAG_CONTROLLER;
        }
        if is_dynamic {
            flags |= LaneOfferState::FLAG_DYNAMIC;
        }
        let parallel_root =
            Self::parallel_scope_root(&self.cursor, scope_id).unwrap_or(ScopeId::none());
        Some(LaneOfferState {
            scope: scope_id,
            entry: StateIndex::from_usize(entry_idx),
            parallel_root,
            frontier: static_facts.frontier,
            static_ready: static_facts.ready,
            flags,
        })
    }

    pub(super) fn active_linger_offer_for_lane(
        &self,
        lane_idx: usize,
    ) -> Option<(ScopeId, StateIndex)> {
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        let Some(scope) = self
            .decision_state
            .active_linger_scope_for_lane(lane_idx, |scope| self.is_linger_route(scope))
        else {
            return None;
        };
        self.cursor.active_linger_offer_entry(scope)
    }
}
