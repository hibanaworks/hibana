use super::{
    ActiveEntrySet, CurrentScopeSelectionMeta, EntryBuffer, FrontierKind, Index, IndexMut,
    LaneOfferState, LaneSet, LaneSetView, LaneWord, ObservedEntrySet, ObservedEntrySummary,
    ScopeId, StateIndex, checked_state_index,
};
pub(crate) struct OfferLaneEntrySlotMasks {
    pub(crate) ptr: *mut u8,
    len: u16,
}

impl OfferLaneEntrySlotMasks {
    #[inline]
    pub(crate) const fn from_parts(ptr: *mut u8, len: usize) -> Self {
        if len > u16::MAX as usize {
            crate::invariant();
        }
        Self {
            ptr,
            len: len as u16,
        }
    }

    #[inline]
    pub(crate) const fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        let mut idx = 0usize;
        while idx < self.len() {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.ptr.add(idx).write(0);
            }
            idx += 1;
        }
    }

    #[inline]
    pub(crate) fn set_logical_mask(&mut self, logical_lane: usize, value: u8) {
        if logical_lane < self.len() {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.ptr.add(logical_lane).write(value);
            }
        }
    }
}

static ZERO_LANE_MASK: u8 = 0;

impl Index<usize> for OfferLaneEntrySlotMasks {
    type Output = u8;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        if index < self.len() {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { &*self.ptr.add(index) }
        } else {
            &ZERO_LANE_MASK
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RootFrontierState {
    pub(crate) root: ScopeId,
    pub(crate) observed_entries: ObservedEntrySummary,
    pub(crate) observed_key_state: ObservedKeyState,
    pub(crate) active_start: u8,
    pub(crate) active_len: u8,
}

impl RootFrontierState {
    pub(crate) const EMPTY: Self = Self {
        root: ScopeId::none(),
        observed_entries: ObservedEntrySummary::EMPTY,
        observed_key_state: ObservedKeyState::Absent,
        active_start: 0,
        active_len: 0,
    };

    #[inline]
    pub(crate) fn observed_key_valid(self) -> bool {
        self.observed_key_state.is_present()
    }

    #[inline]
    pub(crate) fn clear_observed_key_cache(&mut self) {
        self.observed_entries = ObservedEntrySummary::EMPTY;
        self.observed_key_state = ObservedKeyState::Absent;
    }

    #[inline]
    pub(crate) fn mark_observed_key_cached(&mut self) {
        self.observed_key_state = ObservedKeyState::Present;
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObservedKeyState {
    Absent = 0,
    Present = 1,
}

impl ObservedKeyState {
    #[inline]
    pub(crate) const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrontierObservationMetaSlot {
    pub(crate) entry_summary_fingerprint: u8,
    pub(crate) scope_generation: u16,
    pub(crate) route_change_generation: u16,
}

impl FrontierObservationMetaSlot {
    pub(crate) const EMPTY: Self = Self {
        entry_summary_fingerprint: 0,
        scope_generation: 0,
        route_change_generation: 0,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrontierObservationSlot {
    pub(crate) entry: StateIndex,
    pub(crate) meta: FrontierObservationMetaSlot,
}

impl FrontierObservationSlot {
    pub(crate) const EMPTY: Self = Self {
        entry: StateIndex::ABSENT,
        meta: FrontierObservationMetaSlot::EMPTY,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct FrontierObservationKey {
    pub(crate) slots: EntryBuffer<FrontierObservationSlot>,
    offer_lanes: LaneSet,
}

impl FrontierObservationKey {
    pub(crate) const EMPTY: Self = Self {
        slots: EntryBuffer::EMPTY,
        offer_lanes: LaneSet::EMPTY,
    };

    #[inline]
    pub(crate) const fn from_parts(
        slots: *mut FrontierObservationSlot,
        capacity: usize,
        offer_lane_words: *mut LaneWord,
        lane_word_len: usize,
    ) -> Self {
        Self {
            slots: EntryBuffer::from_parts(slots, capacity),
            offer_lanes: LaneSet::from_parts(offer_lane_words, lane_word_len),
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        let mut idx = 0usize;
        while idx < self.slots.capacity() {
            self.slots[idx] = FrontierObservationSlot::EMPTY;
            idx += 1;
        }
        self.offer_lanes.clear();
    }

    #[inline]
    pub(crate) fn observed_entries(self, summary: ObservedEntrySummary) -> ObservedEntrySet {
        ObservedEntrySet::from_parts_with_summary(self.slots.ptr, self.slots.capacity(), summary)
    }

    #[inline]
    pub(crate) fn copy_from(&mut self, src: Self) {
        self.clear();
        let len = cached_frontier_observation_slots_len(src.slots);
        let mut idx = 0usize;
        while idx < len {
            self.slots[idx] = src.slots[idx];
            idx += 1;
        }
        self.offer_lanes.copy_from(src.offer_lanes());
    }

    #[inline]
    pub(crate) fn set_active_entries_from(&mut self, src: ActiveEntrySet) {
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
    pub(crate) fn len(&self) -> usize {
        cached_frontier_observation_slots_len(self.slots)
    }

    #[inline]
    pub(crate) fn entry_state(&self, idx: usize) -> StateIndex {
        if idx >= self.slots.capacity() {
            return StateIndex::ABSENT;
        }
        self.slots[idx].entry
    }

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
    pub(crate) fn contains_entry(&self, entry_idx: usize) -> bool {
        self.slot_for_entry(entry_idx).is_some()
    }

    #[inline]
    pub(crate) fn entries_equal(&self, other: &Self) -> bool {
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
    pub(crate) fn exact_entries_match(&self, active_entries: ActiveEntrySet) -> bool {
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
    pub(crate) fn slot(&self, idx: usize) -> FrontierObservationMetaSlot {
        self.slots[idx].meta
    }

    #[inline]
    pub(crate) fn slot_mut(&mut self, idx: usize) -> &mut FrontierObservationMetaSlot {
        &mut self.slots[idx].meta
    }

    #[inline]
    pub(crate) fn offer_lanes(&self) -> LaneSetView<'_> {
        self.offer_lanes.view()
    }

    #[inline]
    pub(crate) fn lane_sets_equal(&self, other: &Self) -> bool {
        self.offer_lanes().equals(other.offer_lanes())
    }

    pub(crate) fn set_offer_lanes(&mut self, lanes: LaneSetView) {
        self.offer_lanes.copy_from(lanes);
    }

    #[inline]
    pub(crate) fn insert_offer_lane(&mut self, lane_idx: usize) {
        self.offer_lanes.insert(lane_idx);
    }
}

impl PartialEq for FrontierObservationKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let len = self.len();
        if len != other.len() || !self.lane_sets_equal(other) {
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
pub(crate) fn cached_frontier_observation_slots_len(
    slots: EntryBuffer<FrontierObservationSlot>,
) -> usize {
    let mut len = 0usize;
    while len < slots.capacity() {
        if slots[len].entry.is_absent() {
            break;
        }
        len += 1;
    }
    len
}

#[derive(Clone, Copy)]
pub(crate) struct OfferEntrySummary {
    pub(crate) frontier_mask: u8,
    pub(crate) flags: u8,
}

impl OfferEntrySummary {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_INTRINSIC_READY: u8 = 1 << 2;

    pub(crate) const EMPTY: Self = Self {
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(crate) fn observe_lane(&mut self, info: LaneOfferState) {
        self.frontier_mask |= info.frontier.bit();
        if info.is_controller() {
            self.flags |= Self::FLAG_CONTROLLER;
        }
        if info.is_dynamic() {
            self.flags |= Self::FLAG_DYNAMIC;
        }
        if info.intrinsic_ready() {
            self.flags |= Self::FLAG_INTRINSIC_READY;
        }
    }

    #[inline]
    pub(crate) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(crate) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(crate) fn intrinsic_ready(self) -> bool {
        (self.flags & Self::FLAG_INTRINSIC_READY) != 0
    }

    #[inline]
    pub(crate) fn observation_fingerprint(self) -> u8 {
        self.frontier_mask | (self.flags << 4)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct OfferEntryState {
    presence: OfferEntryPresence,
    pub(crate) lane_idx: u8,
    pub(crate) parallel_root: ScopeId,
    pub(crate) frontier: FrontierKind,
    pub(crate) scope_id: ScopeId,
    pub(crate) selection_meta: CurrentScopeSelectionMeta,
    pub(crate) summary: OfferEntrySummary,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum OfferEntryPresence {
    Inactive,
    Active,
}

impl OfferEntryState {
    pub(crate) const EMPTY: Self = Self {
        presence: OfferEntryPresence::Inactive,
        lane_idx: u8::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: ScopeId::none(),
        selection_meta: CurrentScopeSelectionMeta::EMPTY,
        summary: OfferEntrySummary::EMPTY,
    };

    #[inline]
    pub(crate) const fn active(
        lane_idx: u8,
        parallel_root: ScopeId,
        frontier: FrontierKind,
        scope_id: ScopeId,
        selection_meta: CurrentScopeSelectionMeta,
        summary: OfferEntrySummary,
    ) -> Self {
        Self {
            presence: OfferEntryPresence::Active,
            lane_idx,
            parallel_root,
            frontier,
            scope_id,
            selection_meta,
            summary,
        }
    }

    #[inline]
    pub(crate) const fn is_active(self) -> bool {
        matches!(self.presence, OfferEntryPresence::Active)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct OfferEntrySlot {
    entry: StateIndex,
    state: OfferEntryState,
}

impl OfferEntrySlot {
    pub(crate) const EMPTY: Self = Self {
        entry: StateIndex::ABSENT,
        state: OfferEntryState::EMPTY,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct OfferEntryTable {
    slots: EntryBuffer<OfferEntrySlot>,
}

impl OfferEntryTable {
    #[inline]
    pub(crate) const fn has_storage(&self) -> bool {
        !self.slots.ptr.is_null() && self.slots.capacity() != 0
    }

    pub(crate) unsafe fn init_from_parts(
        dst: *mut Self,
        slots: *mut OfferEntrySlot,
        capacity: usize,
    ) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).slots).write(EntryBuffer::from_parts(slots, capacity));
        }
        if slots.is_null() {
            return;
        }
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe { slots.add(idx).write(OfferEntrySlot::EMPTY) };
            idx += 1;
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        if self.slots.ptr.is_null() {
            return 0;
        }
        let mut len = 0usize;
        let capacity = self.slots.capacity();
        while len < capacity {
            if self.slots[len].entry.is_absent() {
                break;
            }
            len += 1;
        }
        len
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
    pub(crate) fn get(&self, entry_idx: usize) -> Option<&OfferEntryState> {
        self.slot_for_entry(entry_idx)
            .map(|slot_idx| &self.slots[slot_idx].state)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, entry_idx: usize) -> Option<&mut OfferEntryState> {
        if !self.has_storage() {
            return None;
        }
        let slot_idx = self.slot_for_entry(entry_idx)?;
        Some(&mut self.slots[slot_idx].state)
    }

    pub(crate) fn set(&mut self, entry_idx: usize, state: OfferEntryState) {
        if !self.has_storage() {
            return;
        }
        if !state.is_active() {
            self.clear(entry_idx);
            return;
        }
        let slot = self.ensure_entry_mut(entry_idx);
        *slot = state;
    }

    pub(crate) fn clear(&mut self, entry_idx: usize) {
        if !self.has_storage() {
            return;
        }
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

    fn ensure_entry_mut(&mut self, entry_idx: usize) -> &mut OfferEntryState {
        if !self.has_storage() {
            crate::invariant();
        }
        if let Some(slot_idx) = self.slot_for_entry(entry_idx) {
            return &mut self.slots[slot_idx].state;
        }
        let entry = crate::invariant_some(checked_state_index(entry_idx));
        let len = self.len();
        if len >= self.slots.capacity() {
            crate::invariant();
        }
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

impl Index<usize> for OfferEntryTable {
    type Output = OfferEntryState;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        crate::invariant_some(self.get(index))
    }
}

impl IndexMut<usize> for OfferEntryTable {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.ensure_entry_mut(index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferEntryObservedState {
    pub(crate) scope_id: ScopeId,
    pub(crate) frontier_mask: u8,
    pub(crate) flags: u8,
}

impl OfferEntryObservedState {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_PROGRESS: u8 = 1 << 2;
    pub(crate) const FLAG_READY_ARM: u8 = 1 << 3;
    pub(crate) const FLAG_BINDING_READY: u8 = 1 << 4;
    pub(crate) const FLAG_READY: u8 = 1 << 5;

    #[inline]
    pub(crate) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(crate) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(crate) fn has_progress_evidence(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS) != 0
    }

    #[inline]
    pub(crate) fn has_ready_arm_evidence(self) -> bool {
        (self.flags & Self::FLAG_READY_ARM) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierCandidate {
    pub(crate) scope_id: ScopeId,
    pub(crate) entry_idx: u16,
    pub(crate) parallel_root: ScopeId,
    pub(crate) frontier: FrontierKind,
    pub(crate) flags: u8,
}

impl FrontierCandidate {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_HAS_EVIDENCE: u8 = 1 << 2;
    pub(crate) const FLAG_READY: u8 = 1 << 3;

    pub(crate) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        entry_idx: u16::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: 0,
    };

    #[inline]
    pub(crate) const fn flags_from_observed(observed: OfferEntryObservedState) -> u8 {
        (if (observed.flags & OfferEntryObservedState::FLAG_CONTROLLER) != 0 {
            Self::FLAG_CONTROLLER
        } else {
            0
        }) | (if (observed.flags & OfferEntryObservedState::FLAG_DYNAMIC) != 0 {
            Self::FLAG_DYNAMIC
        } else {
            0
        }) | (if (observed.flags & OfferEntryObservedState::FLAG_PROGRESS) != 0 {
            Self::FLAG_HAS_EVIDENCE
        } else {
            0
        }) | (if (observed.flags & OfferEntryObservedState::FLAG_READY) != 0 {
            Self::FLAG_READY
        } else {
            0
        })
    }

    #[inline]
    pub(crate) const fn has_evidence(self) -> bool {
        (self.flags & Self::FLAG_HAS_EVIDENCE) != 0
    }

    #[inline]
    pub(crate) const fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }
}
