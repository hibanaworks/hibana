use super::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
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
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let lane_limit = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if !active_offer_lanes.contains(lane_idx) {
                lane_idx += 1;
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                lane_idx += 1;
                continue;
            }
            summary.observe_lane(info);
            lane_idx += 1;
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

    pub(in crate::endpoint::kernel) fn set_lane_cursor_to_eff_index(
        &mut self,
        lane_idx: usize,
        eff_index: EffIndex,
    ) {
        self.cursor
            .set_lane_cursor_to_eff_index(lane_idx, eff_index);
        self.refresh_lane_offer_state(lane_idx);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn advance_lane_cursor(
        &mut self,
        lane_idx: usize,
        eff_index: EffIndex,
    ) {
        self.cursor.advance_lane_to_eff_index(lane_idx, eff_index);
        self.refresh_lane_offer_state(lane_idx);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn complete_lane_phase(&mut self, lane_idx: usize) {
        self.cursor.complete_lane_phase(lane_idx);
        self.refresh_lane_offer_state(lane_idx);
    }

    pub(in crate::endpoint::kernel) fn advance_phase_skipping_inactive(&mut self) {
        self.cursor.advance_phase_without_sync();
        while self.phase_guard_mismatch() {
            self.cursor.advance_phase_without_sync();
        }
        self.cursor.sync_idx_to_phase_start();
        self.sync_lane_offer_state();
    }

    pub(in crate::endpoint::kernel) fn compute_lane_offer_state(
        &self,
        lane_idx: usize,
    ) -> Option<LaneOfferState> {
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        let (mut entry_idx, mut scope_id) =
            if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
                (idx, self.cursor.node_scope_id_at(idx))
            } else {
                let (scope_id, entry) = self.active_linger_offer_for_lane(lane_idx)?;
                (state_index_to_usize(entry), scope_id)
            };
        let mut region = self.cursor.scope_region_by_id(scope_id)?;
        if region.kind != ScopeKind::Route {
            return None;
        }
        let mut entry = self
            .cursor
            .route_scope_offer_entry(region.scope_id)
            .unwrap_or(StateIndex::MAX);
        if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
            let canonical_entry = state_index_to_usize(entry);
            if canonical_entry >= region.start && canonical_entry < region.end {
                let selected_arm = self.route_arm_for(lane_idx as u8, scope_id);
                if region.linger || selected_arm.is_none() {
                    // Keep offer-state participation only while this scope is unresolved.
                    // Once a non-linger route arm has been selected, re-entering offer_entry
                    // would replay a settled decision.
                    entry_idx = canonical_entry;
                } else if let Some((linger_scope, linger_entry)) =
                    self.active_linger_offer_for_lane(lane_idx)
                {
                    scope_id = linger_scope;
                    entry_idx = state_index_to_usize(linger_entry);
                    region = self.cursor.scope_region_by_id(scope_id)?;
                    if region.kind != ScopeKind::Route {
                        return None;
                    }
                    entry = self
                        .cursor
                        .route_scope_offer_entry(region.scope_id)
                        .unwrap_or(StateIndex::MAX);
                    if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
                        return None;
                    }
                } else {
                    return None;
                }
            } else {
                if let Some((linger_scope, linger_entry)) =
                    self.active_linger_offer_for_lane(lane_idx)
                {
                    scope_id = linger_scope;
                    entry_idx = state_index_to_usize(linger_entry);
                    region = self.cursor.scope_region_by_id(scope_id)?;
                    if region.kind != ScopeKind::Route {
                        return None;
                    }
                    entry = self
                        .cursor
                        .route_scope_offer_entry(region.scope_id)
                        .unwrap_or(StateIndex::MAX);
                    if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        }
        let entry_idx = if entry.is_max() {
            entry_idx
        } else {
            state_index_to_usize(entry)
        };
        let is_controller = self.cursor.is_route_controller(region.scope_id);
        let is_dynamic = self
            .cursor
            .route_scope_controller_policy(region.scope_id)
            .map(|(policy, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        let static_facts = Self::frontier_static_facts_at(
            &self.cursor,
            &self.control_semantics(),
            region.scope_id,
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
            Self::parallel_scope_root(&self.cursor, region.scope_id).unwrap_or(ScopeId::none());
        Some(LaneOfferState {
            scope: region.scope_id,
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
            .route_state
            .active_linger_scope_for_lane(lane_idx, |scope| self.is_linger_route(scope))
        else {
            return None;
        };
        if let Some(region) = self.cursor.scope_region_by_id(scope)
            && region.kind == ScopeKind::Route
        {
            return Some((scope, StateIndex::from_usize(region.start)));
        }
        None
    }
}
