//! Mutable frontier-state owner for endpoint kernel runtime bookkeeping.

use super::frontier::{
    ActiveEntrySet, FrontierObservationKey, LaneOfferState, ObservedEntrySet, OfferEntryState,
    RootFrontierState,
};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::MAX_LANES;

#[cfg(feature = "std")]
fn boxed_repeat_array<T: Clone, const N: usize>(value: T) -> std::boxed::Box<[T; N]> {
    let values: std::boxed::Box<[T]> = std::vec![value; N].into_boxed_slice();
    match values.try_into() {
        Ok(fixed) => fixed,
        Err(_) => panic!("fixed array length"),
    }
}

pub(super) struct FrontierState {
    pub(super) root_frontier_len: u8,
    #[cfg(feature = "std")]
    pub(super) root_frontier_state: std::boxed::Box<[RootFrontierState; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    pub(super) root_frontier_state: [RootFrontierState; MAX_LANES],
    #[cfg(feature = "std")]
    pub(super) root_frontier_slot_by_ordinal:
        std::boxed::Box<[u8; ScopeId::ORDINAL_CAPACITY as usize]>,
    #[cfg(not(feature = "std"))]
    pub(super) root_frontier_slot_by_ordinal: [u8; ScopeId::ORDINAL_CAPACITY as usize],
    #[cfg(feature = "std")]
    pub(super) offer_entry_state:
        std::boxed::Box<[OfferEntryState; crate::global::typestate::MAX_STATES]>,
    #[cfg(not(feature = "std"))]
    pub(super) offer_entry_state: [OfferEntryState; crate::global::typestate::MAX_STATES],
    pub(super) global_active_entries: ActiveEntrySet,
    pub(super) global_offer_lane_mask: u8,
    #[cfg(feature = "std")]
    pub(super) global_offer_lane_entry_slot_masks: std::boxed::Box<[u8; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    pub(super) global_offer_lane_entry_slot_masks: [u8; MAX_LANES],
    pub(super) frontier_observation_epoch: u32,
    pub(super) global_frontier_observed_epoch: u32,
    pub(super) global_frontier_observed_key: FrontierObservationKey,
    pub(super) global_frontier_observed: ObservedEntrySet,
}

impl FrontierState {
    #[cfg(feature = "std")]
    pub(super) fn new() -> Self {
        Self {
            root_frontier_len: 0,
            root_frontier_state: boxed_repeat_array(RootFrontierState::EMPTY),
            root_frontier_slot_by_ordinal: boxed_repeat_array(u8::MAX),
            offer_entry_state: boxed_repeat_array(OfferEntryState::EMPTY),
            global_active_entries: ActiveEntrySet::EMPTY,
            global_offer_lane_mask: 0,
            global_offer_lane_entry_slot_masks: boxed_repeat_array(0u8),
            frontier_observation_epoch: 0,
            global_frontier_observed_epoch: 0,
            global_frontier_observed_key: FrontierObservationKey::EMPTY,
            global_frontier_observed: ObservedEntrySet::EMPTY,
        }
    }

    #[cfg(not(feature = "std"))]
    pub(super) fn new() -> Self {
        Self {
            root_frontier_len: 0,
            root_frontier_state: [RootFrontierState::EMPTY; MAX_LANES],
            root_frontier_slot_by_ordinal: [u8::MAX; ScopeId::ORDINAL_CAPACITY as usize],
            offer_entry_state: [OfferEntryState::EMPTY; crate::global::typestate::MAX_STATES],
            global_active_entries: ActiveEntrySet::EMPTY,
            global_offer_lane_mask: 0,
            global_offer_lane_entry_slot_masks: [0; MAX_LANES],
            frontier_observation_epoch: 0,
            global_frontier_observed_epoch: 0,
            global_frontier_observed_key: FrontierObservationKey::EMPTY,
            global_frontier_observed: ObservedEntrySet::EMPTY,
        }
    }

    #[inline]
    pub(super) fn root_frontier_slot(&self, root: ScopeId) -> Option<usize> {
        let ordinal = root.ordinal() as usize;
        let slot_idx = *self.root_frontier_slot_by_ordinal.get(ordinal)?;
        if slot_idx == u8::MAX {
            return None;
        }
        let slot_idx = slot_idx as usize;
        if slot_idx >= self.root_frontier_len as usize {
            return None;
        }
        let slot = self.root_frontier_state[slot_idx];
        (slot.root == root).then_some(slot_idx)
    }

    #[inline]
    pub(super) fn root_frontier_active_mask(&self, root: ScopeId) -> u8 {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].active_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn root_frontier_active_entries(&self, root: ScopeId) -> ActiveEntrySet {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].active_entries)
            .unwrap_or(ActiveEntrySet::EMPTY)
    }

