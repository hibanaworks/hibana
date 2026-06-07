//! Mutable frontier-state owner for endpoint kernel runtime bookkeeping.
//!
//! # Unsafe Owner Contract
//!
//! This module owns mutable frontier-state buffers for one endpoint runtime
//! image. Unsafe blocks here may expose table entries only within the initialized
//! capacity recorded on the same frontier state object.

use core::ops::{Index, IndexMut};

#[cfg(test)]
use super::frontier::ObservedEntrySummary;
use super::frontier::{
    ActiveEntrySet, ActiveEntrySlot, EntryBuffer, FrontierObservationKey, FrontierObservationSlot,
    LaneOfferState, ObservedEntrySet, RootFrontierState,
};
use super::frontier::{OfferEntrySlot, OfferEntryState, OfferEntryTable};
#[cfg(test)]
use crate::endpoint::kernel::offer::FrontierObservationDomain;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{LaneSet, LaneWord};

pub(super) struct RootFrontierTable {
    ptr: *mut RootFrontierState,
    active_entries: *mut ActiveEntrySlot,
    observed_key_slots: *mut FrontierObservationSlot,
    observed_key_offer_lanes: *mut LaneWord,
    capacity: u16,
    pool_capacity: u8,
    observed_key_lane_word_count: u8,
}

