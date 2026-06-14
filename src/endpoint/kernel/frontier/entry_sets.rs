use super::{
    Deref, DerefMut, FRONTIER_SLOT_MASK_BITS, FrontierKind, FrontierObservationMetaSlot,
    FrontierObservationSlot, Index, IndexMut, MAX_STATES, OfferEntryObservedState, StateIndex,
    cached_frontier_observation_slots_len, checked_state_index, slice, state_index_to_usize,
};
#[derive(Clone, Copy)]
pub(crate) struct EntryBuffer<T> {
    pub(crate) ptr: *mut T,
    capacity: u8,
}

impl<T> EntryBuffer<T> {
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        capacity: 0,
    };

    #[cfg(all(test, hibana_repo_tests))]
    #[inline]
    pub(crate) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut T, capacity: usize) {
        if capacity > u8::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).capacity).write(capacity as u8);
        }
    }

    #[inline]
    pub(crate) const fn capacity(&self) -> usize {
        self.capacity as usize
    }

    #[inline]
    pub(crate) const fn from_parts(ptr: *mut T, capacity: usize) -> Self {
        if capacity > u8::MAX as usize {
            crate::invariant();
        }
        Self {
            ptr,
            capacity: capacity as u8,
        }
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts(self.ptr, self.capacity()) }
        }
    }

    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveEntrySlot {
    pub(crate) entry: StateIndex,
    pub(crate) lane_idx: u8,
}

