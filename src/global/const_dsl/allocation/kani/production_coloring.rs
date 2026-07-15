use super::super::{
    BYTE_DOMAIN_MASK_BYTES, lane_matching::LaneEndpointIndex, merge_parallel_lanes,
};
use super::atom;
use crate::global::const_dsl::EffList;

#[kani::proof]
#[kani::unwind(40)]
fn parallel_lane_coloring_reuses_disjoint_class() {
    let mut source = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(2, 3, 0));

    let lane_span = merge_parallel_lanes(&mut source, 0, 1, 2, 1, 1);

    assert!(source.node_at(1).atom_data().lane == 0);
    assert!(lane_span == 1);
}

#[kani::proof]
#[kani::unwind(40)]
fn parallel_lane_coloring_separates_conflicting_class() {
    let mut source = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(1, 2, 0));

    let lane_span = merge_parallel_lanes(&mut source, 0, 1, 2, 1, 1);

    assert!(source.node_at(1).atom_data().lane == 1);
    assert!(lane_span == 2);
}

#[kani::proof]
#[kani::unwind(40)]
fn lane_reuse_conflict_matches_endpoint_equality() {
    let left_from: u8 = kani::any();
    let left_to: u8 = kani::any();
    let right_from: u8 = kani::any();
    let right_to: u8 = kani::any();
    let left = EffList::<1>::new().push(atom(left_from, left_to, 0));
    let right = EffList::<1>::new().push(atom(right_from, right_to, 0));
    let left_index = LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(&left, 0, left.len());
    let right_index =
        LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(&right, 0, right.len());

    let actual = !left_index.lane_is_disjoint_from(0, &right_index, 0);
    let expected = left_from == right_from
        || left_from == right_to
        || left_to == right_from
        || left_to == right_to;

    assert!(actual == expected);
}
