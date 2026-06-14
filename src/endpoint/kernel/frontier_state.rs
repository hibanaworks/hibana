//! Mutable frontier-state owner for endpoint kernel runtime bookkeeping.
//!
//! # Unsafe Owner Contract
//!
//! This module owns mutable frontier-state buffers for one endpoint runtime
//! image. Unsafe blocks here may expose table entries only within the initialized
//! capacity recorded on the same frontier state object.

use core::ops::{Index, IndexMut};

use super::frontier::{
    ActiveEntrySet, ActiveEntrySlot, EntryBuffer, FrontierObservationKey, FrontierObservationSlot,
    LaneOfferState, ObservedEntrySet, RootFrontierState,
};
use super::frontier::{OfferEntrySlot, OfferEntryState, OfferEntryTable};
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

pub(super) struct RootFrontierStorage {
    pub(super) rows: *mut RootFrontierState,
    pub(super) active_entries: *mut ActiveEntrySlot,
    pub(super) observed_key_slots: *mut FrontierObservationSlot,
    pub(super) observed_key_offer_lanes: *mut LaneWord,
}

pub(super) struct RootFrontierCapacity {
    pub(super) row_count: usize,
    pub(super) pool_capacity: usize,
    pub(super) observed_key_lane_word_count: usize,
}

pub(super) struct FrontierStateStorage {
    pub(super) root: RootFrontierStorage,
    pub(super) offer_entry_slots: *mut OfferEntrySlot,
}

pub(super) struct FrontierStateCapacity {
    pub(super) root: RootFrontierCapacity,
    pub(super) max_offer_entries: usize,
}

impl RootFrontierTable {
    unsafe fn init_from_parts(
        dst: *mut Self,
        storage: RootFrontierStorage,
        capacity: RootFrontierCapacity,
    ) {
        if capacity.row_count > u16::MAX as usize {
            crate::invariant();
        }
        if capacity.pool_capacity > u8::MAX as usize {
            crate::invariant();
        }
        if capacity.observed_key_lane_word_count > u8::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(storage.rows);
            core::ptr::addr_of_mut!((*dst).active_entries).write(storage.active_entries);
            core::ptr::addr_of_mut!((*dst).observed_key_slots).write(storage.observed_key_slots);
            core::ptr::addr_of_mut!((*dst).observed_key_offer_lanes)
                .write(storage.observed_key_offer_lanes);
            core::ptr::addr_of_mut!((*dst).capacity).write(capacity.row_count as u16);
            core::ptr::addr_of_mut!((*dst).pool_capacity).write(capacity.pool_capacity as u8);
            core::ptr::addr_of_mut!((*dst).observed_key_lane_word_count)
                .write(capacity.observed_key_lane_word_count as u8);
        }
        let mut slot_idx = 0usize;
        while slot_idx < capacity.row_count {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                storage.rows.add(slot_idx).write(RootFrontierState::EMPTY);
            }
            let base =
                crate::invariant_some(slot_idx.checked_mul(capacity.observed_key_lane_word_count));
            let mut word_idx = 0usize;
            while word_idx < capacity.observed_key_lane_word_count {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    storage
                        .observed_key_offer_lanes
                        .add(base + word_idx)
                        .write(0);
                }
                word_idx += 1;
            }
            slot_idx += 1;
        }
        let mut entry_idx = 0usize;
        while entry_idx < capacity.pool_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                storage
                    .active_entries
                    .add(entry_idx)
                    .write(ActiveEntrySlot::EMPTY);
                storage
                    .observed_key_slots
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
            self.observed_key_offer_lanes.add(crate::invariant_some(
                slot_idx.checked_mul(self.observed_key_lane_word_count()),
            ))
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
        if slot_idx >= self.len() {
            crate::invariant();
        }
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
        if self[slot_idx].active_len == u8::MAX {
            crate::invariant();
        }
        self[slot_idx].active_len += 1;
        self[slot_idx].clear_observed_key_cache();
        self.clear_row_observed_key_lanes(slot_idx);
        let row_len = self.len();
        let mut idx = slot_idx + 1;
        while idx < row_len {
            if self[idx].active_start == u8::MAX {
                crate::invariant();
            }
            self[idx].active_start += 1;
            idx += 1;
        }
        true
    }

    fn remove_root_active_entry(&mut self, slot_idx: usize, entry_idx: usize) -> bool {
        if slot_idx >= self.len() {
            crate::invariant();
        }
        let row = self[slot_idx];
        let start = row.active_start as usize;
        let len = row.active_len as usize;
        let mut remove_rel = 0usize;
        let entry = crate::invariant_some(super::frontier::checked_state_index(entry_idx));
        while remove_rel < len {
            if
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { (*self.active_entries.add(start + remove_rel)).entry } == entry {
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
        if self[slot_idx].active_len == 0 {
            crate::invariant();
        }
        self[slot_idx].active_len -= 1;
        self[slot_idx].clear_observed_key_cache();
        self.clear_row_observed_key_lanes(slot_idx);
        let row_len = self.len();
        let mut idx = slot_idx + 1;
        while idx < row_len {
            if self[idx].active_start == 0 {
                crate::invariant();
            }
            self[idx].active_start -= 1;
            idx += 1;
        }
        true
    }

    pub(super) fn replace_root_observed_key(
        &mut self,
        slot_idx: usize,
        key: FrontierObservationKey,
    ) {
        if slot_idx >= self.len() {
            crate::invariant();
        }
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
        if new_len != active_len {
            crate::invariant();
        }
        if !key.exact_entries_match(self.active_entry_set(slot_idx)) {
            crate::invariant();
        }
        dst.copy_from(key);
        self[slot_idx].mark_observed_key_cached();
    }

    #[inline]
    pub(super) fn clear_root_observed_key(&mut self, slot_idx: usize) {
        self.replace_root_observed_key(slot_idx, FrontierObservationKey::EMPTY);
    }

    fn remove_root_row(&mut self, slot_idx: usize) {
        let len = self.len();
        if slot_idx >= len {
            crate::invariant();
        }

        let active_span = self[slot_idx].active_len as usize;
        if active_span != 0 {
            let start = self[slot_idx].active_start as usize;
            let used = self.active_pool_used();
            let mut idx = start;
            while idx + active_span < used {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    self.active_entries
                        .add(idx)
                        .write(*self.active_entries.add(idx + active_span));
                    self.observed_key_slots
                        .add(idx)
                        .write(*self.observed_key_slots.add(idx + active_span));
                }
                idx += 1;
            }
            let mut clear_idx = used - active_span;
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
            if active_span != 0 {
                if active_span > shifted.active_start as usize {
                    crate::invariant();
                }
                shifted.active_start -= active_span as u8;
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
        if index >= self.capacity() {
            crate::invariant();
        }
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &*self.ptr.add(index) }
    }
}

impl IndexMut<usize> for RootFrontierTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.capacity() {
            crate::invariant();
        }
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &mut *self.ptr.add(index) }
    }
}

