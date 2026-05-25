use super::*;

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(super) fn refresh_cached_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let Some(slot_idx) = active_entries.slot_for_entry(entry_idx) else {
            return false;
        };
        let Some(_entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let compare_len = observation_key.len();
        let mut compare_idx = 0usize;
        while compare_idx < compare_len {
            if compare_idx != slot_idx
                && cached_key.slot(compare_idx) != observation_key.slot(compare_idx)
            {
                return false;
            }
            compare_idx += 1;
        }
        if cached_key.slot(slot_idx) == observation_key.slot(slot_idx) {
            return true;
        }
        if !self
            .endpoint
            .recompute_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
            )
        {
            return false;
        }
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    #[inline]
    pub(super) fn working_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let port = self.endpoint.port_for_lane(self.endpoint.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.endpoint.cursor.frontier_scratch_layout();
        let frontier_entry_capacity = self.endpoint.cursor.max_frontier_entries();
        let mut key = frontier_working_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        key.copy_from(cached_key);
        let mut observed = frontier_observed_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        observed.copy_from(cached_observed_entries);
        (key, observed)
    }

    pub(super) fn refresh_shifted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let Some((old_slot_idx, new_slot_idx)) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_move(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        if !cached_observed_entries.move_entry_slot(entry_idx, new_slot_idx) {
            return false;
        }
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::move_slot_in_array(
            &mut cached_key.slots,
            active_entries.len(),
            old_slot_idx,
            new_slot_idx,
        );
        if !cached_key.entries_equal(&observation_key) {
            return false;
        }
        if cached_key.slot(new_slot_idx) != observation_key.slot(new_slot_idx) {
            let Some(observed) = self
                .endpoint
                .offer_entry_observed_state_cached(entry_idx)
                .or_else(|| {
                    self.endpoint
                        .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                })
            else {
                return false;
            };
            if !self
                .endpoint
                .replace_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                    observed,
                )
            {
                return false;
            }
        }
        *cached_key.slot_mut(new_slot_idx) = observation_key.slot(new_slot_idx);
        if cached_key.slots != observation_key.slots {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    pub(super) fn refresh_inserted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if !self.endpoint.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(insert_slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_insert(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let active_offer_lanes = self.endpoint.route_state.active_offer_lanes();
        let mut changed_external_lane = false;
        let mut check_lane = |lane_idx: usize| {
            let entry_owns_lane = active_offer_lanes.contains(lane_idx)
                && state_index_to_usize(self.endpoint.route_state.lane_offer_state(lane_idx).entry)
                    == entry_idx;
            if !entry_owns_lane
                && (cached_key.offer_lanes().contains(lane_idx)
                    != observation_key.offer_lanes().contains(lane_idx)
                    || cached_key.binding_nonempty_lanes().contains(lane_idx)
                        != observation_key.binding_nonempty_lanes().contains(lane_idx))
            {
                changed_external_lane = true;
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(observation_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        if changed_external_lane {
            return false;
        }
        let len = cached_observed_entries.len();
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::insert_slot_in_array(
            &mut cached_key.slots,
            len,
            insert_slot_idx,
            FrontierObservationSlot {
                entry,
                meta: observation_key.slot(insert_slot_idx),
            },
        );
        cached_key.set_offer_lanes(observation_key.offer_lanes());
        cached_key.set_binding_nonempty_lanes(observation_key.binding_nonempty_lanes());
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let Some(observed) = self
            .endpoint
            .offer_entry_observed_state_cached(entry_idx)
            .or_else(|| {
                self.endpoint
                    .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            })
        else {
            return false;
        };
        if !cached_observed_entries.insert_observation_at_slot_with_frontier_mask(
            entry_idx,
            insert_slot_idx,
            FrontierObservationSlot {
                entry,
                meta: observation_key.slot(insert_slot_idx),
            },
            observed,
            self.endpoint
                .offer_entry_frontier_mask(entry_idx, entry_state),
        ) {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    pub(super) fn refresh_removed_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(removed_slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_remove(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let slot_masks = Self::frontier_observation_offer_lane_entry_slot_masks(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let mut changed_slotted_lane = false;
        let mut check_lane = |lane_idx: usize| {
            if (cached_key.offer_lanes().contains(lane_idx)
                != observation_key.offer_lanes().contains(lane_idx)
                || cached_key.binding_nonempty_lanes().contains(lane_idx)
                    != observation_key.binding_nonempty_lanes().contains(lane_idx))
                && slot_masks[lane_idx] != 0
            {
                changed_slotted_lane = true;
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(observation_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut check_lane,
        );
        if changed_slotted_lane {
            return false;
        }
        if !cached_observed_entries.remove_observation(entry_idx) {
            return false;
        }
        let cached_len = cached_key.len();
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::remove_slot_from_array(
            &mut cached_key.slots,
            cached_len,
            removed_slot_idx,
            FrontierObservationSlot::EMPTY,
        );
        cached_key.set_offer_lanes(observation_key.offer_lanes());
        cached_key.set_binding_nonempty_lanes(observation_key.binding_nonempty_lanes());
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    pub(super) fn refresh_replaced_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let Some((slot_idx, old_entry_idx, new_entry_idx)) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::cached_entry_slot_replace(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let Some(observed) = self
            .endpoint
            .offer_entry_observed_state_cached(new_entry_idx)
            .or_else(|| {
                self.endpoint
                    .recompute_offer_entry_observed_state_non_consuming(new_entry_idx)
            })
        else {
            return false;
        };
        let Some(new_entry_state) = self.endpoint.offer_entry_state_snapshot(new_entry_idx) else {
            return false;
        };
        if !cached_observed_entries.replace_entry_at_slot_with_frontier_mask(
            old_entry_idx,
            new_entry_idx,
            FrontierObservationSlot {
                entry: observation_key.entry_state(slot_idx),
                meta: observation_key.slot(slot_idx),
            },
            observed,
            self.endpoint
                .offer_entry_frontier_mask(new_entry_idx, new_entry_state),
        ) {
            return false;
        }
        cached_key.slots[slot_idx].entry = observation_key.entry_state(slot_idx);
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    pub(super) fn refresh_cached_frontier_observation_scope_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        scope_id: ScopeId,
    ) {
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if !cached_key.lane_sets_equal(&observation_key) {
            return;
        }
        let scope_generation = self.endpoint.scope_evidence_generation_for_scope(scope_id);
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || self.endpoint.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            if cached_key.slot(slot_idx).scope_generation == scope_generation {
                continue;
            }
            let summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
            {
                return;
            }
            let Some(lane_idx) = self
                .endpoint
                .offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                return;
            };
            let route_change_epoch = self
                .endpoint
                .ports
                .get(lane_idx)
                .and_then(Option::as_ref)
                .map(|port| port.route_change_epoch())
                .unwrap_or(0);
            if cached_key.slot(slot_idx).route_change_epoch != route_change_epoch {
                return;
            }
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                )
            {
                return;
            }
            cached_key.slot_mut(slot_idx).scope_generation = scope_generation;
            patched = true;
        }
        if !patched {
            return;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    pub(super) fn refresh_cached_frontier_observation_binding_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        if previous_nonempty
            == self
                .endpoint
                .binding_inbox
                .nonempty_lanes()
                .contains(lane_idx)
        {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        if !observation_key.offer_lanes().contains(lane_idx) {
            return;
        }
        if !cached_key
            .offer_lanes()
            .equals_until(observation_key.offer_lanes(), lane_limit)
            || !cached_key
                .binding_nonempty_lanes()
                .equals_until_except_lane(
                    observation_key.binding_nonempty_lanes(),
                    lane_limit,
                    lane_idx,
                )
        {
            return;
        }
        let mut affected_slot_mask = Self::frontier_observation_offer_lane_entry_slot_masks(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        )[lane_idx];
        if affected_slot_mask == 0 {
            return;
        }
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut affected_slot_mask,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                return;
            };
            let summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || cached_key.slot(slot_idx).entry_summary_fingerprint
                    != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self.endpoint.scope_evidence_generation_for_scope(
                        self.endpoint.offer_entry_scope_id(entry_idx, entry_state),
                    )
            {
                return;
            }
            let Some(representative_lane) = self
                .endpoint
                .offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                return;
            };
            let route_change_epoch = self
                .endpoint
                .ports
                .get(representative_lane)
                .and_then(Option::as_ref)
                .map(|port| port.route_change_epoch())
                .unwrap_or(0);
            if cached_key.slot(slot_idx).route_change_epoch != route_change_epoch {
                return;
            }
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                )
            {
                return;
            }
        }
        cached_key.set_binding_nonempty_lanes(observation_key.binding_nonempty_lanes());
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    pub(super) fn refresh_cached_frontier_observation_route_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        let route_change_epoch = self
            .endpoint
            .ports
            .get(lane_idx)
            .and_then(Option::as_ref)
            .map(|port| port.route_change_epoch())
            .unwrap_or(0);
        if route_change_epoch == previous_change_epoch {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_active_entries(current_parallel_root)
        } else {
            self.endpoint.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        if !cached_key.lane_sets_equal(&observation_key) {
            return;
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || self
                    .endpoint
                    .offer_entry_representative_lane_idx(entry_idx, entry_state)
                    != Some(lane_idx)
            {
                continue;
            }
            let summary = self.endpoint.compute_offer_entry_static_summary(entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self.endpoint.scope_evidence_generation_for_scope(
                        self.endpoint.offer_entry_scope_id(entry_idx, entry_state),
                    )
            {
                return;
            }
            if cached_key.slot(slot_idx).route_change_epoch == route_change_epoch {
                continue;
            }
            if !self
                .endpoint
                .recompute_offer_entry_observation_with_frontier_mask(
                    &mut cached_observed_entries,
                    entry_idx,
                )
            {
                return;
            }
            cached_key.slot_mut(slot_idx).route_change_epoch = route_change_epoch;
            patched = true;
        }
        if !patched {
            return;
        }
        let _ = self.endpoint.next_frontier_observation_epoch();
        Self::store_frontier_observation(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    pub(super) fn refresh_frontier_observation_cache_for_scope(&mut self, scope_id: ScopeId) {
        let global_active_entries = self.endpoint.global_active_entries();
        let mut active_entries = global_active_entries.occupancy_mask();
        let mut frontier_scratch = self.endpoint.frontier_scratch_view();
        let roots = frontier_scratch.root_scopes_mut();
        roots.fill(ScopeId::none());
        let mut root_len = 0usize;
        let mut matches_scope = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut active_entries,
            )
        {
            let Some(entry_idx) = global_active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.endpoint.offer_entry_has_active_lanes(entry_idx)
                || self.endpoint.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            matches_scope = true;
            let Some(parallel_root) = self
                .endpoint
                .offer_entry_parallel_root_from_state(entry_idx, entry_state)
            else {
                continue;
            };
            let mut seen_root = false;
            let mut idx = 0usize;
            while idx < root_len {
                if roots[idx] == parallel_root {
                    seen_root = true;
                    break;
                }
                idx += 1;
            }
            if !seen_root && root_len < roots.len() {
                roots[root_len] = parallel_root;
                root_len += 1;
            }
        }
        if !matches_scope {
            return;
        }
        self.refresh_cached_frontier_observation_scope_entries(ScopeId::none(), false, scope_id);
        let mut idx = 0usize;
        while idx < root_len {
            self.refresh_cached_frontier_observation_scope_entries(roots[idx], true, scope_id);
            idx += 1;
        }
    }

    pub(super) fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        self.refresh_cached_frontier_observation_binding_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_nonempty,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.endpoint.frontier_state.root_frontier_len() {
            let root = self.endpoint.frontier_state.root_frontier_state[slot_idx].root;
            if Self::frontier_observation_offer_lane_entry_slot_masks(self.endpoint, root, true)
                [lane_idx]
                != 0
            {
                self.refresh_cached_frontier_observation_binding_lane_entries(
                    root,
                    true,
                    lane_idx,
                    previous_nonempty,
                );
            }
            slot_idx += 1;
        }
    }

    pub(super) fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        if lane_idx >= self.endpoint.cursor.logical_lane_count() {
            return;
        }
        self.refresh_cached_frontier_observation_route_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_change_epoch,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.endpoint.frontier_state.root_frontier_len() {
            let root = self.endpoint.frontier_state.root_frontier_state[slot_idx].root;
            self.refresh_cached_frontier_observation_route_lane_entries(
                root,
                true,
                lane_idx,
                previous_change_epoch,
            );
            slot_idx += 1;
        }
    }

    pub(super) fn cached_frontier_changed_entry_slot_mask(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.entries_equal(&observation_key)
        {
            return None;
        }
        let mut changed_slot_mask = 0u8;
        let slot_len = observation_key.len();
        let mut slot_idx = 0usize;
        while slot_idx < slot_len {
            if cached_key.slot(slot_idx) != observation_key.slot(slot_idx) {
                changed_slot_mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        let slot_masks = Self::frontier_observation_offer_lane_entry_slot_masks(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let lane_limit = self.endpoint.cursor.logical_lane_count();
        let mut mark_changed_lane = |lane_idx: usize| {
            if cached_key.offer_lanes().contains(lane_idx)
                != observation_key.offer_lanes().contains(lane_idx)
                || cached_key.binding_nonempty_lanes().contains(lane_idx)
                    != observation_key.binding_nonempty_lanes().contains(lane_idx)
            {
                changed_slot_mask |= slot_masks[lane_idx];
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut mark_changed_lane);
        Self::for_each_set_lane(
            observation_key.offer_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Some(changed_slot_mask)
    }
}
