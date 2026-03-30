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
    pub(in crate::endpoint::kernel) fn root_frontier_slot(&self, root: ScopeId) -> Option<usize> {
        if root.is_none() {
            return None;
        }
        self.frontier_state.root_frontier_slot(root.canonical())
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
        self.frontier_state.root_frontier_offer_lane_mask(root)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_active_mask(&self, entry_idx: usize) -> u8 {
        self.frontier_state.offer_entry_active_mask(entry_idx)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn active_frontier_entries(
        &self,
        current_parallel: Option<ScopeId>,
    ) -> ActiveEntrySet {
        current_parallel
            .map(|root| self.root_frontier_active_entries(root))
            .unwrap_or(self.frontier_state.global_active_entries())
    }

    pub(super) fn offer_lane_mask_for_active_entries(&self, active_entries: ActiveEntrySet) -> u8 {
        let mut offer_lane_mask = 0u8;
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                continue;
            };
            if state.active_mask == 0 {
                continue;
            }
            offer_lane_mask |= state.offer_lane_mask;
        }
        offer_lane_mask
    }

    pub(super) fn offer_lane_entry_slot_masks_for_active_entries(
        &self,
        active_entries: ActiveEntrySet,
    ) -> [u8; MAX_LANES] {
        let mut slot_masks = [0u8; MAX_LANES];
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                continue;
            };
            if state.active_mask == 0 {
                continue;
            }
            let mut lane_mask = state.offer_lane_mask;
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut lane_mask) {
                slot_masks[lane_idx] |= 1u8 << slot_idx;
            }
        }
        slot_masks
    }

    pub(super) fn recompute_global_offer_lane_mask(&mut self) {
        self.frontier_state.set_global_offer_lane_mask(
            self.offer_lane_mask_for_active_entries(self.frontier_state.global_active_entries()),
        );
    }

    pub(super) fn recompute_global_offer_lane_entry_slot_masks(&mut self) {
        self.frontier_state.set_global_offer_lane_entry_slot_masks(
            self.offer_lane_entry_slot_masks_for_active_entries(
                self.frontier_state.global_active_entries(),
            ),
        );
    }

    pub(super) fn recompute_root_frontier_offer_lane_mask(&mut self, root: ScopeId) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let active_entries = self.frontier_state.root_frontier_state[slot_idx].active_entries;
        self.frontier_state.set_root_frontier_offer_lane_mask(
            root,
            self.offer_lane_mask_for_active_entries(active_entries),
        );
    }

    pub(super) fn recompute_root_frontier_offer_lane_entry_slot_masks(&mut self, root: ScopeId) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let active_entries = self.frontier_state.root_frontier_state[slot_idx].active_entries;
        self.frontier_state
            .set_root_frontier_offer_lane_entry_slot_masks(
                root,
                self.offer_lane_entry_slot_masks_for_active_entries(active_entries),
            );
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
        let mut lane_mask = active_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            let info = self.route_state.lane_offer_state[lane_idx];
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                continue;
            }
            summary.observe_lane(info);
        }
        summary
    }

    pub(super) fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        let Some(state) = self.frontier_state.offer_entry_state(entry_idx) else {
            return;
        };
        let previous_root = state.parallel_root;
        if state.active_mask == 0 {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.recompute_global_offer_lane_mask();
            self.recompute_global_offer_lane_entry_slot_masks();
            self.recompute_root_frontier_offer_lane_mask(previous_root);
            self.recompute_root_frontier_offer_lane_entry_slot_masks(previous_root);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        self.detach_offer_entry_from_root_frontier(entry_idx, state.parallel_root);
        self.frontier_state.remove_global_active_entry(entry_idx);
        let lane_idx = state.active_mask.trailing_zeros() as usize;
        let info = self.route_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.recompute_global_offer_lane_mask();
            self.recompute_global_offer_lane_entry_slot_masks();
            self.recompute_root_frontier_offer_lane_mask(previous_root);
            self.recompute_root_frontier_offer_lane_entry_slot_masks(previous_root);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(info.scope);
        let mut offer_lane_mask = 0u8;
        let mut offer_lane_idx = 0usize;
        while offer_lane_idx < offer_lanes_len {
            let offer_lane = offer_lanes[offer_lane_idx] as usize;
            if offer_lane < MAX_LANES {
                offer_lane_mask |= 1u8 << offer_lane;
            }
            offer_lane_idx += 1;
        }
        let selection_meta =
            self.compute_offer_entry_selection_meta(info.scope, info, offer_lanes_len != 0);
        let materialization_meta = self.compute_scope_arm_materialization_meta(info.scope);
        let summary = self.compute_offer_entry_static_summary(state.active_mask, entry_idx);
        let mut next_state = state;
        next_state.lane_idx = lane_idx as u8;
        next_state.parallel_root = info.parallel_root;
        next_state.frontier = info.frontier;
        next_state.scope_id = info.scope;
        next_state.offer_lane_mask = offer_lane_mask;
        next_state.offer_lanes = offer_lanes;
        next_state.offer_lanes_len = offer_lanes_len as u8;
        next_state.selection_meta = selection_meta;
        next_state.label_meta = info.label_meta;
        next_state.materialization_meta = materialization_meta;
        next_state.summary = summary;
        self.frontier_state
            .set_offer_entry_state(entry_idx, next_state);
        self.frontier_state
            .insert_global_active_entry(entry_idx, lane_idx as u8);
        self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        self.recompute_global_offer_lane_mask();
        self.recompute_global_offer_lane_entry_slot_masks();
        self.recompute_root_frontier_offer_lane_mask(previous_root);
        self.recompute_root_frontier_offer_lane_entry_slot_masks(previous_root);
        self.recompute_root_frontier_offer_lane_mask(info.parallel_root);
        self.recompute_root_frontier_offer_lane_entry_slot_masks(info.parallel_root);
        let observed = self
            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            .unwrap_or(OfferEntryObservedState::EMPTY);
        self.frontier_state
            .set_offer_entry_observed(entry_idx, observed);
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
        let Some(state) = self.frontier_state.offer_entry_state(entry_idx) else {
            return;
        };
        let active_mask = state.active_mask & !bit;
        if active_mask == 0 {
            self.detach_offer_entry_from_root_frontier(entry_idx, state.parallel_root);
            self.frontier_state.remove_global_active_entry(entry_idx);
            self.frontier_state.clear_offer_entry_state(entry_idx);
            self.recompute_global_offer_lane_mask();
            self.recompute_global_offer_lane_entry_slot_masks();
            self.recompute_root_frontier_offer_lane_mask(state.parallel_root);
            self.recompute_root_frontier_offer_lane_entry_slot_masks(state.parallel_root);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                state.parallel_root,
                ScopeId::none(),
            );
            return;
        }
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
        let mut state = self
            .frontier_state
            .offer_entry_state(entry_idx)
            .unwrap_or(OfferEntryState::EMPTY);
        let was_inactive = state.active_mask == 0;
        state.active_mask |= bit;
        self.frontier_state.set_offer_entry_state(entry_idx, state);
        if was_inactive {
            self.frontier_state
                .insert_global_active_entry(entry_idx, lane_idx as u8);
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

    pub(super) fn sync_lane_offer_state(&mut self) {
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
            self.route_state
                .set_lane_offer_state(lane_idx, info, self.is_linger_route(info.scope));
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
        let (mut entry_idx, mut scope_id, mut lane_cursor) =
            if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
                let lane_cursor = self.cursor.with_index(idx);
                let scope_id = lane_cursor.node_scope_id();
                (idx, scope_id, lane_cursor)
            } else {
                let (scope_id, entry) = self.active_linger_offer_for_lane(lane_idx)?;
                let entry_idx = state_index_to_usize(entry);
                let lane_cursor = self.cursor.with_index(entry_idx);
                (entry_idx, scope_id, lane_cursor)
            };
        let mut region = lane_cursor.scope_region_by_id(scope_id)?;
        if region.kind != ScopeKind::Route {
            return None;
        }
        let mut entry = lane_cursor
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
                    lane_cursor = self.cursor.with_index(entry_idx);
                } else if let Some((linger_scope, linger_entry)) =
                    self.active_linger_offer_for_lane(lane_idx)
                {
                    scope_id = linger_scope;
                    entry_idx = state_index_to_usize(linger_entry);
                    lane_cursor = self.cursor.with_index(entry_idx);
                    region = lane_cursor.scope_region_by_id(scope_id)?;
                    if region.kind != ScopeKind::Route {
                        return None;
                    }
                    entry = lane_cursor
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
                    lane_cursor = self.cursor.with_index(entry_idx);
                    region = lane_cursor.scope_region_by_id(scope_id)?;
                    if region.kind != ScopeKind::Route {
                        return None;
                    }
                    entry = lane_cursor
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
        let is_controller = lane_cursor.is_route_controller(region.scope_id);
        let is_dynamic = lane_cursor
            .route_scope_controller_policy(region.scope_id)
            .map(|(policy, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        let static_facts = Self::frontier_static_facts(
            &lane_cursor,
            &self.control_semantics(),
            region.scope_id,
            is_controller,
            is_dynamic,
        );
        let label_meta = Self::scope_label_meta(
            &lane_cursor,
            &self.control_semantics(),
            region.scope_id,
            static_facts.loop_meta,
        );
        let mut flags = 0u8;
        if is_controller {
            flags |= LaneOfferState::FLAG_CONTROLLER;
        }
        if is_dynamic {
            flags |= LaneOfferState::FLAG_DYNAMIC;
        }
        let parallel_root =
            Self::parallel_scope_root(&lane_cursor, region.scope_id).unwrap_or(ScopeId::none());
        Some(LaneOfferState {
            scope: region.scope_id,
            entry: StateIndex::from_usize(entry_idx),
            parallel_root,
            frontier: static_facts.frontier,
            loop_meta: static_facts.loop_meta,
            label_meta,
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
