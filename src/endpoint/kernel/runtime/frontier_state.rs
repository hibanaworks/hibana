//! Mutable frontier-state owner for endpoint kernel runtime bookkeeping.

use core::ops::{Index, IndexMut};

#[cfg(test)]
use super::frontier::ObservedEntrySummary;
use super::frontier::{
    ActiveEntrySet, ActiveEntrySlot, EntryBuffer, FrontierObservationKey, FrontierObservationSlot,
    LaneOfferState, ObservedEntrySet, RootFrontierState,
};
#[cfg(test)]
use super::frontier::{OfferEntrySlot, OfferEntryState, OfferEntryTable};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{LaneSet, LaneWord};

pub(super) struct RootFrontierTable {
    ptr: *mut RootFrontierState,
    active_entries: *mut ActiveEntrySlot,
    observed_key_slots: *mut FrontierObservationSlot,
    observed_key_offer_lanes: *mut LaneWord,
    observed_key_binding_nonempty_lanes: *mut LaneWord,
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
        observed_key_binding_nonempty_lanes: *mut LaneWord,
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
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(rows);
            core::ptr::addr_of_mut!((*dst).active_entries).write(active_entries);
            core::ptr::addr_of_mut!((*dst).observed_key_slots).write(observed_key_slots);
            core::ptr::addr_of_mut!((*dst).observed_key_offer_lanes)
                .write(observed_key_offer_lanes);
            core::ptr::addr_of_mut!((*dst).observed_key_binding_nonempty_lanes)
                .write(observed_key_binding_nonempty_lanes);
            core::ptr::addr_of_mut!((*dst).capacity).write(root_frontier_capacity as u16);
            core::ptr::addr_of_mut!((*dst).pool_capacity).write(pool_capacity as u8);
            core::ptr::addr_of_mut!((*dst).observed_key_lane_word_count)
                .write(observed_key_lane_word_count as u8);
        }
        let mut slot_idx = 0usize;
        while slot_idx < root_frontier_capacity {
            unsafe {
                rows.add(slot_idx).write(RootFrontierState::EMPTY);
            }
            let base = slot_idx.saturating_mul(observed_key_lane_word_count);
            let mut word_idx = 0usize;
            while word_idx < observed_key_lane_word_count {
                unsafe {
                    observed_key_offer_lanes.add(base + word_idx).write(0);
                    observed_key_binding_nonempty_lanes
                        .add(base + word_idx)
                        .write(0);
                }
                word_idx += 1;
            }
            slot_idx += 1;
        }
        let mut entry_idx = 0usize;
        while entry_idx < pool_capacity {
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
        unsafe {
            self.observed_key_offer_lanes
                .add(slot_idx.saturating_mul(self.observed_key_lane_word_count()))
        }
    }

    #[inline]
    fn observed_key_binding_nonempty_lanes_ptr(&self, slot_idx: usize) -> *mut LaneWord {
        unsafe {
            self.observed_key_binding_nonempty_lanes
                .add(slot_idx.saturating_mul(self.observed_key_lane_word_count()))
        }
    }

    #[inline]
    fn copy_row_observed_key_lanes(&mut self, dst_slot: usize, src_slot: usize) {
        let word_count = self.observed_key_lane_word_count();
        let src_offer =
            LaneSet::from_parts(self.observed_key_offer_lanes_ptr(src_slot), word_count).view();
        let src_binding = LaneSet::from_parts(
            self.observed_key_binding_nonempty_lanes_ptr(src_slot),
            word_count,
        )
        .view();
        let mut dst_offer =
            LaneSet::from_parts(self.observed_key_offer_lanes_ptr(dst_slot), word_count);
        let mut dst_binding = LaneSet::from_parts(
            self.observed_key_binding_nonempty_lanes_ptr(dst_slot),
            word_count,
        );
        dst_offer.copy_from(src_offer);
        dst_binding.copy_from(src_binding);
    }

    #[inline]
    pub(super) fn observed_key(&self, slot_idx: usize) -> FrontierObservationKey {
        let row = self[slot_idx];
        if row.active_len == 0 || !row.observed_key_valid() {
            return FrontierObservationKey::EMPTY;
        }
        FrontierObservationKey::from_parts(
            unsafe { self.observed_key_slots.add(row.active_start as usize) },
            row.active_len as usize,
            self.observed_key_offer_lanes_ptr(slot_idx),
            self.observed_key_binding_nonempty_lanes_ptr(slot_idx),
            self.observed_key_lane_word_count(),
        )
    }

    #[inline]
    fn clear_row_observed_key_lanes(&mut self, slot_idx: usize) {
        let mut key = FrontierObservationKey::from_parts(
            unsafe {
                self.observed_key_slots
                    .add(self[slot_idx].active_start as usize)
            },
            self[slot_idx].active_len as usize,
            self.observed_key_offer_lanes_ptr(slot_idx),
            self.observed_key_binding_nonempty_lanes_ptr(slot_idx),
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
            let existing = unsafe { *self.active_entries.add(start + insert_rel) };
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
            if unsafe { (*self.active_entries.add(start + remove_rel)).entry }
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
            unsafe { self.observed_key_slots.add(row.active_start as usize) },
            active_len,
            self.observed_key_offer_lanes_ptr(slot_idx),
            self.observed_key_binding_nonempty_lanes_ptr(slot_idx),
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
        unsafe { &*self.ptr.add(index) }
    }
}

