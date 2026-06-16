use super::{
    CursorEndpoint, FrontierObservationDomain, FrontierObservationKey, FrontierObservationSlot,
    ObservedEntrySet, ScopeId, Transport, checked_state_index,
    frontier_observed_entries_view_from_storage,
    frontier_working_observation_key_view_from_storage, lane_port, state_index_to_usize,
};
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn refresh_cached_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let Some(slot_idx) = active_entries.slot_for_entry(entry_idx) else {
            return false;
        };
        if self.offer_entry_state_snapshot(entry_idx).is_none() {
            return false;
        }
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
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
        if !self.recompute_offer_entry_observation_with_frontier_mask(
            &mut cached_observed_entries,
            entry_idx,
        ) {
            return false;
        }
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
        true
    }

    #[inline]
    pub(super) fn working_frontier_observation_cache(
        &mut self,
        domain: FrontierObservationDomain,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        let (cached_key, cached_observed_entries) = Self::frontier_observation_cache(self, domain);
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

    pub(super) fn refresh_shifted_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let observation_key = Self::frontier_observation_key(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let Some((source_slot_idx, new_slot_idx)) =
            CursorEndpoint::<ROLE, T>::cached_entry_slot_move(
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
        CursorEndpoint::<ROLE, T>::move_slot_in_array(
            &mut cached_key.slots,
            active_entries.len(),
            source_slot_idx,
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
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
        true
    }

    pub(super) fn refresh_inserted_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        if self.offer_entry_state_snapshot(entry_idx).is_none() {
            return false;
        }
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return false;
        }
        let observation_key = Self::frontier_observation_key(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(insert_slot_idx) = CursorEndpoint::<ROLE, T>::cached_entry_slot_insert(
            active_entries,
            cached_key,
            entry_idx,
        ) else {
            return false;
        };
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut changed_external_lane = false;
        let mut check_lane = |lane_idx: usize| {
            let entry_owns_lane = active_offer_lanes.contains(lane_idx)
                && state_index_to_usize(self.decision_state.lane_offer_state(lane_idx).entry)
                    == entry_idx;
            if !entry_owns_lane
                && cached_key.offer_lanes().contains(lane_idx)
                    != observation_key.offer_lanes().contains(lane_idx)
            {
                changed_external_lane = true;
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(observation_key.offer_lanes(), lane_limit, &mut check_lane);
        if changed_external_lane {
            return false;
        }
        let len = cached_observed_entries.len();
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        CursorEndpoint::<ROLE, T>::insert_slot_in_array(
            &mut cached_key.slots,
            len,
            insert_slot_idx,
            FrontierObservationSlot {
                entry,
                meta: observation_key.slot(insert_slot_idx),
            },
        );
        cached_key.set_offer_lanes(observation_key.offer_lanes());
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
            self.offer_entry_frontier_mask(entry_idx),
        ) {
            return false;
        }
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
        true
    }

    pub(super) fn refresh_detached_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let observation_key = Self::frontier_observation_key(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(detached_slot_idx) = CursorEndpoint::<ROLE, T>::detached_cached_entry_slot(
            active_entries,
            cached_key,
            entry_idx,
        ) else {
            return false;
        };
        let slot_masks = Self::frontier_observation_offer_lane_entry_slot_masks(self, domain);
        let lane_limit = self.cursor.logical_lane_count();
        let mut changed_slotted_lane = false;
        let mut check_lane = |lane_idx: usize| {
            if cached_key.offer_lanes().contains(lane_idx)
                != observation_key.offer_lanes().contains(lane_idx)
                && slot_masks[lane_idx] != 0
            {
                changed_slotted_lane = true;
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut check_lane);
        Self::for_each_set_lane(observation_key.offer_lanes(), lane_limit, &mut check_lane);
        if changed_slotted_lane {
            return false;
        }
        if !cached_observed_entries.remove_observation(entry_idx) {
            return false;
        }
        let cached_len = cached_key.len();
        CursorEndpoint::<ROLE, T>::remove_slot_from_array(
            &mut cached_key.slots,
            cached_len,
            detached_slot_idx,
            FrontierObservationSlot::EMPTY,
        );
        cached_key.set_offer_lanes(observation_key.offer_lanes());
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
        true
    }

    pub(super) fn refresh_replaced_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let observation_key = Self::frontier_observation_key(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.lane_sets_equal(&observation_key)
        {
            return false;
        }
        let Some((slot_idx, source_entry_idx, new_entry_idx)) =
            CursorEndpoint::<ROLE, T>::cached_entry_slot_replace(
                active_entries,
                cached_key,
                entry_idx,
            )
        else {
            return false;
        };
        let Some(observed) = self
            .offer_entry_observed_state_cached(new_entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(new_entry_idx))
        else {
            return false;
        };
        if self.offer_entry_state_snapshot(new_entry_idx).is_none() {
            return false;
        }
        if !cached_observed_entries.replace_entry_at_slot_with_frontier_mask(
            source_entry_idx,
            new_entry_idx,
            FrontierObservationSlot {
                entry: observation_key.entry_state(slot_idx),
                meta: observation_key.slot(slot_idx),
            },
            observed,
            self.offer_entry_frontier_mask(new_entry_idx),
        ) {
            return false;
        }
        cached_key.slots[slot_idx].entry = observation_key.entry_state(slot_idx);
        *cached_key.slot_mut(slot_idx) = observation_key.slot(slot_idx);
        if !cached_key.entries_equal(&observation_key) || cached_key.slots != observation_key.slots
        {
            return false;
        }
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
        true
    }

    pub(super) fn refresh_cached_frontier_observation_scope_entries(
        &mut self,
        domain: FrontierObservationDomain,
        scope_id: ScopeId,
    ) {
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(self, domain);
        if !cached_key.lane_sets_equal(&observation_key) {
            return;
        }
        let scope_generation = self.scope_evidence_generation_for_scope(scope_id);
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T>::next_slot_in_mask(&mut remaining_entries)
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            if self.offer_entry_state_snapshot(entry_idx).is_none() {
                continue;
            }
            if !self.offer_entry_has_active_lanes(entry_idx)
                || self.offer_entry_scope_id(entry_idx) != scope_id
            {
                continue;
            }
            if cached_key.slot(slot_idx).scope_generation == scope_generation {
                continue;
            }
            let summary = self.compute_offer_entry_summary(entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
            {
                return;
            }
            let Some(lane_idx) = self.offer_entry_representative_lane_idx(entry_idx) else {
                return;
            };
            let route_change_generation = self.port_for_lane(lane_idx).route_change_generation();
            if cached_key.slot(slot_idx).route_change_generation != route_change_generation {
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
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
    }

    pub(super) fn refresh_cached_frontier_observation_route_lane_entries(
        &mut self,
        domain: FrontierObservationDomain,
        lane_idx: usize,
        captured_change_generation: u16,
    ) {
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        let route_change_generation = self.port_for_lane(lane_idx).route_change_generation();
        if route_change_generation == captured_change_generation {
            return;
        }
        let active_entries = Self::frontier_observation_active_entries(self, domain);
        let (mut cached_key, mut cached_observed_entries) =
            self.working_frontier_observation_cache(domain);
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.exact_entries_match(active_entries)
        {
            return;
        }
        let observation_key = Self::frontier_observation_key(self, domain);
        if !cached_key.lane_sets_equal(&observation_key) {
            return;
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T>::next_slot_in_mask(&mut remaining_entries)
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            if self.offer_entry_state_snapshot(entry_idx).is_none() {
                continue;
            }
            if !self.offer_entry_has_active_lanes(entry_idx)
                || self.offer_entry_representative_lane_idx(entry_idx) != Some(lane_idx)
            {
                continue;
            }
            let summary = self.compute_offer_entry_summary(entry_idx);
            if cached_key.slot(slot_idx).entry_summary_fingerprint
                != summary.observation_fingerprint()
                || cached_key.slot(slot_idx).scope_generation
                    != self
                        .scope_evidence_generation_for_scope(self.offer_entry_scope_id(entry_idx))
            {
                return;
            }
            if cached_key.slot(slot_idx).route_change_generation == route_change_generation {
                continue;
            }
            if !self.recompute_offer_entry_observation_with_frontier_mask(
                &mut cached_observed_entries,
                entry_idx,
            ) {
                return;
            }
            cached_key.slot_mut(slot_idx).route_change_generation = route_change_generation;
            patched = true;
        }
        if !patched {
            return;
        }
        self.advance_frontier_observation_generation();
        Self::store_frontier_observation(self, domain, cached_key, cached_observed_entries);
    }
}
