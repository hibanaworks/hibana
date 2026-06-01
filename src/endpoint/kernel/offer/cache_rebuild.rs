use super::{
    ActiveEntrySet, Clock, CursorEndpoint, EndpointSlot, EpochTable, FrontierObservationDomain,
    FrontierObservationKey, FrontierObservationSlot, LabelUniverse, MintConfigMarker,
    ObservedEntrySet, ScopeId, Transport,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    pub(in crate::endpoint::kernel) fn refresh_frontier_observed_entries_from_cache(
        &mut self,
        domain: FrontierObservationDomain,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        let mut changed_slot_mask =
            self.cached_frontier_changed_entry_slot_mask(domain, observation_key, cached_key)?;
        if changed_slot_mask == 0 {
            return Some(cached_observed_entries);
        }
        let mut refreshed = self.empty_observed_entries_scratch();
        refreshed.copy_from(cached_observed_entries);
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut changed_slot_mask,
            )
        {
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
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.offer_entry_has_active_lanes(entry_idx) {
                continue;
            }
            let observed = Self::cached_offer_entry_observed_state_for_rebuild(
                self,
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

    pub(super) fn refresh_frontier_observed_entries(
        &mut self,
        domain: FrontierObservationDomain,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        if let Some(refreshed) = self.refresh_frontier_observed_entries_from_cache(
            domain,
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

    pub(super) fn refresh_frontier_observation_cache_from_cached_entries(
        &mut self,
        domain: FrontierObservationDomain,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let observation_key = Self::frontier_observation_key(self, domain);
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(self, domain);
        let Some(observed_entries) = self.refresh_frontier_observed_entries_from_cache(
            domain,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) else {
            return false;
        };
        let _ = self.next_frontier_observation_epoch();
        Self::store_frontier_observation(self, domain, observation_key, observed_entries);
        true
    }

    pub(super) fn refresh_frontier_observation_cache_impl(
        &mut self,
        domain: FrontierObservationDomain,
    ) {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let observation_key = Self::frontier_observation_key(self, domain);
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(self, domain);
        if self.refresh_structural_frontier_observation_cache(domain, active_entries, cached_key) {
            return;
        }
        let observed_entries = self.refresh_frontier_observed_entries(
            domain,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        );
        let _ = self.next_frontier_observation_epoch();
        Self::store_frontier_observation(self, domain, observation_key, observed_entries);
    }

    pub(super) fn refresh_structural_frontier_observation_cache(
        &mut self,
        domain: FrontierObservationDomain,
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let active_len = active_entries.len();
        let cached_len = cached_key.len();
        if active_len == cached_len {
            if let Some(entry_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_replaced_entry_idx(
                    active_entries,
                    cached_key,
                )
                && self.refresh_replaced_frontier_observation_entry(domain, entry_idx)
            {
                return true;
            }
            if CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_shifted_entry_idx(
                active_entries,
                cached_key,
            )
            .is_some()
            {
                let mut remaining_slots = active_entries.occupancy_mask();
                while let Some(slot_idx) =
                    CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                        &mut remaining_slots,
                    )
                {
                    let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                        continue;
                    };
                    if active_entries.entry_state(slot_idx) == cached_key.entry_state(slot_idx) {
                        continue;
                    }
                    if self.refresh_shifted_frontier_observation_entry(domain, entry_idx) {
                        return true;
                    }
                }
            }
            if CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::same_active_entry_set(
                active_entries,
                cached_key,
            ) && self.refresh_permuted_frontier_observation_entries(domain, active_entries)
            {
                return true;
            }
            if self.refresh_multi_replaced_frontier_observation_entries(domain, active_entries) {
                return true;
            }
            return false;
        }
        if active_len + 1 == cached_len
            && let Some(entry_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_removed_entry_idx(
                    active_entries,
                    cached_key,
                )
            && self.refresh_removed_frontier_observation_entry(domain, entry_idx)
        {
            return true;
        }
        if active_len == cached_len + 1
            && let Some(entry_idx) =
                CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::structural_inserted_entry_idx(
                    active_entries,
                    cached_key,
                )
            && self.refresh_inserted_frontier_observation_entry(domain, entry_idx)
        {
            return true;
        }
        false
    }

    pub(super) fn refresh_permuted_frontier_observation_entries(
        &mut self,
        domain: FrontierObservationDomain,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key = Self::frontier_observation_key(self, domain);
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(self, domain);
        if cached_key == FrontierObservationKey::EMPTY
            || !CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::same_active_entry_set(
                active_entries,
                cached_key,
            )
        {
            return false;
        }
        let mut refreshed = self.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if !self.offer_entry_has_active_lanes(entry_idx) {
                return false;
            }
            let observed = Self::cached_offer_entry_observed_state_for_rebuild(
                self,
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
        Self::store_frontier_observation(self, domain, observation_key, refreshed);
        true
    }

    pub(super) fn refresh_multi_replaced_frontier_observation_entries(
        &mut self,
        domain: FrontierObservationDomain,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key = Self::frontier_observation_key(self, domain);
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(self, domain);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let active_len = active_entries.len();
        if active_len == 0
            || active_len != cached_key.len()
            || CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::same_active_entry_set(
                active_entries,
                cached_key,
            )
        {
            return false;
        }
        let mut refreshed = self.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut reused_cached = false;
        let mut recomputed = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                return false;
            };
            if !self.offer_entry_has_active_lanes(entry_idx) {
                return false;
            }
            let observed = if let Some(observed) =
                Self::cached_offer_entry_observed_state_for_rebuild(
                    self,
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
        Self::store_frontier_observation(self, domain, observation_key, refreshed);
        true
    }

    pub(super) fn refresh_frontier_observation_cache_for_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) {
        if self.refresh_single_frontier_observation_entry(domain, entry_idx) {
            return;
        }
        let (cached_key, _) = Self::frontier_observation_cache(self, domain);
        if cached_key == FrontierObservationKey::EMPTY {
            self.refresh_frontier_observation_cache_impl(domain);
            return;
        }
        if self.refresh_cached_frontier_observation_entry(domain, entry_idx)
            || self.refresh_frontier_observation_cache_from_cached_entries(domain)
            || self.refresh_replaced_frontier_observation_entry(domain, entry_idx)
            || self.refresh_removed_frontier_observation_entry(domain, entry_idx)
            || self.refresh_inserted_frontier_observation_entry(domain, entry_idx)
            || self.refresh_shifted_frontier_observation_entry(domain, entry_idx)
        {
            return;
        }
        self.refresh_frontier_observation_cache_impl(domain);
    }

    pub(super) fn refresh_single_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        if !active_entries.contains_only(entry_idx) {
            return false;
        }
        let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
            return false;
        };
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(self, domain);
        if !observation_key.exact_entries_match(active_entries) {
            return false;
        }
        let Some(observed) = self
            .offer_entry_observed_state_cached(entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
        else {
            return false;
        };
        let mut observed_entries = self.empty_observed_entries_scratch();
        if !observed_entries.insert_observation_at_slot_with_frontier_mask(
            entry_idx,
            0,
            FrontierObservationSlot {
                entry: observation_key.entry_state(0),
                meta: observation_key.slot(0),
            },
            observed,
            self.offer_entry_frontier_mask(entry_idx, entry_state),
        ) {
            return false;
        }
        let _ = self.next_frontier_observation_epoch();
        Self::store_frontier_observation(self, domain, observation_key, observed_entries);
        true
    }

    pub(super) fn refresh_frontier_observation_caches_for_entry(
        &mut self,
        entry_idx: usize,
        previous_root: ScopeId,
        current_root: ScopeId,
    ) {
        self.refresh_frontier_observation_cache_for_entry(
            FrontierObservationDomain::global(),
            entry_idx,
        );
        if !previous_root.is_none() {
            self.refresh_frontier_observation_cache_for_entry(
                FrontierObservationDomain::root(previous_root),
                entry_idx,
            );
        }
        if !current_root.is_none() && current_root != previous_root {
            self.refresh_frontier_observation_cache_for_entry(
                FrontierObservationDomain::root(current_root),
                entry_idx,
            );
        }
    }
}
