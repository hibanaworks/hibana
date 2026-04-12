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

pub(super) struct RootFrontierTable {
    ptr: *mut RootFrontierState,
    active_entries: *mut ActiveEntrySlot,
    observed_key_slots: *mut FrontierObservationSlot,
    capacity: u8,
    pool_capacity: u8,
}

impl RootFrontierTable {
    unsafe fn init_from_parts(
        dst: *mut Self,
        rows: *mut RootFrontierState,
        active_entries: *mut ActiveEntrySlot,
        observed_key_slots: *mut FrontierObservationSlot,
        root_frontier_capacity: usize,
        pool_capacity: usize,
    ) {
        if root_frontier_capacity > u8::MAX as usize {
            panic!("root frontier row capacity overflow");
        }
        if pool_capacity > u8::MAX as usize {
            panic!("root frontier pool capacity overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(rows);
            core::ptr::addr_of_mut!((*dst).active_entries).write(active_entries);
            core::ptr::addr_of_mut!((*dst).observed_key_slots).write(observed_key_slots);
            core::ptr::addr_of_mut!((*dst).capacity).write(root_frontier_capacity as u8);
            core::ptr::addr_of_mut!((*dst).pool_capacity).write(pool_capacity as u8);
        }
        let mut slot_idx = 0usize;
        while slot_idx < root_frontier_capacity {
            unsafe {
                rows.add(slot_idx).write(RootFrontierState::EMPTY);
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
    pub(super) fn observed_key(&self, slot_idx: usize) -> FrontierObservationKey {
        let row = self[slot_idx];
        if row.active_len == 0 || !row.observed_key_valid() {
            return FrontierObservationKey::EMPTY;
        }
        let mut key = FrontierObservationKey::from_parts(
            unsafe { self.observed_key_slots.add(row.active_start as usize) },
            row.active_len as usize,
        );
        key.offer_lane_mask = row.observed_key_offer_lane_mask();
        key.binding_nonempty_mask = row.observed_binding_nonempty_mask;
        key
    }

    #[inline]
    fn clear_row(&mut self, slot_idx: usize) {
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
        let start = row.active_start as usize;
        let active_len = row.active_len as usize;
        let new_len = key.len();
        if new_len == 0 {
            let mut idx = 0usize;
            while idx < active_len {
                unsafe {
                    self.observed_key_slots
                        .add(start + idx)
                        .write(FrontierObservationSlot::EMPTY);
                }
                idx += 1;
            }
            self[slot_idx].clear_observed_key_cache();
            return;
        }
        assert_eq!(
            new_len, active_len,
            "root frontier observed-key length must track active entries"
        );
        debug_assert!(key.exact_entries_match(self.active_entry_set(slot_idx)));
        let mut idx = 0usize;
        while idx < new_len {
            unsafe {
                self.observed_key_slots
                    .add(start + idx)
                    .write(key.slots[idx]);
            }
            idx += 1;
        }
        self[slot_idx].set_observed_key_cache_masks(key.offer_lane_mask, key.binding_nonempty_mask);
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
    #[cfg(test)]
    pub(super) global_frontier_observed_offer_lane_mask: u8,
    #[cfg(test)]
    pub(super) global_frontier_observed_binding_nonempty_mask: u8,
    pub(super) global_frontier_scratch_initialized: bool,
}

impl FrontierState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        root_rows: *mut RootFrontierState,
        root_active_entries: *mut ActiveEntrySlot,
        root_observed_key_slots: *mut FrontierObservationSlot,
        #[cfg(test)] offer_entry_slots: *mut OfferEntrySlot,
        root_frontier_capacity: usize,
        max_frontier_entries: usize,
        #[cfg(test)] max_offer_entries: usize,
    ) {
        unsafe {
            RootFrontierTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).root_frontier_state),
                root_rows,
                root_active_entries,
                root_observed_key_slots,
                root_frontier_capacity,
                max_frontier_entries,
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
            #[cfg(test)]
            core::ptr::addr_of_mut!((*dst).global_frontier_observed_offer_lane_mask).write(0);
            #[cfg(test)]
            core::ptr::addr_of_mut!((*dst).global_frontier_observed_binding_nonempty_mask).write(0);
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
        self.offer_entry_state.clear(entry_idx);
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn set_offer_entry_active_mask(&mut self, entry_idx: usize, active_mask: u8) {
        if let Some(state) = self.offer_entry_state.get_mut(entry_idx) {
            state.active_mask = active_mask;
        }
    }

    #[cfg(test)]
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
            self.global_frontier_observed_offer_lane_mask = 0;
            self.global_frontier_observed_binding_nonempty_mask = 0;
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

    #[cfg(test)]
    #[inline]
    pub(super) fn global_frontier_observed_entry_bit(
        &self,
        global_frontier_observed_key: FrontierObservationKey,
        entry_idx: usize,
    ) -> u8 {
        self.global_frontier_observed_entries(global_frontier_observed_key)
            .entry_bit(entry_idx)
    }

    #[cfg(test)]
    #[inline]
    #[cfg(test)]
    pub(super) fn overwrite_global_frontier_observed(
        &mut self,
        global_frontier_observed_key: &mut FrontierObservationKey,
        src: ObservedEntrySet,
    ) {
        global_frontier_observed_key.copy_slots_from_observed_entries(src);
        self.global_frontier_observed = src.summary();
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
        self.global_frontier_observed_offer_lane_mask = key.offer_lane_mask;
        self.global_frontier_observed_binding_nonempty_mask = key.binding_nonempty_mask;
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

    fn test_scope(raw: u64) -> ScopeId {
        ScopeId::from_raw(raw)
    }

    fn observed_key(entries: &[usize]) -> FrontierObservationKey {
        let mut active = ActiveEntrySet::EMPTY;
        let mut idx = 0usize;
        while idx < entries.len() {
            assert!(active.insert_entry(entries[idx], idx as u8));
            idx += 1;
        }
        let mut key = FrontierObservationKey::EMPTY;
        key.set_active_entries_from(active);
        key
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
        let mut root_rows = [RootFrontierState::EMPTY; 2];
        let mut root_active = [ActiveEntrySlot::EMPTY; 4];
        let mut root_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut offer_slots = [OfferEntrySlot::EMPTY; 4];
        let mut observed_slots = [FrontierObservationSlot::EMPTY; 4];
        let mut frontier_state = MaybeUninit::<FrontierState>::uninit();
        unsafe {
            FrontierState::init_empty(
                frontier_state.as_mut_ptr(),
                root_rows.as_mut_ptr(),
                root_active.as_mut_ptr(),
                root_observed.as_mut_ptr(),
                offer_slots.as_mut_ptr(),
                2,
                4,
                4,
            );
        }
        let mut frontier_state = unsafe { frontier_state.assume_init() };
        let cached_key = observed_key(&[7, 9]);
        let mut stored_key = FrontierObservationKey::from_parts(global_observed.as_mut_ptr(), 4);
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
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                3,
                4,
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
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                3,
                4,
            );
        }
        let mut table = unsafe { table.assume_init() };

        table.prepare_row(0, test_scope(1));
        table.prepare_row(1, test_scope(2));
        assert!(table.insert_root_active_entry(0, 10, 0));
        assert!(table.insert_root_active_entry(0, 11, 1));
        assert!(table.insert_root_active_entry(1, 20, 0));
        table.replace_root_observed_key(0, observed_key(&[10, 11]));
        table.replace_root_observed_key(1, observed_key(&[20]));

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
        let mut table = MaybeUninit::<RootFrontierTable>::uninit();
        unsafe {
            RootFrontierTable::init_from_parts(
                table.as_mut_ptr(),
                rows.as_mut_ptr(),
                active.as_mut_ptr(),
                observed.as_mut_ptr(),
                3,
                4,
            );
        }
        let mut table = unsafe { table.assume_init() };

        table.prepare_row(0, test_scope(1));
        assert!(table.insert_root_active_entry(0, 10, 0));
        table.replace_root_observed_key(0, observed_key(&[10]));
        assert_eq!(table.observed_key(0).len(), 1);

        assert!(table.insert_root_active_entry(0, 11, 1));
        assert_eq!(table.observed_key(0).len(), 0);
        assert!(table[0].observed_entries == ObservedEntrySummary::EMPTY);
    }

    #[test]
    fn next_observation_epoch_wrap_clears_shared_root_observed_pool() {
        let mut global_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut root_rows = [RootFrontierState::EMPTY; 2];
        let mut root_active = [ActiveEntrySlot::EMPTY; 4];
        let mut root_observed = [FrontierObservationSlot::EMPTY; 4];
        let mut offer_slots = [OfferEntrySlot::EMPTY; 4];
        let mut frontier_state = MaybeUninit::<FrontierState>::uninit();
        unsafe {
            FrontierState::init_empty(
                frontier_state.as_mut_ptr(),
                root_rows.as_mut_ptr(),
                root_active.as_mut_ptr(),
                root_observed.as_mut_ptr(),
                offer_slots.as_mut_ptr(),
                2,
                4,
                4,
            );
        }
        let mut frontier_state = unsafe { frontier_state.assume_init() };
        let mut global_frontier_observed_key =
            FrontierObservationKey::from_parts(global_observed.as_mut_ptr(), 4);
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
        frontier_state
            .root_frontier_state
            .replace_root_observed_key(0, observed_key(&[3, 5]));
        frontier_state.root_frontier_state[0].observed_entries = ObservedEntrySummary {
            controller_mask: 1,
            dynamic_controller_mask: 1,
            progress_mask: 1,
            ready_arm_mask: 1,
            ready_mask: 1,
        };
        global_frontier_observed_key.copy_from(observed_key(&[7]));
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