impl RootFrontierTable {
    unsafe fn init_from_parts(
        dst: *mut Self,
        rows: *mut RootFrontierState,
        active_entries: *mut ActiveEntrySlot,
        observed_key_slots: *mut FrontierObservationSlot,
        observed_key_offer_lanes: *mut LaneWord,
        root_frontier_capacity: usize,
        pool_capacity: usize,
        observed_key_lane_word_count: usize,
    ) {
        if root_frontier_capacity > u16::MAX as usize {
            panic!("root frontier row capacity overflow");
        }
        if pool_capacity > u8::MAX as usize {
            panic!("root frontier pool capacity overflow");
        }
        if observed_key_lane_word_count > u8::MAX as usize {
            panic!("root frontier lane-word count overflow");
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(rows);
            core::ptr::addr_of_mut!((*dst).active_entries).write(active_entries);
            core::ptr::addr_of_mut!((*dst).observed_key_slots).write(observed_key_slots);
            core::ptr::addr_of_mut!((*dst).observed_key_offer_lanes)
                .write(observed_key_offer_lanes);
            core::ptr::addr_of_mut!((*dst).capacity).write(root_frontier_capacity as u16);
            core::ptr::addr_of_mut!((*dst).pool_capacity).write(pool_capacity as u8);
            core::ptr::addr_of_mut!((*dst).observed_key_lane_word_count)
                .write(observed_key_lane_word_count as u8);
        }
        let mut slot_idx = 0usize;
        while slot_idx < root_frontier_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                rows.add(slot_idx).write(RootFrontierState::EMPTY);
            }
            let base = slot_idx.saturating_mul(observed_key_lane_word_count);
            let mut word_idx = 0usize;
            while word_idx < observed_key_lane_word_count {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    observed_key_offer_lanes.add(base + word_idx).write(0);
                }
                word_idx += 1;
            }
            slot_idx += 1;
        }
        let mut entry_idx = 0usize;
        while entry_idx < pool_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                active_entries.add(entry_idx).write(ActiveEntrySlot::EMPTY);
                observed_key_slots
                    .add(entry_idx)
                    .write(FrontierObservationSlot::EMPTY);
            }
            entry_idx += 1;
        }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity as usize
    }

    #[inline]
    fn pool_capacity(&self) -> usize {
        self.pool_capacity as usize
    }

    #[inline]
    fn len(&self) -> usize {
        let mut len = 0usize;
        while len < self.capacity() {
            if self[len].root.is_none() {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline]
    fn active_pool_used(&self) -> usize {
        let len = self.len();
        if len == 0 {
            return 0;
        }
        let tail = self[len - 1];
        tail.active_start as usize + tail.active_len as usize
    }

    #[inline]
    fn active_entry_set(&self, slot_idx: usize) -> ActiveEntrySet {
        let row = self[slot_idx];
        if row.active_len == 0 {
            return ActiveEntrySet::EMPTY;
        }
        ActiveEntrySet {
            slots: EntryBuffer::from_parts(
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe { self.active_entries.add(row.active_start as usize) },
                row.active_len as usize,
            ),
        }
    }

    #[inline]
    fn observed_key_lane_word_count(&self) -> usize {
        self.observed_key_lane_word_count as usize
    }

    #[inline]
    fn observed_key_offer_lanes_ptr(&self, slot_idx: usize) -> *mut LaneWord {
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            self.observed_key_offer_lanes
                .add(slot_idx.saturating_mul(self.observed_key_lane_word_count()))
        }
    }

    #[inline]
    fn copy_row_observed_key_lanes(&mut self, dst_slot: usize, src_slot: usize) {
        let word_count = self.observed_key_lane_word_count();
        let src_offer_set =
            LaneSet::from_parts(self.observed_key_offer_lanes_ptr(src_slot), word_count);
        let src_offer = src_offer_set.view();
        let mut dst_offer =
            LaneSet::from_parts(self.observed_key_offer_lanes_ptr(dst_slot), word_count);
        dst_offer.copy_from(src_offer);
    }

    #[inline]
    pub(super) fn observed_key(&self, slot_idx: usize) -> FrontierObservationKey {
        let row = self[slot_idx];
        if row.active_len == 0 || !row.observed_key_valid() {
            return FrontierObservationKey::EMPTY;
        }
        FrontierObservationKey::from_parts(
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { self.observed_key_slots.add(row.active_start as usize) },
            row.active_len as usize,
            self.observed_key_offer_lanes_ptr(slot_idx),
            self.observed_key_lane_word_count(),
        )
    }

    #[inline]
    fn clear_row_observed_key_lanes(&mut self, slot_idx: usize) {
        let mut key = FrontierObservationKey::from_parts(
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                self.observed_key_slots
                    .add(self[slot_idx].active_start as usize)
            },
            self[slot_idx].active_len as usize,
            self.observed_key_offer_lanes_ptr(slot_idx),
            self.observed_key_lane_word_count(),
        );
        key.clear();
    }

    #[inline]
    fn clear_row(&mut self, slot_idx: usize) {
        self.clear_row_observed_key_lanes(slot_idx);
        self[slot_idx] = RootFrontierState::EMPTY;
    }

    #[inline]
    fn prepare_row(&mut self, slot_idx: usize, root: ScopeId) {
        let active_start = self.active_pool_used();
        let row = &mut self[slot_idx];
        row.root = root;
        row.active_start = active_start as u8;
        row.active_len = 0;
        row.clear_observed_key_cache();
        self.clear_row_observed_key_lanes(slot_idx);
    }

    fn insert_root_active_entry(
        &mut self,
        slot_idx: usize,
        entry_idx: usize,
        lane_idx: u8,
    ) -> bool {
        assert!(slot_idx < self.len(), "root frontier slot out of bounds");
        let Some(entry) = super::frontier::checked_state_index(entry_idx) else {
            return false;
        };
        let row = self[slot_idx];
        let start = row.active_start as usize;
        let len = row.active_len as usize;
        let mut insert_rel = 0usize;
        while insert_rel < len {
            let existing = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.active_entries.add(start + insert_rel) };
            if existing.entry == entry {
                return false;
            }
            if existing.lane_idx > lane_idx
                || (existing.lane_idx == lane_idx && existing.entry.raw() > entry.raw())
            {
                break;
            }
            insert_rel += 1;
        }
        let used = self.active_pool_used();
        if used >= self.pool_capacity() {
            return false;
        }
        let insert_idx = start + insert_rel;
        let mut idx = used;
        while idx > insert_idx {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.active_entries
                    .add(idx)
                    .write(*self.active_entries.add(idx - 1));
                self.observed_key_slots
                    .add(idx)
                    .write(*self.observed_key_slots.add(idx - 1));
            }
            idx -= 1;
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.active_entries
                .add(insert_idx)
                .write(ActiveEntrySlot { entry, lane_idx });
            self.observed_key_slots
                .add(insert_idx)
                .write(FrontierObservationSlot::EMPTY);
        }
        self[slot_idx].active_len = self[slot_idx].active_len.saturating_add(1);
        self[slot_idx].clear_observed_key_cache();
        self.clear_row_observed_key_lanes(slot_idx);
        let row_len = self.len();
        let mut idx = slot_idx + 1;
        while idx < row_len {
            self[idx].active_start = self[idx].active_start.saturating_add(1);
            idx += 1;
        }
        true
    }

    fn remove_root_active_entry(&mut self, slot_idx: usize, entry_idx: usize) -> bool {
        assert!(slot_idx < self.len(), "root frontier slot out of bounds");
        let row = self[slot_idx];
        let start = row.active_start as usize;
        let len = row.active_len as usize;
        let mut remove_rel = 0usize;
        while remove_rel < len {
            if
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { (*self.active_entries.add(start + remove_rel)).entry }
                == super::frontier::checked_state_index(entry_idx)
                    .unwrap_or(crate::global::typestate::StateIndex::MAX)
            {
                break;
            }
            remove_rel += 1;
        }
        if remove_rel >= len {
            return false;
        }
        let used = self.active_pool_used();
        let remove_idx = start + remove_rel;
        let mut idx = remove_idx;
        while idx + 1 < used {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.active_entries
                    .add(idx)
                    .write(*self.active_entries.add(idx + 1));
                self.observed_key_slots
                    .add(idx)
                    .write(*self.observed_key_slots.add(idx + 1));
            }
            idx += 1;
        }
        if used != 0 {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.active_entries
                    .add(used - 1)
                    .write(ActiveEntrySlot::EMPTY);
                self.observed_key_slots
                    .add(used - 1)
                    .write(FrontierObservationSlot::EMPTY);
            }
        }
        self[slot_idx].active_len = self[slot_idx].active_len.saturating_sub(1);
        self[slot_idx].clear_observed_key_cache();
        self.clear_row_observed_key_lanes(slot_idx);
        let row_len = self.len();
        let mut idx = slot_idx + 1;
        while idx < row_len {
            self[idx].active_start = self[idx].active_start.saturating_sub(1);
            idx += 1;
        }
        true
    }

    pub(super) fn replace_root_observed_key(
        &mut self,
        slot_idx: usize,
        key: FrontierObservationKey,
    ) {
        assert!(slot_idx < self.len(), "root frontier slot out of bounds");
        let row = self[slot_idx];
        let active_len = row.active_len as usize;
        let new_len = key.len();
        let mut dst = FrontierObservationKey::from_parts(
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { self.observed_key_slots.add(row.active_start as usize) },
            active_len,
            self.observed_key_offer_lanes_ptr(slot_idx),
            self.observed_key_lane_word_count(),
        );
        if new_len == 0 {
            dst.clear();
            self[slot_idx].clear_observed_key_cache();
            return;
        }
        assert_eq!(
            new_len, active_len,
            "root frontier observed-key length must track active entries"
        );
        debug_assert!(key.exact_entries_match(self.active_entry_set(slot_idx)));
        dst.copy_from(key);
        self[slot_idx].observed_key_present = true;
    }

    #[inline]
    pub(super) fn clear_root_observed_key(&mut self, slot_idx: usize) {
        self.replace_root_observed_key(slot_idx, FrontierObservationKey::EMPTY);
    }

    fn remove_root_row(&mut self, slot_idx: usize) {
        let len = self.len();
        if slot_idx >= len {
            return;
        }

        let active_removed = self[slot_idx].active_len as usize;
        if active_removed != 0 {
            let start = self[slot_idx].active_start as usize;
            let used = self.active_pool_used();
            let mut idx = start;
            while idx + active_removed < used {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    self.active_entries
                        .add(idx)
                        .write(*self.active_entries.add(idx + active_removed));
                    self.observed_key_slots
                        .add(idx)
                        .write(*self.observed_key_slots.add(idx + active_removed));
                }
                idx += 1;
            }
            let mut clear_idx = used - active_removed;
            while clear_idx < used {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    self.active_entries
                        .add(clear_idx)
                        .write(ActiveEntrySlot::EMPTY);
                    self.observed_key_slots
                        .add(clear_idx)
                        .write(FrontierObservationSlot::EMPTY);
                }
                clear_idx += 1;
            }
        }

        let mut idx = slot_idx + 1;
        while idx < len {
            let mut shifted = self[idx];
            if active_removed != 0 {
                shifted.active_start = shifted.active_start.saturating_sub(active_removed as u8);
            }
            self.copy_row_observed_key_lanes(idx - 1, idx);
            self[idx - 1] = shifted;
            idx += 1;
        }
        self.clear_row(len - 1);
    }
}

