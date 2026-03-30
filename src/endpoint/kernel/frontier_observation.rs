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
    pub(in crate::endpoint::kernel) fn frontier_observation_lane_mask(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> u8 {
        if use_root_observed_entries {
            self.root_frontier_offer_lane_mask(current_parallel_root)
        } else {
            self.frontier_state.global_offer_lane_mask()
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_offer_lane_entry_slot_masks(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> [u8; MAX_LANES] {
        if use_root_observed_entries {
            return self
                .root_frontier_slot(current_parallel_root)
                .map(|slot_idx| {
                    self.frontier_state.root_frontier_state[slot_idx].offer_lane_entry_slot_masks
                })
                .unwrap_or([0; MAX_LANES]);
        }
        self.frontier_state.global_offer_lane_entry_slot_masks()
    }

    pub(super) fn frontier_observation_key(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> FrontierObservationKey {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.frontier_state.global_active_entries()
        };
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        let active_entry_indices = active_entries.entries;
        let mut entry_summary_fingerprints = [0; MAX_LANES];
        let mut scope_generations = [0; MAX_LANES];
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.frontier_state.offer_entry_state(entry_idx) else {
                continue;
            };
            entry_summary_fingerprints[slot_idx] = entry_state.summary.observation_fingerprint();
            scope_generations[slot_idx] =
                self.scope_evidence_generation_for_scope(entry_state.scope_id);
        }
        let mut route_change_epochs = [0; MAX_LANES];
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.frontier_state.offer_entry_state(entry_idx) else {
                continue;
            };
            let lane_idx = entry_state.lane_idx as usize;
            if lane_idx >= MAX_LANES {
                continue;
            }
            route_change_epochs[slot_idx] = self.ports[lane_idx]
                .as_ref()
                .map(Port::route_change_epoch)
                .unwrap_or(0);
        }
        FrontierObservationKey {
            active_entries: active_entry_indices,
            entry_summary_fingerprints,
            scope_generations,
            offer_lane_mask,
            binding_nonempty_mask: self.binding_inbox.nonempty_mask & offer_lane_mask,
            route_change_epochs,
        }
    }

    #[inline]
    pub(super) fn cached_frontier_observed_entries(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        self.frontier_state.cached_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            key,
        )
    }

    #[inline]
    pub(super) fn frontier_observation_cache(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        self.frontier_state
            .frontier_observation_cache(current_parallel_root, use_root_observed_entries)
    }

    #[inline]
    pub(super) fn store_frontier_observation(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observed_epoch: u32,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        self.frontier_state.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            key,
            observed_entries,
        );
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
            self.frontier_state.global_active_entries()
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
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
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
            self.frontier_state.global_active_entries
        };
        let Some(slot_idx) = active_entries.slot_for_entry(entry_idx) else {
            return false;
        };
        let Some(entry_state) = self
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
        else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let mut expected_fingerprints = cached_key.entry_summary_fingerprints;
        expected_fingerprints[slot_idx] = observation_key.entry_summary_fingerprints[slot_idx];
        let mut expected_scope_generations = cached_key.scope_generations;
        expected_scope_generations[slot_idx] = observation_key.scope_generations[slot_idx];
        let mut expected_route_change_epochs = cached_key.route_change_epochs;
        expected_route_change_epochs[slot_idx] = observation_key.route_change_epochs[slot_idx];
        if expected_fingerprints != observation_key.entry_summary_fingerprints
            || expected_scope_generations != observation_key.scope_generations
            || expected_route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let slot_unchanged = cached_key.entry_summary_fingerprints[slot_idx]
            == observation_key.entry_summary_fingerprints[slot_idx]
            && cached_key.scope_generations[slot_idx]
                == observation_key.scope_generations[slot_idx]
            && cached_key.route_change_epochs[slot_idx]
                == observation_key.route_change_epochs[slot_idx];
        if slot_unchanged {
            return true;
        }
        let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
        else {
            return false;
        };
        if !cached_observed_entries.replace_observation(entry_idx, observed) {
            return false;
        }
        cached_key.entry_summary_fingerprints[slot_idx] =
            observation_key.entry_summary_fingerprints[slot_idx];
        cached_key.scope_generations[slot_idx] = observation_key.scope_generations[slot_idx];
        cached_key.route_change_epochs[slot_idx] = observation_key.route_change_epochs[slot_idx];
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
        true
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
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_key.active_entries);
        if active_len == cached_len {
            if let Some(entry_idx) =
                Self::structural_replaced_entry_idx(active_entries, cached_key.active_entries)
                && self.refresh_replaced_frontier_observation_entry(
                    current_parallel_root,
                    use_root_observed_entries,
                    entry_idx,
                )
            {
                return true;
            }
            if Self::structural_shifted_entry_idx(active_entries, cached_key.active_entries)
                .is_some()
            {
                let mut remaining_slots = active_entries.occupancy_mask();
                while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
                    let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                        continue;
                    };
                    if active_entries.entries[slot_idx] == cached_key.active_entries[slot_idx] {
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
            if Self::same_active_entry_set(active_entries, cached_key.active_entries)
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
            && let Some(entry_idx) =
                Self::structural_removed_entry_idx(active_entries, cached_key.active_entries)
            && self.refresh_removed_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        if active_len == cached_len + 1
            && let Some(entry_idx) =
                Self::structural_inserted_entry_idx(active_entries, cached_key.active_entries)
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
            || !Self::same_active_entry_set(active_entries, cached_key.active_entries)
        {
            return false;
        }
        let mut refreshed = ObservedEntrySet::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    entry_state,
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
            refreshed.observe(observed_bit, observed);
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
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
        let active_len = active_entries.len as usize;
        if active_len == 0
            || active_len != Self::cached_active_entries_len(cached_key.active_entries)
            || Self::same_active_entry_set(active_entries, cached_key.active_entries)
        {
            return false;
        }
        let mut refreshed = ObservedEntrySet::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut reused_cached = false;
        let mut recomputed = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = if let Some(observed) = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    entry_state,
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
            refreshed.observe(observed_bit, observed);
        }
        if !reused_cached || !recomputed {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
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
            self.frontier_state.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let Some((old_slot_idx, new_slot_idx)) =
            Self::cached_entry_slot_move(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        if !cached_observed_entries.move_entry_slot(entry_idx, new_slot_idx) {
            return false;
        }
        let mut shifted_key = cached_key;
        Self::move_slot_in_array(
            &mut shifted_key.active_entries,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        Self::move_slot_in_array(
            &mut shifted_key.entry_summary_fingerprints,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        Self::move_slot_in_array(
            &mut shifted_key.scope_generations,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        Self::move_slot_in_array(
            &mut shifted_key.route_change_epochs,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        if shifted_key.active_entries != observation_key.active_entries {
            return false;
        }
        if shifted_key.entry_summary_fingerprints[new_slot_idx]
            != observation_key.entry_summary_fingerprints[new_slot_idx]
            || shifted_key.scope_generations[new_slot_idx]
                != observation_key.scope_generations[new_slot_idx]
            || shifted_key.route_change_epochs[new_slot_idx]
                != observation_key.route_change_epochs[new_slot_idx]
        {
            let Some(observed) = self
                .offer_entry_observed_state_cached(entry_idx)
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
            else {
                return false;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return false;
            }
        }
        shifted_key.entry_summary_fingerprints[new_slot_idx] =
            observation_key.entry_summary_fingerprints[new_slot_idx];
        shifted_key.scope_generations[new_slot_idx] =
            observation_key.scope_generations[new_slot_idx];
        shifted_key.route_change_epochs[new_slot_idx] =
            observation_key.route_change_epochs[new_slot_idx];
        if shifted_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || shifted_key.scope_generations != observation_key.scope_generations
            || shifted_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            shifted_key,
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
            self.frontier_state.global_active_entries
        };
        let Some(entry_state) = self
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
        else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(insert_slot_idx) =
            Self::cached_entry_slot_insert(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        if ((cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            & !entry_state.offer_lane_mask)
            != 0
            || ((cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask)
                & !entry_state.offer_lane_mask)
                != 0
        {
            return false;
        }
        let len = cached_observed_entries.len as usize;
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let mut inserted_key = cached_key;
        Self::insert_slot_in_array(
            &mut inserted_key.active_entries,
            len,
            insert_slot_idx,
            entry,
        );
        Self::insert_slot_in_array(
            &mut inserted_key.entry_summary_fingerprints,
            len,
            insert_slot_idx,
            observation_key.entry_summary_fingerprints[insert_slot_idx],
        );
        Self::insert_slot_in_array(
            &mut inserted_key.scope_generations,
            len,
            insert_slot_idx,
            observation_key.scope_generations[insert_slot_idx],
        );
        Self::insert_slot_in_array(
            &mut inserted_key.route_change_epochs,
            len,
            insert_slot_idx,
            observation_key.route_change_epochs[insert_slot_idx],
        );
        inserted_key.offer_lane_mask = observation_key.offer_lane_mask;
        inserted_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if inserted_key.active_entries != observation_key.active_entries
            || inserted_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || inserted_key.scope_generations != observation_key.scope_generations
            || inserted_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let Some(observed) = self
            .offer_entry_observed_state_cached(entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
        else {
            return false;
        };
        if !cached_observed_entries.insert_observation_at_slot(entry_idx, insert_slot_idx, observed)
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            inserted_key,
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
            self.frontier_state.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(removed_slot_idx) =
            Self::cached_entry_slot_remove(active_entries, cached_key.active_entries, entry_idx)
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
        let cached_len = cached_key
            .active_entries
            .iter()
            .position(|entry| entry.is_max())
            .unwrap_or(MAX_LANES);
        let mut removed_key = cached_key;
        Self::remove_slot_from_array(
            &mut removed_key.active_entries,
            cached_len,
            removed_slot_idx,
            StateIndex::MAX,
        );
        Self::remove_slot_from_array(
            &mut removed_key.entry_summary_fingerprints,
            cached_len,
            removed_slot_idx,
            0,
        );
        Self::remove_slot_from_array(
            &mut removed_key.scope_generations,
            cached_len,
            removed_slot_idx,
            0,
        );
        Self::remove_slot_from_array(
            &mut removed_key.route_change_epochs,
            cached_len,
            removed_slot_idx,
            0,
        );
        removed_key.offer_lane_mask = observation_key.offer_lane_mask;
        removed_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if removed_key.active_entries != observation_key.active_entries
            || removed_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || removed_key.scope_generations != observation_key.scope_generations
            || removed_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            removed_key,
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
            self.frontier_state.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let Some((slot_idx, old_entry_idx, new_entry_idx)) =
            Self::cached_entry_slot_replace(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        let Some(observed) = self
            .offer_entry_observed_state_cached(new_entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(new_entry_idx))
        else {
            return false;
        };
        if !cached_observed_entries.replace_entry_at_slot(old_entry_idx, new_entry_idx, observed) {
            return false;
        }
        let mut replaced_key = cached_key;
        replaced_key.active_entries[slot_idx] = observation_key.active_entries[slot_idx];
        replaced_key.entry_summary_fingerprints[slot_idx] =
            observation_key.entry_summary_fingerprints[slot_idx];
        replaced_key.scope_generations[slot_idx] = observation_key.scope_generations[slot_idx];
        replaced_key.route_change_epochs[slot_idx] = observation_key.route_change_epochs[slot_idx];
        if replaced_key.active_entries != observation_key.active_entries
            || replaced_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || replaced_key.scope_generations != observation_key.scope_generations
            || replaced_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            replaced_key,
            cached_observed_entries,
        );
        true
    }

    pub(super) fn cached_entry_slot_move(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<(usize, usize)> {
        let new_slot_idx = active_entries.slot_for_entry(entry_idx)?;
        let len = active_entries.len as usize;
        let entry = checked_state_index(entry_idx)?;
        let mut old_slot_idx = 0usize;
        while old_slot_idx < len {
            if cached_entries[old_slot_idx] == entry {
                break;
            }
            old_slot_idx += 1;
        }
        if old_slot_idx >= len || old_slot_idx == new_slot_idx {
            return None;
        }
        let mut shifted = cached_entries;
        Self::move_slot_in_array(&mut shifted, len, old_slot_idx, new_slot_idx);
        if shifted[..len] != active_entries.entries[..len] {
            return None;
        }
        Some((old_slot_idx, new_slot_idx))
    }

    pub(super) fn cached_entry_slot_insert(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<usize> {
        let insert_slot_idx = active_entries.slot_for_entry(entry_idx)?;
        let len = active_entries.len as usize;
        if len == 0 {
            return None;
        }
        let cached_len = len - 1;
        let entry = checked_state_index(entry_idx)?;
        let mut slot_idx = 0usize;
        while slot_idx < cached_len {
            if cached_entries[slot_idx] == entry {
                return None;
            }
            slot_idx += 1;
        }
        let mut inserted = cached_entries;
        Self::insert_slot_in_array(&mut inserted, cached_len, insert_slot_idx, entry);
        if inserted[..len] != active_entries.entries[..len] {
            return None;
        }
        Some(insert_slot_idx)
    }

    pub(super) fn cached_entry_slot_remove(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<usize> {
        let len = active_entries.len as usize;
        if len >= MAX_LANES {
            return None;
        }
        let cached_len = len + 1;
        let entry = u16::try_from(entry_idx).ok()?;
        let mut removed_slot_idx = 0usize;
        while removed_slot_idx < cached_len {
            if cached_entries[removed_slot_idx] == entry {
                break;
            }
            removed_slot_idx += 1;
        }
        if removed_slot_idx >= cached_len {
            return None;
        }
        let mut removed = cached_entries;
        Self::remove_slot_from_array(&mut removed, cached_len, removed_slot_idx, StateIndex::MAX);
        if removed[..len] != active_entries.entries[..len] {
            return None;
        }
        Some(removed_slot_idx)
    }

    pub(super) fn cached_entry_slot_replace(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<(usize, usize, usize)> {
        let len = active_entries.len as usize;
        if len == 0 {
            return None;
        }
        let entry = u16::try_from(entry_idx).ok()?;
        let mut replaced_slot_idx = None;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let cached_entry = cached_entries[slot_idx];
            let active_entry = active_entries.entries[slot_idx];
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
        let old_entry_idx = state_index_to_usize(cached_entries[slot_idx]);
        let new_entry_idx = state_index_to_usize(active_entries.entries[slot_idx]);
        Some((slot_idx, old_entry_idx, new_entry_idx))
    }

    #[inline]
    pub(super) fn cached_active_entries_len(cached_entries: [StateIndex; MAX_LANES]) -> usize {
        cached_entries
            .iter()
            .position(|entry| entry.is_max())
            .unwrap_or(MAX_LANES)
    }

    #[inline]
    pub(super) fn cached_active_entries_contains(
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let len = Self::cached_active_entries_len(cached_entries);
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if cached_entries[slot_idx] == entry {
                return true;
            }
            slot_idx += 1;
        }
        false
    }

    pub(super) fn structural_inserted_entry_idx(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len + 1 {
            return None;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut inserted = None;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if Self::cached_active_entries_contains(cached_entries, entry_idx) {
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
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if cached_len != active_len + 1 {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut removed = None;
        while slot_idx < cached_len {
            let entry_idx = state_index_to_usize(cached_entries[slot_idx]);
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
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len {
            return None;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut inserted = None;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if Self::cached_active_entries_contains(cached_entries, entry_idx) {
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
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut shifted = None;
        while slot_idx < active_len {
            let entry_idx = state_index_to_usize(active_entries.entries[slot_idx]);
            if !Self::cached_active_entries_contains(cached_entries, entry_idx) {
                return None;
            }
            if cached_entries[slot_idx] != active_entries.entries[slot_idx] {
                shifted.get_or_insert(entry_idx);
            }
            slot_idx += 1;
        }
        shifted
    }

    pub(super) fn same_active_entry_set(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> bool {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len {
            return false;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            if !Self::cached_active_entries_contains(cached_entries, entry_idx) {
                return false;
            }
        }
        true
    }

    pub(super) fn move_slot_in_array<V: Copy>(
        array: &mut [V; MAX_LANES],
        len: usize,
        old_slot_idx: usize,
        new_slot_idx: usize,
    ) {
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
        array: &mut [V; MAX_LANES],
        len: usize,
        slot_idx: usize,
        value: V,
    ) {
        if len >= MAX_LANES || slot_idx > len {
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
        array: &mut [V; MAX_LANES],
        len: usize,
        slot_idx: usize,
        fill: V,
    ) {
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
            self.frontier_state.global_active_entries
        };
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
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
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                continue;
            };
            if entry_state.active_mask == 0 || entry_state.scope_id != scope_id {
                continue;
            }
            if cached_key.scope_generations[slot_idx] == scope_generation {
                continue;
            }
            if cached_key.entry_summary_fingerprints[slot_idx]
                != entry_state.summary.observation_fingerprint()
            {
                return;
            }
            let lane_idx = entry_state.lane_idx as usize;
            if lane_idx >= MAX_LANES {
                return;
            }
            let route_change_epoch = self.ports[lane_idx]
                .as_ref()
                .map(Port::route_change_epoch)
                .unwrap_or(0);
            if cached_key.route_change_epochs[slot_idx] != route_change_epoch {
                return;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return;
            }
            cached_key.scope_generations[slot_idx] = scope_generation;
            patched = true;
        }
        if !patched {
            return;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
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
            self.frontier_state.global_active_entries
        };
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
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
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                return;
            };
            if entry_state.active_mask == 0
                || cached_key.entry_summary_fingerprints[slot_idx]
                    != entry_state.summary.observation_fingerprint()
                || cached_key.scope_generations[slot_idx]
                    != self.scope_evidence_generation_for_scope(entry_state.scope_id)
            {
                return;
            }
            let representative_lane = entry_state.lane_idx as usize;
            if representative_lane >= MAX_LANES {
                return;
            }
            let route_change_epoch = self.ports[representative_lane]
                .as_ref()
                .map(Port::route_change_epoch)
                .unwrap_or(0);
            if cached_key.route_change_epochs[slot_idx] != route_change_epoch {
                return;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return;
            }
        }
        cached_key.binding_nonempty_mask = binding_nonempty_mask;
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
    }

    pub(super) fn refresh_cached_frontier_observation_route_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_change_epoch: u32,
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
            self.frontier_state.global_active_entries
        };
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
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
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                continue;
            };
            if entry_state.active_mask == 0 || entry_state.lane_idx as usize != lane_idx {
                continue;
            }
            if cached_key.entry_summary_fingerprints[slot_idx]
                != entry_state.summary.observation_fingerprint()
                || cached_key.scope_generations[slot_idx]
                    != self.scope_evidence_generation_for_scope(entry_state.scope_id)
            {
                return;
            }
            if cached_key.route_change_epochs[slot_idx] == route_change_epoch {
                continue;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return;
            }
            cached_key.route_change_epochs[slot_idx] = route_change_epoch;
            patched = true;
        }
        if !patched {
            return;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
    }

    #[inline]
    pub(super) fn refresh_frontier_observation_cache_for_scope(&mut self, scope_id: ScopeId) {
        let mut active_entries = self.frontier_state.global_active_entries.occupancy_mask();
        let mut roots = [ScopeId::none(); MAX_LANES];
        let mut root_len = 0usize;
        let mut matches_scope = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut active_entries) {
            let Some(entry_idx) = self.frontier_state.global_active_entries.entry_at(slot_idx)
            else {
                continue;
            };
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                continue;
            };
            if entry_state.active_mask == 0 || entry_state.scope_id != scope_id {
                continue;
            }
            matches_scope = true;
            if entry_state.parallel_root.is_none() {
                continue;
            }
            let mut seen_root = false;
            let mut idx = 0usize;
            while idx < root_len {
                if roots[idx] == entry_state.parallel_root {
                    seen_root = true;
                    break;
                }
                idx += 1;
            }
            if !seen_root && root_len < MAX_LANES {
                roots[root_len] = entry_state.parallel_root;
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
        while slot_idx < self.frontier_state.root_frontier_len as usize {
            if self.frontier_state.root_frontier_state[slot_idx].offer_lane_entry_slot_masks
                [lane_idx]
                != 0
            {
                let root = self.frontier_state.root_frontier_state[slot_idx].root;
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
        previous_change_epoch: u32,
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
        while slot_idx < self.frontier_state.root_frontier_len as usize {
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
            self.frontier_state.global_active_entries
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
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
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
        let entry_state = self
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()?;
        if entry_state.active_mask == 0 {
            return None;
        }
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            self.preview_offer_entry_evidence_non_consuming(entry_state);
        let (observed, _) = self.offer_entry_candidate_from_observation(
            entry_idx,
            entry_state,
            binding_ready,
            has_ack,
            has_ready_arm_evidence,
        );
        self.frontier_state
            .set_offer_entry_observed(entry_idx, observed);
        Some(observed)
    }

    #[inline]
    pub(super) fn offer_entry_observed_state_cached(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        let state = self.frontier_state.offer_entry_state.get(entry_idx)?;
        if state.active_mask == 0 || state.observed.scope_id != state.scope_id {
            return None;
        }
        Some(state.observed)
    }

    pub(super) fn cached_frontier_changed_entry_slot_mask(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != observation_key.active_entries
        {
            return None;
        }
        let mut changed_slot_mask = 0u8;
        let mut slot_idx = 0usize;
        while slot_idx < MAX_LANES {
            if observation_key.active_entries[slot_idx].is_max() {
                break;
            }
            if cached_key.entry_summary_fingerprints[slot_idx]
                != observation_key.entry_summary_fingerprints[slot_idx]
                || cached_key.scope_generations[slot_idx]
                    != observation_key.scope_generations[slot_idx]
                || cached_key.route_change_epochs[slot_idx]
                    != observation_key.route_change_epochs[slot_idx]
            {
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
        let mut refreshed = cached_observed_entries;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut changed_slot_mask) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return None;
            };
            if !refreshed.replace_observation(entry_idx, observed) {
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
        let mut composed = ObservedEntrySet::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(_slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(_slot_idx) else {
                continue;
            };
            let Some(entry_state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            else {
                continue;
            };
            if entry_state.active_mask == 0 {
                continue;
            }
            let observed = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    entry_state,
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
            composed.observe(observed_bit, observed);
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
        entry_state: OfferEntryState,
        cached_slot_idx: usize,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        if cached_slot_idx >= MAX_LANES
            || cached_key.active_entries[cached_slot_idx] != entry
            || cached_key.active_entries[cached_slot_idx].is_max()
            || cached_key.entry_summary_fingerprints[cached_slot_idx]
                != entry_state.summary.observation_fingerprint()
            || cached_key.scope_generations[cached_slot_idx]
                != self.scope_evidence_generation_for_scope(entry_state.scope_id)
        {
            return false;
        }
        let changed_binding_mask =
            cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask;
        if (changed_binding_mask & entry_state.offer_lane_mask) != 0 {
            return false;
        }
        if cached_key.route_change_epochs[cached_slot_idx]
            != observation_key.route_change_epochs[cached_slot_idx]
        {
            return false;
        }
        true
    }

    #[inline]
    pub(super) fn cached_offer_entry_observed_state_for_rebuild(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
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
        Some(cached_offer_entry_observed_state(
            entry_state.scope_id,
            entry_state.summary,
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
    pub(in crate::endpoint::kernel) fn next_frontier_observation_epoch(&mut self) -> u32 {
        self.frontier_state.next_observation_epoch()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_entries(&self) -> ObservedEntrySet {
        self.frontier_state.global_frontier_observed_entries()
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
