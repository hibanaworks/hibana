//! Mutable frontier-state owner for endpoint kernel runtime bookkeeping.
//!
//! # Unsafe Owner Contract
//!
//! This module owns mutable frontier-state buffers for one endpoint runtime
//! image. Unsafe blocks here may expose table entries only within the initialized
//! capacity recorded on the same frontier state object.

use core::ops::{Index, IndexMut};

use super::frontier::{
    ActiveEntrySet, ActiveEntrySlot, FrontierVisitSet, LaneOfferState, OfferEntryKey,
    RootFrontierState,
};
use crate::global::{
    const_dsl::ScopeId, role_program::frontier_visit_capacity, typestate::StateIndex,
};

pub(super) struct RootFrontierTable {
    ptr: *mut RootFrontierState,
    active_entries: *mut ActiveEntrySlot,
    capacity: u16,
    pool_capacity: u16,
}

pub(super) struct RootFrontierStorage {
    pub(super) rows: *mut RootFrontierState,
    pub(super) active_entries: *mut ActiveEntrySlot,
}

pub(super) struct RootFrontierCapacity {
    pub(super) row_count: usize,
    pub(super) pool_capacity: usize,
}

pub(super) struct FrontierStateStorage {
    pub(super) root: RootFrontierStorage,
    pub(super) visited_entries: *mut StateIndex,
}

