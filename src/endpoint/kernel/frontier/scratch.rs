#[cfg(not(test))]
use super::GlobalFrontierObservedState;
use super::{
    ActiveEntrySet, ActiveEntrySlot, EntryBuffer, FrontierCandidate, FrontierObservationKey,
    FrontierObservationSlot, LaneWord, ObservedEntrySet, OfferLaneEntrySlotMasks, ScopeId,
    align_up, max_usize, mem, slice,
};
// # Unsafe Owner Contract
//
// This fragment owns the route-frontier scratch layout and typed views over the
// caller-provided scratch arena. Unsafe pointer arithmetic is bounded by
// `FrontierScratchLayout`; each view is derived from one arena base, aligned by
// the layout calculator, and never outlives the enclosing frontier operation.

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
    cached_observation_key_offer_lanes: FrontierScratchSection,
    observation_key_slots: FrontierScratchSection,
    observation_key_offer_lanes: FrontierScratchSection,
    working_observation_key_slots: FrontierScratchSection,
    working_observation_key_offer_lanes: FrontierScratchSection,
    observed_entry_slots: FrontierScratchSection,
    offer_lane_entry_slot_masks: FrontierScratchSection,
    candidates: FrontierScratchSection,
    visited_scopes: FrontierScratchSection,
    root_scopes: FrontierScratchSection,
    total_bytes: usize,
    total_align: usize,
}

impl FrontierScratchLayout {
    #[inline(always)]
    pub(crate) const fn new(
        max_frontier_entries: usize,
        logical_lane_count: usize,
        lane_word_count: usize,
    ) -> Self {
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

        let cached_observation_key_offer_lanes =
            Self::section_array::<LaneWord>(offset, lane_word_count);
        offset =
            cached_observation_key_offer_lanes.offset + cached_observation_key_offer_lanes.bytes;
        total_align = max_usize(total_align, cached_observation_key_offer_lanes.align);

        let observation_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = observation_key_slots.offset + observation_key_slots.bytes;
        total_align = max_usize(total_align, observation_key_slots.align);

        let observation_key_offer_lanes = Self::section_array::<LaneWord>(offset, lane_word_count);
        offset = observation_key_offer_lanes.offset + observation_key_offer_lanes.bytes;
        total_align = max_usize(total_align, observation_key_offer_lanes.align);

        let working_observation_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = working_observation_key_slots.offset + working_observation_key_slots.bytes;
        total_align = max_usize(total_align, working_observation_key_slots.align);

        let working_observation_key_offer_lanes =
            Self::section_array::<LaneWord>(offset, lane_word_count);
        offset =
            working_observation_key_offer_lanes.offset + working_observation_key_offer_lanes.bytes;
        total_align = max_usize(total_align, working_observation_key_offer_lanes.align);

        let observed_entry_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = observed_entry_slots.offset + observed_entry_slots.bytes;
        total_align = max_usize(total_align, observed_entry_slots.align);

        let offer_lane_entry_slot_masks = Self::section_array::<u8>(offset, logical_lane_count);
        offset = offer_lane_entry_slot_masks.offset + offer_lane_entry_slot_masks.bytes;
        total_align = max_usize(total_align, offer_lane_entry_slot_masks.align);

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
            cached_observation_key_offer_lanes,
            observation_key_slots,
            observation_key_offer_lanes,
            working_observation_key_slots,
            working_observation_key_offer_lanes,
            observed_entry_slots,
            offer_lane_entry_slot_masks,
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
    pub(crate) const fn cached_observation_key_offer_lanes(self) -> FrontierScratchSection {
        self.cached_observation_key_offer_lanes
    }

    #[inline(always)]
    pub(crate) const fn observation_key_slots(self) -> FrontierScratchSection {
        self.observation_key_slots
    }

    #[inline(always)]
    pub(crate) const fn observation_key_offer_lanes(self) -> FrontierScratchSection {
        self.observation_key_offer_lanes
    }

    #[inline(always)]
    pub(crate) const fn working_observation_key_slots(self) -> FrontierScratchSection {
        self.working_observation_key_slots
    }

    #[inline(always)]
    pub(crate) const fn working_observation_key_offer_lanes(self) -> FrontierScratchSection {
        self.working_observation_key_offer_lanes
    }

    #[inline(always)]
    pub(crate) const fn observed_entry_slots(self) -> FrontierScratchSection {
        self.observed_entry_slots
    }

    #[inline(always)]
    pub(crate) const fn offer_lane_entry_slot_masks(self) -> FrontierScratchSection {
        self.offer_lane_entry_slot_masks
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
pub(crate) struct FrontierScratchView {
    candidates: *mut FrontierCandidate,
    frontier_entry_capacity: u8,
    visited_scopes: *mut ScopeId,
    root_scopes: *mut ScopeId,
}

#[inline]
fn frontier_scratch_storage_ptr(scratch_ptr: *mut [u8], layout: FrontierScratchLayout) -> *mut u8 {
    let scratch = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *scratch_ptr };
    debug_assert!(
        scratch.len() >= layout.total_bytes(),
        "frontier scratch reservation must cover compiled layout"
    );
    scratch.as_mut_ptr()
}

#[cfg(not(test))]
#[inline]
pub(crate) fn frontier_global_observed_state_ptr_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
) -> *mut GlobalFrontierObservedState {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
    unsafe {
        storage
            .add(layout.global_observed_state().offset())
            .cast::<GlobalFrontierObservedState>()
    }
}

#[inline]
pub(crate) fn frontier_observation_key_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierObservationKey {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    FrontierObservationKey::from_parts(
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.observation_key_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.observation_key_offer_lanes().offset())
                .cast::<LaneWord>()
        },
        layout.observation_key_offer_lanes().count(),
    )
}

