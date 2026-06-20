use super::{
    ActiveEntrySet, ActiveEntrySlot, EntryBuffer, FrontierCandidate, FrontierObservationSlot,
    ObservedEntrySet, ScopeId, align_up, max_usize, mem, slice,
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

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn count(self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierScratchLayout {
    global_active_entry_slots: FrontierScratchSection,
    observed_entry_slots: FrontierScratchSection,
    candidates: FrontierScratchSection,
    visited_scopes: FrontierScratchSection,
    total_bytes: usize,
    total_align: usize,
}

impl FrontierScratchLayout {
    pub(crate) const fn new(max_frontier_entries: usize, _lane_word_count: usize) -> Self {
        let mut offset = 0usize;
        let mut total_align = 1usize;

        let global_active_entry_slots =
            Self::section_array::<ActiveEntrySlot>(offset, max_frontier_entries);
        offset = global_active_entry_slots.offset + global_active_entry_slots.bytes;
        total_align = max_usize(total_align, global_active_entry_slots.align);

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

        Self {
            global_active_entry_slots,
            observed_entry_slots,
            candidates,
            visited_scopes,
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
    const fn section_array<T>(offset: usize, count: usize) -> FrontierScratchSection {
        let align = mem::align_of::<T>();
        let bytes = checked_usize_mul(mem::size_of::<T>(), count);
        FrontierScratchSection {
            offset: align_up(offset, align),
            align,
            bytes,
            count,
        }
    }
}

#[inline(always)]
const fn checked_usize_mul(lhs: usize, rhs: usize) -> usize {
    if lhs != 0 && rhs > usize::MAX / lhs {
        crate::invariant();
    }
    lhs * rhs
}

#[derive(Clone, Copy)]
pub(crate) struct FrontierScratchView {
    candidates: *mut FrontierCandidate,
    frontier_entry_capacity: u8,
    visited_scopes: *mut ScopeId,
}

#[inline]
fn frontier_scratch_storage_ptr(scratch_ptr: *mut [u8], layout: FrontierScratchLayout) -> *mut u8 {
    let scratch = /* SAFETY: endpoint frontier owns `scratch_ptr` for the
    current affine operation. The backing slice remains resident for this poll,
    and this helper only derives the arena base after checking the layout byte
    budget. */
        unsafe { &mut *scratch_ptr };
    if scratch.len() < layout.total_bytes() {
        crate::invariant();
    }
    scratch.as_mut_ptr()
}

#[inline]
fn frontier_section_ptr<T>(storage: *mut u8, section: FrontierScratchSection) -> *mut T {
    if section.align != mem::align_of::<T>() {
        crate::invariant();
    }
    if section.count != 0 && section.bytes / section.count != mem::size_of::<T>() {
        crate::invariant();
    }
    /* SAFETY: `FrontierScratchLayout` produced this section from the same arena
    base and `frontier_scratch_storage_ptr` checked that the arena has
    `layout.total_bytes()`. The section records the type alignment and byte
    length used for this typed column. */
    unsafe { storage.add(section.offset()).cast::<T>() }
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
            frontier_section_ptr(storage, layout.global_active_entry_slots()),
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
        frontier_section_ptr(storage, layout.observed_entry_slots()),
        frontier_entry_capacity,
    )
}

impl FrontierScratchView {
    #[inline]
    pub(crate) unsafe fn from_parts(
        storage: *mut u8,
        layout: FrontierScratchLayout,
        frontier_entry_capacity: usize,
    ) -> Self {
        Self {
            candidates: frontier_section_ptr(storage, layout.candidates()),
            frontier_entry_capacity: frontier_entry_capacity as u8,
            visited_scopes: frontier_section_ptr(storage, layout.visited_scopes()),
        }
    }

    #[inline]
    pub(crate) fn candidates_mut(&mut self) -> &mut [FrontierCandidate] {
        /* SAFETY: `candidates` points at the `FrontierCandidate` section of the
        scratch arena, and `frontier_entry_capacity` is the count used to build
        that section; this mutable slice is scoped to `&mut self`. */
        unsafe { slice::from_raw_parts_mut(self.candidates, self.frontier_entry_capacity as usize) }
    }

    #[inline]
    pub(crate) fn visited_scopes_mut(&mut self) -> &mut [ScopeId] {
        /* SAFETY: `visited_scopes` is the initialized `ScopeId` scratch column
        paired with this frontier view; `&mut self` keeps this slice as the only
        live mutable borrow for the shared entry capacity. */
        unsafe {
            slice::from_raw_parts_mut(self.visited_scopes, self.frontier_entry_capacity as usize)
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
    use super::FrontierScratchLayout;

    #[test]
    fn global_frontier_scratch_sections_track_max_frontier_entries() {
        let layout = FrontierScratchLayout::new(5, 2);
        assert_eq!(layout.global_active_entry_slots().count(), 5);
        assert_eq!(layout.observed_entry_slots().count(), 5);
    }
}
