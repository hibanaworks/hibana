use super::*;
use crate::global::role_program::PackedLaneRange;
use core::mem::MaybeUninit;

#[test]
fn existing_route_reselection_preserves_exact_reference_count() {
    let mut sole = RouteScopeSelectedArmSlot { arm: 0, refs: 1 };
    sole.commit_existing_lane_reselection(0, 1);
    assert_eq!(sole.arm, 1);
    assert_eq!(sole.refs, 1);

    let mut shared = RouteScopeSelectedArmSlot { arm: 1, refs: 7 };
    shared.commit_existing_lane_reselection(0, 1);
    assert_eq!(shared.arm, 1);
    assert_eq!(shared.refs, 7);
}

#[test]
fn route_commit_row_set_builder_accepts_more_than_64_route_scopes() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), 71);
    }
    let mut builder = unsafe { builder.assume_init() };
    let list = builder.begin();

    assert_eq!(list.len(), 0);
}

#[test]
fn prepared_route_commit_rows_use_builder_capacity_not_fixed_inline_cap() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), 9);
    }
    let mut builder = unsafe { builder.assume_init() };
    let rows =
        SelectedRouteCommitRowsRef::from_resident_range_for_lane(PackedLaneRange::new(7, 9), 3);
    let value = builder
        .seal(rows)
        .expect("valid nine-row route chain must seal without an inline cap");

    assert_eq!(value.len(), 9);
    assert_eq!(value.selected_lane(), Some(3));
    assert_eq!(builder.begin().len(), 0);
}

#[test]
fn route_commit_builder_preserves_exact_zero_route_capacity() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), 0);
    }
    let mut builder = unsafe { builder.assume_init() };

    assert_eq!(
        builder
            .seal(SelectedRouteCommitRowsRef::EMPTY)
            .expect("empty route set")
            .len(),
        0
    );
    assert!(
        builder
            .seal(SelectedRouteCommitRowsRef::from_resident_range_for_lane(
                PackedLaneRange::new(0, 1),
                0,
            ))
            .is_err()
    );
}

#[test]
fn prepared_route_commit_rows_accept_257_entries() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), 257);
    }
    let mut builder = unsafe { builder.assume_init() };
    let rows =
        SelectedRouteCommitRowsRef::from_resident_range_for_lane(PackedLaneRange::new(7, 257), 3);
    let value = builder
        .seal(rows)
        .expect("descriptor-domain route chain must not truncate at u8::MAX");

    assert_eq!(value.len(), 257);
    assert_eq!(value.selected_lane(), Some(3));
}

#[test]
fn selected_route_commit_rows_reject_lane_mismatch_without_erasing_rows() {
    let empty = SelectedRouteCommitRows {
        routes: SelectedRouteCommitRowsRef::EMPTY,
        max_len: 0,
    }
    .finish_for_lane(4)
    .expect("canonical empty route rows");
    assert!(empty.is_empty());

    let rows =
        SelectedRouteCommitRowsRef::from_resident_range_for_lane(PackedLaneRange::new(7, 9), 3);
    let exact = SelectedRouteCommitRows::from_seed(rows)
        .expect("nonempty route rows")
        .finish_for_lane(3)
        .expect("matching route lane");
    assert_eq!(exact.len(), 9);
    assert_eq!(exact.selected_lane(), Some(3));

    let rejected = SelectedRouteCommitRows::from_seed(rows)
        .expect("nonempty route rows")
        .finish_for_lane(4);
    assert!(rejected.is_err());
}

#[test]
fn route_arm_history_accepts_257_descriptor_relations() {
    const CAPACITY: usize = 257;
    let mut states = std::vec![RouteArmState::EMPTY; CAPACITY];
    let mut lengths = [0u16; 1];
    let mut dense = [DenseLaneOrdinal::new(0).expect("dense lane zero")];
    let mut view = MaybeUninit::<RouteArmHistoryView>::uninit();
    unsafe {
        RouteArmHistoryView::init(
            view.as_mut_ptr(),
            states.as_mut_ptr(),
            lengths.as_mut_ptr(),
            dense.as_mut_ptr(),
            dense.len(),
            lengths.len(),
            CAPACITY,
        );
    }
    let mut view = unsafe { view.assume_init() };
    for ordinal in 0..CAPACITY {
        assert!(view.push(0, ScopeId::route(ordinal as u16), (ordinal & 1) as u8));
    }

    assert_eq!(view.capacity(), CAPACITY);
    assert_eq!(view.len(), CAPACITY);
    assert_eq!(view.lane_len(0), CAPACITY);
    assert_eq!(view.get(0, 256).scope, ScopeId::route(256));
    assert!(!view.push(1, ScopeId::route(257), 0));
}

#[test]
fn route_arm_history_is_sparse_across_lanes() {
    let mut states = [RouteArmState::EMPTY; 4];
    let mut lengths = [0u16; 2];
    let mut dense = [DENSE_LANE_ABSENT; 10];
    dense[2] = DenseLaneOrdinal::new(0).expect("first dense lane");
    dense[9] = DenseLaneOrdinal::new(1).expect("second dense lane");
    let mut view = MaybeUninit::<RouteArmHistoryView>::uninit();
    unsafe {
        RouteArmHistoryView::init(
            view.as_mut_ptr(),
            states.as_mut_ptr(),
            lengths.as_mut_ptr(),
            dense.as_mut_ptr(),
            dense.len(),
            lengths.len(),
            states.len(),
        );
    }
    let mut view = unsafe { view.assume_init() };

    assert!(view.push(9, ScopeId::route(1), 0));
    assert!(view.push(2, ScopeId::route(2), 1));
    assert!(view.push(9, ScopeId::route(3), 1));
    assert_eq!(view.lane_len(9), 2);
    assert_eq!(view.lane_len(2), 1);
    assert_eq!(view.get(9, 1).scope, ScopeId::route(3));

    assert!(view.remove(9, 0));
    assert_eq!(view.len(), 2);
    assert_eq!(view.lane_len(9), 1);
    assert_eq!(view.get(9, 0).scope, ScopeId::route(3));
    assert_eq!(view.get(2, 0).scope, ScopeId::route(2));
}

#[test]
fn decode_commit_row_set_builder_accepts_more_than_64_route_scopes() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), 71);
    }
    let mut builder = unsafe { builder.assume_init() };
    let list = builder.begin();

    assert_eq!(list.len(), 0);
}