#[inline]
pub(crate) fn frontier_cached_observation_key_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierObservationKey {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    FrontierObservationKey::from_parts(
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.cached_observation_key_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.cached_observation_key_offer_lanes().offset())
                .cast::<LaneWord>()
        },
        layout.cached_observation_key_offer_lanes().count(),
    )
}

#[inline]
pub(crate) fn frontier_working_observation_key_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierObservationKey {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    FrontierObservationKey::from_parts(
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.working_observation_key_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.working_observation_key_offer_lanes().offset())
                .cast::<LaneWord>()
        },
        layout.working_observation_key_offer_lanes().count(),
    )
}

#[inline]
pub(crate) fn frontier_global_active_entries_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> ActiveEntrySet {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    ActiveEntrySet {
        slots: EntryBuffer::from_parts(
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
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
pub(crate) fn frontier_observed_entries_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> ObservedEntrySet {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    ObservedEntrySet::from_parts(
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.observed_entry_slots().offset())
                .cast::<FrontierObservationSlot>()
        },
        frontier_entry_capacity,
    )
}

#[inline]
pub(crate) fn frontier_offer_lane_entry_slot_masks_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
) -> OfferLaneEntrySlotMasks {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    let mut masks = OfferLaneEntrySlotMasks::from_parts(
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            storage
                .add(layout.offer_lane_entry_slot_masks().offset())
                .cast::<u8>()
        },
        layout.offer_lane_entry_slot_masks().count(),
    );
    masks.clear();
    masks
}

impl FrontierScratchView {
    #[inline]
    pub(crate) unsafe fn from_parts(
        storage: *mut u8,
        layout: FrontierScratchLayout,
        frontier_entry_capacity: usize,
    ) -> Self {
        Self {
            candidates: /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe {
                storage
                    .add(layout.candidates().offset())
                    .cast::<FrontierCandidate>()
            },
            frontier_entry_capacity: frontier_entry_capacity as u8,
            visited_scopes: /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe {
                storage
                    .add(layout.visited_scopes().offset())
                    .cast::<ScopeId>()
            },
            root_scopes: /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(layout.root_scopes().offset()).cast::<ScopeId>() },
        }
    }

    #[inline]
    pub(crate) fn candidates_mut(&mut self) -> &mut [FrontierCandidate] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { slice::from_raw_parts_mut(self.candidates, self.frontier_entry_capacity as usize) }
    }

    #[inline]
    pub(crate) fn visited_scopes_mut(&mut self) -> &mut [ScopeId] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe {
            slice::from_raw_parts_mut(self.visited_scopes, self.frontier_entry_capacity as usize)
        }
    }

    #[inline]
    pub(crate) fn root_scopes_mut(&mut self) -> &mut [ScopeId] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe {
            slice::from_raw_parts_mut(self.root_scopes, self.frontier_entry_capacity as usize)
        }
    }
}

