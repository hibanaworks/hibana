use super::{
    RouteArmHistoryView, RouteScopeSelectedArmSlot, SelectedRouteCommitRows,
    SelectedRouteCommitRowsRef,
};
use crate::endpoint::kernel::evidence::RouteArmState;
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{DenseLaneOrdinal, PackedLaneRange};

#[kani::proof]
fn existing_route_reselection_preserves_exact_reference_count() {
    let selected_arm = kani::any::<bool>() as u8;
    let current_arm = 1 - selected_arm;
    let shared = kani::any::<bool>();
    let candidate_refs = kani::any::<u16>();
    let symbolic_refs = if candidate_refs == 0 {
        1
    } else {
        candidate_refs
    };
    let mut slot = if shared {
        RouteScopeSelectedArmSlot {
            arm: selected_arm,
            refs: symbolic_refs,
        }
    } else {
        RouteScopeSelectedArmSlot {
            arm: current_arm,
            refs: 1,
        }
    };
    let expected_refs = slot.refs;

    slot.commit_existing_lane_reselection(current_arm, selected_arm);

    assert_eq!(slot.arm, selected_arm);
    assert_eq!(slot.refs, expected_refs);
}

#[kani::proof]
fn selected_arm_release_is_exact_and_never_underflows() {
    let arm = kani::any::<bool>() as u8;
    let candidate_refs = kani::any::<u16>();
    let refs = if candidate_refs == 0 {
        1
    } else {
        candidate_refs
    };
    let next = RouteScopeSelectedArmSlot { arm, refs }.prepared_release();

    if refs == 1 {
        assert_eq!(next.arm, u8::MAX);
        assert_eq!(next.refs, 0);
    } else {
        assert_eq!(next.arm, arm);
        assert_eq!(next.refs, refs - 1);
    }
}

#[kani::proof]
fn selected_route_commit_rows_preserve_full_descriptor_range() {
    let candidate = (kani::any::<u16>(), kani::any::<u16>());
    let candidate_end = u32::from(candidate.0) + u32::from(candidate.1);
    let (start, len) = if candidate.1 != 0 && candidate_end <= u32::from(u16::MAX) {
        candidate
    } else {
        (0, 1)
    };
    let lane = kani::any::<u8>();

    let range = PackedLaneRange::new(start as usize, len as usize);
    let rows = SelectedRouteCommitRowsRef::from_resident_range_for_lane(range, lane);

    assert!(!rows.is_empty());
    assert_eq!(rows.range().raw(), range.raw());
    assert_eq!(rows.len(), len as usize);
    assert_eq!(rows.selected_lane(), Some(lane));
    kani::cover!(start == u16::MAX - 1 && len == 1);
}

#[kani::proof]
fn selected_route_commit_rows_accept_257_entries() {
    let lane = kani::any::<u8>();
    let range = PackedLaneRange::new(0, 257);
    let rows = SelectedRouteCommitRowsRef::from_resident_range_for_lane(range, lane);

    assert_eq!(rows.len(), 257);
    assert_eq!(rows.selected_lane(), Some(lane));
}

#[kani::proof]
fn selected_route_commit_rows_finish_is_lane_exact_and_fail_closed() {
    let lane = kani::any::<u8>();
    let mismatched_lane = lane.wrapping_add(1);

    let empty = SelectedRouteCommitRows {
        routes: SelectedRouteCommitRowsRef::EMPTY,
        max_len: 0,
    }
    .finish_for_lane(lane)
    .expect("canonical empty route rows");
    assert!(empty.is_empty());

    let rows = SelectedRouteCommitRowsRef::from_resident_range_for_lane(
        PackedLaneRange::new(7, 257),
        lane,
    );

    let exact = SelectedRouteCommitRows::from_seed(rows)
        .expect("nonempty route rows")
        .finish_for_lane(lane)
        .expect("matching route lane");
    assert_eq!(exact.len(), 257);
    assert_eq!(exact.selected_lane(), Some(lane));

    let rejected = SelectedRouteCommitRows::from_seed(rows)
        .expect("nonempty route rows")
        .finish_for_lane(mismatched_lane);
    assert!(rejected.is_err());
}

#[kani::proof]
fn sparse_route_history_preserves_lane_partition() {
    let left_selection = kani::any::<bool>() as u8;
    let right_selection = kani::any::<bool>() as u8;
    let mut storage = [RouteArmState::EMPTY; 3];
    let mut lengths = [0u16; 2];
    let mut dense = [
        DenseLaneOrdinal::new(0).expect("dense lane zero"),
        DenseLaneOrdinal::new(1).expect("dense lane one"),
    ];
    let mut history = core::mem::MaybeUninit::<RouteArmHistoryView>::uninit();
    /* SAFETY: each pointer names a live, exclusive backing array and the
    supplied counts exactly match those allocations. */
    unsafe {
        RouteArmHistoryView::init(
            history.as_mut_ptr(),
            storage.as_mut_ptr(),
            lengths.as_mut_ptr(),
            dense.as_mut_ptr(),
            dense.len(),
            lengths.len(),
            storage.len(),
        );
    }
    /* SAFETY: `RouteArmHistoryView::init` initialized every view field. */
    let mut history = unsafe { history.assume_init() };

    assert!(history.push(0, ScopeId::route(1), left_selection));
    assert!(history.push(1, ScopeId::route(2), right_selection));
    assert!(history.push(0, ScopeId::route(3), right_selection));
    assert_eq!(history.lane_len(0), 2);
    assert_eq!(history.lane_len(1), 1);
    assert_eq!(history.get(0, 0).arm, left_selection);
    assert_eq!(history.get(0, 1).scope, ScopeId::route(3));
    assert_eq!(history.get(1, 0).arm, right_selection);

    assert!(history.remove(0, 0));
    assert_eq!(history.lane_len(0), 1);
    assert_eq!(history.get(0, 0).scope, ScopeId::route(3));
    assert_eq!(history.get(1, 0).scope, ScopeId::route(2));
}

#[kani::proof]
#[kani::unwind(258)]
fn sparse_route_history_accepts_257_descriptor_relations() {
    let mut storage = [RouteArmState::EMPTY; 257];
    let mut lengths = [0u16; 1];
    let mut dense = [DenseLaneOrdinal::new(0).expect("dense lane zero")];
    let mut history = core::mem::MaybeUninit::<RouteArmHistoryView>::uninit();
    /* SAFETY: the exact 257-row capacity equals `storage.len()`, and all lane
    metadata arrays remain live and exclusively borrowed for the view. */
    unsafe {
        RouteArmHistoryView::init(
            history.as_mut_ptr(),
            storage.as_mut_ptr(),
            lengths.as_mut_ptr(),
            dense.as_mut_ptr(),
            dense.len(),
            lengths.len(),
            storage.len(),
        );
    }
    /* SAFETY: `RouteArmHistoryView::init` initialized every view field. */
    let history = unsafe { history.assume_init() };

    assert_eq!(history.capacity(), 257);
}