    #[inline]
    pub(super) fn root_frontier_offer_lane_mask(&self, root: ScopeId) -> u8 {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].offer_lane_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn root_frontier_observed_entries(&self, root: ScopeId) -> ObservedEntrySet {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].observed_entries)
            .unwrap_or(ObservedEntrySet::EMPTY)
    }

    #[inline]
    pub(super) fn offer_entry_state(&self, entry_idx: usize) -> Option<OfferEntryState> {
        self.offer_entry_state.get(entry_idx).copied()
    }

    #[inline]
    pub(super) fn offer_entry_active_mask(&self, entry_idx: usize) -> u8 {
        self.offer_entry_state
            .get(entry_idx)
            .map(|state| state.active_mask)
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn set_offer_entry_state(&mut self, entry_idx: usize, state: OfferEntryState) {
        if let Some(slot) = self.offer_entry_state.get_mut(entry_idx) {
            *slot = state;
        }
    }

    #[inline]
    pub(super) fn clear_offer_entry_state(&mut self, entry_idx: usize) {
        self.set_offer_entry_state(entry_idx, OfferEntryState::EMPTY);
    }

    #[inline]
    pub(super) fn set_offer_entry_active_mask(&mut self, entry_idx: usize, active_mask: u8) {
        if let Some(state) = self.offer_entry_state.get_mut(entry_idx) {
            state.active_mask = active_mask;
        }
    }

    #[inline]
    pub(super) fn set_offer_entry_observed(
        &mut self,
        entry_idx: usize,
        observed: super::frontier::OfferEntryObservedState,
    ) {
        if let Some(state) = self.offer_entry_state.get_mut(entry_idx) {
            state.observed = observed;
        }
    }

    #[inline]
    pub(super) fn global_active_entries(&self) -> ActiveEntrySet {
        self.global_active_entries
    }

    #[inline]
    pub(super) fn insert_global_active_entry(&mut self, entry_idx: usize, lane_idx: u8) {
        self.global_active_entries.insert_entry(entry_idx, lane_idx);
    }

    #[inline]
    pub(super) fn remove_global_active_entry(&mut self, entry_idx: usize) {
        self.global_active_entries.remove_entry(entry_idx);
    }

    #[inline]
    pub(super) fn global_offer_lane_mask(&self) -> u8 {
        self.global_offer_lane_mask
    }

    #[inline]
    pub(super) fn set_global_offer_lane_mask(&mut self, mask: u8) {
        self.global_offer_lane_mask = mask;
    }

    #[inline]
    pub(super) fn global_offer_lane_entry_slot_masks(&self) -> [u8; MAX_LANES] {
        #[cfg(feature = "std")]
        {
            *self.global_offer_lane_entry_slot_masks
        }
        #[cfg(not(feature = "std"))]
        {
            self.global_offer_lane_entry_slot_masks
        }
    }

    #[inline]
    pub(super) fn set_global_offer_lane_entry_slot_masks(&mut self, masks: [u8; MAX_LANES]) {
        #[cfg(feature = "std")]
        {
            *self.global_offer_lane_entry_slot_masks = masks;
        }
        #[cfg(not(feature = "std"))]
        {
            self.global_offer_lane_entry_slot_masks = masks;
        }
    }

    pub(super) fn next_observation_epoch(&mut self) -> u32 {
        let next = self.frontier_observation_epoch.wrapping_add(1);
        if next == 0 {
            self.frontier_observation_epoch = 1;
            self.global_frontier_observed_epoch = 0;
            self.global_frontier_observed_key = FrontierObservationKey::EMPTY;
            self.global_frontier_observed = ObservedEntrySet::EMPTY;
            let len = self.root_frontier_len as usize;
            let mut idx = 0usize;
            while idx < len {
                self.root_frontier_state[idx].observed_epoch = 0;
                self.root_frontier_state[idx].observed_key = FrontierObservationKey::EMPTY;
                self.root_frontier_state[idx].observed_entries = ObservedEntrySet::EMPTY;
                idx += 1;
            }
            1
        } else {
            self.frontier_observation_epoch = next;
            next
        }
    }

    #[inline]
    pub(super) fn global_frontier_observed_entries(&self) -> ObservedEntrySet {
        self.global_frontier_observed
    }

    #[inline]
    pub(super) fn cached_frontier_observed_entries(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        if use_root_observed_entries {
            let slot_idx = self.root_frontier_slot(current_parallel_root)?;
            let slot = self.root_frontier_state[slot_idx];
            if slot.observed_key != key || slot.observed_entries.dynamic_controller_mask != 0 {
                return None;
            }
            return Some(slot.observed_entries);
        }
        if self.global_frontier_observed_key != key
            || self.global_frontier_observed.dynamic_controller_mask != 0
        {
            return None;
        }
        Some(self.global_frontier_observed)
    }

    #[inline]
    pub(super) fn frontier_observation_cache(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        if use_root_observed_entries {
            let Some(slot_idx) = self.root_frontier_slot(current_parallel_root) else {
                return (FrontierObservationKey::EMPTY, ObservedEntrySet::EMPTY);
            };
            let slot = self.root_frontier_state[slot_idx];
            return (slot.observed_key, slot.observed_entries);
        }
        (
            self.global_frontier_observed_key,
            self.global_frontier_observed,
        )
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
        if use_root_observed_entries {
            let Some(slot_idx) = self.root_frontier_slot(current_parallel_root) else {
                return;
            };
            let slot = &mut self.root_frontier_state[slot_idx];
            slot.observed_epoch = observed_epoch;
            slot.observed_key = key;
            slot.observed_entries = observed_entries;
            return;
        }
        self.global_frontier_observed_epoch = observed_epoch;
        self.global_frontier_observed_key = key;
        self.global_frontier_observed = observed_entries;
    }

    pub(super) fn remove_root_frontier_slot(&mut self, slot_idx: usize) {
        let len = self.root_frontier_len as usize;
        if slot_idx >= len {
            return;
        }
        let removed_root = self.root_frontier_state[slot_idx].root;
        self.root_frontier_slot_by_ordinal[removed_root.ordinal() as usize] = u8::MAX;
        let last = len - 1;
        let mut idx = slot_idx;
        while idx < last {
            let moved = self.root_frontier_state[idx + 1];
            self.root_frontier_state[idx] = moved;
            self.root_frontier_slot_by_ordinal[moved.root.ordinal() as usize] = idx as u8;
            idx += 1;
        }
        self.root_frontier_state[last] = RootFrontierState::EMPTY;
        self.root_frontier_len = last as u8;
    }

    #[inline]
    pub(super) fn attach_offer_entry_to_root_frontier(
        &mut self,
        entry_idx: usize,
        root: ScopeId,
        lane_idx: u8,
    ) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        self.root_frontier_state[slot_idx]
            .active_entries
            .insert_entry(entry_idx, lane_idx);
    }

    #[inline]
    pub(super) fn detach_offer_entry_from_root_frontier(
        &mut self,
        entry_idx: usize,
        root: ScopeId,
    ) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        self.root_frontier_state[slot_idx]
            .active_entries
            .remove_entry(entry_idx);
    }

    #[inline]
    pub(super) fn set_root_frontier_offer_lane_mask(&mut self, root: ScopeId, mask: u8) {
        if let Some(slot_idx) = self.root_frontier_slot(root) {
            self.root_frontier_state[slot_idx].offer_lane_mask = mask;
        }
    }

    #[inline]
    pub(super) fn set_root_frontier_offer_lane_entry_slot_masks(
        &mut self,
        root: ScopeId,
        masks: [u8; MAX_LANES],
    ) {
        if let Some(slot_idx) = self.root_frontier_slot(root) {
            self.root_frontier_state[slot_idx].offer_lane_entry_slot_masks = masks;
        }
    }

    pub(super) fn detach_lane_from_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let bit = 1u8 << lane_idx;
        let slot = &mut self.root_frontier_state[slot_idx];
        slot.active_mask &= !bit;
        slot.controller_mask &= !bit;
        slot.dynamic_controller_mask &= !bit;
        if slot.active_mask == 0 {
            self.remove_root_frontier_slot(slot_idx);
        }
    }

    pub(super) fn attach_lane_to_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        let slot_idx = if let Some(slot_idx) = self.root_frontier_slot(root) {
            slot_idx
        } else {
            let slot_idx = self.root_frontier_len as usize;
            if slot_idx >= MAX_LANES {
                return;
            }
            self.root_frontier_state[slot_idx] = RootFrontierState {
                root,
                ..RootFrontierState::EMPTY
            };
            self.root_frontier_slot_by_ordinal[root.ordinal() as usize] = slot_idx as u8;
            self.root_frontier_len += 1;
            slot_idx
        };
        let bit = 1u8 << lane_idx;
        let slot = &mut self.root_frontier_state[slot_idx];
        slot.active_mask |= bit;
        if info.is_controller() {
            slot.controller_mask |= bit;
        }
        if info.is_dynamic() {
            slot.dynamic_controller_mask |= bit;
        }
    }
}