impl ActiveEntrySlot {
    pub(crate) const EMPTY: Self = Self {
        entry: StateIndex::ABSENT,
        lane_idx: u8::MAX,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct ActiveEntrySet {
    pub(crate) slots: EntryBuffer<ActiveEntrySlot>,
}

impl ActiveEntrySet {
    pub(crate) const EMPTY: Self = Self {
        slots: EntryBuffer::EMPTY,
    };

    #[cfg(all(test, hibana_repo_tests))]
    #[inline]
    pub(crate) const fn from_parts(slots: *mut ActiveEntrySlot, capacity: usize) -> Self {
        Self {
            slots: EntryBuffer::from_parts(slots, capacity),
        }
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[inline]
    pub(crate) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut ActiveEntrySlot,
        capacity: usize,
    ) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            EntryBuffer::init_from_parts(core::ptr::addr_of_mut!((*dst).slots), slots, capacity);
        }
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                slots.add(idx).write(ActiveEntrySlot::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        let capacity = self.slots.capacity();
        let mut idx = 0usize;
        while idx < capacity {
            self.slots[idx] = ActiveEntrySlot::EMPTY;
            idx += 1;
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        let mut len = 0usize;
        while len < self.slots.capacity() {
            if self.slots[len].entry.is_absent() {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline]
    pub(crate) fn occupancy_mask(self) -> u8 {
        let len = self.len();
        if len >= FRONTIER_SLOT_MASK_BITS {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    pub(crate) fn entry_at(self, slot_idx: usize) -> Option<usize> {
        if slot_idx >= self.len() {
            return None;
        }
        Some(state_index_to_usize(self.slots[slot_idx].entry))
    }

    #[inline]
    pub(crate) fn entry_state(self, slot_idx: usize) -> StateIndex {
        if slot_idx >= self.len() {
            return StateIndex::ABSENT;
        }
        self.slots[slot_idx].entry
    }

    #[inline]
    pub(crate) fn contains_only(self, entry_idx: usize) -> bool {
        self.len() == 1 && self.entry_at(0) == Some(entry_idx)
    }

    #[inline]
    pub(crate) fn slot_for_entry(self, entry_idx: usize) -> Option<usize> {
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

    pub(crate) fn insert_entry(&mut self, entry_idx: usize, lane_idx: u8) -> bool {
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

    pub(crate) fn remove_entry(&mut self, entry_idx: usize) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
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
pub(crate) struct ObservedEntrySet {
    pub(crate) slots: EntryBuffer<FrontierObservationSlot>,
    pub(crate) controller_mask: u8,
    pub(crate) dynamic_controller_mask: u8,
    pub(crate) progress_mask: u8,
    pub(crate) ready_arm_mask: u8,
    pub(crate) ready_mask: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObservedEntrySummary {
    pub(crate) controller_mask: u8,
    pub(crate) dynamic_controller_mask: u8,
    pub(crate) progress_mask: u8,
    pub(crate) ready_arm_mask: u8,
    pub(crate) ready_mask: u8,
}

impl ObservedEntrySummary {
    pub(crate) const EMPTY: Self = Self {
        controller_mask: 0,
        dynamic_controller_mask: 0,
        progress_mask: 0,
        ready_arm_mask: 0,
        ready_mask: 0,
    };

    #[inline]
    pub(crate) fn clear(&mut self) {
        *self = Self::EMPTY;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct GlobalFrontierObservedState {
    pub(crate) summary: ObservedEntrySummary,
    pub(crate) observation_generation: u16,
}

impl GlobalFrontierObservedState {
    pub(crate) const EMPTY: Self = Self {
        summary: ObservedEntrySummary::EMPTY,
        observation_generation: 0,
    };
}

impl ObservedEntrySet {
    pub(crate) const EMPTY: Self = Self {
        slots: EntryBuffer::EMPTY,
        controller_mask: 0,
        dynamic_controller_mask: 0,
        progress_mask: 0,
        ready_arm_mask: 0,
        ready_mask: 0,
    };

    #[inline]
    pub(crate) const fn from_parts(slots: *mut FrontierObservationSlot, capacity: usize) -> Self {
        Self::from_parts_with_summary(slots, capacity, ObservedEntrySummary::EMPTY)
    }

    #[inline]
    pub(crate) const fn from_parts_with_summary(
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

    #[inline]
    pub(crate) fn clear(&mut self) {
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
    pub(crate) const fn summary(self) -> ObservedEntrySummary {
        ObservedEntrySummary {
            controller_mask: self.controller_mask,
            dynamic_controller_mask: self.dynamic_controller_mask,
            progress_mask: self.progress_mask,
            ready_arm_mask: self.ready_arm_mask,
            ready_mask: self.ready_mask,
        }
    }

    #[inline]
    pub(crate) fn copy_from(&mut self, src: Self) {
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
    pub(crate) fn len(&self) -> usize {
        cached_frontier_observation_slots_len(self.slots)
    }

    #[inline]
    pub(crate) fn occupancy_mask(self) -> u8 {
        let len = self.len();
        if len >= FRONTIER_SLOT_MASK_BITS {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    pub(crate) fn frontier_mask(self, frontier: FrontierKind) -> u8 {
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
    pub(crate) fn slot_for_entry(self, entry_idx: usize) -> Option<usize> {
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

    pub(crate) fn insert_entry(&mut self, entry_idx: usize) -> Option<(u8, bool)> {
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
    pub(crate) fn entry_bit(self, entry_idx: usize) -> u8 {
        match self.slot_for_entry(entry_idx) {
            Some(slot) => 1u8 << slot,
            None => 0,
        }
    }

    #[inline]
    pub(crate) fn first_entry_idx(self, mask: u8) -> Option<usize> {
        if mask == 0 {
            return None;
        }
        let observed_idx = mask.trailing_zeros() as usize;
        if observed_idx >= self.len() {
            return None;
        }
        let entry = self.slots[observed_idx].entry;
        if entry.is_absent() {
            return None;
        }
        let entry_idx = state_index_to_usize(entry);
        if entry_idx >= MAX_STATES {
            return None;
        }
        Some(entry_idx)
    }

    #[inline]
    pub(crate) fn observe_with_frontier_mask(
        &mut self,
        observed_bit: u8,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) {
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

    #[inline]
    pub(crate) fn replace_observation_with_frontier_mask(
        &mut self,
        entry_idx: usize,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) -> bool {
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

    pub(crate) fn move_entry_slot(&mut self, entry_idx: usize, new_slot_idx: usize) -> bool {
        let Some(source_slot_idx) = self.slot_for_entry(entry_idx) else {
            return false;
        };
        let len = self.len();
        if source_slot_idx >= len || new_slot_idx >= len {
            return false;
        }
        if source_slot_idx == new_slot_idx {
            return true;
        }
        let entry = self.slots[source_slot_idx];
        if source_slot_idx < new_slot_idx {
            let mut slot_idx = source_slot_idx;
            while slot_idx < new_slot_idx {
                self.slots[slot_idx] = self.slots[slot_idx + 1];
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = source_slot_idx;
            while slot_idx > new_slot_idx {
                self.slots[slot_idx] = self.slots[slot_idx - 1];
                slot_idx -= 1;
            }
        }
        self.slots[new_slot_idx] = entry;
        self.controller_mask =
            Self::move_slot_mask(self.controller_mask, len, source_slot_idx, new_slot_idx);
        self.dynamic_controller_mask = Self::move_slot_mask(
            self.dynamic_controller_mask,
            len,
            source_slot_idx,
            new_slot_idx,
        );
        self.progress_mask =
            Self::move_slot_mask(self.progress_mask, len, source_slot_idx, new_slot_idx);
        self.ready_arm_mask =
            Self::move_slot_mask(self.ready_arm_mask, len, source_slot_idx, new_slot_idx);
        self.ready_mask = Self::move_slot_mask(self.ready_mask, len, source_slot_idx, new_slot_idx);
        true
    }

    pub(crate) fn insert_observation_at_slot_with_frontier_mask(
        &mut self,
        entry_idx: usize,
        slot_idx: usize,
        slot: FrontierObservationSlot,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) -> bool {
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
        if slot.entry != entry {
            crate::invariant();
        }
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

    pub(crate) fn remove_observation(&mut self, entry_idx: usize) -> bool {
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

    pub(crate) fn replace_entry_at_slot_with_frontier_mask(
        &mut self,
        source_entry_idx: usize,
        new_entry_idx: usize,
        slot: FrontierObservationSlot,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
    ) -> bool {
        if source_entry_idx >= MAX_STATES || new_entry_idx >= MAX_STATES {
            return false;
        }
        let Some(slot_idx) = self.slot_for_entry(source_entry_idx) else {
            return false;
        };
        let len = self.len();
        if slot_idx >= len {
            return false;
        }
        let Some(source_entry) = checked_state_index(source_entry_idx) else {
            return false;
        };
        let Some(new_entry) = checked_state_index(new_entry_idx) else {
            return false;
        };
        if self.slots[slot_idx].entry != source_entry {
            return false;
        }
        if self.slot_for_entry(new_entry_idx).is_some() {
            return false;
        }
        let observed_bit = 1u8 << slot_idx;
        if slot.entry != new_entry {
            crate::invariant();
        }
        self.slots[slot_idx] = slot;
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.observe_with_frontier_mask(observed_bit, observed, frontier_mask);
        true
    }

    pub(crate) fn move_slot_mask(
        mask: u8,
        len: usize,
        source_slot_idx: usize,
        new_slot_idx: usize,
    ) -> u8 {
        let mut remapped = 0u8;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let source_slot = if source_slot_idx < new_slot_idx {
                if slot_idx < source_slot_idx || slot_idx > new_slot_idx {
                    slot_idx
                } else if slot_idx == new_slot_idx {
                    source_slot_idx
                } else {
                    slot_idx + 1
                }
            } else if slot_idx < new_slot_idx || slot_idx > source_slot_idx {
                slot_idx
            } else if slot_idx == new_slot_idx {
                source_slot_idx
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

    pub(crate) fn insert_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
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

    pub(crate) fn remove_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
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