pub(super) struct FrontierState {
    pub(super) root_frontier_state: RootFrontierTable,
    pub(super) offer_entry_state: OfferEntryTable,
    pub(super) global_frontier_scratch_state: FrontierScratchState,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontierScratchState {
    Uninitialized = 0,
    Initialized = 1,
}

impl FrontierScratchState {
    #[inline]
    pub(super) const fn is_initialized(self) -> bool {
        matches!(self, Self::Initialized)
    }
}

impl FrontierState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        storage: FrontierStateStorage,
        capacity: FrontierStateCapacity,
    ) {
        unsafe {
            // SAFETY: `FrontierState` is initialized in one caller-owned endpoint
            // arena. The root and offer-entry sub-tables receive disjoint backing
            // buffers with capacities recorded on their respective owners before
            // safe frontier methods can observe them.
            RootFrontierTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).root_frontier_state),
                storage.root,
                capacity.root,
            );
            OfferEntryTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).offer_entry_state),
                storage.offer_entry_slots,
                capacity.max_offer_entries,
            );
            core::ptr::addr_of_mut!((*dst).global_frontier_scratch_state)
                .write(FrontierScratchState::Uninitialized);
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
        match self.root_frontier_slot(root) {
            Some(slot) => self
                .root_frontier_state
                .active_entry_set(slot)
                .occupancy_mask(),
            None => 0,
        }
    }

    #[inline]
    pub(super) fn root_frontier_active_entries(&self, root: ScopeId) -> ActiveEntrySet {
        match self.root_frontier_slot(root) {
            Some(slot) => self.root_frontier_state.active_entry_set(slot),
            None => ActiveEntrySet::EMPTY,
        }
    }

    #[inline]
    pub(super) fn root_frontier_observed_entries(&self, root: ScopeId) -> ObservedEntrySet {
        match self.root_frontier_slot(root) {
            Some(slot) => {
                let row = self.root_frontier_state[slot];
                self.root_frontier_state
                    .observed_key(slot)
                    .observed_entries(row.observed_entries)
            }
            None => ObservedEntrySet::EMPTY,
        }
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
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            crate::invariant();
        };
        if !self
            .root_frontier_state
            .insert_root_active_entry(slot_idx, entry_idx, lane_idx)
        {
            crate::invariant();
        }
    }

    #[inline]
    pub(super) fn detach_offer_entry_from_root_frontier(
        &mut self,
        entry_idx: usize,
        root: ScopeId,
    ) {
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            crate::invariant();
        };
        if !self
            .root_frontier_state
            .remove_root_active_entry(slot_idx, entry_idx)
        {
            crate::invariant();
        }
    }

    pub(super) fn detach_lane_from_root_frontier(&mut self, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        if self.root_frontier_state.active_entry_set(slot_idx).len() == 0 {
            self.remove_root_frontier_slot(slot_idx);
        }
    }

    pub(super) fn attach_lane_to_root_frontier(&mut self, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        if self.root_frontier_slot(root).is_none() {
            let slot_idx = self.root_frontier_len();
            if slot_idx >= self.root_frontier_state.capacity() {
                crate::invariant();
            }
            self.root_frontier_state.prepare_row(slot_idx, root);
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "frontier_state/tests.rs"]
mod tests;
