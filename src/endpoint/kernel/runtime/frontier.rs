//! Frontier-selection helpers for `offer()`.

use core::{
    convert::TryFrom,
    future::poll_fn,
    mem,
    ops::{Deref, DerefMut, Index, IndexMut},
    slice,
    task::Poll,
};

#[cfg(test)]
use super::evidence::ScopeLabelMeta;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::LaneMask;
#[cfg(test)]
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{MAX_STATES, StateIndex, state_index_to_usize};

const FRONTIER_SLOT_MASK_BITS: usize = u8::BITS as usize;
const OFFER_LANE_MASK_LANES: usize = LaneMask::BITS as usize;

#[cfg(test)]
use super::offer::{CurrentScopeSelectionMeta, ScopeArmMaterializationMeta};

#[cfg(test)]
use core::cell::{Cell, UnsafeCell};
#[cfg(test)]
use std::thread_local;

#[cfg(test)]
const TEST_FRONTIER_POOL_CAPACITY: usize = 4096;

#[cfg(test)]
const TEST_OFFER_ENTRY_CAPACITY: usize = if MAX_LANES < 8 { 8 } else { MAX_LANES };

#[cfg(test)]
#[derive(Clone, Copy)]
struct ActiveEntrySetTestStorage {
    slots: [ActiveEntrySlot; MAX_LANES],
}

#[cfg(test)]
impl ActiveEntrySetTestStorage {
    const EMPTY: Self = Self {
        slots: [ActiveEntrySlot::EMPTY; MAX_LANES],
    };
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct ObservedEntrySetTestStorage {
    slots: [FrontierObservationSlot; MAX_LANES],
}

#[cfg(test)]
impl ObservedEntrySetTestStorage {
    const EMPTY: Self = Self {
        slots: [FrontierObservationSlot::EMPTY; MAX_LANES],
    };
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct FrontierObservationKeyTestStorage {
    slots: [FrontierObservationSlot; MAX_LANES],
}

#[cfg(test)]
impl FrontierObservationKeyTestStorage {
    const EMPTY: Self = Self {
        slots: [FrontierObservationSlot::EMPTY; MAX_LANES],
    };
}

#[cfg(test)]
thread_local! {
    static ACTIVE_ENTRY_SET_TEST_POOL: UnsafeCell<[ActiveEntrySetTestStorage; TEST_FRONTIER_POOL_CAPACITY]> =
        const { UnsafeCell::new([ActiveEntrySetTestStorage::EMPTY; TEST_FRONTIER_POOL_CAPACITY]) };
    static ACTIVE_ENTRY_SET_TEST_NEXT: Cell<usize> = const { Cell::new(0) };
    static OBSERVED_ENTRY_SET_TEST_POOL: UnsafeCell<[ObservedEntrySetTestStorage; TEST_FRONTIER_POOL_CAPACITY]> =
        const { UnsafeCell::new([ObservedEntrySetTestStorage::EMPTY; TEST_FRONTIER_POOL_CAPACITY]) };
    static OBSERVED_ENTRY_SET_TEST_NEXT: Cell<usize> = const { Cell::new(0) };
    static FRONTIER_OBSERVATION_KEY_TEST_POOL: UnsafeCell<[FrontierObservationKeyTestStorage; TEST_FRONTIER_POOL_CAPACITY]> =
        const { UnsafeCell::new([FrontierObservationKeyTestStorage::EMPTY; TEST_FRONTIER_POOL_CAPACITY]) };
    static FRONTIER_OBSERVATION_KEY_TEST_NEXT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontierKind {
    Route,
    Loop,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    #[inline]
    pub(super) const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Route => 1,
            Self::Loop => 2,
            Self::Parallel => 3,
            Self::PassiveObserver => 4,
        }
    }

    #[inline]
    pub(super) const fn bit(self) -> u8 {
        match self {
            Self::Route => 1 << 0,
            Self::Loop => 1 << 1,
            Self::Parallel => 1 << 2,
            Self::PassiveObserver => 1 << 3,
        }
    }
}

#[inline]
pub(super) fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

#[derive(Clone, Copy)]
pub(super) struct LaneOfferState {
    pub(super) scope: ScopeId,
    pub(super) entry: StateIndex,
    pub(super) parallel_root: ScopeId,
    pub(super) frontier: FrontierKind,
    pub(super) static_ready: bool,
    pub(super) flags: u8,
}

impl LaneOfferState {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: StateIndex::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        static_ready: false,
        flags: 0,
    };

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) fn static_ready(self) -> bool {
        self.static_ready
    }
}

#[derive(Clone, Copy)]
pub(super) struct EntryBuffer<T> {
    ptr: *mut T,
    capacity: u8,
}

impl<T> EntryBuffer<T> {
    pub(super) const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        capacity: 0,
    };

    #[cfg(test)]
    #[inline]
    pub(super) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut T, capacity: usize) {
        if capacity > u8::MAX as usize {
            panic!("entry buffer capacity overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).capacity).write(capacity as u8);
        }
    }

    #[inline]
    pub(super) const fn capacity(&self) -> usize {
        self.capacity as usize
    }

    #[inline]
    pub(super) const fn from_parts(ptr: *mut T, capacity: usize) -> Self {
        if capacity > u8::MAX as usize {
            panic!("entry buffer capacity overflow");
        }
        Self {
            ptr,
            capacity: capacity as u8,
        }
    }

    #[inline]
    pub(super) fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.ptr, self.capacity()) }
        }
    }

    #[inline]
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.ptr, self.capacity()) }
        }
    }
}

impl<T> Deref for EntryBuffer<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for EntryBuffer<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T: PartialEq> PartialEq for EntryBuffer<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq> Eq for EntryBuffer<T> {}

impl<T, I> Index<I> for EntryBuffer<T>
where
    [T]: Index<I>,
{
    type Output = <[T] as Index<I>>::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T, I> IndexMut<I> for EntryBuffer<T>
where
    [T]: IndexMut<I>,
{
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.as_mut_slice()[index]
    }
}

#[cfg(test)]
fn alloc_active_entry_set_test_storage() -> *mut ActiveEntrySlot {
    ACTIVE_ENTRY_SET_TEST_NEXT.with(|next| {
        ACTIVE_ENTRY_SET_TEST_POOL.with(|pool| {
            let idx = next.get();
            if idx >= TEST_FRONTIER_POOL_CAPACITY {
                panic!("active entry set test storage exhausted");
            }
            next.set(idx + 1);
            let storage = unsafe { &mut (*pool.get())[idx] };
            storage.slots.fill(ActiveEntrySlot::EMPTY);
            storage.slots.as_mut_ptr()
        })
    })
}

#[cfg(test)]
fn alloc_observed_entry_set_test_storage() -> *mut FrontierObservationSlot {
    OBSERVED_ENTRY_SET_TEST_NEXT.with(|next| {
        OBSERVED_ENTRY_SET_TEST_POOL.with(|pool| {
            let idx = next.get();
            if idx >= TEST_FRONTIER_POOL_CAPACITY {
                panic!("observed entry set test storage exhausted");
            }
            next.set(idx + 1);
            let storage = unsafe { &mut (*pool.get())[idx] };
            storage.slots.fill(FrontierObservationSlot::EMPTY);
            storage.slots.as_mut_ptr()
        })
    })
}

