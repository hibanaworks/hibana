use super::FrontierVisitSet;
use crate::global::role_program::{
    RuntimeRoleFootprint, compact_local_step_count, frontier_visit_byte_count,
    local_cursor_position_count,
};

fn visit_set<const N: usize>(storage: &mut [u8; N], position_count: usize) -> FrontierVisitSet {
    assert_eq!(frontier_visit_byte_count(position_count), N);
    /* SAFETY: the exact byte count for `position_count` equals the live,
    exclusively borrowed storage extent. */
    unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), position_count) }
}

#[kani::proof]
fn route_frontier_visit_capacity_is_exact_cursor_position_domain() {
    let local_step_count = kani::any::<u16>() as usize;
    let has_route_scope = kani::any::<bool>();
    let footprint = RuntimeRoleFootprint {
        max_route_commit_count: 0,
        route_arm_state_capacity: 0,
        local_step_count,
        route_scope_count: usize::from(has_route_scope),
        active_lane_count: kani::any::<u8>() as usize,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    };

    if has_route_scope {
        assert_eq!(
            footprint.frontier_visit_position_count(),
            local_step_count + 1
        );
        assert_eq!(
            footprint.frontier_visit_byte_count(),
            (local_step_count + 1).div_ceil(u8::BITS as usize)
        );
    } else {
        assert_eq!(footprint.frontier_visit_position_count(), 0);
        assert_eq!(footprint.frontier_visit_byte_count(), 0);
    }
}

#[kani::proof]
fn rolled_reentry_can_visit_more_cursor_positions_than_active_lanes() {
    let mut storage = [0u8; 6];
    let mut visited = visit_set(&mut storage, 47);

    visited.record(46);
    visited.record(20);
    visited.record(2);

    assert_eq!(visited.len(), 3);
    assert!(visited.contains(46));
    assert!(visited.contains(20));
    assert!(visited.contains(2));
}

#[kani::proof]
fn visited_cursor_position_identity_is_exact_and_never_silent() {
    let first_position = kani::any::<u8>() % 16;
    let second_position = kani::any::<u8>() % 16;
    let second_position = if first_position == second_position {
        (first_position + 1) % 16
    } else {
        second_position
    };
    let first = first_position as usize;
    let second = second_position as usize;
    let mut storage = [0u8; 2];
    let mut visited = visit_set(&mut storage, 16);

    visited.record(first);
    visited.record(second);

    assert_eq!(visited.len(), 2);
    assert!(visited.contains(first));
    assert!(visited.contains(second));
}

#[kani::proof]
fn repeated_alignment_source_remains_detectable_without_capacity_growth() {
    let source_position = kani::any::<u8>() as usize;
    let mut storage = [0u8; 32];
    let mut visited = visit_set(&mut storage, 256);

    visited.record(source_position);
    assert!(visited.contains(source_position));
    visited.record(source_position);

    assert_eq!(visited.len(), 1);
    assert!(visited.contains(source_position));
}

#[kani::proof]
fn terminal_cursor_position_is_not_an_absent_event_identity() {
    let local_step_count = u16::MAX as usize;
    let position_count = local_cursor_position_count(local_step_count);
    let byte_count = frontier_visit_byte_count(position_count);

    assert_eq!(position_count, u16::MAX as usize + 1);
    assert_eq!(byte_count, 8192);
    assert!(local_step_count < position_count);
    assert!(local_step_count / (u8::BITS as usize) < byte_count);
}

#[kani::proof]
#[kani::should_panic]
fn local_step_count_rejects_values_beyond_the_packed_descriptor_domain() {
    let _ = compact_local_step_count(u16::MAX as usize + 1);
}