impl Index<usize> for RootFrontierTable {
    type Output = RootFrontierState;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        assert!(index < self.capacity(), "root frontier slot out of bounds");
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &*self.ptr.add(index) }
    }
}

impl IndexMut<usize> for RootFrontierTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        assert!(index < self.capacity(), "root frontier slot out of bounds");
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &mut *self.ptr.add(index) }
    }
}

pub(super) struct FrontierState {
    pub(super) root_frontier_state: RootFrontierTable,
    pub(super) offer_entry_state: OfferEntryTable,
    #[cfg(test)]
    pub(super) frontier_observation_epoch: u16,
    #[cfg(test)]
    pub(super) global_frontier_observed: ObservedEntrySummary,
    pub(super) global_frontier_scratch_initialized: bool,
}

impl FrontierState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        root_rows: *mut RootFrontierState,
        root_active_entries: *mut ActiveEntrySlot,
        root_observed_key_slots: *mut FrontierObservationSlot,
        root_observed_offer_lanes: *mut LaneWord,
        offer_entry_slots: *mut OfferEntrySlot,
        root_frontier_capacity: usize,
        max_frontier_entries: usize,
        root_observed_lane_word_count: usize,
        max_offer_entries: usize,
    ) {
        unsafe {
            // SAFETY: `FrontierState` is initialized in one caller-owned endpoint
            // arena. The root and offer-entry sub-tables receive disjoint backing
            // buffers with capacities recorded on their respective owners before
            // safe frontier methods can observe them.
            RootFrontierTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).root_frontier_state),
                root_rows,
                root_active_entries,
                root_observed_key_slots,
                root_observed_offer_lanes,
                root_frontier_capacity,
                max_frontier_entries,
                root_observed_lane_word_count,
            );
            OfferEntryTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).offer_entry_state),
                offer_entry_slots,
                max_offer_entries,
            );
            let _ = max_frontier_entries;
            #[cfg(test)]
            core::ptr::addr_of_mut!((*dst).frontier_observation_epoch).write(0);
            #[cfg(test)]
            core::ptr::addr_of_mut!((*dst).global_frontier_observed)
                .write(ObservedEntrySummary::EMPTY);
            core::ptr::addr_of_mut!((*dst).global_frontier_scratch_initialized).write(false);
        }
    }

    #[inline]
    pub(super) fn root_frontier_len(&self) -> usize {
        self.root_frontier_state.len()
    }

    #[inline]
    pub(super) fn root_frontier_slot(&self, root: ScopeId) -> Option<usize> {
        let len = self.root_frontier_len();
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let slot = self.root_frontier_state[slot_idx];
            if slot.root == root {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn root_frontier_active_mask(&self, root: ScopeId) -> u8 {
        self.root_frontier_slot(root)
            .map(|slot| {
                self.root_frontier_state
                    .active_entry_set(slot)
                    .occupancy_mask()
            })
            .unwrap_or(0)
    }

    #[inline]
    pub(super) fn root_frontier_active_entries(&self, root: ScopeId) -> ActiveEntrySet {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state.active_entry_set(slot))
            .unwrap_or(ActiveEntrySet::EMPTY)
    }

    #[inline]
    pub(super) fn root_frontier_observed_entries(&self, root: ScopeId) -> ObservedEntrySet {
        self.root_frontier_slot(root)
            .map(|slot| {
                let row = self.root_frontier_state[slot];
                self.root_frontier_state
                    .observed_key(slot)
                    .observed_entries(row.observed_entries)
            })
            .unwrap_or(ObservedEntrySet::EMPTY)
    }

    #[inline]
    pub(super) fn offer_entry_state_mut(
        &mut self,
        entry_idx: usize,
    ) -> Option<&mut OfferEntryState> {
        if !self.offer_entry_state.has_storage() {
            return None;
        }
        self.offer_entry_state.get_mut(entry_idx)
    }

    #[inline]
    pub(super) fn set_offer_entry_state(&mut self, entry_idx: usize, state: OfferEntryState) {
        self.offer_entry_state.set(entry_idx, state);
    }

    #[inline]
    pub(super) fn clear_offer_entry_state(&mut self, entry_idx: usize) {
        self.set_offer_entry_state(entry_idx, OfferEntryState::EMPTY);
    }

    #[cfg(test)]
    pub(super) fn next_observation_epoch(
        &mut self,
        global_frontier_observed_key: &mut FrontierObservationKey,
    ) -> u16 {
        let next = self.frontier_observation_epoch.wrapping_add(1);
        if next == 0 {
            self.frontier_observation_epoch = 1;
            global_frontier_observed_key.clear();
            self.global_frontier_observed.clear();
            let len = self.root_frontier_len();
            let mut idx = 0usize;
            while idx < len {
                self.root_frontier_state.clear_root_observed_key(idx);
                self.root_frontier_state[idx].observed_entries.clear();
                idx += 1;
            }
            1
        } else {
            self.frontier_observation_epoch = next;
            next
        }
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn global_frontier_observed_entries(
        &self,
        global_frontier_observed_key: FrontierObservationKey,
    ) -> ObservedEntrySet {
        global_frontier_observed_key.observed_entries(self.global_frontier_observed)
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn cached_frontier_observed_entries(
        &self,
        domain: FrontierObservationDomain,
        global_frontier_observed_key: FrontierObservationKey,
        key: &FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        if domain.uses_root_entries() {
            let slot_idx = self.root_frontier_slot(domain.root_scope())?;
            let slot = self.root_frontier_state[slot_idx];
            let observed_key = self.root_frontier_state.observed_key(slot_idx);
            if observed_key != *key || slot.observed_entries.dynamic_controller_mask != 0 {
                return None;
            }
            return Some(observed_key.observed_entries(slot.observed_entries));
        }
        if global_frontier_observed_key != *key
            || self.global_frontier_observed.dynamic_controller_mask != 0
        {
            return None;
        }
        Some(self.global_frontier_observed_entries(global_frontier_observed_key))
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn frontier_observation_cache(
        &self,
        domain: FrontierObservationDomain,
        global_frontier_observed_key: FrontierObservationKey,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        if domain.uses_root_entries() {
            let Some(slot_idx) = self.root_frontier_slot(domain.root_scope()) else {
                return (FrontierObservationKey::EMPTY, ObservedEntrySet::EMPTY);
            };
            let row = self.root_frontier_state[slot_idx];
            let observed_key = self.root_frontier_state.observed_key(slot_idx);
            return (
                observed_key,
                observed_key.observed_entries(row.observed_entries),
            );
        }
        (
            global_frontier_observed_key,
            self.global_frontier_observed_entries(global_frontier_observed_key),
        )
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn store_frontier_observation(
        &mut self,
        domain: FrontierObservationDomain,
        mut global_frontier_observed_key: FrontierObservationKey,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        if domain.uses_root_entries() {
            let Some(slot_idx) = self.root_frontier_slot(domain.root_scope()) else {
                return;
            };
            self.root_frontier_state
                .replace_root_observed_key(slot_idx, key);
            let slot = &mut self.root_frontier_state[slot_idx];
            slot.observed_entries = observed_entries.summary();
            return;
        }
        global_frontier_observed_key.copy_from(key);
        self.global_frontier_observed = observed_entries.summary();
    }

    pub(super) fn remove_root_frontier_slot(&mut self, slot_idx: usize) {
        self.root_frontier_state.remove_root_row(slot_idx);
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
        let _ = self
            .root_frontier_state
            .insert_root_active_entry(slot_idx, entry_idx, lane_idx);
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
        let _ = self
            .root_frontier_state
            .remove_root_active_entry(slot_idx, entry_idx);
    }

    pub(super) fn detach_lane_from_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let _ = lane_idx;
        if self.root_frontier_state.active_entry_set(slot_idx).len() == 0 {
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
            let slot_idx = self.root_frontier_len();
            if slot_idx >= self.root_frontier_state.capacity() {
                return;
            }
            self.root_frontier_state.prepare_row(slot_idx, root);
            slot_idx
        };
        let _ = lane_idx;
        let _ = slot_idx;
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "frontier_state/tests.rs"]
mod tests;
