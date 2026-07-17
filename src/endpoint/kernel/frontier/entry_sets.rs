use super::{
    FrontierObservationSlot, MAX_STATES, OfferEntryAdmission, OfferEntryKey,
    OfferEntryObservedState, cached_frontier_observation_slots_len, checked_state_index,
    state_index_to_usize,
};

mod buffer;
use buffer::{EntryBuffer, EntryView};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveEntrySlot {
    pub(crate) key: OfferEntryKey,
    pub(crate) lane_idx: u8,
}

impl ActiveEntrySlot {
    pub(crate) const EMPTY: Self = Self {
        key: OfferEntryKey::EMPTY,
        lane_idx: u8::MAX,
    };

    #[inline]
    pub(crate) const fn new(key: OfferEntryKey, lane_idx: u8) -> Option<Self> {
        if key.is_absent() {
            None
        } else {
            Some(Self { key, lane_idx })
        }
    }

    #[inline]
    pub(crate) const fn precedes(self, other: Self) -> bool {
        self.lane_idx < other.lane_idx
            || (self.lane_idx == other.lane_idx
                && (self.key.entry().raw() < other.key.entry().raw()
                    || (self.key.entry().raw() == other.key.entry().raw()
                        && self.key.scope().raw() < other.key.scope().raw())))
    }
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
            if self.slots[len].key.is_absent() {
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
    pub(crate) fn contains_only(&self, key: OfferEntryKey) -> bool {
        self.len() == 1 && self.slot_at(0).is_some_and(|slot| slot.key == key)
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
            if self.slots[len].key.is_absent() {
                break;
            }
            len += 1;
        }
        len
    }

    pub(crate) fn insert_key(&mut self, key: OfferEntryKey, lane_idx: u8) {
        let Some(incoming) = ActiveEntrySlot::new(key, lane_idx) else {
            crate::invariant();
        };
        let len = self.len();
        let mut insert_idx = 0usize;
        while insert_idx < len {
            let existing = self.slots[insert_idx];
            if existing.key == key {
                return;
            }
            if incoming.precedes(existing) {
                break;
            }
            insert_idx += 1;
        }
        if len >= self.slots.capacity() {
            crate::invariant();
        }
        let mut shift_idx = len;
        while shift_idx > insert_idx {
            self.slots[shift_idx] = self.slots[shift_idx - 1];
            shift_idx -= 1;
        }
        self.slots[insert_idx] = incoming;
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
    pub(crate) fn entry_group_end(&self, slot_idx: usize) -> Option<usize> {
        let entry = self.slots.as_slice().get(slot_idx)?.entry;
        if entry.is_absent() {
            return None;
        }
        let len = self.len();
        let mut end = slot_idx + 1;
        while end < len && self.slots[end].entry == entry {
            end += 1;
        }
        Some(end)
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

    pub(crate) fn first_selectable_ready_entry_except(
        &self,
        excluded_entry_idx: usize,
    ) -> Option<usize> {
        let mut slot_idx = 0usize;
        while slot_idx < self.len() {
            let slot = crate::invariant_some(self.slot(slot_idx));
            let entry_idx = crate::invariant_some(self.entry_idx(slot_idx));
            if entry_idx != excluded_entry_idx && slot.is_selectable() && slot.is_ready() {
                return Some(entry_idx);
            }
            slot_idx += 1;
        }
        None
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

    pub(crate) fn push_exact_observation(
        &mut self,
        observed: OfferEntryObservedState,
        admission: OfferEntryAdmission,
    ) -> usize {
        let incoming = FrontierObservationSlot::from_exact_observation(observed, admission);
        let len = self.len();
        if len >= self.slots.capacity() {
            crate::invariant();
        }
        let mut insert_idx = 0usize;
        while insert_idx < len && self.slots[insert_idx].entry.raw() <= incoming.entry.raw() {
            insert_idx += 1;
        }
        let mut shift_idx = len;
        while shift_idx > insert_idx {
            self.slots[shift_idx] = self.slots[shift_idx - 1];
            shift_idx -= 1;
        }
        self.slots[insert_idx] = incoming;
        insert_idx
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
