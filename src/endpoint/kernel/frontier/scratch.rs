use super::{
    ActiveEntrySetBuilder, ActiveEntrySlot, FrontierObservationSlot, ObservedEntrySetBuilder, mem,
    slice,
};
use crate::global::role_program::LANE_DOMAIN_SIZE;
use crate::runtime_core::layout::{add, align_up, mul};

#[inline(always)]
const fn max_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}
use core::marker::PhantomData;
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

    #[inline(always)]
    const fn end(self) -> usize {
        add(self.offset, self.bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierScratchLayout {
    global_active_entry_slots: FrontierScratchSection,
    observed_entry_slots: FrontierScratchSection,
    total_bytes: usize,
    total_align: usize,
}

impl FrontierScratchLayout {
    pub(crate) const fn new(max_frontier_entries: usize) -> Self {
        if max_frontier_entries > LANE_DOMAIN_SIZE {
            crate::invariant();
        }
        let mut offset = 0usize;
        let mut total_align = 1usize;

        let global_active_entry_slots =
            Self::section_array::<ActiveEntrySlot>(offset, max_frontier_entries);
        offset = add(
            global_active_entry_slots.offset,
            global_active_entry_slots.bytes,
        );
        total_align = max_usize(total_align, global_active_entry_slots.align);

        let observed_entry_slots =
            Self::section_array::<FrontierObservationSlot>(offset, max_frontier_entries);
        offset = add(observed_entry_slots.offset, observed_entry_slots.bytes);
        total_align = max_usize(total_align, observed_entry_slots.align);

        Self {
            global_active_entry_slots,
            observed_entry_slots,
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
    const fn section_array<T>(offset: usize, count: usize) -> FrontierScratchSection {
        let align = mem::align_of::<T>();
        let bytes = mul(mem::size_of::<T>(), count);
        FrontierScratchSection {
            offset: align_up(offset, align),
            align,
            bytes,
            count,
        }
    }
}

pub(crate) struct FrontierScratchSectionLease<'lease, T> {
    ptr: *mut T,
    count: u16,
    _lease: PhantomData<&'lease mut [T]>,
}

pub(crate) struct FrontierScratchWorkspace<'lease> {
    pub(in crate::endpoint::kernel) global_active_entries:
        FrontierScratchSectionLease<'lease, ActiveEntrySlot>,
    pub(in crate::endpoint::kernel) observed_entries:
        FrontierScratchSectionLease<'lease, FrontierObservationSlot>,
}

#[inline]
fn frontier_scratch_storage_ptr(scratch: &mut [u8], layout: FrontierScratchLayout) -> *mut u8 {
    if scratch.len() < layout.total_bytes() {
        crate::invariant();
    }
    scratch.as_mut_ptr()
}

#[inline]
unsafe fn frontier_section_ptr<T>(storage: *mut u8, section: FrontierScratchSection) -> *mut T {
    if storage.is_null() {
        crate::invariant();
    }
    if section.align != mem::align_of::<T>() {
        crate::invariant();
    }
    if section.bytes != mul(mem::size_of::<T>(), section.count) {
        crate::invariant();
    }
    /* SAFETY: the unsafe caller provides the live arena described by
    `FrontierScratchLayout`; this offset remains inside that allocation. */
    let ptr = unsafe { storage.add(section.offset()).cast::<T>() };
    if !ptr.is_aligned() {
        crate::invariant();
    }
    /* `section.align` was checked against `T`; the actual derived pointer is
    also checked above before any typed view can be published. */
    ptr
}

impl<'lease, T> FrontierScratchSectionLease<'lease, T> {
    #[inline]
    unsafe fn from_storage(storage: *mut u8, section: FrontierScratchSection, initial: T) -> Self
    where
        T: Copy,
    {
        let count = section.count();
        if count > u16::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: the workspace constructor checked one live arena and
        disjoint section layout before issuing this affine section lease. */
        let ptr: *mut T = unsafe { frontier_section_ptr(storage, section) };
        let mut index = 0usize;
        while index < count {
            /* SAFETY: `index < count` bounds one aligned section cell. Raw
            write establishes a valid `T` before any typed reference exists,
            regardless of the caller-provided scratch bytes. */
            unsafe { ptr.add(index).write(initial) };
            index += 1;
        }
        Self {
            ptr,
            count: count as u16,
            _lease: PhantomData,
        }
    }

    #[inline(always)]
    const fn count(&self) -> usize {
        self.count as usize
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [T] {
        /* SAFETY: the workspace owner issues exactly one affine lease for this
        initialized, aligned section; its bounds and alias lifetime are tied to
        `&mut self`. */
        unsafe { slice::from_raw_parts_mut(self.ptr, self.count()) }
    }
}

#[inline]
pub(crate) fn frontier_global_active_entries_view<'a>(
    section: &'a mut FrontierScratchSectionLease<'_, ActiveEntrySlot>,
) -> ActiveEntrySetBuilder<'a> {
    ActiveEntrySetBuilder::from_slice(section.as_mut_slice())
}

#[inline]
pub(crate) fn frontier_observed_entries_view<'a>(
    section: &'a mut FrontierScratchSectionLease<'_, FrontierObservationSlot>,
) -> ObservedEntrySetBuilder<'a> {
    ObservedEntrySetBuilder::from_slice(section.as_mut_slice())
}

impl<'lease> FrontierScratchWorkspace<'lease> {
    #[inline]
    pub(crate) fn from_storage(scratch: &'lease mut [u8], layout: FrontierScratchLayout) -> Self {
        let active = layout.global_active_entry_slots();
        let observed = layout.observed_entry_slots();
        if active.end() > observed.offset() || observed.end() > layout.total_bytes() {
            crate::invariant();
        }
        let storage = frontier_scratch_storage_ptr(scratch, layout);
        /* SAFETY: the ordered section checks above prove that both typed spans
        are disjoint inside the live scratch arena. The workspace is the sole
        owner of both section leases. Each raw byte span is initialized with a
        valid value before a typed slice can be borrowed. */
        unsafe {
            Self {
                global_active_entries: FrontierScratchSectionLease::from_storage(
                    storage,
                    active,
                    ActiveEntrySlot::EMPTY,
                ),
                observed_entries: FrontierScratchSectionLease::from_storage(
                    storage,
                    observed,
                    FrontierObservationSlot::EMPTY,
                ),
            }
        }
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
