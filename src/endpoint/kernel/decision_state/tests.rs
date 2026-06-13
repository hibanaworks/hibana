use super::*;
use crate::global::role_program::PackedLaneRange;
use core::mem::MaybeUninit;

#[test]
fn route_commit_row_set_builder_accepts_more_than_64_route_scopes() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), core::ptr::null_mut(), 71);
    }
    let mut builder = unsafe { builder.assume_init() };
    let list = builder
        .begin()
        .expect("route commit row set builder derives from route scope count");

    assert_eq!(list.len(), 0);
}

#[test]
fn prepared_route_commit_rows_use_builder_capacity_not_fixed_inline_cap() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), core::ptr::null_mut(), 9);
    }
    let mut builder = unsafe { builder.assume_init() };
    let rows =
        SelectedRouteCommitRowsRef::from_resident_range_for_lane(PackedLaneRange::new(7, 9), 3);
    let value = builder
        .seal(rows)
        .expect("valid nine-row route chain must seal without an inline cap");

    assert_eq!(value.len(), 9);
    assert_eq!(value.selected_lane(), Some(3));
    assert!(
        builder.begin().is_ok(),
        "sealing borrowed route rows must leave the builder reusable"
    );
}

#[test]
fn decode_commit_row_set_builder_accepts_more_than_64_route_scopes() {
    let mut builder = MaybeUninit::<RouteCommitRowSetBuilder>::uninit();
    unsafe {
        RouteCommitRowSetBuilder::init(builder.as_mut_ptr(), core::ptr::null_mut(), 71);
    }
    let mut builder = unsafe { builder.assume_init() };
    let list = builder
        .begin()
        .expect("decode commit plan uses shared route-scope row builder");

    assert_eq!(list.len(), 0);
}
