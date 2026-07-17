use super::{
    FrontierObservationSlot, MAX_STATES, OfferEntryAdmission, OfferEntryObservedState, StateIndex,
    cached_frontier_observation_slots_len, checked_state_index, state_index_to_usize,
};

mod buffer;
use buffer::{EntryBuffer, EntryView};

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
    slots: EntryView<ActiveEntrySlot>,
}

impl ActiveEntrySet {
    pub(crate) const EMPTY: Self = Self {
        slots: EntryView::EMPTY,
    };

    #[inline]
    pub(crate) const unsafe fn from_parts(slots: *const ActiveEntrySlot, capacity: usize) -> Self {
        Self {
            /* SAFETY: caller owns the active-entry resident span and keeps it
            initialized and immutable while this read-only view is used. */
            slots: unsafe { EntryView::from_parts(slots, capacity) },
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
    pub(crate) fn slot_at(&self, slot_idx: usize) -> Option<ActiveEntrySlot> {
        if slot_idx >= self.len() {
            return None;
        }
        Some(self.slots[slot_idx])
    }

    #[inline]
    pub(crate) fn contains_only(&self, entry_idx: usize) -> bool {
        self.len() == 1
            && self
                .slot_at(0)
                .is_some_and(|slot| state_index_to_usize(slot.entry) == entry_idx)
    }
}

pub(crate) struct ActiveEntrySetBuilder {
    slots: EntryBuffer<ActiveEntrySlot>,
}

impl ActiveEntrySetBuilder {
    #[inline]
    pub(crate) const unsafe fn from_parts(slots: *mut ActiveEntrySlot, capacity: usize) -> Self {
        Self {
            /* SAFETY: caller grants this builder exclusive access to the
            active-entry span until `seal` consumes the mutation capability. */
            slots: unsafe { EntryBuffer::from_parts(slots, capacity) },
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

    #[cfg(kani)]
    #[inline]
    pub(crate) const fn capacity(&self) -> usize {
        self.slots.capacity()
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
    pub(crate) fn slot_for_entry(&self, entry_idx: usize) -> Option<usize> {
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
        if len >= self.slots.capacity() {
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

    #[inline]
    pub(crate) const fn seal(self) -> ActiveEntrySet {
        ActiveEntrySet {
            slots: self.slots.into_view(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ObservedEntrySet {
    slots: EntryView<FrontierObservationSlot>,
}

impl ObservedEntrySet {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        cached_frontier_observation_slots_len(self.slots.as_slice())
    }

    #[inline]
    pub(crate) fn slot_for_entry(&self, entry_idx: usize) -> Option<usize> {
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
    pub(crate) fn entry_idx(&self, slot_idx: usize) -> Option<usize> {
        let slot = *self.slots.as_slice().get(slot_idx)?;
        if slot.entry.is_absent() {
            return None;
        }
        let entry_idx = state_index_to_usize(slot.entry);
        (entry_idx < MAX_STATES).then_some(entry_idx)
    }

    #[inline]
    pub(crate) fn slot(&self, slot_idx: usize) -> Option<FrontierObservationSlot> {
        if slot_idx >= self.len() {
            None
        } else {
            Some(self.slots[slot_idx])
        }
    }
}

pub(crate) struct ObservedEntrySetBuilder {
    slots: EntryBuffer<FrontierObservationSlot>,
}

impl ObservedEntrySetBuilder {
    #[inline]
    pub(crate) const unsafe fn from_parts(
        slots: *mut FrontierObservationSlot,
        capacity: usize,
    ) -> Self {
        Self {
            /* SAFETY: caller grants this builder exclusive access to the
            observation span until `seal` consumes the mutation capability. */
            slots: unsafe { EntryBuffer::from_parts(slots, capacity) },
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        let mut idx = 0usize;
        while idx < self.slots.capacity() {
            self.slots[idx] = FrontierObservationSlot::EMPTY;
            idx += 1;
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        cached_frontier_observation_slots_len(self.slots.as_slice())
    }

    #[inline]
    pub(crate) fn slot_for_entry(&self, entry_idx: usize) -> Option<usize> {
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

    pub(crate) fn insert_entry(&mut self, entry_idx: usize) -> Option<(usize, bool)> {
        if entry_idx >= MAX_STATES {
            return None;
        }
        let entry = checked_state_index(entry_idx)?;
        if let Some(observed_idx) = self.slot_for_entry(entry_idx) {
            return Some((observed_idx, false));
        }
        let observed_idx = self.len();
        if observed_idx >= self.slots.capacity() {
            return None;
        }
        self.slots[observed_idx] = FrontierObservationSlot::new(entry);
        Some((observed_idx, true))
    }

    #[inline]
    pub(crate) fn record_observation(
        &mut self,
        slot_idx: usize,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
        admission: OfferEntryAdmission,
    ) {
        if slot_idx >= self.len() {
            crate::invariant();
        }
        self.slots[slot_idx].record(observed, frontier_mask, admission);
    }

    #[inline]
    pub(crate) const fn seal(self) -> ObservedEntrySet {
        ObservedEntrySet {
            slots: self.slots.into_view(),
        }
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
