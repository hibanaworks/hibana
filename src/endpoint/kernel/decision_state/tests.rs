use super::*;
use crate::global::role_program::PackedLaneRange;
use core::mem::MaybeUninit;

#[test]
fn rooted_route_chain_prefixes_match_independent_membership_and_merging() {
    use crate::{
        Endpoint,
        g::{self, Msg},
        global::{
            compiled::images::RoleDescriptorRef, role_program::RoleImageRef,
            typestate::EventCursorState,
        },
        runtime::program::project,
    };

    fn compare(image: &'static RoleImageRef) {
        let mut chains = std::vec![SelectedRouteCommitRowsRef::EMPTY];
        for slot in 0..image.footprint().route_scope_count {
            for arm in 0..2 {
                chains.push(SelectedRouteCommitRowsRef::from_resident_range_for_lane(
                    image.route_commit_range_by_slot(slot, arm),
                    0,
                ));
            }
        }
        let members = |chain: SelectedRouteCommitRowsRef| {
            (0..chain.len())
                .map(|index| {
                    match image
                        .route_commit_row_at(chain.range().start() + index)
                        .to_conflict()
                        .unwrap()
                    {
                        LocalConflict::RouteArm { scope, arm } => (scope, arm),
                        LocalConflict::Unconditional | LocalConflict::SharedRoute => {
                            panic!("a sealed chain contains only route decisions")
                        }
                    }
                })
                .collect::<std::vec::Vec<_>>()
        };
        let mut state = MaybeUninit::<EventCursorState>::uninit();
        let mut cursor = MaybeUninit::<EventCursor>::uninit();
        let mut heads = [0u16; 256];
        let mut labels = [0u16; 256];
        let mut completed = std::vec![0u32; image.footprint().completion_bit_count().div_ceil(32)];
        // SAFETY: the static image and disjoint, stationary backing columns
        // outlive the cursor. No cursor or backing pointer escapes this test.
        let cursor = unsafe {
            EventCursor::init_from_compiled(
                cursor.as_mut_ptr(),
                state.as_mut_ptr(),
                heads.as_mut_ptr(),
                labels.as_mut_ptr(),
                completed.as_mut_ptr(),
                RoleDescriptorRef::from_resident(image),
            );
            cursor.assume_init_mut()
        };
        let pool_end = chains
            .iter()
            .filter(|chain| !chain.is_empty())
            .map(|chain| chain.range().end())
            .max()
            .unwrap_or(0);
        for start in 0..pool_end {
            for end in start + 1..=pool_end {
                let range = PackedLaneRange::new(start, end - start);
                let candidate = SelectedRouteCommitRowsRef::from_resident_range_for_lane(range, 0);
                let registered = chains
                    .iter()
                    .any(|chain| !chain.is_empty() && chain.range().raw() == range.raw());
                assert_eq!(candidate.is_complete_chain(cursor), registered);
                if registered {
                    let entries = members(candidate);
                    for (index, (scope, _)) in entries.iter().enumerate() {
                        assert!(entries[..index].iter().all(|(prior, _)| prior != scope));
                    }
                }
                for &chain in &chains {
                    assert_eq!(
                        candidate.matches_range(chain.range()),
                        registered && candidate.range().raw() == chain.range().raw()
                    );
                }
            }
        }
        for &left in &chains {
            let left_members = members(left);
            for length in 0..=left.len() {
                let prefix = SelectedRouteCommitRowsRef::from_resident_range_for_lane(
                    PackedLaneRange::new(left.range().start(), length),
                    0,
                );
                for scope in (0..image.footprint().route_scope_count)
                    .map(|slot| image.route_scope_by_slot(slot).unwrap())
                    .chain([
                        ScopeId::none(),
                        ScopeId::route(ScopeId::MAX_LOCAL_ORDINAL),
                        ScopeId::roll_scope(0),
                        ScopeId::parallel(0),
                    ])
                {
                    let expected = left_members[..length]
                        .iter()
                        .find_map(|&(candidate, arm)| (candidate == scope).then_some(arm));
                    assert_eq!(prefix.arm_for_scope(cursor, scope), expected);
                }
            }
            for &right in &chains {
                let right_members = members(right);
                let left_contains = right_members.iter().all(|row| left_members.contains(row));
                let right_contains = left_members.iter().all(|row| right_members.contains(row));
                assert_eq!(left.starts_with(cursor, right), left_contains);
                if right.is_empty() {
                    continue;
                }
                let &(scope, arm) = right_members.last().unwrap();
                let mut selected = SelectedRouteCommitRows {
                    routes: left,
                    max_len: image.footprint().max_route_commit_count as u16,
                };
                let result =
                    selected.merge_chain(cursor, 0, PackedEventConflict::route_arm(scope, arm));
                assert_eq!(result.is_ok(), left_contains || right_contains);
                if result.is_ok() {
                    let expected = if left_contains {
                        &left_members
                    } else {
                        &right_members
                    };
                    assert_eq!(&members(selected.routes), expected);
                } else {
                    assert_eq!(members(selected.routes), left_members);
                }
            }
        }
    }

    let parallel = g::par(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::send::<0, 1, Msg<3, ()>>(),
        )
        .roll(),
        g::route(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
    );
    compare(project::<0>(&parallel).role_image_ref());
    compare(project::<1>(&parallel).role_image_ref());

    type Loan<'r> = Msg<6, Endpoint<'r, 2>>;
    let delegated = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::route(
                g::seq(g::send::<2, 3, Msg<7, ()>>(), g::send::<3, 2, Msg<8, ()>>()),
                g::seq(
                    g::send::<2, 3, Msg<9, ()>>(),
                    g::send::<3, 2, Msg<10, ()>>(),
                ),
            ),
            g::send::<1, 0, Loan<'_>>(),
        ),
    )
    .roll();
    compare(project::<0>(&delegated).role_image_ref());
    compare(project::<1>(&delegated).role_image_ref());
    compare(project::<2>(&delegated).role_image_ref());
    compare(project::<3>(&delegated).role_image_ref());
}

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

    assert_eq!(list.routes.len(), 0);
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
    assert_eq!(builder.begin().routes.len(), 0);
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

    assert_eq!(list.routes.len(), 0);
}
