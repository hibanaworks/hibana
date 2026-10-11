use super::{
    ActiveEntrySlot, FrontierScratchLayout, FrontierScratchWorkspace,
    frontier_global_active_entries_view, frontier_observed_entries_view,
};
use crate::global::role_program::LANE_DOMAIN_SIZE;

#[kani::proof]
fn frontier_scratch_capacity_is_derived_once_from_its_layout() {
    let candidate: u16 = kani::any();
    let capacity = if candidate as usize <= LANE_DOMAIN_SIZE {
        candidate
    } else {
        LANE_DOMAIN_SIZE as u16
    };
    let layout = FrontierScratchLayout::new(capacity as usize);

    assert_eq!(
        layout.global_active_entry_slots().count(),
        capacity as usize
    );
    assert_eq!(layout.observed_entry_slots().count(), capacity as usize);
    assert!(layout.global_active_entry_slots().end() <= layout.observed_entry_slots().offset());
    assert!(layout.observed_entry_slots().end() <= layout.total_bytes());
}

#[kani::proof]
fn lane_domain_frontier_workspace_fits_compact_resident_budget() {
    let layout = FrontierScratchLayout::new(LANE_DOMAIN_SIZE);
    let guard_bytes = layout.total_bytes() + layout.total_align() - 1;

    assert!(guard_bytes <= u16::MAX as usize);
}

#[kani::proof]
#[kani::should_panic]
fn frontier_scratch_rejects_capacity_beyond_lane_domain() {
    let _ = FrontierScratchLayout::new(LANE_DOMAIN_SIZE + 1);
}

#[kani::proof]
#[kani::should_panic]
fn zero_capacity_frontier_scratch_rejects_misaligned_storage_before_slice_publication() {
    let mut storage = [core::mem::MaybeUninit::<ActiveEntrySlot>::uninit(); 2];
    let layout = FrontierScratchLayout::new(0);
    /* SAFETY: the shifted pointer remains inside `storage` but deliberately
    violates `ActiveEntrySlot` alignment. */
    let misaligned = unsafe { storage.as_mut_ptr().cast::<u8>().add(1) };
    let scratch = unsafe { core::slice::from_raw_parts_mut(misaligned, 0) };
    let _ = FrontierScratchWorkspace::from_storage(scratch, layout);
}

#[kani::proof]
fn zero_capacity_frontier_scratch_yields_empty_typed_views() {
    let layout = FrontierScratchLayout::new(0);
    let storage = core::ptr::NonNull::<ActiveEntrySlot>::dangling()
        .as_ptr()
        .cast::<u8>();
    /* SAFETY: the scratch owner supplies an aligned non-null pointer, the slice
    has zero length and therefore reads no initialized bytes, and this harness
    creates no second reference to the empty storage range. */
    let scratch_bytes = unsafe { core::slice::from_raw_parts_mut(storage, 0) };
    let mut scratch = FrontierScratchWorkspace::from_storage(scratch_bytes, layout);

    assert!(frontier_global_active_entries_view(&mut scratch.global_active_entries).len() == 0);
    assert!(frontier_observed_entries_view(&mut scratch.observed_entries).len() == 0);
}

#[kani::proof]
fn arbitrary_scratch_bytes_are_canonicalized_before_typed_publication() {
    const LAYOUT: FrontierScratchLayout = FrontierScratchLayout::new(1);
    let mut storage: [u8; LAYOUT.total_bytes()] = kani::any();
    let mut scratch = FrontierScratchWorkspace::from_storage(&mut storage, LAYOUT);

    let active = frontier_global_active_entries_view(&mut scratch.global_active_entries);
    let observed = frontier_observed_entries_view(&mut scratch.observed_entries);

    assert!(active.len() == 0);
    assert!(observed.len() == 0);
}
