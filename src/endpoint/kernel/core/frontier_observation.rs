mod cache_slots;

#[cfg(not(test))]
use super::super::frontier::{
    GlobalFrontierObservedState, frontier_global_observed_state_ptr_from_storage,
};
use super::{
    ActiveEntrySet, CursorEndpoint, EndpointSlot, EpochTable, FrontierKind,
    FrontierObservationDomain, FrontierObservationKey, FrontierScratchLayout, LabelUniverse,
    MintConfigMarker, ObservedEntrySet, OfferEntryObservedState, OfferEntryState, Port, ScopeId,
    Transport, cached_offer_entry_observed_state, checked_state_index,
    frontier_cached_observation_key_view_from_storage,
    frontier_global_active_entries_view_from_storage, frontier_observed_entries_view_from_storage,
    lane_port, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_scratch_parts(
        &self,
    ) -> (*mut [u8], FrontierScratchLayout, usize) {
        let port = self.port_for_lane(self.primary_lane);
        (
            lane_port::frontier_scratch_ptr(port),
            self.cursor.frontier_scratch_layout(),
            self.cursor.max_frontier_entries(),
        )
    }

    #[cfg(not(test))]
    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_state(
        &self,
    ) -> GlobalFrontierObservedState {
        if !self.frontier_state.global_frontier_scratch_initialized {
            return GlobalFrontierObservedState::EMPTY;
        }
        let (scratch_ptr, layout, _) = self.global_frontier_scratch_parts();
        /* SAFETY: frontier observation storage is carved from the endpoint scratch layout at checked aligned offsets. */
        unsafe { *frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout) }
    }

    #[cfg(not(test))]
    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_state_mut(
        &mut self,
    ) -> &mut GlobalFrontierObservedState {
        self.init_global_frontier_scratch_if_needed();
        let (scratch_ptr, layout, _) = self.global_frontier_scratch_parts();
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe { &mut *frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout) }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn init_global_frontier_scratch_if_needed(&mut self) {
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
        }
        #[cfg(not(test))]
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
    pub(in crate::endpoint::kernel) fn cached_global_frontier_observation_key(
        &self,
    ) -> FrontierObservationKey {
        if !self.frontier_state.global_frontier_scratch_initialized {
            return FrontierObservationKey::EMPTY;
        }
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        let key = frontier_cached_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        key
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn empty_observed_entries_scratch(
        &mut self,
    ) -> ObservedEntrySet {
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
    pub(in crate::endpoint::kernel) fn cached_frontier_observed_entries(
        &self,
        domain: FrontierObservationDomain,
        key: FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        self.cached_frontier_observed_entries_ref(domain, &key)
    }

    #[inline]
    pub(super) fn cached_frontier_observed_entries_ref(
        &self,
        domain: FrontierObservationDomain,
        key: &FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        #[cfg(test)]
        {
            return self.frontier_state.cached_frontier_observed_entries(
                domain,
                self.cached_global_frontier_observation_key(),
                key,
            );
        }
        #[cfg(not(test))]
        {
            if domain.uses_root_entries() {
                let slot_idx = self
                    .frontier_state
                    .root_frontier_slot(domain.root_scope())?;
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
    pub(in crate::endpoint::kernel) fn frontier_observation_cache_snapshot(
        &self,
        domain: FrontierObservationDomain,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        #[cfg(test)]
        {
            return self
                .frontier_state
                .frontier_observation_cache(domain, self.cached_global_frontier_observation_key());
        }
        #[cfg(not(test))]
        {
            if domain.uses_root_entries() {
                let Some(slot_idx) = self.frontier_state.root_frontier_slot(domain.root_scope())
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
    pub(in crate::endpoint::kernel) fn write_frontier_observation_snapshot(
        &mut self,
        domain: FrontierObservationDomain,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        #[cfg(test)]
        {
            if !domain.uses_root_entries() {
                self.init_global_frontier_scratch_if_needed();
            }
            let cached_global_key = self.cached_global_frontier_observation_key();
            self.frontier_state.store_frontier_observation(
                domain,
                cached_global_key,
                key,
                observed_entries,
            );
        }
        #[cfg(not(test))]
        {
            if domain.uses_root_entries() {
                let Some(slot_idx) = self.frontier_state.root_frontier_slot(domain.root_scope())
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
            self.init_global_frontier_scratch_if_needed();
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
        }
    }

    pub(in crate::endpoint::kernel) fn replace_offer_entry_observation_with_frontier_mask(
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

    pub(in crate::endpoint::kernel) fn recompute_offer_entry_observation_with_frontier_mask(
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

    pub(in crate::endpoint::kernel) fn cached_entry_slot_move(
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
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let shifted = if slot_idx == new_slot_idx {
                cached_key.entry_state(old_slot_idx)
            } else if old_slot_idx < new_slot_idx
                && slot_idx >= old_slot_idx
                && slot_idx < new_slot_idx
            {
                cached_key.entry_state(slot_idx + 1)
            } else if old_slot_idx > new_slot_idx
                && slot_idx > new_slot_idx
                && slot_idx <= old_slot_idx
            {
                cached_key.entry_state(slot_idx - 1)
            } else {
                cached_key.entry_state(slot_idx)
            };
            if active_entries.entry_state(slot_idx) != shifted {
                return None;
            }
            slot_idx += 1;
        }
        Some((old_slot_idx, new_slot_idx))
    }

    pub(in crate::endpoint::kernel) fn cached_entry_slot_insert(
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
        let mut active_slot_idx = 0usize;
        while active_slot_idx < len {
            let inserted = if active_slot_idx == insert_slot_idx {
                entry
            } else if active_slot_idx < insert_slot_idx {
                cached_key.entry_state(active_slot_idx)
            } else {
                cached_key.entry_state(active_slot_idx - 1)
            };
            if active_entries.entry_state(active_slot_idx) != inserted {
                return None;
            }
            active_slot_idx += 1;
        }
        Some(insert_slot_idx)
    }

    pub(in crate::endpoint::kernel) fn cached_entry_slot_remove(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<usize> {
        let len = active_entries.len();
        let cached_len = len + 1;
        let entry = checked_state_index(entry_idx)?;
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
        let mut active_slot_idx = 0usize;
        while active_slot_idx < len {
            let removed = if active_slot_idx < removed_slot_idx {
                cached_key.entry_state(active_slot_idx)
            } else {
                cached_key.entry_state(active_slot_idx + 1)
            };
            if active_entries.entry_state(active_slot_idx) != removed {
                return None;
            }
            active_slot_idx += 1;
        }
        Some(removed_slot_idx)
    }

    pub(in crate::endpoint::kernel) fn cached_entry_slot_replace(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> Option<(usize, usize, usize)> {
        let len = active_entries.len();
        if len == 0 {
            return None;
        }
        let entry = checked_state_index(entry_idx)?;
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
}
