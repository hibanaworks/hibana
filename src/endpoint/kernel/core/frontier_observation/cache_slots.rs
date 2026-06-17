use super::super::super::frontier::{
    GlobalFrontierObservedState, frontier_cached_observation_key_view_from_storage,
    frontier_global_observed_state_ptr_from_storage,
};
use super::{
    ActiveEntrySet, CursorEndpoint, FrontierKind, FrontierObservationDomain,
    FrontierObservationKey, ObservedEntrySet, OfferEntryObservedState, ScopeId,
    cached_offer_entry_observed_state, checked_state_index, state_index_to_usize,
};
use crate::endpoint::kernel::offer::CurrentReentryControllerEvidence;
use crate::transport::Transport;
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
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

    pub(in crate::endpoint::kernel) fn structural_inserted_entry_idx(
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
        while let Some(slot_idx) = Self::next_slot_in_mask(&mut remaining_slots) {
            let entry_idx = active_entries.entry_at(slot_idx)?;
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

    pub(in crate::endpoint::kernel) fn structural_detached_entry_idx(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> Option<usize> {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if cached_len != active_len + 1 {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut detached_entry = None;
        while slot_idx < cached_len {
            let entry_idx = state_index_to_usize(cached_key.entry_state(slot_idx));
            if active_entries.slot_for_entry(entry_idx).is_some() {
                slot_idx += 1;
                continue;
            }
            if detached_entry.is_some() {
                return None;
            }
            detached_entry = Some(entry_idx);
            slot_idx += 1;
        }
        detached_entry
    }

    pub(in crate::endpoint::kernel) fn structural_replaced_entry_idx(
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
        while let Some(slot_idx) = Self::next_slot_in_mask(&mut remaining_slots) {
            let entry_idx = active_entries.entry_at(slot_idx)?;
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

    pub(in crate::endpoint::kernel) fn structural_shifted_entry_idx(
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

    pub(in crate::endpoint::kernel) fn same_active_entry_set(
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        let active_len = active_entries.len();
        let cached_len = Self::cached_active_entries_len(cached_key);
        if active_len != cached_len {
            return false;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_slot_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            if !Self::cached_active_entries_contains(cached_key, entry_idx) {
                return false;
            }
        }
        true
    }

    pub(in crate::endpoint::kernel) fn move_slot_in_array<V: Copy>(
        array: &mut [V],
        len: usize,
        source_slot_idx: usize,
        new_slot_idx: usize,
    ) {
        if len > array.len() {
            crate::invariant();
        }
        if source_slot_idx == new_slot_idx || source_slot_idx >= len || new_slot_idx >= len {
            return;
        }
        let value = array[source_slot_idx];
        if source_slot_idx < new_slot_idx {
            let mut slot_idx = source_slot_idx;
            while slot_idx < new_slot_idx {
                array[slot_idx] = array[slot_idx + 1];
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = source_slot_idx;
            while slot_idx > new_slot_idx {
                array[slot_idx] = array[slot_idx - 1];
                slot_idx -= 1;
            }
        }
        array[new_slot_idx] = value;
    }

    pub(in crate::endpoint::kernel) fn insert_slot_in_array<V: Copy>(
        array: &mut [V],
        len: usize,
        slot_idx: usize,
        value: V,
    ) {
        if len > array.len() {
            crate::invariant();
        }
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

    pub(in crate::endpoint::kernel) fn remove_slot_from_array<V: Copy>(
        array: &mut [V],
        len: usize,
        slot_idx: usize,
        fill: V,
    ) {
        if len > array.len() {
            crate::invariant();
        }
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

    #[inline]
    pub(in crate::endpoint::kernel) fn recompute_offer_entry_observed_state_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return None;
        }
        let evidence = self.preview_offer_entry_evidence_non_consuming(entry_idx);
        let (observed, _) = self.offer_entry_candidate_from_observation(entry_idx, evidence);
        Some(observed)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_observed_state_cached(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return None;
        }
        let parallel_root = match self.offer_entry_parallel_root(entry_idx) {
            Some(root) => root,
            None => ScopeId::none(),
        };
        let domain = if parallel_root.is_none() {
            FrontierObservationDomain::global()
        } else {
            FrontierObservationDomain::root(parallel_root)
        };
        let (_, cached_observed_entries) = self.frontier_observation_cache_snapshot(domain);
        let cached_bit = cached_observed_entries.entry_bit(entry_idx);
        if cached_bit == 0 {
            return None;
        }
        let summary = self.compute_offer_entry_summary(entry_idx);
        Some(cached_offer_entry_observed_state(
            self.offer_entry_scope_id(entry_idx),
            summary,
            cached_observed_entries,
            cached_bit,
        ))
    }

    #[inline]
    pub(super) fn frontier_observation_entry_reusable(
        &self,
        entry_idx: usize,
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
        if cached_slot_idx >= cached_key.len()
            || cached_key.entry_state(cached_slot_idx) != entry
            || cached_key.entry_state(cached_slot_idx).is_absent()
            || observation_key
                .slot(observation_slot_idx)
                .entry_summary_fingerprint
                != self
                    .compute_offer_entry_summary(entry_idx)
                    .observation_fingerprint()
            || observation_key.slot(observation_slot_idx).scope_generation
                != self.scope_evidence_generation_for_scope(self.offer_entry_scope_id(entry_idx))
        {
            return false;
        }
        if !cached_key.lane_sets_equal(&observation_key) {
            return false;
        }
        let Some(representative_lane) = self.offer_entry_representative_lane_idx(entry_idx) else {
            return false;
        };
        if observation_key
            .slot(observation_slot_idx)
            .route_change_generation
            != self
                .port_for_lane(representative_lane)
                .route_change_generation()
        {
            return false;
        }
        true
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn reusable_cached_offer_entry_observed_state(
        &self,
        entry_idx: usize,
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
            cached_slot_idx,
            observation_key,
            cached_key,
        ) {
            return None;
        }
        let summary = self.compute_offer_entry_summary(entry_idx);
        Some(cached_offer_entry_observed_state(
            self.offer_entry_scope_id(entry_idx),
            summary,
            cached_observed_entries,
            cached_bit,
        ))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn advance_frontier_observation_generation(&mut self) {
        let next = self
            .global_frontier_observed_state()
            .observation_generation
            .wrapping_add(1);
        if next == 0 {
            if self
                .frontier_state
                .global_frontier_scratch_state
                .is_initialized()
            {
                let (scratch_ptr, layout, frontier_entry_capacity) =
                    self.global_frontier_scratch_parts();
                let mut cached_key = frontier_cached_observation_key_view_from_storage(
                    scratch_ptr,
                    layout,
                    frontier_entry_capacity,
                );
                cached_key.clear();
                /* SAFETY: frontier scratch has not been initialized yet in
                this branch. The observed-state cell is written after clearing
                cached-key scratch and before the initialized flag is stored. */
                unsafe {
                    frontier_global_observed_state_ptr_from_storage(scratch_ptr, layout).write(
                        GlobalFrontierObservedState {
                            observation_generation: 1,
                            ..GlobalFrontierObservedState::EMPTY
                        },
                    );
                }
            } else {
                self.global_frontier_observed_state_mut()
                    .observation_generation = 1;
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
        } else {
            self.global_frontier_observed_state_mut()
                .observation_generation = next;
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_observed_entries(&self) -> ObservedEntrySet {
        let cached_key = self.cached_global_frontier_observation_key();
        cached_key.observed_entries(self.global_frontier_observed_state().summary)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observed_entries(
        &self,
        domain: FrontierObservationDomain,
    ) -> ObservedEntrySet {
        if domain.uses_root_entries() {
            self.root_frontier_observed_entries(domain.root_scope())
        } else {
            self.global_frontier_observed_entries()
        }
    }

    pub(in crate::endpoint::kernel) fn root_frontier_progress_sibling_exists(
        &self,
        root: ScopeId,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        reentry_controller_evidence: CurrentReentryControllerEvidence,
    ) -> bool {
        self.observed_frontier_progress_sibling_exists(
            self.root_frontier_observed_entries(root),
            current_entry_idx,
            current_frontier,
            reentry_controller_evidence,
        )
    }

    pub(in crate::endpoint::kernel) fn global_frontier_progress_sibling_exists(
        &self,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        reentry_controller_evidence: CurrentReentryControllerEvidence,
    ) -> bool {
        self.observed_frontier_progress_sibling_exists(
            self.global_frontier_observed_entries(),
            current_entry_idx,
            current_frontier,
            reentry_controller_evidence,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_frontier_progress_sibling_exists(
        &self,
        observed_entries: ObservedEntrySet,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        reentry_controller_evidence: CurrentReentryControllerEvidence,
    ) -> bool {
        let mut sibling_mask = observed_entries.progress_mask;
        sibling_mask &= !observed_entries.entry_bit(current_entry_idx);
        if !reentry_controller_evidence.allows_cross_frontier_progress_sibling() {
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
