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
    pub(in crate::endpoint::kernel) fn offer_refresh_mask(&self) -> u8 {
        self.cursor
            .current_phase()
            .map(|phase| phase.lane_mask)
            .unwrap_or(0)
            | self.route_state.lane_linger_mask
            | self.route_state.lane_offer_linger_mask
    }

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
    pub(in crate::endpoint::kernel) fn root_frontier_offer_lane_mask(&self, root: ScopeId) -> u8 {
        self.offer_lane_mask_for_active_entries(self.root_frontier_active_entries(root))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_active_mask(&self, entry_idx: usize) -> u8 {
        self.offer_entry_state_snapshot(entry_idx)
            .map(|state| state.active_mask)
            .unwrap_or(0)
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

    pub(super) fn detach_lane_from_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
        self.frontier_state
            .detach_lane_from_root_frontier(lane_idx, info);
    }

    pub(super) fn attach_lane_to_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
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

    pub(super) fn compute_offer_entry_static_summary(
        &self,
        active_mask: u8,
        entry_idx: usize,
    ) -> OfferEntryStaticSummary {
        let mut summary = OfferEntryStaticSummary::EMPTY;
        #[cfg(test)]
        let mut matched_lane = false;
        let mut lane_mask = active_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            let info = self.route_state.lane_offer_state(lane_idx);
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                continue;
            }
            summary.observe_lane(info);
            #[cfg(test)]
            {
                matched_lane = true;
            }
        }
        #[cfg(test)]
        if !matched_lane {
            if let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) {
                if entry_state.active_mask == active_mask {
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
        self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx)
            .frontier_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_frontier_mask_for_entry(
        &self,
        entry_idx: usize,
    ) -> Option<u8> {
        let entry_state = self.offer_entry_state_snapshot(entry_idx)?;
        if entry_state.active_mask == 0 {
            return None;
        }
        Some(self.offer_entry_frontier_mask(entry_idx, entry_state))
    }

    pub(super) fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
            return;
        };
        let previous_root = self
            .offer_entry_parallel_root_from_state(entry_idx, entry_state)
            .unwrap_or(ScopeId::none());
        let active_mask = entry_state.active_mask;
        if active_mask == 0 {
            #[cfg(test)]
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        self.detach_offer_entry_from_root_frontier(entry_idx, previous_root);
        self.ensure_global_frontier_scratch_initialized();
        let mut global_active_entries = self.global_active_entries();
        global_active_entries.remove_entry(entry_idx);
        let lane_idx = active_mask.trailing_zeros() as usize;
        let info = self.route_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            #[cfg(test)]
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        #[cfg(test)]
        let (_, offer_lanes_len) = self.offer_lanes_for_scope(info.scope);
        #[cfg(test)]
        let selection_meta =
            self.compute_offer_entry_selection_meta(info.scope, info, offer_lanes_len != 0);
        #[cfg(test)]
        let loop_meta = Self::scope_loop_meta_at(
            &self.cursor,
            &self.control_semantics(),
            info.scope,
            entry_idx,
        );
        #[cfg(test)]
        let label_meta = Self::scope_label_meta_at(
            &self.cursor,
            &self.control_semantics(),
            info.scope,
            loop_meta,
            entry_idx,
        );
        #[cfg(test)]
        let materialization_meta = self.compute_scope_arm_materialization_meta(info.scope);
        #[cfg(test)]
        let offer_lane_mask = self.offer_lane_mask_for_scope_id(info.scope);
        #[cfg(test)]
        let test_summary = self.compute_offer_entry_static_summary(active_mask, entry_idx);
        #[cfg(test)]
        {
            let Some(state) = self.frontier_state.offer_entry_state_mut(entry_idx) else {
                return;
            };
            state.lane_idx = lane_idx as u8;
            state.parallel_root = info.parallel_root;
            state.frontier = info.frontier;
            state.scope_id = info.scope;
            state.offer_lane_mask = offer_lane_mask;
            state.selection_meta = selection_meta;
            state.label_meta = label_meta;
            state.materialization_meta = materialization_meta;
            state.summary = test_summary;
        }
        self.ensure_global_frontier_scratch_initialized();
        let mut global_active_entries = self.global_active_entries();
        global_active_entries.insert_entry(entry_idx, lane_idx as u8);
        self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        #[cfg(test)]
        let observed = self
            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            .unwrap_or_else(|| {
                unreachable!("test observed state must recompute for active offer entry")
            });
        #[cfg(test)]
        self.frontier_state
            .set_offer_entry_observed(entry_idx, observed);
        #[cfg(not(test))]
        let _ = self.recompute_offer_entry_observed_state_non_consuming(entry_idx);
        self.refresh_frontier_observation_caches_for_entry(
            entry_idx,
            previous_root,
            info.parallel_root,
        );
    }

    pub(super) fn detach_lane_from_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let bit = 1u8 << lane_idx;
        let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
            return;
        };
        let parallel_root = self
            .offer_entry_parallel_root_from_state(entry_idx, entry_state)
            .unwrap_or(ScopeId::none());
        let active_mask = entry_state.active_mask;
        let active_mask = active_mask & !bit;
        if active_mask == 0 {
            self.detach_offer_entry_from_root_frontier(entry_idx, parallel_root);
            self.ensure_global_frontier_scratch_initialized();
            let mut global_active_entries = self.global_active_entries();
            global_active_entries.remove_entry(entry_idx);
            #[cfg(test)]
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                parallel_root,
                ScopeId::none(),
            );
            return;
        }
        #[cfg(test)]
        self.frontier_state
            .set_offer_entry_active_mask(entry_idx, active_mask);
        self.refresh_offer_entry_state(entry_idx);
    }

    pub(super) fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let bit = 1u8 << lane_idx;
        #[cfg(not(test))]
        let was_inactive = (self.offer_entry_active_mask_from_route_state(entry_idx) & !bit) == 0;
        #[cfg(test)]
        let was_inactive = if let Some(state) = self.frontier_state.offer_entry_state_mut(entry_idx)
        {
            let was_inactive = state.active_mask == 0;
            state.active_mask |= bit;
            was_inactive
        } else {
            let mut state = OfferEntryState::EMPTY;
            state.active_mask = bit;
            self.frontier_state.set_offer_entry_state(entry_idx, state);
            true
        };
        if was_inactive {
            self.ensure_global_frontier_scratch_initialized();
            let mut global_active_entries = self.global_active_entries();
            global_active_entries.insert_entry(entry_idx, lane_idx as u8);
            self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    #[inline]
    pub(super) fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        let old = self.route_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.detach_lane_from_root_frontier(lane_idx, old);
    }

    pub(crate) fn sync_lane_offer_state(&mut self) {
        let refresh_mask = self.offer_refresh_mask();
        let mut stale_mask = self.route_state.active_offer_mask & !refresh_mask;
        while stale_mask != 0 {
            let lane_idx = stale_mask.trailing_zeros() as usize;
            stale_mask &= !(1u8 << lane_idx);
            self.clear_lane_offer_state(lane_idx);
        }
        let mut lane_mask = refresh_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    pub(super) fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let old = self.route_state.clear_lane_offer_state(lane_idx);
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.detach_lane_from_root_frontier(lane_idx, old);
        if let Some(info) = self.compute_lane_offer_state(lane_idx) {
            let is_linger = self.is_linger_route(info.scope);
            self.route_state
                .set_lane_offer_state(lane_idx, info, is_linger);
            self.attach_lane_to_root_frontier(lane_idx, info);
            self.attach_lane_to_offer_entry(lane_idx, info);
        }
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

    pub(in crate::endpoint::kernel) fn advance_phase_skipping_inactive(&mut self) {
        self.cursor.advance_phase_without_sync();
        while self.phase_guard_mismatch() {
            self.cursor.advance_phase_without_sync();
        }
        self.cursor.sync_idx_to_phase_start();
        self.sync_lane_offer_state();
    }

    pub(super) fn compute_lane_offer_state(&self, lane_idx: usize) -> Option<LaneOfferState> {
        if lane_idx >= MAX_LANES {
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
        if lane_idx >= MAX_LANES {
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