#[cfg(test)]
fn alloc_frontier_observation_key_test_storage() -> *mut FrontierObservationSlot {
    FRONTIER_OBSERVATION_KEY_TEST_NEXT.with(|next| {
        FRONTIER_OBSERVATION_KEY_TEST_POOL.with(|pool| {
            let idx = next.get();
            if idx >= TEST_FRONTIER_POOL_CAPACITY {
                panic!("frontier observation key test storage exhausted");
            }
            next.set(idx + 1);
            let storage = unsafe { &mut (*pool.get())[idx] };
            storage.slots.fill(FrontierObservationSlot::EMPTY);
            storage.slots.as_mut_ptr()
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ActiveEntrySlot {
    pub(super) entry: StateIndex,
    pub(super) lane_idx: u8,
}

impl ActiveEntrySlot {
    pub(super) const EMPTY: Self = Self {
        entry: StateIndex::MAX,
        lane_idx: u8::MAX,
    };
}

#[derive(Clone, Copy)]
pub(super) struct ActiveEntrySet {
    pub(super) slots: EntryBuffer<ActiveEntrySlot>,
}

impl ActiveEntrySet {
    pub(super) const EMPTY: Self = Self {
        slots: EntryBuffer::EMPTY,
    };

    #[cfg(test)]
    #[inline]
    pub(super) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut ActiveEntrySlot,
        capacity: usize,
    ) {
        unsafe {
            EntryBuffer::init_from_parts(core::ptr::addr_of_mut!((*dst).slots), slots, capacity);
        }
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                slots.add(idx).write(ActiveEntrySlot::EMPTY);
            }
            idx += 1;
        }
    }

    #[cfg(test)]
    fn ensure_test_storage(&mut self) {
        if !self.slots.ptr.is_null() {
            return;
        }
        let slots = alloc_active_entry_set_test_storage();
        unsafe { Self::init_from_parts(self as *mut Self, slots, MAX_LANES) };
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        #[cfg(test)]
        self.ensure_test_storage();
        let capacity = self.slots.capacity();
        let mut idx = 0usize;
        while idx < capacity {
            self.slots[idx] = ActiveEntrySlot::EMPTY;
            idx += 1;
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn copy_from(&mut self, src: Self) {
        #[cfg(test)]
        self.ensure_test_storage();
        self.clear();
        let len = src.len();
        let mut idx = 0usize;
        while idx < len {
            self.slots[idx] = src.slots[idx];
            idx += 1;
        }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        let mut len = 0usize;
        while len < self.slots.capacity() {
            if self.slots[len].entry.is_max() {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline]
    pub(super) fn occupancy_mask(self) -> u8 {
        let len = self.len();
        if len >= FRONTIER_SLOT_MASK_BITS {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    pub(super) fn entry_at(self, slot_idx: usize) -> Option<usize> {
        if slot_idx >= self.len() {
            return None;
        }
        Some(state_index_to_usize(self.slots[slot_idx].entry))
    }

    #[inline]
    pub(super) fn entry_state(self, slot_idx: usize) -> StateIndex {
        if slot_idx >= self.len() {
            return StateIndex::MAX;
        }
        self.slots[slot_idx].entry
    }

    #[inline]
    pub(super) fn contains_only(self, entry_idx: usize) -> bool {
        self.len() == 1 && self.entry_at(0) == Some(entry_idx)
    }

    #[inline]
    pub(super) fn slot_for_entry(self, entry_idx: usize) -> Option<usize> {
        let entry = checked_state_index(entry_idx)?;
        let len = self.len();
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if self.slots[slot_idx].entry == entry {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    pub(super) fn insert_entry(&mut self, entry_idx: usize, lane_idx: u8) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let len = self.len();
        let mut insert_idx = 0usize;
        while insert_idx < len {
            let existing = self.slots[insert_idx];
            if existing.entry == entry {
                return false;
            }
            if existing.lane_idx > lane_idx
                || (existing.lane_idx == lane_idx && existing.entry.raw() > entry.raw())
            {
                break;
            }
            insert_idx += 1;
        }
        if len >= self.slots.capacity() || len >= FRONTIER_SLOT_MASK_BITS {
            return false;
        }
        let mut shift_idx = len;
        while shift_idx > insert_idx {
            self.slots[shift_idx] = self.slots[shift_idx - 1];
            shift_idx -= 1;
        }
        self.slots[insert_idx] = ActiveEntrySlot { entry, lane_idx };
        true
    }

    pub(super) fn remove_entry(&mut self, entry_idx: usize) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        let Ok(entry) = u16::try_from(entry_idx) else {
            return false;
        };
        let len = self.len();
        let mut idx = 0usize;
        while idx < len {
            if self.slots[idx].entry == entry {
                break;
            }
            idx += 1;
        }
        if idx >= len {
            return false;
        }
        while idx + 1 < len {
            self.slots[idx] = self.slots[idx + 1];
            idx += 1;
        }
        self.slots[len - 1] = ActiveEntrySlot::EMPTY;
        true
    }
}

#[derive(Clone, Copy)]
pub(super) struct ObservedEntrySet {
    pub(super) slots: EntryBuffer<FrontierObservationSlot>,
    pub(super) controller_mask: u8,
    pub(super) dynamic_controller_mask: u8,
    pub(super) progress_mask: u8,
    pub(super) ready_arm_mask: u8,
    pub(super) ready_mask: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ObservedEntrySummary {
    pub(super) controller_mask: u8,
    pub(super) dynamic_controller_mask: u8,
    pub(super) progress_mask: u8,
    pub(super) ready_arm_mask: u8,
    pub(super) ready_mask: u8,
}

impl ObservedEntrySummary {
    pub(super) const EMPTY: Self = Self {
        controller_mask: 0,
        dynamic_controller_mask: 0,
        progress_mask: 0,
        ready_arm_mask: 0,
        ready_mask: 0,
    };

    #[inline]
    pub(super) fn clear(&mut self) {
        *self = Self::EMPTY;
    }
}

#[cfg(not(test))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct GlobalFrontierObservedState {
    pub(super) summary: ObservedEntrySummary,
    pub(super) observation_epoch: u16,
    pub(super) offer_lane_mask: LaneMask,
    pub(super) binding_nonempty_mask: LaneMask,
}

#[cfg(not(test))]
impl GlobalFrontierObservedState {
    pub(super) const EMPTY: Self = Self {
        summary: ObservedEntrySummary::EMPTY,
        observation_epoch: 0,
        offer_lane_mask: 0,
        binding_nonempty_mask: 0,
    };
}

impl ObservedEntrySet {
    pub(super) const EMPTY: Self = Self {
        slots: EntryBuffer::EMPTY,
        controller_mask: 0,
        dynamic_controller_mask: 0,
        progress_mask: 0,
        ready_arm_mask: 0,
        ready_mask: 0,
    };

    #[cfg(test)]
    #[cfg(test)]
    #[inline]
    pub(super) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut FrontierObservationSlot,
        capacity: usize,
    ) {
        unsafe {
            EntryBuffer::init_from_parts(core::ptr::addr_of_mut!((*dst).slots), slots, capacity);
            core::ptr::addr_of_mut!((*dst).controller_mask).write(0);
            core::ptr::addr_of_mut!((*dst).dynamic_controller_mask).write(0);
            core::ptr::addr_of_mut!((*dst).progress_mask).write(0);
            core::ptr::addr_of_mut!((*dst).ready_arm_mask).write(0);
            core::ptr::addr_of_mut!((*dst).ready_mask).write(0);
        }
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                slots.add(idx).write(FrontierObservationSlot::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    pub(super) const fn from_parts(slots: *mut FrontierObservationSlot, capacity: usize) -> Self {
        Self::from_parts_with_summary(slots, capacity, ObservedEntrySummary::EMPTY)
    }

    #[inline]
    pub(super) const fn from_parts_with_summary(
        slots: *mut FrontierObservationSlot,
        capacity: usize,
        summary: ObservedEntrySummary,
    ) -> Self {
        Self {
            slots: EntryBuffer::from_parts(slots, capacity),
            controller_mask: summary.controller_mask,
            dynamic_controller_mask: summary.dynamic_controller_mask,
            progress_mask: summary.progress_mask,
            ready_arm_mask: summary.ready_arm_mask,
            ready_mask: summary.ready_mask,
        }
    }

    #[cfg(test)]
    fn ensure_test_storage(&mut self) {
        if !self.slots.ptr.is_null() {
            return;
        }
        let slots = alloc_observed_entry_set_test_storage();
        unsafe { Self::init_from_parts(self as *mut Self, slots, MAX_LANES) };
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        #[cfg(test)]
        self.ensure_test_storage();
        let mut idx = 0usize;
        while idx < self.slots.capacity() {
            self.slots[idx] = FrontierObservationSlot::EMPTY;
            idx += 1;
        }
        self.controller_mask = 0;
        self.dynamic_controller_mask = 0;
        self.progress_mask = 0;
        self.ready_arm_mask = 0;
        self.ready_mask = 0;
    }

    #[inline]
    pub(super) const fn summary(self) -> ObservedEntrySummary {
        ObservedEntrySummary {
            controller_mask: self.controller_mask,
            dynamic_controller_mask: self.dynamic_controller_mask,
            progress_mask: self.progress_mask,
            ready_arm_mask: self.ready_arm_mask,
            ready_mask: self.ready_mask,
        }
    }

    #[inline]
    pub(super) fn copy_from(&mut self, src: Self) {
        #[cfg(test)]
        self.ensure_test_storage();
        self.clear();
        let len = src.len();
        let mut idx = 0usize;
        while idx < len {
            self.slots[idx] = src.slots[idx];
            idx += 1;
        }
        self.controller_mask = src.controller_mask;
        self.dynamic_controller_mask = src.dynamic_controller_mask;
        self.progress_mask = src.progress_mask;
        self.ready_arm_mask = src.ready_arm_mask;
        self.ready_mask = src.ready_mask;
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        cached_frontier_observation_slots_len(self.slots)
    }

    #[inline]
    pub(super) fn occupancy_mask(self) -> u8 {
        let len = self.len();
        if len >= FRONTIER_SLOT_MASK_BITS {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    pub(super) fn frontier_mask(self, frontier: FrontierKind) -> u8 {
        let mut mask = 0u8;
        let mut slot_idx = 0usize;
        let len = self.len();
        while slot_idx < len {
            if (self.slots[slot_idx].meta.entry_summary_fingerprint & frontier.bit()) != 0 {
                mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        mask
    }

    #[inline]
    pub(super) fn slot_for_entry(self, entry_idx: usize) -> Option<usize> {
        let entry = checked_state_index(entry_idx)?;
        let len = self.len();
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if self.slots[slot_idx].entry == entry {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    pub(super) fn insert_entry(&mut self, entry_idx: usize) -> Option<(u8, bool)> {
        #[cfg(test)]
        self.ensure_test_storage();
        if entry_idx >= MAX_STATES {
            return None;
        }
        let entry = checked_state_index(entry_idx)?;
        if let Some(observed_idx) = self.slot_for_entry(entry_idx) {
            return Some((1u8 << observed_idx, false));
        }
        let observed_idx = self.len();
        if observed_idx >= self.slots.capacity() || observed_idx >= FRONTIER_SLOT_MASK_BITS {
            return None;
        }
        self.slots[observed_idx] = FrontierObservationSlot {
            entry,
            meta: FrontierObservationMetaSlot::EMPTY,
        };
        Some((1u8 << observed_idx, true))
    }

    #[inline]
    pub(super) fn entry_bit(self, entry_idx: usize) -> u8 {
        self.slot_for_entry(entry_idx).map_or(0, |slot| 1u8 << slot)
    }

    #[inline]
    pub(super) fn first_entry_idx(self, mask: u8) -> Option<usize> {
        if mask == 0 {
            return None;
        }
        let observed_idx = mask.trailing_zeros() as usize;
        if observed_idx >= self.len() {
            return None;
        }
        Some(state_index_to_usize(self.slots[observed_idx].entry))
    }

    #[inline]
    pub(super) fn observe_with_frontier_mask(
        &mut self,
        observed_bit: u8,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) {
        #[cfg(test)]
        self.ensure_test_storage();
        if observed.is_controller() {
            self.controller_mask |= observed_bit;
        }
        if observed.is_dynamic() {
            self.dynamic_controller_mask |= observed_bit;
        }
        if observed.has_progress_evidence() {
            self.progress_mask |= observed_bit;
        }
        if observed.has_ready_arm_evidence() {
            self.ready_arm_mask |= observed_bit;
        }
        if (observed.flags & OfferEntryObservedState::FLAG_READY) != 0 {
            self.ready_mask |= observed_bit;
        }
        if observed_bit != 0 {
            let slot_idx = observed_bit.trailing_zeros() as usize;
            if slot_idx < self.len() {
                let summary_bits = &mut self.slots[slot_idx].meta.entry_summary_fingerprint;
                *summary_bits = (*summary_bits & !0x0f) | (frontier_mask & 0x0f);
            }
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn observe(&mut self, observed_bit: u8, observed: OfferEntryObservedState) {
        self.observe_with_frontier_mask(observed_bit, observed, observed.frontier_mask);
    }

    #[inline]
    pub(super) fn replace_observation_with_frontier_mask(
        &mut self,
        entry_idx: usize,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        let observed_bit = self.entry_bit(entry_idx);
        if observed_bit == 0 {
            return false;
        }
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.observe_with_frontier_mask(observed_bit, observed, frontier_mask);
        true
    }

    pub(super) fn move_entry_slot(&mut self, entry_idx: usize, new_slot_idx: usize) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        let Some(old_slot_idx) = self.slot_for_entry(entry_idx) else {
            return false;
        };
        let len = self.len();
        if old_slot_idx >= len || new_slot_idx >= len {
            return false;
        }
        if old_slot_idx == new_slot_idx {
            return true;
        }
        let entry = self.slots[old_slot_idx];
        if old_slot_idx < new_slot_idx {
            let mut slot_idx = old_slot_idx;
            while slot_idx < new_slot_idx {
                self.slots[slot_idx] = self.slots[slot_idx + 1];
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = old_slot_idx;
            while slot_idx > new_slot_idx {
                self.slots[slot_idx] = self.slots[slot_idx - 1];
                slot_idx -= 1;
            }
        }
        self.slots[new_slot_idx] = entry;
        self.controller_mask =
            Self::move_slot_mask(self.controller_mask, len, old_slot_idx, new_slot_idx);
        self.dynamic_controller_mask = Self::move_slot_mask(
            self.dynamic_controller_mask,
            len,
            old_slot_idx,
            new_slot_idx,
        );
        self.progress_mask =
            Self::move_slot_mask(self.progress_mask, len, old_slot_idx, new_slot_idx);
        self.ready_arm_mask =
            Self::move_slot_mask(self.ready_arm_mask, len, old_slot_idx, new_slot_idx);
        self.ready_mask = Self::move_slot_mask(self.ready_mask, len, old_slot_idx, new_slot_idx);
        true
    }

    pub(super) fn insert_observation_at_slot_with_frontier_mask(
        &mut self,
        entry_idx: usize,
        slot_idx: usize,
        slot: FrontierObservationSlot,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        if entry_idx >= MAX_STATES {
            return false;
        }
        let len = self.len();
        if len >= self.slots.capacity() || len >= FRONTIER_SLOT_MASK_BITS || slot_idx > len {
            return false;
        }
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        if self.slot_for_entry(entry_idx).is_some() {
            return false;
        }
        let mut shift_idx = len;
        while shift_idx > slot_idx {
            self.slots[shift_idx] = self.slots[shift_idx - 1];
            shift_idx -= 1;
        }
        debug_assert!(slot.entry == entry);
        self.slots[slot_idx] = slot;
        self.controller_mask = Self::insert_slot_mask(self.controller_mask, len, slot_idx);
        self.dynamic_controller_mask =
            Self::insert_slot_mask(self.dynamic_controller_mask, len, slot_idx);
        self.progress_mask = Self::insert_slot_mask(self.progress_mask, len, slot_idx);
        self.ready_arm_mask = Self::insert_slot_mask(self.ready_arm_mask, len, slot_idx);
        self.ready_mask = Self::insert_slot_mask(self.ready_mask, len, slot_idx);
        self.observe_with_frontier_mask(1u8 << slot_idx, observed, frontier_mask);
        true
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn insert_observation_at_slot(
        &mut self,
        entry_idx: usize,
        slot_idx: usize,
        slot: FrontierObservationSlot,
        observed: OfferEntryObservedState,
    ) -> bool {
        self.insert_observation_at_slot_with_frontier_mask(
            entry_idx,
            slot_idx,
            slot,
            observed,
            observed.frontier_mask,
        )
    }

    pub(super) fn remove_observation(&mut self, entry_idx: usize) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        let Some(slot_idx) = self.slot_for_entry(entry_idx) else {
            return false;
        };
        let len = self.len();
        if slot_idx >= len {
            return false;
        }
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        if self.slots[slot_idx].entry != entry {
            return false;
        }
        let mut shift_idx = slot_idx;
        while shift_idx + 1 < len {
            self.slots[shift_idx] = self.slots[shift_idx + 1];
            shift_idx += 1;
        }
        self.slots[len - 1] = FrontierObservationSlot::EMPTY;
        self.controller_mask = Self::remove_slot_mask(self.controller_mask, len, slot_idx);
        self.dynamic_controller_mask =
            Self::remove_slot_mask(self.dynamic_controller_mask, len, slot_idx);
        self.progress_mask = Self::remove_slot_mask(self.progress_mask, len, slot_idx);
        self.ready_arm_mask = Self::remove_slot_mask(self.ready_arm_mask, len, slot_idx);
        self.ready_mask = Self::remove_slot_mask(self.ready_mask, len, slot_idx);
        true
    }

    pub(super) fn replace_entry_at_slot_with_frontier_mask(
        &mut self,
        old_entry_idx: usize,
        new_entry_idx: usize,
        slot: FrontierObservationSlot,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) -> bool {
        #[cfg(test)]
        self.ensure_test_storage();
        if old_entry_idx >= MAX_STATES || new_entry_idx >= MAX_STATES {
            return false;
        }
        let Some(slot_idx) = self.slot_for_entry(old_entry_idx) else {
            return false;
        };
        let len = self.len();
        if slot_idx >= len {
            return false;
        }
        let Some(old_entry) = checked_state_index(old_entry_idx) else {
            return false;
        };
        let Some(new_entry) = checked_state_index(new_entry_idx) else {
            return false;
        };
        if self.slots[slot_idx].entry != old_entry {
            return false;
        }
        if self.slot_for_entry(new_entry_idx).is_some() {
            return false;
        }
        let observed_bit = 1u8 << slot_idx;
        debug_assert!(slot.entry == new_entry);
        self.slots[slot_idx] = slot;
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.observe_with_frontier_mask(observed_bit, observed, frontier_mask);
        true
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn replace_entry_at_slot(
        &mut self,
        old_entry_idx: usize,
        new_entry_idx: usize,
        slot: FrontierObservationSlot,
        observed: OfferEntryObservedState,
    ) -> bool {
        self.replace_entry_at_slot_with_frontier_mask(
            old_entry_idx,
            new_entry_idx,
            slot,
            observed,
            observed.frontier_mask,
        )
    }

    pub(super) fn move_slot_mask(
        mask: u8,
        len: usize,
        old_slot_idx: usize,
        new_slot_idx: usize,
    ) -> u8 {
        let mut remapped = 0u8;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let source_slot = if old_slot_idx < new_slot_idx {
                if slot_idx < old_slot_idx || slot_idx > new_slot_idx {
                    slot_idx
                } else if slot_idx == new_slot_idx {
                    old_slot_idx
                } else {
                    slot_idx + 1
                }
            } else if slot_idx < new_slot_idx || slot_idx > old_slot_idx {
                slot_idx
            } else if slot_idx == new_slot_idx {
                old_slot_idx
            } else {
                slot_idx - 1
            };
            if ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        remapped
    }

    pub(super) fn insert_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
        let mut remapped = 0u8;
        let mut new_slot_idx = 0usize;
        while new_slot_idx <= len {
            if new_slot_idx == slot_idx {
                new_slot_idx += 1;
                continue;
            }
            let source_slot = if new_slot_idx < slot_idx {
                new_slot_idx
            } else {
                new_slot_idx - 1
            };
            if source_slot < len && ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << new_slot_idx;
            }
            new_slot_idx += 1;
        }
        remapped
    }

    pub(super) fn remove_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
        if len == 0 || slot_idx >= len {
            return 0;
        }
        let mut remapped = 0u8;
        let mut new_slot_idx = 0usize;
        while new_slot_idx + 1 < len {
            let source_slot = if new_slot_idx < slot_idx {
                new_slot_idx
            } else {
                new_slot_idx + 1
            };
            if ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << new_slot_idx;
            }
            new_slot_idx += 1;
        }
        remapped
    }
}

#[derive(Clone, Copy)]
pub(super) struct OfferLaneEntrySlotMasks {
    masks: [u8; OFFER_LANE_MASK_LANES],
}

impl OfferLaneEntrySlotMasks {
    pub(super) const EMPTY: Self = Self {
        masks: [0; OFFER_LANE_MASK_LANES],
    };

    #[inline]
    pub(super) fn set_logical_mask(&mut self, logical_lane: usize, value: u8) {
        if logical_lane < self.masks.len() {
            self.masks[logical_lane] = value;
        }
    }
}

static ZERO_LANE_MASK: u8 = 0;

impl Index<usize> for OfferLaneEntrySlotMasks {
    type Output = u8;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.masks.get(index).unwrap_or(&ZERO_LANE_MASK)
    }
}

#[derive(Clone, Copy)]
pub(super) struct RootFrontierState {
    pub(super) root: ScopeId,
    pub(super) observed_entries: ObservedEntrySummary,
    pub(super) observed_offer_lane_mask: LaneMask,
    pub(super) observed_binding_nonempty_mask: LaneMask,
    pub(super) observed_key_present: bool,
    pub(super) active_start: u8,
    pub(super) active_len: u8,
}

impl RootFrontierState {
    pub(super) const EMPTY: Self = Self {
        root: ScopeId::none(),
        observed_entries: ObservedEntrySummary::EMPTY,
        observed_offer_lane_mask: 0,
        observed_binding_nonempty_mask: 0,
        observed_key_present: false,
        active_start: 0,
        active_len: 0,
    };

    #[inline]
    pub(super) fn observed_key_valid(self) -> bool {
        self.observed_key_present
    }

    #[inline]
    pub(super) fn observed_key_offer_lane_mask(self) -> LaneMask {
        self.observed_offer_lane_mask
    }

    #[inline]
    pub(super) fn clear_observed_key_cache(&mut self) {
        self.observed_entries = ObservedEntrySummary::EMPTY;
        self.observed_offer_lane_mask = 0;
        self.observed_binding_nonempty_mask = 0;
        self.observed_key_present = false;
    }

    #[inline]
    pub(super) fn set_observed_key_cache_masks(
        &mut self,
        offer_lane_mask: LaneMask,
        binding_nonempty_mask: LaneMask,
    ) {
        self.observed_offer_lane_mask = offer_lane_mask;
        self.observed_binding_nonempty_mask = binding_nonempty_mask;
        self.observed_key_present = true;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FrontierObservationMetaSlot {
    pub(super) entry_summary_fingerprint: u8,
    pub(super) scope_generation: u16,
    pub(super) route_change_epoch: u16,
}

impl FrontierObservationMetaSlot {
    pub(super) const EMPTY: Self = Self {
        entry_summary_fingerprint: 0,
        scope_generation: 0,
        route_change_epoch: 0,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FrontierObservationSlot {
    pub(super) entry: StateIndex,
    pub(super) meta: FrontierObservationMetaSlot,
}

impl FrontierObservationSlot {
    pub(super) const EMPTY: Self = Self {
        entry: StateIndex::MAX,
        meta: FrontierObservationMetaSlot::EMPTY,
    };
}

#[derive(Clone, Copy)]
pub(super) struct FrontierObservationKey {
    pub(super) slots: EntryBuffer<FrontierObservationSlot>,
    pub(super) offer_lane_mask: LaneMask,
    pub(super) binding_nonempty_mask: LaneMask,
}

impl FrontierObservationKey {
    pub(super) const EMPTY: Self = Self {
        slots: EntryBuffer::EMPTY,
        offer_lane_mask: 0,
        binding_nonempty_mask: 0,
    };

    #[cfg(test)]
    #[inline]
    pub(super) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut FrontierObservationSlot,
        capacity: usize,
    ) {
        unsafe {
            EntryBuffer::init_from_parts(core::ptr::addr_of_mut!((*dst).slots), slots, capacity);
            core::ptr::addr_of_mut!((*dst).offer_lane_mask).write(0);
            core::ptr::addr_of_mut!((*dst).binding_nonempty_mask).write(0);
        }
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                slots.add(idx).write(FrontierObservationSlot::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    pub(super) const fn from_parts(slots: *mut FrontierObservationSlot, capacity: usize) -> Self {
        Self {
            slots: EntryBuffer::from_parts(slots, capacity),
            offer_lane_mask: 0,
            binding_nonempty_mask: 0,
        }
    }

    #[cfg(test)]
    fn ensure_test_storage(&mut self) {
        if !self.slots.ptr.is_null() {
            return;
        }
        let slots = alloc_frontier_observation_key_test_storage();
        unsafe { Self::init_from_parts(self as *mut Self, slots, MAX_LANES) };
    }

    #[cfg(test)]
    pub(super) fn from_active_entries_for_test(src: ActiveEntrySet) -> Self {
        let mut key = Self::EMPTY;
        key.set_active_entries_from(src);
        key
    }

    #[inline]
    pub(super) fn clear(&mut self) {
        #[cfg(test)]
        self.ensure_test_storage();
        let mut idx = 0usize;
        while idx < self.slots.capacity() {
            self.slots[idx] = FrontierObservationSlot::EMPTY;
            idx += 1;
        }
        self.offer_lane_mask = 0;
        self.binding_nonempty_mask = 0;
    }

    #[inline]
    pub(super) fn observed_entries(self, summary: ObservedEntrySummary) -> ObservedEntrySet {
        ObservedEntrySet::from_parts_with_summary(self.slots.ptr, self.slots.capacity(), summary)
    }

    #[inline]
    pub(super) fn copy_from(&mut self, src: Self) {
        #[cfg(test)]
        self.ensure_test_storage();
        self.clear();
        let len = cached_frontier_observation_slots_len(src.slots);
        let mut idx = 0usize;
        while idx < len {
            self.slots[idx] = src.slots[idx];
            idx += 1;
        }
        self.offer_lane_mask = src.offer_lane_mask;
        self.binding_nonempty_mask = src.binding_nonempty_mask;
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn copy_slots_from_observed_entries(&mut self, src: ObservedEntrySet) {
        #[cfg(test)]
        self.ensure_test_storage();
        let capacity = self.slots.capacity();
        let mut idx = 0usize;
        while idx < capacity {
            self.slots[idx] = FrontierObservationSlot::EMPTY;
            idx += 1;
        }
        let len = src.len();
        let mut slot_idx = 0usize;
        while slot_idx < len {
            self.slots[slot_idx] = src.slots[slot_idx];
            slot_idx += 1;
        }
    }

    #[inline]
    pub(super) fn set_active_entries_from(&mut self, src: ActiveEntrySet) {
        #[cfg(test)]
        self.ensure_test_storage();
        let mut idx = 0usize;
        while idx < self.slots.capacity() {
            self.slots[idx] = FrontierObservationSlot::EMPTY;
            idx += 1;
        }
        let len = src.len();
        let mut idx = 0usize;
        while idx < len {
            self.slots[idx].entry = src.entry_state(idx);
            idx += 1;
        }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        cached_frontier_observation_slots_len(self.slots)
    }

    #[inline]
    pub(super) fn entry_state(&self, idx: usize) -> StateIndex {
        if idx >= self.slots.capacity() {
            return StateIndex::MAX;
        }
        self.slots[idx].entry
    }

    pub(super) fn slot_for_entry(&self, entry_idx: usize) -> Option<usize> {
        let entry = checked_state_index(entry_idx)?;
        let len = self.len();
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if self.slots[slot_idx].entry == entry {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn contains_entry(&self, entry_idx: usize) -> bool {
        self.slot_for_entry(entry_idx).is_some()
    }

    #[inline]
    pub(super) fn entries_equal(&self, other: &Self) -> bool {
        let len = self.len();
        if len != other.len() {
            return false;
        }
        let mut idx = 0usize;
        while idx < len {
            if self.entry_state(idx) != other.entry_state(idx) {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline]
    pub(super) fn exact_entries_match(&self, active_entries: ActiveEntrySet) -> bool {
        let len = active_entries.len();
        if self.len() != len {
            return false;
        }
        let mut idx = 0usize;
        while idx < len {
            if self.entry_state(idx) != active_entries.entry_state(idx) {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline]
    pub(super) fn slot(&self, idx: usize) -> FrontierObservationMetaSlot {
        self.slots[idx].meta
    }

    #[inline]
    pub(super) fn slot_mut(&mut self, idx: usize) -> &mut FrontierObservationMetaSlot {
        &mut self.slots[idx].meta
    }
}

impl PartialEq for FrontierObservationKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let len = self.len();
        if len != other.len()
            || self.offer_lane_mask != other.offer_lane_mask
            || self.binding_nonempty_mask != other.binding_nonempty_mask
        {
            return false;
        }
        let mut idx = 0usize;
        while idx < len {
            if self.entry_state(idx) != other.entry_state(idx) || self.slot(idx) != other.slot(idx)
            {
                return false;
            }
            idx += 1;
        }
        true
    }
}

impl Eq for FrontierObservationKey {}

#[inline]
pub(super) fn cached_frontier_observation_slots_len(
    slots: EntryBuffer<FrontierObservationSlot>,
) -> usize {
    let mut len = 0usize;
    while len < slots.capacity() {
        if slots[len].entry.is_max() {
            break;
        }
        len += 1;
    }
    len
}

#[derive(Clone, Copy)]
pub(super) struct OfferEntryStaticSummary {
    pub(super) frontier_mask: u8,
    pub(super) flags: u8,
}

impl OfferEntryStaticSummary {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const FLAG_STATIC_READY: u8 = 1 << 2;

    pub(super) const EMPTY: Self = Self {
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(super) fn observe_lane(&mut self, info: LaneOfferState) {
        self.frontier_mask |= info.frontier.bit();
        if info.is_controller() {
            self.flags |= Self::FLAG_CONTROLLER;
        }
        if info.is_dynamic() {
            self.flags |= Self::FLAG_DYNAMIC;
        }
        if info.static_ready() {
            self.flags |= Self::FLAG_STATIC_READY;
        }
    }

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) fn static_ready(self) -> bool {
        (self.flags & Self::FLAG_STATIC_READY) != 0
    }

    #[inline]
    pub(super) fn observation_fingerprint(self) -> u8 {
        self.frontier_mask | (self.flags << 4)
    }
}

#[derive(Clone, Copy)]
pub(super) struct OfferEntryState {
    pub(super) active_mask: LaneMask,
    #[cfg(test)]
    pub(super) lane_idx: u8,
    #[cfg(test)]
    pub(super) parallel_root: ScopeId,
    #[cfg(test)]
    pub(super) frontier: FrontierKind,
    #[cfg(test)]
    pub(super) scope_id: ScopeId,
    #[cfg(test)]
    pub(super) offer_lane_mask: LaneMask,
    #[cfg(test)]
    pub(super) selection_meta: CurrentScopeSelectionMeta,
    #[cfg(test)]
    pub(super) label_meta: ScopeLabelMeta,
    #[cfg(test)]
    pub(super) materialization_meta: ScopeArmMaterializationMeta,
    #[cfg(test)]
    pub(super) summary: OfferEntryStaticSummary,
    #[cfg(test)]
    pub(super) observed: OfferEntryObservedState,
}

impl OfferEntryState {
    pub(super) const EMPTY: Self = Self {
        active_mask: 0,
        #[cfg(test)]
        lane_idx: u8::MAX,
        #[cfg(test)]
        parallel_root: ScopeId::none(),
        #[cfg(test)]
        frontier: FrontierKind::Route,
        #[cfg(test)]
        scope_id: ScopeId::none(),
        #[cfg(test)]
        offer_lane_mask: 0,
        #[cfg(test)]
        selection_meta: CurrentScopeSelectionMeta::EMPTY,
        #[cfg(test)]
        label_meta: ScopeLabelMeta::EMPTY,
        #[cfg(test)]
        materialization_meta: ScopeArmMaterializationMeta::EMPTY,
        #[cfg(test)]
        summary: OfferEntryStaticSummary::EMPTY,
        #[cfg(test)]
        observed: OfferEntryObservedState::EMPTY,
    };
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) struct OfferEntrySlot {
    entry: StateIndex,
    state: OfferEntryState,
}

#[cfg(test)]
impl OfferEntrySlot {
    pub(super) const EMPTY: Self = Self {
        entry: StateIndex::MAX,
        state: OfferEntryState::EMPTY,
    };
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct OfferEntryTableTestStorage {
    slots: [OfferEntrySlot; TEST_OFFER_ENTRY_CAPACITY],
}

#[cfg(test)]
impl OfferEntryTableTestStorage {
    const EMPTY: Self = Self {
        slots: [OfferEntrySlot::EMPTY; TEST_OFFER_ENTRY_CAPACITY],
    };
}

#[cfg(test)]
thread_local! {
    static OFFER_ENTRY_TABLE_TEST_POOL: UnsafeCell<[OfferEntryTableTestStorage; TEST_FRONTIER_POOL_CAPACITY]> =
        const { UnsafeCell::new([OfferEntryTableTestStorage::EMPTY; TEST_FRONTIER_POOL_CAPACITY]) };
    static OFFER_ENTRY_TABLE_TEST_NEXT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn alloc_offer_entry_table_test_storage(capacity: usize) -> *mut OfferEntrySlot {
    assert!(
        capacity <= TEST_OFFER_ENTRY_CAPACITY,
        "offer entry table capacity {capacity} exceeds test storage {TEST_OFFER_ENTRY_CAPACITY}"
    );
    OFFER_ENTRY_TABLE_TEST_NEXT.with(|next| {
        OFFER_ENTRY_TABLE_TEST_POOL.with(|pool| {
            let idx = next.get();
            if idx >= TEST_FRONTIER_POOL_CAPACITY {
                panic!("offer entry table test storage exhausted");
            }
            next.set(idx + 1);
            let storage = unsafe { &mut (*pool.get())[idx] };
            storage.slots.fill(OfferEntrySlot::EMPTY);
            storage.slots.as_mut_ptr()
        })
    })
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) struct OfferEntryTable {
    slots: EntryBuffer<OfferEntrySlot>,
}

#[cfg(test)]
impl OfferEntryTable {
    pub(super) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut OfferEntrySlot,
        capacity: usize,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).slots).write(EntryBuffer::from_parts(slots, capacity));
        }
        if slots.is_null() {
            return;
        }
        let mut idx = 0usize;
        while idx < capacity {
            unsafe { slots.add(idx).write(OfferEntrySlot::EMPTY) };
            idx += 1;
        }
    }

    #[inline]
    fn len(&self) -> usize {
        if self.slots.ptr.is_null() {
            return 0;
        }
        let mut len = 0usize;
        let capacity = self.slots.capacity();
        while len < capacity {
            if self.slots[len].entry.is_max() {
                break;
            }
            len += 1;
        }
        len
    }

    #[cfg(test)]
    fn ensure_test_storage(&mut self) {
        if !self.slots.ptr.is_null() {
            return;
        }
        let capacity = self.slots.capacity();
        if capacity == 0 {
            return;
        }
        let slots = alloc_offer_entry_table_test_storage(capacity);
        unsafe { Self::init_from_parts(self as *mut Self, slots, capacity) };
    }

    #[inline]
    fn slot_for_entry(&self, entry_idx: usize) -> Option<usize> {
        let entry = checked_state_index(entry_idx)?;
        let len = self.len();
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if self.slots[slot_idx].entry == entry {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn get(&self, entry_idx: usize) -> Option<&OfferEntryState> {
        self.slot_for_entry(entry_idx)
            .map(|slot_idx| &self.slots[slot_idx].state)
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn get_mut(&mut self, entry_idx: usize) -> Option<&mut OfferEntryState> {
        self.ensure_test_storage();
        let slot_idx = self.slot_for_entry(entry_idx)?;
        Some(&mut self.slots[slot_idx].state)
    }

    #[cfg(test)]
    pub(super) fn set(&mut self, entry_idx: usize, state: OfferEntryState) {
        self.ensure_test_storage();
        if state.active_mask == 0 {
            self.clear(entry_idx);
            return;
        }
        let slot = self.ensure_entry_mut(entry_idx);
        *slot = state;
    }

    #[cfg(test)]
    pub(super) fn clear(&mut self, entry_idx: usize) {
        self.ensure_test_storage();
        let Some(slot_idx) = self.slot_for_entry(entry_idx) else {
            return;
        };
        let len = self.len();
        let mut idx = slot_idx;
        while idx + 1 < len {
            self.slots[idx] = self.slots[idx + 1];
            idx += 1;
        }
        if len != 0 {
            self.slots[len - 1] = OfferEntrySlot::EMPTY;
        }
    }

    #[cfg(test)]
    fn ensure_entry_mut(&mut self, entry_idx: usize) -> &mut OfferEntryState {
        self.ensure_test_storage();
        if let Some(slot_idx) = self.slot_for_entry(entry_idx) {
            return &mut self.slots[slot_idx].state;
        }
        let entry = checked_state_index(entry_idx).expect("offer entry index must fit StateIndex");
        let len = self.len();
        assert!(
            len < self.slots.capacity(),
            "offer entry table overflow: distinct offer entries must fit compiled capacity"
        );
        let mut insert_idx = 0usize;
        while insert_idx < len && self.slots[insert_idx].entry.raw() < entry.raw() {
            insert_idx += 1;
        }
        let mut shift_idx = len;
        while shift_idx > insert_idx {
            self.slots[shift_idx] = self.slots[shift_idx - 1];
            shift_idx -= 1;
        }
        self.slots[insert_idx] = OfferEntrySlot {
            entry,
            state: OfferEntryState::EMPTY,
        };
        &mut self.slots[insert_idx].state
    }
}

#[cfg(test)]
static EMPTY_OFFER_ENTRY_STATE: OfferEntryState = OfferEntryState::EMPTY;

#[cfg(test)]
impl Index<usize> for OfferEntryTable {
    type Output = OfferEntryState;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap_or(&EMPTY_OFFER_ENTRY_STATE)
    }
}

#[cfg(test)]
impl IndexMut<usize> for OfferEntryTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.ensure_entry_mut(index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OfferEntryObservedState {
    #[cfg(test)]
    pub(super) scope_id: ScopeId,
    #[cfg(test)]
    pub(super) frontier_mask: u8,
    pub(super) flags: u8,
}

impl OfferEntryObservedState {
    #[cfg(test)]
    pub(super) const EMPTY: Self = Self {
        #[cfg(test)]
        scope_id: ScopeId::none(),
        #[cfg(test)]
        frontier_mask: 0,
        flags: 0,
    };
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const FLAG_PROGRESS: u8 = 1 << 2;
    pub(super) const FLAG_READY_ARM: u8 = 1 << 3;
    pub(super) const FLAG_BINDING_READY: u8 = 1 << 4;
    pub(super) const FLAG_READY: u8 = 1 << 5;

    #[inline]
    pub(super) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(super) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) fn has_progress_evidence(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS) != 0
    }

    #[inline]
    pub(super) fn has_ready_arm_evidence(self) -> bool {
        (self.flags & Self::FLAG_READY_ARM) != 0
    }

    #[inline]
    pub(super) fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn binding_ready(self) -> bool {
        (self.flags & Self::FLAG_BINDING_READY) != 0
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn matches_frontier(self, frontier: FrontierKind) -> bool {
        (self.frontier_mask & frontier.bit()) != 0
    }
}

pub(crate) const MAX_ROUTE_ARM_STACK: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrontierCandidate {
    pub(super) scope_id: ScopeId,
    pub(super) entry_idx: u16,
    pub(super) parallel_root: ScopeId,
    pub(super) frontier: FrontierKind,
    pub(super) flags: u8,
}

impl FrontierCandidate {
    pub(super) const FLAG_CONTROLLER: u8 = 1;
    pub(super) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(super) const FLAG_HAS_EVIDENCE: u8 = 1 << 2;
    pub(super) const FLAG_READY: u8 = 1 << 3;

    pub(super) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        entry_idx: u16::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: 0,
    };

    #[inline]
    pub(super) const fn pack_flags(
        is_controller: bool,
        is_dynamic: bool,
        has_evidence: bool,
        ready: bool,
    ) -> u8 {
        (if is_controller {
            Self::FLAG_CONTROLLER
        } else {
            0
        }) | (if is_dynamic { Self::FLAG_DYNAMIC } else { 0 })
            | (if has_evidence {
                Self::FLAG_HAS_EVIDENCE
            } else {
                0
            })
            | (if ready { Self::FLAG_READY } else { 0 })
    }

    #[inline]
    pub(super) const fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    #[cfg(test)]
    pub(super) const fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(super) const fn has_evidence(self) -> bool {
        (self.flags & Self::FLAG_HAS_EVIDENCE) != 0
    }

    #[inline]
    pub(super) const fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierScratchSection {
    offset: usize,
    align: usize,
    bytes: usize,
    count: usize,
}

impl FrontierScratchSection {
    #[inline(always)]
    pub(crate) const fn offset(self) -> usize {
        self.offset
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn count(self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierScratchLayout {
    #[cfg(not(test))]
    global_observed_state: FrontierScratchSection,
    global_active_entry_slots: FrontierScratchSection,
    cached_observation_key_slots: FrontierScratchSection,
    observation_key_slots: FrontierScratchSection,
    working_observation_key_slots: FrontierScratchSection,
    observed_entry_slots: FrontierScratchSection,
    candidates: FrontierScratchSection,
    visited_scopes: FrontierScratchSection,
    root_scopes: FrontierScratchSection,
    total_bytes: usize,
    total_align: usize,
}

impl FrontierScratchLayout {
    #[inline(always)]
    pub(crate) const fn new(max_frontier_entries: usize) -> Self {
        let mut offset = 0usize;
        let mut total_align = 1usize;

        #[cfg(not(test))]
        let global_observed_state = Self::section_array::<GlobalFrontierObservedState>(offset, 1);
        #[cfg(not(test))]
        {
            offset = global_observed_state.offset + global_observed_state.bytes;
            total_align = max_usize(total_align, global_observed_state.align);
        }

        let global_active_entry_slots =
            Self::section_array::<ActiveEntrySlot>(offset, max_frontier_entries);
        offset = global_active_entry_slots.offset + global_active_entry_slots.bytes;
        total_align = max_usize(total_align, global_active_entry_slots.align);

        let cached_observation_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = cached_observation_key_slots.offset + cached_observation_key_slots.bytes;
        total_align = max_usize(total_align, cached_observation_key_slots.align);

        let observation_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = observation_key_slots.offset + observation_key_slots.bytes;
        total_align = max_usize(total_align, observation_key_slots.align);

        let working_observation_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = working_observation_key_slots.offset + working_observation_key_slots.bytes;
        total_align = max_usize(total_align, working_observation_key_slots.align);

        let observed_entry_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = observed_entry_slots.offset + observed_entry_slots.bytes;
        total_align = max_usize(total_align, observed_entry_slots.align);

        let candidates = Self::section_array::<FrontierCandidate>(offset, max_frontier_entries);
        offset = candidates.offset + candidates.bytes;
        total_align = max_usize(total_align, candidates.align);

        let visited_scopes = Self::section_array::<ScopeId>(offset, max_frontier_entries);
        offset = visited_scopes.offset + visited_scopes.bytes;
        total_align = max_usize(total_align, visited_scopes.align);

        let root_scopes = Self::section_array::<ScopeId>(offset, max_frontier_entries);
        offset = root_scopes.offset + root_scopes.bytes;
        total_align = max_usize(total_align, root_scopes.align);

        Self {
            #[cfg(not(test))]
            global_observed_state,
            global_active_entry_slots,
            cached_observation_key_slots,
            observation_key_slots,
            working_observation_key_slots,
            observed_entry_slots,
            candidates,
            visited_scopes,
            root_scopes,
            total_bytes: offset,
            total_align,
        }
    }

    #[inline(always)]
    pub(crate) const fn total_bytes(self) -> usize {
        self.total_bytes
    }

    #[inline(always)]
    pub(crate) const fn total_align(self) -> usize {
        self.total_align
    }

    #[inline(always)]
    pub(crate) const fn global_active_entry_slots(self) -> FrontierScratchSection {
        self.global_active_entry_slots
    }

    #[cfg(not(test))]
    #[inline(always)]
    pub(crate) const fn global_observed_state(self) -> FrontierScratchSection {
        self.global_observed_state
    }

    #[inline(always)]
    pub(crate) const fn cached_observation_key_slots(self) -> FrontierScratchSection {
        self.cached_observation_key_slots
    }

    #[inline(always)]
    pub(crate) const fn observation_key_slots(self) -> FrontierScratchSection {
        self.observation_key_slots
    }

    #[inline(always)]
    pub(crate) const fn working_observation_key_slots(self) -> FrontierScratchSection {
        self.working_observation_key_slots
    }

    #[inline(always)]
    pub(crate) const fn observed_entry_slots(self) -> FrontierScratchSection {
        self.observed_entry_slots
    }

    #[inline(always)]
    pub(crate) const fn candidates(self) -> FrontierScratchSection {
        self.candidates
    }

    #[inline(always)]
    pub(crate) const fn visited_scopes(self) -> FrontierScratchSection {
        self.visited_scopes
    }

    #[inline(always)]
    pub(crate) const fn root_scopes(self) -> FrontierScratchSection {
        self.root_scopes
    }

    #[inline(always)]
    const fn section_array<T>(offset: usize, count: usize) -> FrontierScratchSection {
        let align = mem::align_of::<T>();
        let bytes = mem::size_of::<T>().saturating_mul(count);
        FrontierScratchSection {
            offset: align_up(offset, align),
            align,
            bytes,
            count,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct FrontierScratchView {
    #[cfg(test)]
    working_observation_key: FrontierObservationKey,
    candidates: *mut FrontierCandidate,
    frontier_entry_capacity: u8,
    visited_scopes: *mut ScopeId,
    root_scopes: *mut ScopeId,
}

#[inline]
fn frontier_scratch_storage_ptr(scratch_ptr: *mut [u8], layout: FrontierScratchLayout) -> *mut u8 {
    let scratch = unsafe { &mut *scratch_ptr };
    debug_assert!(
        scratch.len() >= layout.total_bytes(),
        "frontier scratch reservation must cover compiled layout"
    );
    scratch.as_mut_ptr()
}

#[cfg(not(test))]
#[inline]
pub(super) fn frontier_global_observed_state_ptr_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
) -> *mut GlobalFrontierObservedState {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    unsafe {
        storage
            .add(layout.global_observed_state().offset())
            .cast::<GlobalFrontierObservedState>()
    }
}

#[inline]
pub(super) fn frontier_observation_key_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierObservationKey {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    FrontierObservationKey::from_parts(
        unsafe {
            storage
                .add(layout.observation_key_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
    )
}

#[inline]
pub(super) fn frontier_cached_observation_key_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierObservationKey {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    FrontierObservationKey::from_parts(
        unsafe {
            storage
                .add(layout.cached_observation_key_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
    )
}

#[inline]
pub(super) fn frontier_working_observation_key_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierObservationKey {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    FrontierObservationKey::from_parts(
        unsafe {
            storage
                .add(layout.working_observation_key_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
    )
}

#[inline]
pub(super) fn frontier_global_active_entries_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> ActiveEntrySet {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    ActiveEntrySet {
        slots: EntryBuffer::from_parts(
            unsafe {
                storage
                    .add(layout.global_active_entry_slots().offset())
                    .cast::<ActiveEntrySlot>()
            },
            frontier_entry_capacity,
        ),
    }
}

#[inline]
pub(super) fn frontier_observed_entries_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> ObservedEntrySet {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    ObservedEntrySet::from_parts(
        unsafe {
            storage
                .add(layout.observed_entry_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
    )
}

impl FrontierScratchView {
    #[inline]
    pub(super) unsafe fn from_parts(
        storage: *mut u8,
        layout: FrontierScratchLayout,
        _logical_lane_count: usize,
        frontier_entry_capacity: usize,
    ) -> Self {
        let _ = _logical_lane_count;
        Self {
            #[cfg(test)]
            working_observation_key: FrontierObservationKey::from_parts(
                unsafe {
                    storage
                        .add(layout.working_observation_key_slots().offset())
                        .cast::<FrontierObservationSlot>()
                },
                frontier_entry_capacity,
            ),
            candidates: unsafe {
                storage
                    .add(layout.candidates().offset())
                    .cast::<FrontierCandidate>()
            },
            frontier_entry_capacity: frontier_entry_capacity as u8,
            visited_scopes: unsafe {
                storage
                    .add(layout.visited_scopes().offset())
                    .cast::<ScopeId>()
            },
            root_scopes: unsafe { storage.add(layout.root_scopes().offset()).cast::<ScopeId>() },
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn working_observation_key_from(
        &mut self,
        src: FrontierObservationKey,
    ) -> FrontierObservationKey {
        let key = &mut self.working_observation_key;
        key.copy_from(src);
        *key
    }

    #[inline]
    pub(super) fn candidates_mut(&mut self) -> &mut [FrontierCandidate] {
        unsafe { slice::from_raw_parts_mut(self.candidates, self.frontier_entry_capacity as usize) }
    }

    #[inline]
    pub(super) fn visited_scopes_mut(&mut self) -> &mut [ScopeId] {
        unsafe {
            slice::from_raw_parts_mut(self.visited_scopes, self.frontier_entry_capacity as usize)
        }
    }

    #[inline]
    pub(super) fn root_scopes_mut(&mut self) -> &mut [ScopeId] {
        unsafe {
            slice::from_raw_parts_mut(self.root_scopes, self.frontier_entry_capacity as usize)
        }
    }
}

#[inline]
pub(super) fn frontier_scratch_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    logical_lane_count: usize,
    frontier_entry_capacity: usize,
) -> FrontierScratchView {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    unsafe {
        FrontierScratchView::from_parts(
            storage,
            layout,
            logical_lane_count,
            frontier_entry_capacity,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::FrontierScratchLayout;

    #[test]
    fn global_frontier_scratch_sections_track_max_frontier_entries() {
        let layout = FrontierScratchLayout::new(5);
        assert_eq!(layout.global_active_entry_slots().count(), 5);
        assert_eq!(layout.cached_observation_key_slots().count(), 5);
        assert_eq!(layout.observation_key_slots().count(), 5);
        assert_eq!(layout.working_observation_key_slots().count(), 5);
        assert_eq!(layout.observed_entry_slots().count(), 5);
    }
}

#[inline]
pub(super) fn frontier_snapshot_from_scratch(
    scratch: &mut FrontierScratchView,
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
) -> FrontierSnapshot {
    let candidate_capacity = scratch.candidates_mut().len();
    unsafe {
        FrontierSnapshot::from_parts(
            current_scope,
            current_entry_idx,
            current_parallel_root,
            current_frontier,
            scratch.candidates_mut().as_mut_ptr(),
            candidate_capacity,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrontierSnapshot {
    pub(super) current_scope: ScopeId,
    pub(super) current_entry_idx: usize,
    pub(super) current_parallel_root: ScopeId,
    pub(super) current_frontier: FrontierKind,
    candidates: *mut FrontierCandidate,
    candidate_capacity: usize,
    pub(super) candidate_len: usize,
}

impl FrontierSnapshot {
    #[inline]
    pub(super) unsafe fn from_parts(
        current_scope: ScopeId,
        current_entry_idx: usize,
        current_parallel_root: ScopeId,
        current_frontier: FrontierKind,
        candidates: *mut FrontierCandidate,
        candidate_capacity: usize,
    ) -> Self {
        let mut idx = 0usize;
        while idx < candidate_capacity {
            unsafe {
                candidates.add(idx).write(FrontierCandidate::EMPTY);
            }
            idx += 1;
        }
        Self {
            current_scope,
            current_entry_idx,
            current_parallel_root,
            current_frontier,
            candidates,
            candidate_capacity,
            candidate_len: 0,
        }
    }

    #[inline]
    pub(super) fn push_candidate(&mut self, candidate: FrontierCandidate) -> bool {
        if self.candidate_len >= self.candidate_capacity {
            return false;
        }
        unsafe {
            self.candidates.add(self.candidate_len).write(candidate);
        }
        self.candidate_len += 1;
        true
    }

    #[inline]
    fn candidate_at(self, idx: usize) -> FrontierCandidate {
        debug_assert!(idx < self.candidate_len);
        unsafe { *self.candidates.add(idx) }
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn candidate(self, idx: usize) -> Option<FrontierCandidate> {
        if idx < self.candidate_len {
            Some(self.candidate_at(idx))
        } else {
            None
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn test_from_slice(
        current_scope: ScopeId,
        current_entry_idx: usize,
        current_parallel_root: ScopeId,
        current_frontier: FrontierKind,
        candidates: &mut [FrontierCandidate],
        candidate_len: usize,
    ) -> Self {
        let len = core::cmp::min(candidate_len, candidates.len());
        Self {
            current_scope,
            current_entry_idx,
            current_parallel_root,
            current_frontier,
            candidates: candidates.as_mut_ptr(),
            candidate_capacity: candidates.len(),
            candidate_len: len,
        }
    }

    #[inline]
    pub(super) fn matches_parallel_root(self, candidate: FrontierCandidate) -> bool {
        self.current_parallel_root.is_none()
            || candidate.parallel_root == self.current_parallel_root
    }

    pub(super) fn select_yield_candidate(
        self,
        visited: FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx as usize != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx as usize != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }

    pub(super) fn select_exhausted_controller_candidate(
        self,
        visited: FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx as usize != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.is_controller()
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidate_at(idx);
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx as usize != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.is_controller()
                && candidate.ready()
                && candidate.has_evidence()
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrontierVisitSet {
    slots: *mut ScopeId,
    capacity: usize,
    pub(super) len: usize,
}

impl FrontierVisitSet {
    #[inline]
    #[cfg(test)]
    pub(super) const fn empty() -> Self {
        Self {
            slots: core::ptr::null_mut(),
            capacity: 0,
            len: 0,
        }
    }

    #[inline]
    pub(super) unsafe fn from_parts(slots: *mut ScopeId, capacity: usize) -> Self {
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                slots.add(idx).write(ScopeId::none());
            }
            idx += 1;
        }
        Self {
            slots,
            capacity,
            len: 0,
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn test_from_slice(slots: &mut [ScopeId]) -> Self {
        unsafe { Self::from_parts(slots.as_mut_ptr(), slots.len()) }
    }

    #[inline]
    pub(super) fn contains(self, scope: ScopeId) -> bool {
        let mut idx = 0usize;
        while idx < self.len {
            if unsafe { *self.slots.add(idx) } == scope {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(super) fn record(&mut self, scope: ScopeId) {
        if scope.is_none() || self.contains(scope) || self.len >= self.capacity {
            return;
        }
        unsafe {
            self.slots.add(self.len).write(scope);
        }
        self.len += 1;
    }
}

#[inline]
pub(super) fn frontier_visit_set_from_scratch(
    scratch: &mut FrontierScratchView,
) -> FrontierVisitSet {
    let capacity = scratch.visited_scopes_mut().len();
    unsafe { FrontierVisitSet::from_parts(scratch.visited_scopes_mut().as_mut_ptr(), capacity) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontierDeferOutcome {
    Continue,
    Yielded,
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EvidenceFingerprint(u8);

impl EvidenceFingerprint {
    #[inline]
    pub(super) const fn new(
        has_ack: bool,
        has_ready_arm_evidence: bool,
        binding_ready: bool,
    ) -> Self {
        let mut bits = 0u8;
        if has_ack {
            bits |= 1 << 0;
        }
        if has_ready_arm_evidence {
            bits |= 1 << 1;
        }
        if binding_ready {
            bits |= 1 << 2;
        }
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OfferLivenessState {
    pub(super) policy: crate::runtime::config::LivenessPolicy,
    pub(super) remaining_defer: u8,
    pub(super) remaining_no_evidence_defer: u8,
    pub(super) forced_poll_attempts: u8,
    pub(super) last_fingerprint: Option<EvidenceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferBudgetOutcome {
    Continue,
    Exhausted,
}

impl OfferLivenessState {
    #[inline]
    pub(super) fn new(policy: crate::runtime::config::LivenessPolicy) -> Self {
        Self {
            policy,
            remaining_defer: policy.max_defer_per_offer,
            remaining_no_evidence_defer: policy.max_no_evidence_defer,
            forced_poll_attempts: 0,
            last_fingerprint: None,
        }
    }

    #[inline]
    pub(super) fn on_defer(&mut self, fingerprint: EvidenceFingerprint) -> DeferBudgetOutcome {
        if self.remaining_defer == 0 {
            return DeferBudgetOutcome::Exhausted;
        }
        self.remaining_defer = self.remaining_defer.saturating_sub(1);
        let has_new_evidence = self.last_fingerprint != Some(fingerprint);
        self.last_fingerprint = Some(fingerprint);
        if !has_new_evidence {
            if self.remaining_no_evidence_defer == 0 {
                return DeferBudgetOutcome::Exhausted;
            }
            self.remaining_no_evidence_defer = self.remaining_no_evidence_defer.saturating_sub(1);
        }
        DeferBudgetOutcome::Continue
    }

    #[inline]
    pub(super) const fn can_force_poll(self) -> bool {
        self.policy.force_poll_on_exhaustion
            && self.forced_poll_attempts < self.policy.max_forced_poll_attempts
    }

    #[inline]
    pub(super) fn mark_forced_poll(&mut self) {
        self.forced_poll_attempts = self.forced_poll_attempts.saturating_add(1);
    }

    #[inline]
    pub(super) const fn exhaust_reason(self) -> u16 {
        self.policy.exhaust_reason
    }
}

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[inline(always)]
const fn max_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OfferSelectPriority {
    CurrentOfferEntry,
    DynamicControllerUnique,
    ControllerUnique,
    CandidateUnique,
}

#[inline]
pub(super) fn choose_offer_priority(
    current_is_candidate: bool,
    dynamic_controller_count: usize,
    controller_count: usize,
    candidate_count: usize,
) -> Option<OfferSelectPriority> {
    if current_is_candidate {
        Some(OfferSelectPriority::CurrentOfferEntry)
    } else if dynamic_controller_count == 1 {
        Some(OfferSelectPriority::DynamicControllerUnique)
    } else if controller_count == 1 {
        Some(OfferSelectPriority::ControllerUnique)
    } else if candidate_count == 1 {
        Some(OfferSelectPriority::CandidateUnique)
    } else {
        None
    }
}

#[inline]
pub(super) async fn yield_once() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

#[inline]
pub(super) fn current_entry_is_candidate(
    current_matches_candidate: bool,
    current_is_controller: bool,
    current_has_evidence: bool,
    candidate_count: usize,
    progress_sibling_exists: bool,
) -> bool {
    if !current_matches_candidate {
        return false;
    }
    if current_is_controller
        && !current_has_evidence
        && progress_sibling_exists
        && candidate_count > 0
    {
        return false;
    }
    true
}

#[inline]
pub(super) fn current_entry_matches_after_filter(
    current_matches_candidate: bool,
    current_has_offer_lanes: bool,
    current_idx: usize,
    hint_filter: Option<usize>,
) -> bool {
    if !current_matches_candidate || !current_has_offer_lanes {
        return false;
    }
    if let Some(filtered_idx) = hint_filter {
        return current_idx == filtered_idx;
    }
    true
}

#[inline]
pub(super) fn should_suppress_current_passive_without_evidence(
    current_frontier: FrontierKind,
    current_is_controller: bool,
    current_has_evidence: bool,
    controller_progress_sibling_exists: bool,
) -> bool {
    current_frontier == FrontierKind::PassiveObserver
        && !current_is_controller
        && !current_has_evidence
        && controller_progress_sibling_exists
}

#[cfg(test)]
#[inline]
pub(super) fn candidate_participates_in_frontier_arbitration(
    entry_idx: usize,
    current_idx: usize,
    has_progress_evidence: bool,
    current_entry_unrunnable: bool,
) -> bool {
    entry_idx == current_idx
        || has_progress_evidence
        || (current_entry_unrunnable && entry_idx != current_idx)
}

#[cfg(test)]
#[inline]
pub(super) fn controller_candidate_ready(
    is_controller: bool,
    entry_idx: usize,
    current_idx: usize,
    has_progress_evidence: bool,
) -> bool {
    !is_controller || entry_idx == current_idx || has_progress_evidence
}

#[inline]
pub(super) fn candidate_has_progress_evidence(
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> bool {
    has_ready_arm_evidence || ack_is_progress || binding_ready
}

#[inline]
pub(super) fn offer_entry_observed_state(
    _scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> OfferEntryObservedState {
    let has_progress_evidence =
        candidate_has_progress_evidence(has_ready_arm_evidence, ack_is_progress, binding_ready);
    let ready =
        has_ready_arm_evidence || ack_is_progress || binding_ready || summary.static_ready();
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if has_progress_evidence {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if has_ready_arm_evidence {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if binding_ready {
        flags |= OfferEntryObservedState::FLAG_BINDING_READY;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        #[cfg(test)]
        scope_id: _scope_id,
        #[cfg(test)]
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[inline]
pub(super) fn offer_entry_frontier_candidate(
    scope_id: ScopeId,
    entry_idx: usize,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    observed: OfferEntryObservedState,
) -> FrontierCandidate {
    debug_assert!(
        u16::try_from(entry_idx).is_ok(),
        "offer entry index must fit u16"
    );
    FrontierCandidate {
        scope_id,
        entry_idx: entry_idx as u16,
        parallel_root,
        frontier,
        flags: FrontierCandidate::pack_flags(
            observed.is_controller(),
            observed.is_dynamic(),
            observed.has_progress_evidence(),
            observed.ready(),
        ),
    }
}

#[inline]
pub(super) fn cached_offer_entry_observed_state(
    _scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    observed_entries: ObservedEntrySet,
    observed_bit: u8,
) -> OfferEntryObservedState {
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if (observed_entries.progress_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if (observed_entries.ready_arm_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if (observed_entries.ready_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        #[cfg(test)]
        scope_id: _scope_id,
        #[cfg(test)]
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[cfg(test)]
#[inline]
pub(super) fn record_offer_entry_reentry_candidate(
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    candidate: FrontierCandidate,
    ready_entry_idx: &mut Option<usize>,
    any_entry_idx: &mut Option<usize>,
) {
    if (candidate.scope_id == current_scope && candidate.entry_idx as usize == current_entry_idx)
        || (!current_parallel_root.is_none() && candidate.parallel_root != current_parallel_root)
    {
        return;
    }
    if any_entry_idx.is_none() {
        *any_entry_idx = Some(candidate.entry_idx as usize);
    }
    if candidate.ready() && ready_entry_idx.is_none() {
        *ready_entry_idx = Some(candidate.entry_idx as usize);
    }
}