impl IndexMut<usize> for RootFrontierTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        assert!(index < self.capacity(), "root frontier slot out of bounds");
        unsafe { &mut *self.ptr.add(index) }
    }
}

pub(super) struct FrontierState {
    pub(super) root_frontier_state: RootFrontierTable,
    #[cfg(test)]
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
        root_observed_binding_nonempty_lanes: *mut LaneWord,
        #[cfg(test)] offer_entry_slots: *mut OfferEntrySlot,
        root_frontier_capacity: usize,
        max_frontier_entries: usize,
        root_observed_lane_word_count: usize,
        #[cfg(test)] max_offer_entries: usize,
    ) {
        unsafe {
            RootFrontierTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).root_frontier_state),
                root_rows,
                root_active_entries,
                root_observed_key_slots,
                root_observed_offer_lanes,
                root_observed_binding_nonempty_lanes,
                root_frontier_capacity,
                max_frontier_entries,
                root_observed_lane_word_count,
            );
            #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
    #[inline]
    pub(super) fn set_offer_entry_state(&mut self, entry_idx: usize, state: OfferEntryState) {
        self.offer_entry_state.set(entry_idx, state);
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn clear_offer_entry_state(&mut self, entry_idx: usize) {
        self.set_offer_entry_state(entry_idx, OfferEntryState::EMPTY);
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn set_offer_entry_observed(
        &mut self,
        entry_idx: usize,
        observed: super::frontier::OfferEntryObservedState,
    ) {
        if !self.offer_entry_state.has_storage() {
            return;
        }
        if let Some(state) = self.offer_entry_state.get_mut(entry_idx) {
            state.observed = observed;
        }
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
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        global_frontier_observed_key: FrontierObservationKey,
        key: &FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        if use_root_observed_entries {
            let slot_idx = self.root_frontier_slot(current_parallel_root)?;
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
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        global_frontier_observed_key: FrontierObservationKey,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        if use_root_observed_entries {
            let Some(slot_idx) = self.root_frontier_slot(current_parallel_root) else {
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
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        mut global_frontier_observed_key: FrontierObservationKey,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        if use_root_observed_entries {
            let Some(slot_idx) = self.root_frontier_slot(current_parallel_root) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    use crate::global::const_dsl::ScopeId;
    use crate::global::role_program::lane_word_count;

    fn test_scope(raw: u64) -> ScopeId {
        ScopeId::from_raw(raw)
    }

    fn observed_key_storage(
        entries: &[usize],
    ) -> (
        std::vec::Vec<FrontierObservationSlot>,
        std::vec::Vec<usize>,
        std::vec::Vec<usize>,
        FrontierObservationKey,
    ) {
        let mut active_slots = std::vec::Vec::with_capacity(entries.len().max(1));
        active_slots.resize(entries.len().max(1), ActiveEntrySlot::EMPTY);
        let mut active = ActiveEntrySet::from_parts(active_slots.as_mut_ptr(), active_slots.len());
        active.clear();
        let mut idx = 0usize;
        while idx < entries.len() {
            assert!(active.insert_entry(entries[idx], idx as u8));
            idx += 1;
        }
        let mut key_slots = std::vec::Vec::with_capacity(entries.len().max(1));
        key_slots.resize(entries.len().max(1), FrontierObservationSlot::EMPTY);
        let mut offer_lanes = std::vec::Vec::with_capacity(lane_word_count(1));
        offer_lanes.resize(lane_word_count(1), 0usize);
        let mut binding_nonempty_lanes = std::vec::Vec::with_capacity(lane_word_count(1));
        binding_nonempty_lanes.resize(lane_word_count(1), 0usize);
        let mut key = FrontierObservationKey::from_parts(
            key_slots.as_mut_ptr(),
            key_slots.len(),
            offer_lanes.as_mut_ptr(),
            binding_nonempty_lanes.as_mut_ptr(),
            lane_word_count(1),
        );
        key.clear();
        key.set_active_entries_from(active);
        (key_slots, offer_lanes, binding_nonempty_lanes, key)
    }

    #[test]
    fn root_frontier_table_accepts_full_u8_lane_domain_rows() {
        const ROWS: usize = 256;
        let mut rows = std::vec::Vec::with_capacity(ROWS);
        rows.resize(ROWS, RootFrontierState::EMPTY);
        let mut active = [ActiveEntrySlot::EMPTY; 1];
        let mut observed = [FrontierObservationSlot::EMPTY; 1];
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                ROWS,
                1,
                0,
            );
        }
        let mut table = unsafe { table.assume_init() };

        for idx in 0..ROWS {
            table.prepare_row(idx, test_scope(idx as u64 + 1));
        }

        assert_eq!(table.capacity(), ROWS);
        assert_eq!(table.len(), ROWS);
        assert_eq!(table[255].root, test_scope(256));
    }

    #[test]
    fn global_active_entry_set_stays_packed_after_removal() {
        let mut slots = [ActiveEntrySlot::EMPTY; 4];
        let mut active = ActiveEntrySet::EMPTY;
        unsafe {
            ActiveEntrySet::init_from_parts(
                (&mut active) as *mut ActiveEntrySet,
                slots.as_mut_ptr(),
                4,
            );
        }
        assert!(active.insert_entry(10, 0));
        assert!(active.insert_entry(20, 1));
        assert!(active.remove_entry(10));
        assert_eq!(active.len(), 1);
        assert_eq!(active.entry_at(0), Some(20));
        assert_eq!(active.occupancy_mask(), 0b0000_0001);
    }

    #[test]
    fn global_frontier_cache_uses_external_key_storage() {
        let mut global_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut global_observed_offer_lanes = [0usize; lane_word_count(1)];
        let mut global_observed_binding_nonempty_lanes = [0usize; lane_word_count(1)];
        let mut root_rows = [RootFrontierState::EMPTY; 2];
        let mut root_active = [ActiveEntrySlot::EMPTY; 4];
        let mut root_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut root_observed_offer_lanes = [0usize; 2 * lane_word_count(1)];
        let mut root_observed_binding_nonempty_lanes = [0usize; 2 * lane_word_count(1)];
        let mut offer_slots = [OfferEntrySlot::EMPTY; 4];
        let mut observed_slots = [FrontierObservationSlot::EMPTY; 4];
        let mut frontier_state = MaybeUninit::<FrontierState>::uninit();
        unsafe {
            FrontierState::init_empty(
                frontier_state.as_mut_ptr(),
                root_rows.as_mut_ptr(),
                root_active.as_mut_ptr(),
                root_observed.as_mut_ptr(),
                root_observed_offer_lanes.as_mut_ptr(),
                root_observed_binding_nonempty_lanes.as_mut_ptr(),
                offer_slots.as_mut_ptr(),
                2,
                4,
                lane_word_count(1),
                4,
            );
        }
        let mut frontier_state = unsafe { frontier_state.assume_init() };
        let (_cached_key_slots, _cached_offer_lanes, _cached_binding_nonempty_lanes, cached_key) =
            observed_key_storage(&[7, 9]);
        let mut stored_key = FrontierObservationKey::from_parts(
            global_observed.as_mut_ptr(),
            4,
            global_observed_offer_lanes.as_mut_ptr(),
            global_observed_binding_nonempty_lanes.as_mut_ptr(),
            lane_word_count(1),
        );
        stored_key.clear();
        let mut observed_entries = ObservedEntrySet::from_parts(observed_slots.as_mut_ptr(), 4);
        assert_eq!(observed_entries.insert_entry(7), Some((0b0000_0001, true)));
        assert_eq!(observed_entries.insert_entry(9), Some((0b0000_0010, true)));

        frontier_state.store_frontier_observation(
            ScopeId::none(),
            false,
            stored_key,
            cached_key,
            observed_entries,
        );

        let (cached_key_after_store, cached_entries_after_store) =
            frontier_state.frontier_observation_cache(ScopeId::none(), false, stored_key);
        assert!(cached_key_after_store == cached_key);
        assert_eq!(cached_entries_after_store.entry_bit(7), 0b0000_0001);
        assert_eq!(cached_entries_after_store.entry_bit(9), 0b0000_0010);
        let cached_again = frontier_state.cached_frontier_observed_entries(
            ScopeId::none(),
            false,
            stored_key,
            &cached_key,
        );
        assert!(cached_again.is_some());
        let cached_again = cached_again.unwrap_or(ObservedEntrySet::EMPTY);
        assert_eq!(cached_again.entry_bit(7), 0b0000_0001);
        assert_eq!(cached_again.entry_bit(9), 0b0000_0010);
    }

    #[test]
    fn root_frontier_shared_active_pool_stays_packed_after_row_removal() {
        let mut rows = [RootFrontierState::EMPTY; 4];
        let mut active = [ActiveEntrySlot::EMPTY; 8];
        let mut observed = [FrontierObservationSlot::EMPTY; 8];
        let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
        let mut observed_binding_nonempty_lanes = [0usize; 3 * lane_word_count(1)];
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                observed_offer_lanes.as_mut_ptr(),
                observed_binding_nonempty_lanes.as_mut_ptr(),
                3,
                4,
                lane_word_count(1),
            );
        }
        let mut table = unsafe { table.assume_init() };

        table.prepare_row(0, test_scope(1));
        table.prepare_row(1, test_scope(2));
        assert!(table.insert_root_active_entry(0, 10, 0));
        assert!(table.insert_root_active_entry(1, 20, 1));

        assert_eq!(table.active_pool_used(), 2);
        assert_eq!(table[0].active_start, 0);
        assert_eq!(table[1].active_start, 1);

        table.remove_root_row(0);

        assert_eq!(table.len(), 1);
        assert_eq!(table[0].root, test_scope(2));
        assert_eq!(table[0].active_start, 0);
        assert_eq!(table[0].active_len, 1);
        assert_eq!(table.active_pool_used(), 1);
        assert_eq!(table.active_entry_set(0).entry_at(0), Some(20));
    }

    #[test]
    fn root_frontier_shared_observed_pool_stays_packed_after_row_removal() {
        let mut rows = [RootFrontierState::EMPTY; 4];
        let mut active = [ActiveEntrySlot::EMPTY; 8];
        let mut observed = [FrontierObservationSlot::EMPTY; 8];
        let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
        let mut observed_binding_nonempty_lanes = [0usize; 3 * lane_word_count(1)];
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                observed_offer_lanes.as_mut_ptr(),
                observed_binding_nonempty_lanes.as_mut_ptr(),
                3,
                4,
                lane_word_count(1),
            );
        }
        let mut table = unsafe { table.assume_init() };

        table.prepare_row(0, test_scope(1));
        table.prepare_row(1, test_scope(2));
        assert!(table.insert_root_active_entry(0, 10, 0));
        assert!(table.insert_root_active_entry(0, 11, 1));
        assert!(table.insert_root_active_entry(1, 20, 0));
        let (_key0_slots, _key0_offer_lanes, _key0_binding_nonempty_lanes, key0) =
            observed_key_storage(&[10, 11]);
        let (_key1_slots, _key1_offer_lanes, _key1_binding_nonempty_lanes, key1) =
            observed_key_storage(&[20]);
        table.replace_root_observed_key(0, key0);
        table.replace_root_observed_key(1, key1);

        assert_eq!(table.active_pool_used(), 3);
        assert_eq!(table[0].active_start, 0);
        assert_eq!(table[1].active_start, 2);

        table.remove_root_row(0);

        assert_eq!(table.len(), 1);
        assert_eq!(table[0].root, test_scope(2));
        assert_eq!(table[0].active_start, 0);
        assert_eq!(table[0].active_len, 1);
        let key = table.observed_key(0);
        assert_eq!(key.len(), 1);
        assert!(key.contains_entry(20));
        assert_eq!(table.active_pool_used(), 1);
    }

    #[test]
    fn root_frontier_observed_cache_invalidates_on_active_entry_change() {
        let mut rows = [RootFrontierState::EMPTY; 4];
        let mut active = [ActiveEntrySlot::EMPTY; 8];
        let mut observed = [FrontierObservationSlot::EMPTY; 8];
        let mut observed_offer_lanes = [0usize; 3 * lane_word_count(1)];
        let mut observed_binding_nonempty_lanes = [0usize; 3 * lane_word_count(1)];
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                observed_offer_lanes.as_mut_ptr(),
                observed_binding_nonempty_lanes.as_mut_ptr(),
                3,
                4,
                lane_word_count(1),
            );
        }
        let mut table = unsafe { table.assume_init() };

        table.prepare_row(0, test_scope(1));
        assert!(table.insert_root_active_entry(0, 10, 0));
        let (_key_slots, _key_offer_lanes, _key_binding_nonempty_lanes, key) =
            observed_key_storage(&[10]);
        table.replace_root_observed_key(0, key);
        assert_eq!(table.observed_key(0).len(), 1);

        assert!(table.insert_root_active_entry(0, 11, 1));
        assert_eq!(table.observed_key(0).len(), 0);
        assert!(table[0].observed_entries == ObservedEntrySummary::EMPTY);
    }

    #[test]
    fn next_observation_epoch_wrap_clears_shared_root_observed_pool() {
        let mut global_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut global_observed_offer_lanes = [0usize; lane_word_count(1)];
        let mut global_observed_binding_nonempty_lanes = [0usize; lane_word_count(1)];
        let mut root_rows = [RootFrontierState::EMPTY; 2];
        let mut root_active = [ActiveEntrySlot::EMPTY; 4];
        let mut root_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut root_observed_offer_lanes = [0usize; 2 * lane_word_count(1)];
        let mut root_observed_binding_nonempty_lanes = [0usize; 2 * lane_word_count(1)];
        let mut offer_slots = [OfferEntrySlot::EMPTY; 4];
        let mut frontier_state = MaybeUninit::<FrontierState>::uninit();
        unsafe {
            FrontierState::init_empty(
                frontier_state.as_mut_ptr(),
                root_rows.as_mut_ptr(),
                root_active.as_mut_ptr(),
                root_observed.as_mut_ptr(),
                root_observed_offer_lanes.as_mut_ptr(),
                root_observed_binding_nonempty_lanes.as_mut_ptr(),
                offer_slots.as_mut_ptr(),
                2,
                4,
                lane_word_count(1),
                4,
            );
        }
        let mut frontier_state = unsafe { frontier_state.assume_init() };
        let mut global_frontier_observed_key = FrontierObservationKey::from_parts(
            global_observed.as_mut_ptr(),
            4,
            global_observed_offer_lanes.as_mut_ptr(),
            global_observed_binding_nonempty_lanes.as_mut_ptr(),
            lane_word_count(1),
        );
        global_frontier_observed_key.clear();
        frontier_state
            .root_frontier_state
            .prepare_row(0, test_scope(1));
        assert!(
            frontier_state
                .root_frontier_state
                .insert_root_active_entry(0, 3, 0)
        );
        assert!(
            frontier_state
                .root_frontier_state
                .insert_root_active_entry(0, 5, 1)
        );
        let (_root_key_slots, _root_key_offer_lanes, _root_key_binding_nonempty_lanes, root_key) =
            observed_key_storage(&[3, 5]);
        frontier_state
            .root_frontier_state
            .replace_root_observed_key(0, root_key);
        frontier_state.root_frontier_state[0].observed_entries = ObservedEntrySummary {
            controller_mask: 1,
            dynamic_controller_mask: 1,
            progress_mask: 1,
            ready_arm_mask: 1,
            ready_mask: 1,
        };
        let (
            _global_key_slots,
            _global_key_offer_lanes,
            _global_key_binding_nonempty_lanes,
            global_key,
        ) = observed_key_storage(&[7]);
        global_frontier_observed_key.copy_from(global_key);
        frontier_state.global_frontier_observed = ObservedEntrySummary {
            controller_mask: 1,
            dynamic_controller_mask: 0,
            progress_mask: 1,
            ready_arm_mask: 0,
            ready_mask: 1,
        };
        frontier_state.frontier_observation_epoch = u16::MAX;

        assert_eq!(
            frontier_state.next_observation_epoch(&mut global_frontier_observed_key),
            1
        );
        assert_eq!(global_frontier_observed_key.len(), 0);
        assert!(frontier_state.global_frontier_observed == ObservedEntrySummary::EMPTY);
        assert!(
            frontier_state.root_frontier_state[0].observed_entries == ObservedEntrySummary::EMPTY
        );
        assert_eq!(frontier_state.root_frontier_state.observed_key(0).len(), 0);
    }
}
