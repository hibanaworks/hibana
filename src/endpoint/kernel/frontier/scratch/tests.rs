use super::{
    FrontierScratchLayout, FrontierScratchWorkspace, frontier_global_active_entries_view,
    frontier_observed_entries_view,
};
use crate::global::role_program::LANE_DOMAIN_SIZE;

#[repr(C, align(16))]
struct AlignedStorage([u8; 256]);

#[test]
fn global_frontier_scratch_sections_track_max_frontier_entries() {
    let layout = FrontierScratchLayout::new(5);
    assert_eq!(layout.global_active_entry_slots().count(), 5);
    assert_eq!(layout.observed_entry_slots().count(), 5);
}

#[test]
fn lane_domain_frontier_workspace_fits_compact_resident_budget() {
    let layout = FrontierScratchLayout::new(LANE_DOMAIN_SIZE);
    let guard_bytes = layout.total_bytes() + layout.total_align() - 1;

    assert!(guard_bytes <= u16::MAX as usize);
}

#[test]
#[should_panic]
fn frontier_scratch_rejects_capacity_beyond_lane_domain() {
    let _ = FrontierScratchLayout::new(LANE_DOMAIN_SIZE + 1);
}

#[test]
fn zero_capacity_frontier_scratch_yields_empty_typed_views() {
    let layout = FrontierScratchLayout::new(0);
    let mut storage = AlignedStorage([0; 256]);
    let mut scratch = FrontierScratchWorkspace::from_storage(&mut storage.0[..0], layout);

    assert_eq!(
        frontier_global_active_entries_view(&mut scratch.global_active_entries).len(),
        0
    );
    assert_eq!(
        frontier_observed_entries_view(&mut scratch.observed_entries).len(),
        0
    );
}

#[test]
fn frontier_scratch_workspace_issues_two_disjoint_typed_sections() {
    let layout = FrontierScratchLayout::new(2);
    let mut storage = AlignedStorage([0; 256]);
    assert!(layout.total_bytes() <= storage.0.len());
    let scratch_bytes = &mut storage.0[..layout.total_bytes()];
    let mut scratch = FrontierScratchWorkspace::from_storage(scratch_bytes, layout);

    let mut active = frontier_global_active_entries_view(&mut scratch.global_active_entries);
    let mut observed = frontier_observed_entries_view(&mut scratch.observed_entries);
    active.clear();
    observed.clear();

    assert_eq!(active.len(), 0);
    assert_eq!(observed.len(), 0);
}

#[test]
fn arbitrary_scratch_bytes_are_initialized_before_typed_views_exist() {
    let layout = FrontierScratchLayout::new(2);
    let mut storage = AlignedStorage([u8::MAX; 256]);
    let scratch_bytes = &mut storage.0[..layout.total_bytes()];
    let mut scratch = FrontierScratchWorkspace::from_storage(scratch_bytes, layout);

    let active = frontier_global_active_entries_view(&mut scratch.global_active_entries);
    let observed = frontier_observed_entries_view(&mut scratch.observed_entries);

    assert_eq!(active.len(), 0);
    assert_eq!(observed.len(), 0);
}