pub(super) struct FrontierStateCapacity {
    pub(super) root: RootFrontierCapacity,
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
        if capacity.pool_capacity > u16::MAX as usize {
            crate::invariant();
        }
        if (capacity.row_count != 0 && storage.rows.is_null())
            || (capacity.pool_capacity != 0 && storage.active_entries.is_null())
        {
            crate::invariant();
        }
        /* SAFETY: `FrontierState::init_empty` passes an unpublished root table; row and active-entry columns are installed before borrow. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(storage.rows);
            core::ptr::addr_of_mut!((*dst).active_entries).write(storage.active_entries);
            core::ptr::addr_of_mut!((*dst).capacity).write(capacity.row_count as u16);
            core::ptr::addr_of_mut!((*dst).pool_capacity).write(capacity.pool_capacity as u16);
        }
        let mut slot_idx = 0usize;
        while slot_idx < capacity.row_count {
            /* SAFETY: `slot_idx < row_count` selects one unpublished root row; every row starts EMPTY before exposure. */
            unsafe {
                storage.rows.add(slot_idx).write(RootFrontierState::EMPTY);
            }
            slot_idx += 1;
        }
        let mut entry_idx = 0usize;
        while entry_idx < capacity.pool_capacity {
            /* SAFETY: `entry_idx < pool_capacity` selects one active-entry pool slot initialized before publication. */
            unsafe {
                storage
                    .active_entries
                    .add(entry_idx)
                    .write(ActiveEntrySlot::EMPTY);
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
    fn visit_capacity(&self) -> usize {
        frontier_visit_capacity(self.pool_capacity())
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
    fn checked_active_span(&self, slot_idx: usize) -> (usize, usize, usize) {
        let row = self[slot_idx];
        let start = row.active_start as usize;
        let len = row.active_len as usize;
        let end = match start.checked_add(len) {
            Some(end) => end,
            None => crate::invariant(),
        };
        let used = self.active_pool_used();
        if end > used || used > self.pool_capacity() {
            crate::invariant();
        }
        (start, len, used)
    }

    #[inline]
    fn active_entry_set(&self, slot_idx: usize) -> ActiveEntrySet<'_> {
        let (start, len, _) = self.checked_active_span(slot_idx);
        if len == 0 {
            return ActiveEntrySet::EMPTY;
        }
        /* SAFETY: row active span is allocated from this table's initialized
        active-entry pool and stays immutable while this read-only view is
        consumed by the current endpoint operation. */
        unsafe { ActiveEntrySet::from_parts(self.active_entries.add(start), len) }
    }

    #[inline]
    fn clear_row(&mut self, slot_idx: usize) {
        self[slot_idx] = RootFrontierState::EMPTY;
    }

    #[inline]
    fn prepare_row(&mut self, slot_idx: usize, root: ScopeId) {
        if slot_idx != self.len() || slot_idx >= self.capacity() {
            crate::invariant();
        }
        let active_start = self.active_pool_used();
        if active_start > self.pool_capacity() || active_start > u16::MAX as usize {
            crate::invariant();
        }
        let row = &mut self[slot_idx];
        row.root = root;
        row.active_start = active_start as u16;
        row.active_len = 0;
    }

    fn insert_root_active_entry(&mut self, slot_idx: usize, key: OfferEntryKey, lane_idx: u8) {
        if slot_idx >= self.len() {
            crate::invariant();
        }
        let Some(incoming) = ActiveEntrySlot::new(key, lane_idx) else {
            crate::invariant();
        };
        let (start, len, used) = self.checked_active_span(slot_idx);
        let mut insert_rel = 0usize;
        while insert_rel < len {
            let existing = /* SAFETY: `insert_rel < row.active_len` bounds a shared read inside the root row active-entry span. */ unsafe { *self.active_entries.add(start + insert_rel) };
            if existing.key == key {
                crate::invariant();
            }
            if incoming.precedes(existing) {
                break;
            }
            insert_rel += 1;
        }
        if used >= self.pool_capacity() {
            crate::invariant();
        }
        let row_len = self.len();
        if self[slot_idx].active_len == u16::MAX {
            crate::invariant();
        }
        let mut shifted_row = slot_idx + 1;
        while shifted_row < row_len {
            if self[shifted_row].active_start == u16::MAX {
                crate::invariant();
            }
            shifted_row += 1;
        }
        let insert_idx = start + insert_rel;
        let mut idx = used;
        while idx > insert_idx {
            /* SAFETY: `idx` and `idx - 1` are inside initialized active-entry pool; `&mut self` owns this insert shift. */
            unsafe {
                self.active_entries
                    .add(idx)
                    .write(*self.active_entries.add(idx - 1));
            }
            idx -= 1;
        }
        /* SAFETY: `insert_idx < pool_capacity`; active entry is written before row lengths change. */
        unsafe {
            self.active_entries.add(insert_idx).write(incoming);
        }
        self[slot_idx].active_len += 1;
        let mut idx = slot_idx + 1;
        while idx < row_len {
            self[idx].active_start += 1;
            idx += 1;
        }
    }

    fn remove_root_active_entry(&mut self, slot_idx: usize, key: OfferEntryKey) {
        if slot_idx >= self.len() {
            crate::invariant();
        }
        if key.is_absent() {
            crate::invariant();
        }
        let (start, len, used) = self.checked_active_span(slot_idx);
        let mut remove_rel = 0usize;
        while remove_rel < len {
            if
            /* SAFETY: `remove_rel < row.active_len` bounds this shared read inside the root row active-entry span. */
            unsafe { (*self.active_entries.add(start + remove_rel)).key } == key {
                break;
            }
            remove_rel += 1;
        }
        if remove_rel >= len {
            crate::invariant();
        }
        if self[slot_idx].active_len == 0 {
            crate::invariant();
        }
        let row_len = self.len();
        let mut shifted_row = slot_idx + 1;
        while shifted_row < row_len {
            if self[shifted_row].active_start == 0 {
                crate::invariant();
            }
            shifted_row += 1;
        }
        let remove_idx = start + remove_rel;
        let mut idx = remove_idx;
        while idx + 1 < used {
            /* SAFETY: `idx` and `idx + 1` are inside used active-entry pool; `&mut self` owns removal compaction. */
            unsafe {
                self.active_entries
                    .add(idx)
                    .write(*self.active_entries.add(idx + 1));
            }
            idx += 1;
        }
        if used != 0 {
            /* SAFETY: `used - 1` is the old tail slot after compaction; clearing removes stale cells. */
            unsafe {
                self.active_entries
                    .add(used - 1)
                    .write(ActiveEntrySlot::EMPTY);
            }
        }
        self[slot_idx].active_len -= 1;
        let mut idx = slot_idx + 1;
        while idx < row_len {
            self[idx].active_start -= 1;
            idx += 1;
        }
    }

    fn remove_root_row(&mut self, slot_idx: usize) {
        let len = self.len();
        if slot_idx >= len {
            crate::invariant();
        }

        let (start, active_span, used) = self.checked_active_span(slot_idx);
        let mut shifted_row = slot_idx + 1;
        while shifted_row < len {
            if active_span > self[shifted_row].active_start as usize {
                crate::invariant();
            }
            shifted_row += 1;
        }
        if active_span != 0 {
            let mut idx = start;
            while idx + active_span < used {
                /* SAFETY: `idx` and `idx + active_span` are inside the initialized used active-entry pool; `&mut self` owns the root-row removal shift. */
                unsafe {
                    self.active_entries
                        .add(idx)
                        .write(*self.active_entries.add(idx + active_span));
                }
                idx += 1;
            }
            let mut clear_idx = used - active_span;
            while clear_idx < used {
                /* SAFETY: `clear_idx` walks the old tail span after compaction; clearing removes stale cells. */
                unsafe {
                    self.active_entries
                        .add(clear_idx)
                        .write(ActiveEntrySlot::EMPTY);
                }
                clear_idx += 1;
            }
        }

        let mut idx = slot_idx + 1;
        while idx < len {
            let mut shifted = self[idx];
            if active_span != 0 {
                shifted.active_start -= active_span as u16;
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
        if index >= self.capacity() {
            crate::invariant();
        }
        /* SAFETY: `index < capacity` bounds the initialized root row column; `&self` creates only a shared row borrow. */
        unsafe { &*self.ptr.add(index) }
    }
}

impl IndexMut<usize> for RootFrontierTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.capacity() {
            crate::invariant();
        }
        /* SAFETY: `index < capacity` bounds the initialized root row column, and `&mut self` is the row mutation token. */
        unsafe { &mut *self.ptr.add(index) }
    }
}

pub(super) struct FrontierState {
    pub(super) root_frontier_state: RootFrontierTable,
    visited_entries: *mut StateIndex,
}

impl FrontierState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        storage: FrontierStateStorage,
        capacity: FrontierStateCapacity,
    ) {
        if capacity.root.pool_capacity != 0 && storage.visited_entries.is_null() {
            crate::invariant();
        }
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
            core::ptr::addr_of_mut!((*dst).visited_entries).write(storage.visited_entries);
        }
    }

    #[inline]
    pub(super) fn empty_frontier_visit_set(&mut self) -> FrontierVisitSet {
        /* SAFETY: `visited_entries` is the endpoint-owned arena section sized
        for the current entry plus every active-frontier entry. One public
        offer operation owns the returned initialized prefix until completion. */
        unsafe {
            FrontierVisitSet::from_parts(
                self.visited_entries,
                self.root_frontier_state.visit_capacity(),
            )
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
    pub(super) fn root_frontier_has_active_entries(&self, root: ScopeId) -> bool {
        match self.root_frontier_slot(root) {
            Some(slot) => self.root_frontier_state.active_entry_set(slot).len() != 0,
            None => false,
        }
    }

    #[inline]
    pub(super) fn root_frontier_active_entries(&self, root: ScopeId) -> ActiveEntrySet<'_> {
        match self.root_frontier_slot(root) {
            Some(slot) => self.root_frontier_state.active_entry_set(slot),
            None => ActiveEntrySet::EMPTY,
        }
    }

    pub(super) fn remove_root_frontier_slot(&mut self, slot_idx: usize) {
        self.root_frontier_state.remove_root_row(slot_idx);
    }

    #[inline]
    pub(super) fn attach_offer_entry_to_root_frontier(
        &mut self,
        key: OfferEntryKey,
        root: ScopeId,
        lane_idx: u8,
    ) {
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            crate::invariant();
        };
        self.root_frontier_state
            .insert_root_active_entry(slot_idx, key, lane_idx);
    }

    #[inline]
    pub(super) fn detach_offer_entry_from_root_frontier(
        &mut self,
        key: OfferEntryKey,
        root: ScopeId,
    ) {
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            crate::invariant();
        };
        self.root_frontier_state
            .remove_root_active_entry(slot_idx, key);
    }

    pub(super) fn detach_lane_from_root_frontier(&mut self, info: LaneOfferState) {
        let root = info.parallel_root;
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            crate::invariant();
        };
        if self.root_frontier_state.active_entry_set(slot_idx).len() == 0 {
            self.remove_root_frontier_slot(slot_idx);
        }
    }

    pub(super) fn attach_lane_to_root_frontier(&mut self, info: LaneOfferState) {
        let root = info.parallel_root;
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

#[cfg(kani)]
#[path = "frontier_state/kani.rs"]
mod kani;