#[inline]
pub(crate) fn frontier_scratch_view_from_storage(
    scratch_ptr: *mut [u8],
    layout: FrontierScratchLayout,
    frontier_entry_capacity: usize,
) -> FrontierScratchView {
    let storage = frontier_scratch_storage_ptr(scratch_ptr, layout);
    /* SAFETY: endpoint kernel owns the resident endpoint storage and holds the affine operation borrow for this raw access. */
    unsafe { FrontierScratchView::from_parts(storage, layout, frontier_entry_capacity) }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::{
        FrontierObservationKey, FrontierObservationSlot, FrontierScratchLayout,
        frontier_cached_observation_key_view_from_storage,
        frontier_observation_key_view_from_storage,
    };
    use crate::global::role_program::LaneWord;

    #[test]
    fn global_frontier_scratch_sections_track_max_frontier_entries() {
        let layout = FrontierScratchLayout::new(5, 96, 2);
        assert_eq!(layout.global_active_entry_slots().count(), 5);
        assert_eq!(layout.cached_observation_key_slots().count(), 5);
        assert_eq!(layout.cached_observation_key_offer_lanes().count(), 2);
        assert_eq!(layout.observation_key_slots().count(), 5);
        assert_eq!(layout.observation_key_offer_lanes().count(), 2);
        assert_eq!(layout.working_observation_key_slots().count(), 5);
        assert_eq!(layout.working_observation_key_offer_lanes().count(), 2);
        assert_eq!(layout.observed_entry_slots().count(), 5);
    }

    #[test]
    fn frontier_observation_key_views_track_layout_lane_word_count() {
        let layout = FrontierScratchLayout::new(1, 96, 2);
        let mut scratch = vec![0u8; layout.total_bytes()].into_boxed_slice();
        let scratch_ptr: *mut [u8] = scratch.as_mut();
        let mut key = frontier_observation_key_view_from_storage(scratch_ptr, layout, 1);
        let mut cached = frontier_cached_observation_key_view_from_storage(scratch_ptr, layout, 1);
        let high_lane = LaneWord::BITS as usize + 1;

        key.clear();
        cached.clear();
        key.insert_offer_lane(high_lane);
        cached.copy_from(key);

        assert!(cached.offer_lanes().contains(high_lane));
        assert!(cached.lane_sets_equal(&key));
    }

    #[test]
    fn frontier_observation_key_keeps_exact_lane_sets_beyond_projected_mask() {
        let mut slots_a = [FrontierObservationSlot::EMPTY; 1];
        let mut offer_a = [0usize; 2];
        let mut slots_b = [FrontierObservationSlot::EMPTY; 1];
        let mut offer_b = [0usize; 2];
        let mut key_a =
            FrontierObservationKey::from_parts(slots_a.as_mut_ptr(), 1, offer_a.as_mut_ptr(), 2);
        let mut key_b =
            FrontierObservationKey::from_parts(slots_b.as_mut_ptr(), 1, offer_b.as_mut_ptr(), 2);
        key_a.clear();
        key_b.clear();
        key_a.insert_offer_lane(0);
        key_b.insert_offer_lane(0);
        let high_lane = LaneWord::BITS as usize + 1;
        key_a.insert_offer_lane(high_lane);

        let mut lanes_a = [u8::MAX; 2];
        let mut lanes_b = [u8::MAX; 1];
        assert_eq!(
            key_a
                .offer_lanes()
                .write_lane_indices(high_lane + 1, &mut lanes_a),
            2
        );
        assert_eq!(
            key_b
                .offer_lanes()
                .write_lane_indices(high_lane + 1, &mut lanes_b),
            1
        );
        assert_eq!(lanes_a, [0, high_lane as u8]);
        assert_eq!(lanes_b, [0]);
        assert!(
            !key_a.lane_sets_equal(&key_b),
            "exact lane snapshots must still distinguish high-lane changes"
        );
        assert!(
            key_a != key_b,
            "FrontierObservationKey equality must account for exact lane snapshots"
        );
    }
}
