use super::{
    BYTE_DOMAIN_MASK_BYTES,
    lane_matching::{LaneEndpointIndex, maximum_lane_matching},
    merge_route_frame_labels,
};
use crate::{
    eff::{EffAtom, EffStruct, EventOrigin},
    global::const_dsl::EffList,
};

mod maximum_certificate;
mod production_coloring;
mod roll_coloring;

const fn atom(from: u8, to: u8, lane: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from,
        to,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane,
    })
}

#[kani::proof]
#[kani::unwind(8)]
fn two_by_two_parallel_lane_matching_has_minimum_span() {
    // Role identities affect this 2x2 problem only through the four
    // left/right reuse edges. Shared edge markers and edge-private
    // role identities realize every one of those sixteen graphs with four symbolic
    // conflict bits. The separate index harness proves role-to-set aggregation;
    // this harness verifies the production matching kernel without symbolic
    // role-index writes obscuring the finite graph domain.
    let conflicts: u8 = kani::any::<u8>() & 0x0f;
    let left_index =
        LaneEndpointIndex::<1>::from_two_role_masks(conflicts & 0b0011, conflicts & 0b1100);
    let right_index =
        LaneEndpointIndex::<1>::from_two_role_masks(conflicts & 0b0101, conflicts & 0b1010);

    let zero_zero = conflicts & (1 << 0) == 0;
    let zero_one = conflicts & (1 << 1) == 0;
    let one_zero = conflicts & (1 << 2) == 0;
    let one_one = conflicts & (1 << 3) == 0;
    let perfect = (zero_zero && one_one) || (zero_one && one_zero);
    let any_reuse = zero_zero || zero_one || one_zero || one_one;

    let matching = maximum_lane_matching(&left_index, &right_index, 2, 2);
    let right_zero_lane = matching.left_for_right(0);
    let right_one_lane = matching.left_for_right(1);
    let reused = right_zero_lane.is_some() as u16 + right_one_lane.is_some() as u16;
    let lane_span = 4 - reused;

    assert!(
        lane_span
            == if perfect {
                2
            } else if any_reuse {
                3
            } else {
                4
            }
    );
    assert!(right_zero_lane.is_none() || right_zero_lane != right_one_lane);
    assert!(
        right_zero_lane.is_none()
            || if right_zero_lane == Some(0) {
                zero_zero
            } else {
                one_zero
            }
    );
    assert!(
        right_one_lane.is_none()
            || if right_one_lane == Some(0) {
                zero_one
            } else {
                one_one
            }
    );
}

#[kani::proof]
fn lane_endpoint_index_aggregates_exact_symbolic_membership() {
    let first_from: u8 = kani::any();
    let first_to: u8 = kani::any();
    let first_lane: u8 = kani::any();
    let second_from: u8 = kani::any();
    let second_to: u8 = kani::any();
    let second_lane: u8 = kani::any();
    let query_role: u8 = kani::any();
    let query_lane: u8 = kani::any();
    let source = EffList::<2>::new()
        .push(atom(first_from, first_to, first_lane))
        .push(atom(second_from, second_to, second_lane));

    let index = LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(&source, 0, 2);
    let actual = index.contains_role(query_lane, query_role);
    let expected = (query_lane == first_lane
        && (query_role == first_from || query_role == first_to))
        || (query_lane == second_lane && (query_role == second_from || query_role == second_to));

    assert!(actual == expected);
}

#[kani::proof]
fn two_arm_route_frame_coloring_is_exact() {
    let left_from: u8 = kani::any();
    let left_to: u8 = kani::any();
    let left_lane: u8 = kani::any();
    let right_from: u8 = kani::any();
    let right_to: u8 = kani::any();
    let right_lane: u8 = kani::any();
    let mut source = EffList::<2>::new()
        .push(atom(left_from, left_to, left_lane))
        .push(atom(right_from, right_to, right_lane));

    merge_route_frame_labels(&mut source, 0, 1, 2);
    let same_inbound_key =
        left_from == right_from && left_to == right_to && left_lane == right_lane;

    assert!(source.frame_label_at(0) == 0);
    assert!(source.frame_label_at(1) == if same_inbound_key { 1 } else { 0 });
}
