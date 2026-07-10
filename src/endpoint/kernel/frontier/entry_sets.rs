use super::{
    Deref, DerefMut, FRONTIER_SLOT_MASK_BITS, FrontierKind, FrontierObservationSlot, Index,
    IndexMut, MAX_STATES, OfferEntryObservedState, StateIndex,
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
            /* SAFETY: `EntryBuffer` stores a pointer and u8 capacity created by
            the frontier owner for one initialized entry slice; shared slicing
            is tied to `&self`. */
            unsafe { slice::from_raw_parts(self.ptr, self.capacity()) }
        }
    }

    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            /* SAFETY: `&mut self` is the entry-buffer mutation token, and the
            stored pointer/capacity describe one initialized frontier entry
            slice owned by this buffer. */
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

impl ObservedEntrySet {
    #[inline]
    pub(crate) const fn from_parts(slots: *mut FrontierObservationSlot, capacity: usize) -> Self {
        Self {
            slots: EntryBuffer::from_parts(slots, capacity),
            controller_mask: 0,
            dynamic_controller_mask: 0,
            progress_mask: 0,
            ready_arm_mask: 0,
            ready_mask: 0,
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
            if (self.slots[slot_idx].frontier_mask & frontier.bit()) != 0 {
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
            frontier_mask: 0,
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
                let summary_bits = &mut self.slots[slot_idx].frontier_mask;
                *summary_bits = (*summary_bits & !0x0f) | (frontier_mask & 0x0f);
            }
        }
    }
}
