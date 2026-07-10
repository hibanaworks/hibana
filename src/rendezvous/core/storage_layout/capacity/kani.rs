use super::{
    arena::packed_sidecar_range, endpoint_lease::next_endpoint_lease_generation,
    resident_lease::sidecar_ranges_overlap,
};
use crate::rendezvous::core::endpoint_leases::endpoint_offset_in_gap;

#[kani::proof]
fn endpoint_generation_advances_or_exhausts() {
    let current: u32 = kani::any();
    let next = next_endpoint_lease_generation(current);
    kani::cover!(next.is_some());
    kani::cover!(next.is_none());
    match next {
        Some(next) => {
            assert!(current < u32::MAX);
            assert!(next > current);
            assert!(next != 0);
        }
        None => assert!(current == u32::MAX),
    }
}

#[kani::proof]
fn endpoint_gap_placement_is_aligned_and_bounded() {
    let base: usize = kani::any();
    let gap_start: usize = kani::any();
    let gap_end: usize = kani::any();
    let bytes: usize = kani::any();
    let align: usize = kani::any();
    kani::assume(align.is_power_of_two());

    let placement = endpoint_offset_in_gap(base, gap_start, gap_end, bytes, align);
    kani::cover!(placement.is_some());
    kani::cover!(placement.is_none());
    if let Some(offset) = placement {
        assert!(offset >= gap_start);
        assert!(gap_end >= bytes);
        assert!(offset.checked_add(bytes).is_some_and(|end| end <= gap_end));
        assert!(
            base.checked_add(offset)
                .is_some_and(|absolute| absolute & (align - 1) == 0)
        );
    }
}

#[kani::proof]
fn packed_sidecar_range_is_aligned_and_monotonic() {
    let base: usize = kani::any();
    let frontier: usize = kani::any();
    let bytes: usize = kani::any();
    let align: usize = kani::any();
    kani::assume(align.is_power_of_two());

    let packed = packed_sidecar_range(base, frontier, bytes, align);
    kani::cover!(packed.is_some());
    kani::cover!(packed.is_none());
    if let Some((start, end)) = packed {
        assert!(start >= frontier);
        assert!(end.checked_sub(start) == Some(bytes));
        assert!(
            base.checked_add(start)
                .is_some_and(|absolute| absolute & (align - 1) == 0)
        );
    }
}

#[kani::proof]
fn sidecar_overlap_is_symmetric_and_exact() {
    let left_start: usize = kani::any();
    let left_end: usize = kani::any();
    let right_start: usize = kani::any();
    let right_end: usize = kani::any();
    kani::assume(left_start < left_end);
    kani::assume(right_start < right_end);

    let overlap = sidecar_ranges_overlap(left_start, left_end, right_start, right_end);
    kani::cover!(overlap);
    kani::cover!(!overlap);
    assert!(overlap == sidecar_ranges_overlap(right_start, right_end, left_start, left_end));
    assert!(overlap == !(left_end <= right_start || right_end <= left_start));
}
