use super::{
    AssocTable, RouteTable,
    arena::packed_sidecar_range,
    endpoint_lease::{endpoint_lease_storage_bytes, next_endpoint_lease_generation},
    resident_lease::sidecar_ranges_overlap,
};
use crate::rendezvous::core::{EndpointLeaseRecord, endpoint_leases::endpoint_offset_in_gap};
use crate::session::cluster::core::ResolverBucket;
use crate::session::types::SessionId;

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
        let unaligned = base
            .checked_add(gap_end - bytes)
            .expect("successful placement has a representable upper bound");
        let absolute = base
            .checked_add(offset)
            .expect("successful placement has a representable address");
        assert!(offset >= gap_start);
        assert!(gap_end >= bytes);
        assert!(offset.checked_add(bytes).is_some_and(|end| end <= gap_end));
        assert!(absolute <= unaligned);
        assert!(unaligned - absolute < align);
        assert!(
            base.checked_add(offset)
                .is_some_and(|absolute| absolute & (align - 1) == 0)
        );
    }
}

#[kani::proof]
fn endpoint_lease_storage_layout_is_bounded_and_exact() {
    let capacity = usize::from(kani::any::<u16>());
    let record_bytes = core::mem::size_of::<EndpointLeaseRecord>();
    let storage_bytes = endpoint_lease_storage_bytes(capacity)
        .expect("the full u16 endpoint capacity domain must fit resident storage arithmetic");

    kani::cover!(capacity == 0);
    kani::cover!(capacity == usize::from(u16::MAX));
    assert!(record_bytes != 0);
    assert!(core::mem::align_of::<EndpointLeaseRecord>().is_power_of_two());
    assert!(record_bytes.checked_mul(capacity) == Some(storage_bytes));
    assert!(storage_bytes <= u32::MAX as usize);
}

#[kani::proof]
fn association_storage_layout_is_bounded_and_exact() {
    let capacity = usize::from(kani::any::<u16>());
    let storage_bytes = AssocTable::storage_bytes(capacity);
    let row_bytes = core::mem::size_of::<SessionId>() + 2 * core::mem::size_of::<u8>();

    kani::cover!(capacity == 0);
    kani::cover!(capacity == usize::from(u16::MAX));
    assert!(AssocTable::storage_align().is_power_of_two());
    assert!(row_bytes.checked_mul(capacity) == Some(storage_bytes));
    assert!(storage_bytes <= u32::MAX as usize);
}

#[kani::proof]
fn route_storage_layout_is_bounded_and_exact() {
    let route_slots = usize::from(kani::any::<u16>());
    let lane_slots = usize::from(kani::any::<u16>());
    let empty_bytes = RouteTable::storage_bytes(0, 0);
    let one_frame_bytes = RouteTable::storage_bytes(1, 0);
    let frame_stride = one_frame_bytes - empty_bytes;
    let frames_only = RouteTable::storage_bytes(route_slots, 0);
    let storage_bytes = RouteTable::storage_bytes(route_slots, lane_slots);
    let lane_bytes = core::mem::size_of::<u16>()
        .checked_mul(lane_slots)
        .expect("the full u16 lane capacity domain must fit route storage arithmetic");

    kani::cover!(route_slots == 0 && lane_slots == 0);
    kani::cover!(route_slots == usize::from(u16::MAX) && lane_slots == usize::from(u16::MAX));
    assert!(RouteTable::storage_align().is_power_of_two());
    assert!(frame_stride != 0);
    assert!(
        frame_stride
            .checked_mul(route_slots)
            .and_then(|frames| frames.checked_add(empty_bytes))
            == Some(frames_only)
    );
    assert!(storage_bytes.checked_sub(frames_only) == Some(lane_bytes));
    assert!(storage_bytes <= u32::MAX as usize);
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
fn packed_sidecar_pair_is_aligned_and_disjoint() {
    let base: usize = kani::any();
    let frontier: usize = kani::any();
    let first_bytes: usize = kani::any();
    let second_bytes: usize = kani::any();
    let first_align: usize = kani::any();
    let second_align: usize = kani::any();
    kani::assume(first_align.is_power_of_two());
    kani::assume(second_align.is_power_of_two());

    let pair = packed_sidecar_range(base, frontier, first_bytes, first_align).and_then(
        |(first_start, first_end)| {
            packed_sidecar_range(base, first_end, second_bytes, second_align).map(
                |(second_start, second_end)| (first_start, first_end, second_start, second_end),
            )
        },
    );
    kani::cover!(pair.is_some());
    kani::cover!(pair.is_none());
    if let Some((first_start, first_end, second_start, second_end)) = pair {
        assert!(first_start >= frontier);
        assert!(first_end <= second_start);
        assert!(first_end.checked_sub(first_start) == Some(first_bytes));
        assert!(second_end.checked_sub(second_start) == Some(second_bytes));
        assert!(
            base.checked_add(first_start)
                .is_some_and(|absolute| absolute & (first_align - 1) == 0)
        );
        assert!(
            base.checked_add(second_start)
                .is_some_and(|absolute| absolute & (second_align - 1) == 0)
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

#[kani::proof]
fn resolver_storage_layout_is_bounded_and_exact() {
    let capacity: u16 = kani::any();
    let capacity = usize::from(capacity);
    let entry_bytes = ResolverBucket::storage_bytes(1);
    let storage_bytes = ResolverBucket::storage_bytes(capacity);
    let align = ResolverBucket::storage_align();

    kani::cover!(capacity == 0);
    kani::cover!(capacity == usize::from(u16::MAX));
    assert!(entry_bytes != 0);
    assert!(align.is_power_of_two());
    assert!(entry_bytes.checked_mul(capacity) == Some(storage_bytes));
    assert!(storage_bytes <= u32::MAX as usize);
}
