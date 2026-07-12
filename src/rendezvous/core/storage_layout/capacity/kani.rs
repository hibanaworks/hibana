use super::{
    AssocTable, RouteTable,
    arena::packed_sidecar_range,
    endpoint_lease::{endpoint_lease_storage_bytes, next_endpoint_lease_generation},
    resident_lease::sidecar_ranges_overlap,
};
use crate::rendezvous::core::{
    EndpointLeaseRecord, EndpointLeaseState, RendezvousAccessState,
    endpoint_leases::endpoint_offset_in_gap,
};
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
fn endpoint_membership_seal_is_published_and_idempotent() {
    let raw: u8 = kani::any();
    kani::assume(raw <= EndpointLeaseState::MembershipSealed as u8);
    let state = match raw {
        0 => EndpointLeaseState::Vacant,
        1 => EndpointLeaseState::Reserved,
        2 => EndpointLeaseState::Published,
        3 => EndpointLeaseState::MembershipSealed,
        _ => crate::invariant(),
    };
    match state.seal_membership() {
        Some(sealed) => {
            assert!(state.is_published());
            assert!(sealed == EndpointLeaseState::MembershipSealed);
            assert!(sealed.is_published());
            assert!(sealed.is_membership_sealed());
            assert!(sealed.seal_membership() == Some(sealed));
        }
        None => assert!(!state.is_published()),
    }
}

#[kani::proof]
fn endpoint_operation_and_nested_scratch_transitions_are_exact() {
    let raw: u8 = kani::any();
    kani::assume(raw <= RendezvousAccessState::EndpointScratchLease as u8);
    let state = match raw {
        0 => RendezvousAccessState::Available,
        1 => RendezvousAccessState::RegistryLease,
        2 => RendezvousAccessState::ScratchLease,
        3 => RendezvousAccessState::EndpointOperation,
        4 => RendezvousAccessState::EndpointScratchLease,
        _ => crate::invariant(),
    };

    let operation = state.begin_endpoint_operation();
    assert!(operation.is_some() == (state == RendezvousAccessState::Available));
    if let Some(active) = operation {
        assert!(active == RendezvousAccessState::EndpointOperation);
        assert!(active.finish_endpoint_operation() == Some(RendezvousAccessState::Available));
    }

    let scratch = state.begin_scratch();
    assert!(
        scratch.is_some()
            == matches!(
                state,
                RendezvousAccessState::Available | RendezvousAccessState::EndpointOperation
            )
    );
    if let Some((leased, restore)) = scratch {
        assert!(leased.finish_scratch() == Some(restore));
        if state == RendezvousAccessState::EndpointOperation {
            assert!(leased == RendezvousAccessState::EndpointScratchLease);
            assert!(restore == RendezvousAccessState::EndpointOperation);
        }
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
    let root_bytes = RouteTable::storage_bytes(0);
    let frame_stride = RouteTable::storage_bytes(1) - root_bytes;
    let expected_bytes = frame_stride
        .checked_mul(route_slots)
        .and_then(|frames| frames.checked_add(root_bytes))
        .expect("the full u16 route capacity domain must fit route storage arithmetic");
    let storage_bytes = RouteTable::storage_bytes(route_slots);

    kani::cover!(route_slots == 0);
    kani::cover!(route_slots == usize::from(u16::MAX));
    assert!(RouteTable::storage_align().is_power_of_two());
    assert!(frame_stride != 0);
    assert!(storage_bytes == expected_bytes);
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

fn pack_four_sidecars(
    base: usize,
    mut frontier: usize,
    bytes: [usize; 4],
    aligns: [usize; 4],
    gaps: [usize; 3],
) -> Option<[(usize, usize); 4]> {
    let mut ranges = [(0, 0); 4];
    let mut index = 0usize;
    while index < ranges.len() {
        let range = packed_sidecar_range(base, frontier, bytes[index], aligns[index])?;
        ranges[index] = range;
        frontier = range.1;
        if index < gaps.len() {
            frontier = frontier.checked_add(gaps[index])?;
        }
        index += 1;
    }
    Some(ranges)
}

#[kani::proof]
fn four_resident_sidecars_compact_before_all_source_ranges() {
    let base: usize = kani::any();
    let source_frontier: usize = kani::any();
    let bytes: [usize; 4] = kani::any();
    let gaps: [usize; 3] = kani::any();
    let shifts: [u8; 4] = kani::any();
    let shift_mask = (usize::BITS - 1) as u8;
    let aligns = [
        1usize << usize::from(shifts[0] & shift_mask),
        1usize << usize::from(shifts[1] & shift_mask),
        1usize << usize::from(shifts[2] & shift_mask),
        1usize << usize::from(shifts[3] & shift_mask),
    ];

    let sources = pack_four_sidecars(base, source_frontier, bytes, aligns, gaps);
    let destinations = pack_four_sidecars(base, 0, bytes, aligns, [0; 3]);

    kani::cover!(sources.is_some() && destinations.is_some());
    kani::cover!(sources.is_none());
    kani::cover!(destinations.is_none());
    if let (Some(sources), Some(destinations)) = (sources, destinations) {
        let mut index = 0usize;
        while index < sources.len() {
            assert!(destinations[index].0 <= sources[index].0);
            assert!(destinations[index].1 <= sources[index].1);
            let mut later = index + 1;
            while later < sources.len() {
                assert!(destinations[index].1 <= sources[later].0);
                later += 1;
            }
            index += 1;
        }
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
