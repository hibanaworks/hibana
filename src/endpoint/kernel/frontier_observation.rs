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
    fn global_frontier_scratch_parts(&self) -> (*mut [u8], FrontierScratchLayout, usize) {
        let port = self.port_for_lane(self.primary_lane);
        (
            lane_port::frontier_scratch_ptr(port),
            self.cursor.frontier_scratch_layout(),
            self.cursor.max_frontier_entries(),
        )
    }

    #[cfg(not(test))]
    #[inline]
    fn global_frontier_observed_state(&self) -> GlobalFrontierObservedState {
        if !self.frontier_state.global_frontier_scratch_initialized {
            return GlobalFrontierObservedState::EMPTY;
        }
        let (scratch_ptr, layout, _) = self.global_frontier_scratch_parts();
        unsafe { *frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout) }
    }

    #[cfg(not(test))]
    #[inline]
    fn global_frontier_observed_state_mut(&mut self) -> &mut GlobalFrontierObservedState {
        self.ensure_global_frontier_scratch_initialized();
        let (scratch_ptr, layout, _) = self.global_frontier_scratch_parts();
        unsafe { &mut *frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout) }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn ensure_global_frontier_scratch_initialized(&mut self) {
        if self.frontier_state.global_frontier_scratch_initialized {
            return;
        }
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        let mut active_entries = frontier_global_active_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        active_entries.clear();
        let mut cached_key = frontier_cached_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        cached_key.clear();
        #[cfg(test)]
        {
            self.frontier_state.global_frontier_observed.clear();
            self.frontier_state.global_frontier_observed_offer_lane_mask = 0;
            self.frontier_state
                .global_frontier_observed_binding_nonempty_mask = 0;
        }
        #[cfg(not(test))]
        unsafe {
            frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout)
                .write(GlobalFrontierObservedState::EMPTY);
        }
        self.frontier_state.global_frontier_scratch_initialized = true;
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn global_active_entries(&self) -> ActiveEntrySet {
        if !self.frontier_state.global_frontier_scratch_initialized {
            return ActiveEntrySet::EMPTY;
        }
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        frontier_global_active_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        )
    }

    #[inline]
    fn cached_global_frontier_observation_key(&self) -> FrontierObservationKey {
        if !self.frontier_state.global_frontier_scratch_initialized {
            return FrontierObservationKey::EMPTY;
        }
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        let mut key = frontier_cached_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        #[cfg(test)]
        {
            key.offer_lane_mask = self.frontier_state.global_frontier_observed_offer_lane_mask;
            key.binding_nonempty_mask = self
                .frontier_state
                .global_frontier_observed_binding_nonempty_mask;
        }
        #[cfg(not(test))]
        {
            let global = self.global_frontier_observed_state();
            key.offer_lane_mask = global.offer_lane_mask;
            key.binding_nonempty_mask = global.binding_nonempty_mask;
        }
        key
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn overwrite_global_active_entries_for_test(
        &mut self,
        src: ActiveEntrySet,
    ) {
        self.ensure_global_frontier_scratch_initialized();
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        let mut active_entries = frontier_global_active_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        active_entries.copy_from(src);
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn insert_global_active_entry_for_test(
        &mut self,
        entry_idx: usize,
        lane_idx: u8,
    ) -> bool {
        self.ensure_global_frontier_scratch_initialized();
        let mut active_entries = self.global_active_entries();
        active_entries.insert_entry(entry_idx, lane_idx)
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn remove_global_active_entry_for_test(
        &mut self,
        entry_idx: usize,
    ) -> bool {
        if !self.frontier_state.global_frontier_scratch_initialized {
            return false;
        }
        let mut active_entries = self.global_active_entries();
        active_entries.remove_entry(entry_idx)
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn overwrite_global_frontier_observed_key_for_test(
        &mut self,
        src: FrontierObservationKey,
    ) {
        self.ensure_global_frontier_scratch_initialized();
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        let mut cached_key = frontier_cached_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        cached_key.copy_from(src);
        self.frontier_state.global_frontier_observed_offer_lane_mask = src.offer_lane_mask;
        self.frontier_state
            .global_frontier_observed_binding_nonempty_mask = src.binding_nonempty_mask;
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn overwrite_global_frontier_observed_for_test(
        &mut self,
        src: ObservedEntrySet,
    ) {
        self.ensure_global_frontier_scratch_initialized();
        let mut cached_key = self.cached_global_frontier_observation_key();
        self.frontier_state
            .overwrite_global_frontier_observed(&mut cached_key, src);
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_key_for_test(
        &self,
    ) -> FrontierObservationKey {
        self.cached_global_frontier_observation_key()
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_entry_bit_for_test(
        &self,
        entry_idx: usize,
    ) -> u8 {
        self.frontier_state.global_frontier_observed_entry_bit(
            self.cached_global_frontier_observation_key(),
            entry_idx,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_lane_mask(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> u8 {
        if use_root_observed_entries {
            self.root_frontier_offer_lane_mask(current_parallel_root)
        } else {
            self.offer_lane_mask_for_active_entries(self.global_active_entries())
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_offer_lane_entry_slot_masks(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> OfferLaneEntrySlotMasks {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let mut slot_masks = OfferLaneEntrySlotMasks::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if state.active_mask == 0 {
                continue;
            }
            let mut lane_mask = self.offer_entry_offer_lane_mask(entry_idx, state);
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut lane_mask) {
                slot_masks.set_logical_mask(lane_idx, slot_masks[lane_idx] | (1u8 << slot_idx));
            }
        }
        slot_masks
    }

    pub(super) fn frontier_observation_key(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> FrontierObservationKey {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        let mut key = frontier_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            self.cursor.max_frontier_entries(),
        );
        key.clear();
        key.set_active_entries_from(active_entries);
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            let summary =
                self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
            let slot = key.slot_mut(slot_idx);
            slot.entry_summary_fingerprint = summary.observation_fingerprint();
            slot.scope_generation = self.scope_evidence_generation_for_scope(
                self.offer_entry_scope_id(entry_idx, entry_state),
            );
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            let Some(lane_idx) = self.offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                continue;
            };
            key.slot_mut(slot_idx).route_change_epoch = self
                .ports
                .get(lane_idx)
                .and_then(Option::as_ref)
                .map(Port::route_change_epoch)
                .unwrap_or(0);
        }
        key.offer_lane_mask = offer_lane_mask;
        key.binding_nonempty_mask = self.binding_inbox.nonempty_mask & offer_lane_mask;
        key
    }

    #[inline]
    fn working_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        let frontier_entry_capacity = self.cursor.max_frontier_entries();
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

    #[inline]
    fn empty_observed_entries_scratch(&mut self) -> ObservedEntrySet {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        let mut observed = frontier_observed_entries_view_from_storage(
            scratch_ptr,
            layout,
            self.cursor.max_frontier_entries(),
        );
        observed.clear();
        observed
    }

    #[inline]
    pub(super) fn cached_frontier_observed_entries(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        self.cached_frontier_observed_entries_ref(
            current_parallel_root,
            use_root_observed_entries,
            &key,
        )
    }

    #[inline]
    pub(super) fn cached_frontier_observed_entries_ref(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: &FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        #[cfg(test)]
        {
            return self.frontier_state.cached_frontier_observed_entries(
                current_parallel_root,
                use_root_observed_entries,
                self.cached_global_frontier_observation_key(),
                key,
            );
        }
        #[cfg(not(test))]
        {
            if use_root_observed_entries {
                let slot_idx = self
                    .frontier_state
                    .root_frontier_slot(current_parallel_root)?;
                let slot = self.frontier_state.root_frontier_state[slot_idx];
                let observed_key = self
                    .frontier_state
                    .root_frontier_state
                    .observed_key(slot_idx);
                if observed_key != *key || slot.observed_entries.dynamic_controller_mask != 0 {
                    return None;
                }
                return Some(observed_key.observed_entries(slot.observed_entries));
            }
            let cached_key = self.cached_global_frontier_observation_key();
            let global = self.global_frontier_observed_state();
            if cached_key != *key || global.summary.dynamic_controller_mask != 0 {
                return None;
            }
            Some(cached_key.observed_entries(global.summary))
        }
    }

    #[inline]
    pub(super) fn frontier_observation_cache(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        #[cfg(test)]
        {
            return self.frontier_state.frontier_observation_cache(
                current_parallel_root,
                use_root_observed_entries,
                self.cached_global_frontier_observation_key(),
            );
        }
        #[cfg(not(test))]
        {
            if use_root_observed_entries {
                let Some(slot_idx) = self
                    .frontier_state
                    .root_frontier_slot(current_parallel_root)
                else {
                    return (FrontierObservationKey::EMPTY, ObservedEntrySet::EMPTY);
                };
                let row = self.frontier_state.root_frontier_state[slot_idx];
                let observed_key = self
                    .frontier_state
                    .root_frontier_state
                    .observed_key(slot_idx);
                return (
                    observed_key,
                    observed_key.observed_entries(row.observed_entries),
                );
            }
            let cached_key = self.cached_global_frontier_observation_key();
            let global = self.global_frontier_observed_state();
            (cached_key, cached_key.observed_entries(global.summary))
        }
    }

    #[inline]
    pub(super) fn store_frontier_observation(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        #[cfg(test)]
        {
            if !use_root_observed_entries {
                self.ensure_global_frontier_scratch_initialized();
            }
            let cached_global_key = self.cached_global_frontier_observation_key();
            self.frontier_state.store_frontier_observation(
                current_parallel_root,
                use_root_observed_entries,
                cached_global_key,
                key,
                observed_entries,
            );
        }
        #[cfg(not(test))]
        {
            if use_root_observed_entries {
                let Some(slot_idx) = self
                    .frontier_state
                    .root_frontier_slot(current_parallel_root)
                else {
                    return;
                };
                self.frontier_state
                    .root_frontier_state
                    .replace_root_observed_key(slot_idx, key);
                let slot = &mut self.frontier_state.root_frontier_state[slot_idx];
                slot.observed_entries = observed_entries.summary();
                return;
            }
            self.ensure_global_frontier_scratch_initialized();
            let (scratch_ptr, layout, frontier_entry_capacity) =
                self.global_frontier_scratch_parts();
            let mut cached_key = frontier_cached_observation_key_view_from_storage(
                scratch_ptr,
                layout,
                frontier_entry_capacity,
            );
            cached_key.copy_from(key);
            let global = self.global_frontier_observed_state_mut();
            global.summary = observed_entries.summary();
            global.offer_lane_mask = key.offer_lane_mask;
            global.binding_nonempty_mask = key.binding_nonempty_mask;
        }
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if self.refresh_structural_frontier_observation_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            cached_key,
        ) {
            return;
        }
        let observed_entries = self.refresh_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        );
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            observed_entries,
        );
    }

    pub(super) fn refresh_cached_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let Some(slot_idx) = active_entries.slot_for_entry(entry_idx) else {
            return false;
        };
        let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
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
        let slot_unchanged = cached_key.slot(slot_idx) == observation_key.slot(slot_idx);
        if slot_unchanged {
            return true;
        }
        if !self.recompute_offer_entry_observation_with_frontier_mask(
            &mut cached_observed_entries,
            entry_idx,
        ) {
            return false;
        }
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    fn replace_offer_entry_observation_with_frontier_mask(
        &self,
        observed_entries: &mut ObservedEntrySet,
        entry_idx: usize,
        observed: OfferEntryObservedState,
    ) -> bool {
        let Some(frontier_mask) = self.offer_entry_frontier_mask_for_entry(entry_idx) else {
            return false;
        };
        observed_entries.replace_observation_with_frontier_mask(entry_idx, observed, frontier_mask)
    }

    fn recompute_offer_entry_observation_with_frontier_mask(
        &mut self,
        observed_entries: &mut ObservedEntrySet,
        entry_idx: usize,
    ) -> bool {
        let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
        else {
            return false;
        };
        self.replace_offer_entry_observation_with_frontier_mask(
            observed_entries,
            entry_idx,
            observed,
        )
    }

    pub(super) fn refresh_structural_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let active_len = active_entries.len();
        let cached_len = cached_key.len();
        if active_len == cached_len {
            if let Some(entry_idx) = Self::structural_replaced_entry_idx(active_entries, cached_key)
                && self.refresh_replaced_frontier_observation_entry(
                    current_parallel_root,
                    use_root_observed_entries,
                    entry_idx,
                )
            {
                return true;
            }
            if Self::structural_shifted_entry_idx(active_entries, cached_key).is_some() {
                let mut remaining_slots = active_entries.occupancy_mask();
                while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
                    let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                        continue;
                    };
                    if active_entries.entry_state(slot_idx) == cached_key.entry_state(slot_idx) {
                        continue;
                    }
                    if self.refresh_shifted_frontier_observation_entry(
                        current_parallel_root,
                        use_root_observed_entries,
                        entry_idx,
                    ) {
                        return true;
                    }
                }
            }
            if Self::same_active_entry_set(active_entries, cached_key)
                && self.refresh_permuted_frontier_observation_entries(
                    current_parallel_root,
                    use_root_observed_entries,
                    active_entries,
                )
            {
                return true;
            }
            if self.refresh_multi_replaced_frontier_observation_entries(
                current_parallel_root,
                use_root_observed_entries,
                active_entries,
            ) {
                return true;
            }
            return false;
        }
        if active_len + 1 == cached_len
            && let Some(entry_idx) = Self::structural_removed_entry_idx(active_entries, cached_key)
            && self.refresh_removed_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        if active_len == cached_len + 1
            && let Some(entry_idx) = Self::structural_inserted_entry_idx(active_entries, cached_key)
            && self.refresh_inserted_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        false
    }

    pub(super) fn refresh_permuted_frontier_observation_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !Self::same_active_entry_set(active_entries, cached_key)
        {
            return false;
        }
        let mut refreshed = self.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    &entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                )
                .or_else(|| self.offer_entry_observed_state_cached(entry_idx))
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx));
            let Some(observed) = observed else {
                return false;
            };
            let Some((observed_bit, _)) = refreshed.insert_entry(entry_idx) else {
                return false;
            };
            refreshed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.offer_entry_frontier_mask(entry_idx, entry_state),
            );
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            refreshed,
        );
        true
    }

    pub(super) fn refresh_multi_replaced_frontier_observation_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let active_len = active_entries.len();
        if active_len == 0
            || active_len != cached_key.len()
            || Self::same_active_entry_set(active_entries, cached_key)
        {
            return false;
        }
        let mut refreshed = self.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut reused_cached = false;
        let mut recomputed = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = if let Some(observed) = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    &entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                ) {
                reused_cached = true;
                observed
            } else if let Some(observed) = self.offer_entry_observed_state_cached(entry_idx) {
                reused_cached = true;
                observed
            } else {
                recomputed = true;
                let Some(observed) =
                    self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
                else {
                    return false;
                };
                observed
            };
            let Some((observed_bit, _)) = refreshed.insert_entry(entry_idx) else {
                return false;
            };
            refreshed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.offer_entry_frontier_mask(entry_idx, entry_state),
            );
        }
        if !reused_cached || !recomputed {
            return false;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            refreshed,
        );
        true
    }

    pub(super) fn refresh_shifted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let Some((old_slot_idx, new_slot_idx)) =
            Self::cached_entry_slot_move(active_entries, cached_key, entry_idx)
        else {
            return false;
        };
        if !cached_observed_entries.move_entry_slot(entry_idx, new_slot_idx) {
            return false;
        }
        Self::move_slot_in_array(
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
                .offer_entry_observed_state_cached(entry_idx)
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
            else {
                return false;
            };
            if !self.replace_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
                observed,
            ) {
                return false;
            }
        }
        *cached_key.slot_mut(new_slot_idx) = observation_key.slot(new_slot_idx);
        if cached_key.slots != observation_key.slots {
            return false;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
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
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(insert_slot_idx) =
            Self::cached_entry_slot_insert(active_entries, cached_key, entry_idx)
        else {
            return false;
        };
        if ((cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            & !self.offer_entry_offer_lane_mask(entry_idx, entry_state))
            != 0
            || ((cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask)
                & !self.offer_entry_offer_lane_mask(entry_idx, entry_state))
                != 0
        {
            return false;
        }
        let len = cached_observed_entries.len();
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        Self::insert_slot_in_array(
            &mut cached_key.slots,
            len,
            insert_slot_idx,
            FrontierObservationSlot {
                entry,
                meta: observation_key.slot(insert_slot_idx),
            },
        );
        cached_key.offer_lane_mask = observation_key.offer_lane_mask;
        cached_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let Some(observed) = self
            .offer_entry_observed_state_cached(entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
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
            self.offer_entry_frontier_mask(entry_idx, entry_state),
        ) {
            return false;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
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
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(removed_slot_idx) =
            Self::cached_entry_slot_remove(active_entries, cached_key, entry_idx)
        else {
            return false;
        };
        let changed_lane_mask = (cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            | (cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask);
        if changed_lane_mask != 0 {
            let slot_masks = self.frontier_observation_offer_lane_entry_slot_masks(
                current_parallel_root,
                use_root_observed_entries,
            );
            let mut remaining_lanes = changed_lane_mask;
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut remaining_lanes) {
                if slot_masks[lane_idx] != 0 {
                    return false;
                }
            }
        }
        if !cached_observed_entries.remove_observation(entry_idx) {
            return false;
        }
        let cached_len = cached_key.len();
        Self::remove_slot_from_array(
            &mut cached_key.slots,
            cached_len,
            removed_slot_idx,
            FrontierObservationSlot::EMPTY,
        );
        cached_key.offer_lane_mask = observation_key.offer_lane_mask;
        cached_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
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
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let Some((slot_idx, old_entry_idx, new_entry_idx)) =
            Self::cached_entry_slot_replace(active_entries, cached_key, entry_idx)
        else {
            return false;
        };
        let Some(observed) = self
            .offer_entry_observed_state_cached(new_entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(new_entry_idx))
        else {
            return false;
        };
        let Some(new_entry_state) = self.offer_entry_state_snapshot(new_entry_idx) else {
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
            self.offer_entry_frontier_mask(new_entry_idx, new_entry_state),
        ) {
            return false;
        }
        cached_key.slots[slot_idx].entry = observation_key.entry_state(slot_idx);
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    pub(super) fn cached_entry_slot_move(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<(usize, usize)> {
        let new_slot_idx = active_entries.slot_for_entry(entry_idx)?;
        let len = active_entries.len();
        let entry = checked_state_index(entry_idx)?;
        let mut old_slot_idx = 0usize;
        while old_slot_idx < len {
            if cached_key.entry_state(old_slot_idx) == entry {
                break;
            }
            old_slot_idx += 1;
        }
        if old_slot_idx >= len || old_slot_idx == new_slot_idx {
            return None;
        }
        let mut shifted = [StateIndex::MAX; MAX_LANES];
        let mut copy_idx = 0usize;
        while copy_idx < len {
            shifted[copy_idx] = cached_key.entry_state(copy_idx);
            copy_idx += 1;
        }
        Self::move_slot_in_array(&mut shifted, len, old_slot_idx, new_slot_idx);
        if !active_entries.entries_prefix_matches(&shifted, len) {
            return None;
        }
        Some((old_slot_idx, new_slot_idx))
    }

    pub(super) fn cached_entry_slot_insert(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<usize> {
        let insert_slot_idx = active_entries.slot_for_entry(entry_idx)?;
        let len = active_entries.len();
        if len == 0 {
            return None;
        }
        let cached_len = len - 1;
        let entry = checked_state_index(entry_idx)?;
        let mut slot_idx = 0usize;
        while slot_idx < cached_len {
            if cached_key.entry_state(slot_idx) == entry {
                return None;
            }
            slot_idx += 1;
        }
        let mut inserted = [StateIndex::MAX; MAX_LANES];
        let mut copy_idx = 0usize;
        while copy_idx < cached_len {
            inserted[copy_idx] = cached_key.entry_state(copy_idx);
            copy_idx += 1;
        }
        Self::insert_slot_in_array(&mut inserted, cached_len, insert_slot_idx, entry);
        if !active_entries.entries_prefix_matches(&inserted, len) {
            return None;
        }
        Some(insert_slot_idx)
    }

    pub(super) fn cached_entry_slot_remove(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<usize> {
        let len = active_entries.len();
        if len >= MAX_LANES {
            return None;
        }
        let cached_len = len + 1;
        let entry = u16::try_from(entry_idx).ok()?;
        let mut removed_slot_idx = 0usize;
        while removed_slot_idx < cached_len {
            if cached_key.entry_state(removed_slot_idx) == entry {
                break;
            }
            removed_slot_idx += 1;
        }
        if removed_slot_idx >= cached_len {
            return None;
        }
        let mut removed = [StateIndex::MAX; MAX_LANES];
        let mut copy_idx = 0usize;
        while copy_idx < cached_len {
            removed[copy_idx] = cached_key.entry_state(copy_idx);
            copy_idx += 1;
        }
        Self::remove_slot_from_array(&mut removed, cached_len, removed_slot_idx, StateIndex::MAX);
        if !active_entries.entries_prefix_matches(&removed, len) {
            return None;
        }
        Some(removed_slot_idx)
    }

    pub(super) fn cached_entry_slot_replace(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<(usize, usize, usize)> {
        let len = active_entries.len();
        if len == 0 {
            return None;
        }
        let entry = u16::try_from(entry_idx).ok()?;
        let mut replaced_slot_idx = None;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let cached_entry = cached_key.entry_state(slot_idx);
            let active_entry = active_entries.entry_state(slot_idx);
            if cached_entry != active_entry {
                if replaced_slot_idx.is_some() {
                    return None;
                }
                if cached_entry != entry && active_entry != entry {
                    return None;
                }
                replaced_slot_idx = Some(slot_idx);
            }
            slot_idx += 1;
        }
        let slot_idx = replaced_slot_idx?;
        let old_entry_idx = state_index_to_usize(cached_key.entry_state(slot_idx));
        let new_entry_idx = state_index_to_usize(active_entries.entry_state(slot_idx));
        Some((slot_idx, old_entry_idx, new_entry_idx))
    }

    #[inline]
    pub(super) fn cached_active_entries_len(cached_key: FrontierObservationKey) -> usize {
        cached_key.len()
    }

    #[inline]
    pub(super) fn cached_active_entries_contains(
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> bool {
        cached_key.contains_entry(entry_idx)
    }

    #[inline]
    pub(super) fn cached_active_entry_slot(
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<usize> {
        cached_key.slot_for_entry(entry_idx)
    }

    pub(super) fn structural_inserted_entry_idx(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> Option<usize> {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if active_len != cached_len + 1 {
            return None;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut inserted = None;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if Self::cached_active_entries_contains(cached_key, entry_idx) {
                continue;
            }
            if inserted.is_some() {
                return None;
            }
            inserted = Some(entry_idx);
        }
        inserted
    }

    pub(super) fn structural_removed_entry_idx(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> Option<usize> {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if cached_len != active_len + 1 {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut removed = None;
        while slot_idx < cached_len {
            let entry_idx = state_index_to_usize(cached_key.entry_state(slot_idx));
            if active_entries.slot_for_entry(entry_idx).is_some() {
                slot_idx += 1;
                continue;
            }
            if removed.is_some() {
                return None;
            }
            removed = Some(entry_idx);
            slot_idx += 1;
        }
        removed
    }

    pub(super) fn structural_replaced_entry_idx(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> Option<usize> {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if active_len != cached_len {
            return None;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut inserted = None;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if Self::cached_active_entries_contains(cached_key, entry_idx) {
                continue;
            }
            if inserted.is_some() {
                return None;
            }
            inserted = Some(entry_idx);
        }
        inserted
    }

    pub(super) fn structural_shifted_entry_idx(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> Option<usize> {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if active_len != cached_len {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut shifted = None;
        while slot_idx < active_len {
            let entry_idx = state_index_to_usize(active_entries.entry_state(slot_idx));
            if !Self::cached_active_entries_contains(cached_key, entry_idx) {
                return None;
            }
            if cached_key.entry_state(slot_idx) != active_entries.entry_state(slot_idx) {
                shifted.get_or_insert(entry_idx);
            }
            slot_idx += 1;
        }
        shifted
    }

    pub(super) fn same_active_entry_set(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if active_len != cached_len {
            return false;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            if !Self::cached_active_entries_contains(cached_key, entry_idx) {
                return false;
            }
        }
        true
    }

    pub(super) fn move_slot_in_array<V: Copy>(
        array: &mut [V],
        len: usize,
        old_slot_idx: usize,
        new_slot_idx: usize,
    ) {
        let len = len.min(array.len());
        if old_slot_idx == new_slot_idx || old_slot_idx >= len || new_slot_idx >= len {
            return;
        }
        let value = array[old_slot_idx];
        if old_slot_idx < new_slot_idx {
            let mut slot_idx = old_slot_idx;
            while slot_idx < new_slot_idx {
                array[slot_idx] = array[slot_idx + 1];
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = old_slot_idx;
            while slot_idx > new_slot_idx {
                array[slot_idx] = array[slot_idx - 1];
                slot_idx -= 1;
            }
        }
        array[new_slot_idx] = value;
    }

    pub(super) fn insert_slot_in_array<V: Copy>(
        array: &mut [V],
        len: usize,
        slot_idx: usize,
        value: V,
    ) {
        let len = len.min(array.len());
        if len >= array.len() || slot_idx > len {
            return;
        }
        let mut shift_idx = len;
        while shift_idx > slot_idx {
            array[shift_idx] = array[shift_idx - 1];
            shift_idx -= 1;
        }
        array[slot_idx] = value;
    }

    pub(super) fn remove_slot_from_array<V: Copy>(
        array: &mut [V],
        len: usize,
        slot_idx: usize,
        fill: V,
    ) {
        let len = len.min(array.len());
        if len == 0 || slot_idx >= len {
            return;
        }
        let mut shift_idx = slot_idx;
        while shift_idx + 1 < len {
            array[shift_idx] = array[shift_idx + 1];
            shift_idx += 1;
        }
        array[len - 1] = fill;
    }

    pub(super) fn refresh_cached_frontier_observation_scope_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        scope_id: ScopeId,
    ) {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask
            || cached_key.binding_nonempty_mask
                != (self.binding_inbox.nonempty_mask & offer_lane_mask)
        {
            return;
        }
        let scope_generation = self.scope_evidence_generation_for_scope(scope_id);
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0
                || self.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            if cached_key.slot(slot_idx).scope_generation == scope_generation {
                continue;
            }
            let summary =
                self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
            {
                return;
            }
            let Some(lane_idx) = self.offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                return;
            };
            let route_change_epoch = self
                .ports
                .get(lane_idx)
                .and_then(Option::as_ref)
                .map(Port::route_change_epoch)
                .unwrap_or(0);
            if cached_key.slot(slot_idx).route_change_epoch != route_change_epoch {
                return;
            }
            if !self.recompute_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
            ) {
                return;
            }
            cached_key.slot_mut(slot_idx).scope_generation = scope_generation;
            patched = true;
        }
        if !patched {
            return;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
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
        previous_nonempty_mask: u8,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_bit = 1u8 << lane_idx;
        if ((previous_nonempty_mask ^ self.binding_inbox.nonempty_mask) & lane_bit) == 0 {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask || (offer_lane_mask & lane_bit) == 0 {
            return;
        }
        let binding_nonempty_mask = self.binding_inbox.nonempty_mask & offer_lane_mask;
        if ((cached_key.binding_nonempty_mask ^ binding_nonempty_mask) & !lane_bit) != 0 {
            return;
        }
        let mut affected_slot_mask = self.frontier_observation_offer_lane_entry_slot_masks(
            current_parallel_root,
            use_root_observed_entries,
        )[lane_idx];
        if affected_slot_mask == 0 {
            return;
        }
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut affected_slot_mask) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                return;
            };
            let summary =
                self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
            if entry_state.active_mask == 0
                || cached_key.slot(slot_idx).entry_summary_fingerprint
                    != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self.scope_evidence_generation_for_scope(
                        self.offer_entry_scope_id(entry_idx, entry_state),
                    )
            {
                return;
            }
            let Some(representative_lane) =
                self.offer_entry_representative_lane_idx(entry_idx, entry_state)
            else {
                return;
            };
            let route_change_epoch = self
                .ports
                .get(representative_lane)
                .and_then(Option::as_ref)
                .map(Port::route_change_epoch)
                .unwrap_or(0);
            if cached_key.slot(slot_idx).route_change_epoch != route_change_epoch {
                return;
            }
            if !self.recompute_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
            ) {
                return;
            }
        }
        cached_key.binding_nonempty_mask = binding_nonempty_mask;
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
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
        if lane_idx >= MAX_LANES {
            return;
        }
        let route_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        if route_change_epoch == previous_change_epoch {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let (mut cached_key, mut cached_observed_entries) = self
            .working_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask
            || cached_key.binding_nonempty_mask
                != (self.binding_inbox.nonempty_mask & offer_lane_mask)
        {
            return;
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0
                || self.offer_entry_representative_lane_idx(entry_idx, entry_state)
                    != Some(lane_idx)
            {
                continue;
            }
            let summary =
                self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self.scope_evidence_generation_for_scope(
                        self.offer_entry_scope_id(entry_idx, entry_state),
                    )
            {
                return;
            }
            if cached_key.slot(slot_idx).route_change_epoch == route_change_epoch {
                continue;
            }
            if !self.recompute_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
            ) {
                return;
            }
            cached_key.slot_mut(slot_idx).route_change_epoch = route_change_epoch;
            patched = true;
        }
        if !patched {
            return;
        }
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            cached_key,
            cached_observed_entries,
        );
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_cache_for_scope(&mut self, scope_id: ScopeId) {
        let mut active_entries = self.global_active_entries().occupancy_mask();
        let mut frontier_scratch = self.frontier_scratch_view();
        let roots = frontier_scratch.root_scopes_mut();
        roots.fill(ScopeId::none());
        let mut root_len = 0usize;
        let mut matches_scope = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut active_entries) {
            let Some(entry_idx) = self.global_active_entries().entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0
                || self.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            matches_scope = true;
            let Some(parallel_root) =
                self.offer_entry_parallel_root_from_state(entry_idx, entry_state)
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

    #[inline]
    pub(super) fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty_mask: u8,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        self.refresh_cached_frontier_observation_binding_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_nonempty_mask,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.frontier_state.root_frontier_len() {
            let root = self.frontier_state.root_frontier_state[slot_idx].root;
            if self.frontier_observation_offer_lane_entry_slot_masks(root, true)[lane_idx] != 0 {
                self.refresh_cached_frontier_observation_binding_lane_entries(
                    root,
                    true,
                    lane_idx,
                    previous_nonempty_mask,
                );
            }
            slot_idx += 1;
        }
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        self.refresh_cached_frontier_observation_route_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_change_epoch,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.frontier_state.root_frontier_len() {
            let root = self.frontier_state.root_frontier_state[slot_idx].root;
            self.refresh_cached_frontier_observation_route_lane_entries(
                root,
                true,
                lane_idx,
                previous_change_epoch,
            );
            slot_idx += 1;
        }
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_cache_from_cached_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries()
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        let Some(observed_entries) = self.refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) else {
            return false;
        };
        let _ = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            observed_entries,
        );
        true
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_cache_for_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) {
        let (cached_key, _) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            self.refresh_frontier_observation_cache(
                current_parallel_root,
                use_root_observed_entries,
            );
            return;
        }
        if self.refresh_cached_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_frontier_observation_cache_from_cached_entries(
            current_parallel_root,
            use_root_observed_entries,
        ) || self.refresh_replaced_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_removed_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_inserted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_shifted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) {
            return;
        }
        self.refresh_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_caches_for_entry(
        &mut self,
        entry_idx: usize,
        previous_root: ScopeId,
        current_root: ScopeId,
    ) {
        self.refresh_frontier_observation_cache_for_entry(ScopeId::none(), false, entry_idx);
        if !previous_root.is_none() {
            self.refresh_frontier_observation_cache_for_entry(previous_root, true, entry_idx);
        }
        if !current_root.is_none() && current_root != previous_root {
            self.refresh_frontier_observation_cache_for_entry(current_root, true, entry_idx);
        }
    }

    #[inline]
    pub(super) fn recompute_offer_entry_observed_state_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        let entry_state = self.offer_entry_state_snapshot(entry_idx)?;
        if entry_state.active_mask == 0 {
            return None;
        }
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            self.preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);
        let (observed, _) = self.offer_entry_candidate_from_observation(
            entry_idx,
            entry_state,
            binding_ready,
            has_ack,
            has_ready_arm_evidence,
        );
        #[cfg(test)]
        self.frontier_state
            .set_offer_entry_observed(entry_idx, observed);
        Some(observed)
    }

    #[inline]
    pub(super) fn offer_entry_observed_state_cached(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        let state = self.offer_entry_state_snapshot(entry_idx)?;
        if state.active_mask == 0 {
            return None;
        }
        #[cfg(test)]
        {
            return (state.observed != OfferEntryObservedState::EMPTY).then_some(state.observed);
        }
        #[cfg(not(test))]
        {
            let parallel_root = self
                .offer_entry_parallel_root_from_state(entry_idx, state)
                .unwrap_or(ScopeId::none());
            let use_root_observed_entries = !parallel_root.is_none();
            let (_, cached_observed_entries) =
                self.frontier_observation_cache(parallel_root, use_root_observed_entries);
            let cached_bit = cached_observed_entries.entry_bit(entry_idx);
            if cached_bit == 0 {
                return None;
            }
            let summary = self.compute_offer_entry_static_summary(state.active_mask, entry_idx);
            return Some(cached_offer_entry_observed_state(
                self.offer_entry_scope_id(entry_idx, state),
                summary,
                cached_observed_entries,
                cached_bit,
            ));
        }
    }

    pub(super) fn cached_frontier_changed_entry_slot_mask(
        &self,
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
        let mut slot_idx = 0usize;
        while slot_idx < MAX_LANES {
            if observation_key.entry_state(slot_idx).is_max() {
                break;
            }
            if cached_key.slot(slot_idx) != observation_key.slot(slot_idx) {
                changed_slot_mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        let mut changed_lane_mask = cached_key.offer_lane_mask ^ observation_key.offer_lane_mask;
        changed_lane_mask |=
            cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask;
        if changed_lane_mask != 0 {
            let slot_masks = self.frontier_observation_offer_lane_entry_slot_masks(
                current_parallel_root,
                use_root_observed_entries,
            );
            let mut remaining_lanes = changed_lane_mask;
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut remaining_lanes) {
                changed_slot_mask |= slot_masks[lane_idx];
            }
        }
        Some(changed_slot_mask)
    }

    pub(super) fn refresh_frontier_observed_entries_from_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        let mut changed_slot_mask = self.cached_frontier_changed_entry_slot_mask(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            cached_key,
        )?;
        if changed_slot_mask == 0 {
            return Some(cached_observed_entries);
        }
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        let mut refreshed = frontier_observed_entries_view_from_storage(
            scratch_ptr,
            layout,
            self.cursor.max_frontier_entries(),
        );
        refreshed.copy_from(cached_observed_entries);
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut changed_slot_mask) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if !self.recompute_offer_entry_observation_with_frontier_mask(&mut refreshed, entry_idx)
            {
                return None;
            }
        }
        Some(refreshed)
    }

    pub(super) fn compose_frontier_observed_entries(
        &mut self,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        let mut composed = self.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(_slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(_slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if entry_state.active_mask == 0 {
                continue;
            }
            let observed = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    &entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                )
                .or_else(|| self.offer_entry_observed_state_cached(entry_idx))
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx));
            let Some(observed) = observed else {
                continue;
            };
            let Some((observed_bit, _)) = composed.insert_entry(entry_idx) else {
                continue;
            };
            composed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.offer_entry_frontier_mask(entry_idx, entry_state),
            );
        }
        composed
    }

    #[cfg(test)]
    pub(super) fn patch_frontier_observed_entries_from_cached_structure(
        &mut self,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        Some(self.compose_frontier_observed_entries(
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ))
    }

    #[inline]
    pub(super) fn frontier_observation_entry_reusable(
        &self,
        entry_idx: usize,
        entry_state: &OfferEntryState,
        cached_slot_idx: usize,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let Some(observation_slot_idx) = Self::cached_active_entry_slot(observation_key, entry_idx)
        else {
            return false;
        };
        if cached_slot_idx >= MAX_LANES
            || cached_key.entry_state(cached_slot_idx) != entry
            || cached_key.entry_state(cached_slot_idx).is_max()
            || observation_key
                .slot(observation_slot_idx)
                .entry_summary_fingerprint
                != self
                    .compute_offer_entry_static_summary(entry_state.active_mask, entry_idx)
                    .observation_fingerprint()
            || observation_key.slot(observation_slot_idx).scope_generation
                != self.scope_evidence_generation_for_scope(
                    self.offer_entry_scope_id(entry_idx, *entry_state),
                )
        {
            return false;
        }
        let changed_binding_mask =
            cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask;
        if (changed_binding_mask & self.offer_entry_offer_lane_mask(entry_idx, *entry_state)) != 0 {
            return false;
        }
        let Some(representative_lane) =
            self.offer_entry_representative_lane_idx(entry_idx, *entry_state)
        else {
            return false;
        };
        if observation_key
            .slot(observation_slot_idx)
            .route_change_epoch
            != self
                .ports
                .get(representative_lane)
                .and_then(Option::as_ref)
                .map(Port::route_change_epoch)
                .unwrap_or(0)
        {
            return false;
        }
        true
    }

    #[inline]
    pub(super) fn cached_offer_entry_observed_state_for_rebuild(
        &self,
        entry_idx: usize,
        entry_state: &OfferEntryState,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<OfferEntryObservedState> {
        if cached_key == FrontierObservationKey::EMPTY {
            return None;
        }
        let cached_bit = cached_observed_entries.entry_bit(entry_idx);
        if cached_bit == 0 || (cached_observed_entries.dynamic_controller_mask & cached_bit) != 0 {
            return None;
        }
        let cached_slot_idx = cached_bit.trailing_zeros() as usize;
        if !self.frontier_observation_entry_reusable(
            entry_idx,
            entry_state,
            cached_slot_idx,
            observation_key,
            cached_key,
        ) {
            return None;
        }
        let summary = self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
        Some(cached_offer_entry_observed_state(
            self.offer_entry_scope_id(entry_idx, *entry_state),
            summary,
            cached_observed_entries,
            cached_bit,
        ))
    }

    #[inline]
    pub(super) fn refresh_frontier_observed_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        if let Some(refreshed) = self.refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) {
            return refreshed;
        }
        self.compose_frontier_observed_entries(
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn next_frontier_observation_epoch(&mut self) -> u16 {
        #[cfg(test)]
        {
            let mut cached_key = self.cached_global_frontier_observation_key();
            self.frontier_state.next_observation_epoch(&mut cached_key)
        }
        #[cfg(not(test))]
        {
            let next = self
                .global_frontier_observed_state()
                .observation_epoch
                .wrapping_add(1);
            if next == 0 {
                if self.frontier_state.global_frontier_scratch_initialized {
                    let (scratch_ptr, layout, frontier_entry_capacity) =
                        self.global_frontier_scratch_parts();
                    let mut cached_key = frontier_cached_observation_key_view_from_storage(
                        scratch_ptr,
                        layout,
                        frontier_entry_capacity,
                    );
                    cached_key.clear();
                    unsafe {
                        frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout).write(
                            GlobalFrontierObservedState {
                                observation_epoch: 1,
                                ..GlobalFrontierObservedState::EMPTY
                            },
                        );
                    }
                } else {
                    self.global_frontier_observed_state_mut().observation_epoch = 1;
                }
                let len = self.frontier_state.root_frontier_len();
                let mut idx = 0usize;
                while idx < len {
                    self.frontier_state
                        .root_frontier_state
                        .clear_root_observed_key(idx);
                    self.frontier_state.root_frontier_state[idx]
                        .observed_entries
                        .clear();
                    idx += 1;
                }
                1
            } else {
                self.global_frontier_observed_state_mut().observation_epoch = next;
                next
            }
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_entries(&self) -> ObservedEntrySet {
        #[cfg(test)]
        {
            return self
                .frontier_state
                .global_frontier_observed_entries(self.cached_global_frontier_observation_key());
        }
        #[cfg(not(test))]
        {
            let cached_key = self.cached_global_frontier_observation_key();
            cached_key.observed_entries(self.global_frontier_observed_state().summary)
        }
    }

    pub(in crate::endpoint::kernel) fn root_frontier_progress_sibling_exists(
        &self,
        root: ScopeId,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        loop_controller_without_evidence: bool,
    ) -> bool {
        self.observed_frontier_progress_sibling_exists(
            self.root_frontier_observed_entries(root),
            current_entry_idx,
            current_frontier,
            loop_controller_without_evidence,
        )
    }

    pub(in crate::endpoint::kernel) fn global_frontier_progress_sibling_exists(
        &self,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        loop_controller_without_evidence: bool,
    ) -> bool {
        self.observed_frontier_progress_sibling_exists(
            self.global_frontier_observed_entries(),
            current_entry_idx,
            current_frontier,
            loop_controller_without_evidence,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_frontier_progress_sibling_exists(
        &self,
        observed_entries: ObservedEntrySet,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        loop_controller_without_evidence: bool,
    ) -> bool {
        let mut sibling_mask = observed_entries.progress_mask;
        sibling_mask &= !observed_entries.entry_bit(current_entry_idx);
        if !loop_controller_without_evidence {
            sibling_mask &= observed_entries.frontier_mask(current_frontier);
        }
        sibling_mask != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn root_frontier_observed_entries(
        &self,
        root: ScopeId,
    ) -> ObservedEntrySet {
        self.frontier_state.root_frontier_observed_entries(root)
    }
}
