use super::{binary_route_arm_index, dependency_conflict_for_scope};
use crate::global::{
    const_dsl::{ScopeId, ScopeMarkerView},
    typestate::{LocalConflict, PackedEventConflict},
};

pub(super) const fn route_scope_conflict_for_commit(
    markers: ScopeMarkerView<'_>,
    view_len: usize,
    scope: ScopeId,
) -> PackedEventConflict {
    let conflict =
        PackedEventConflict::from_conflict(dependency_conflict_for_scope(markers, view_len, scope));
    match conflict.to_conflict() {
        Some(LocalConflict::RouteArm { scope: parent, .. }) if parent.same(scope) => {
            PackedEventConflict::none()
        }
        Some(_) | None => conflict,
    }
}

pub(in crate::global::role_program::image_impl) const fn route_commit_row_count(
    markers: ScopeMarkerView<'_>,
    view_len: usize,
    scope: ScopeId,
    arm: u8,
) -> usize {
    let arm = binary_route_arm_index(arm) as u8;
    let mut len = 0usize;
    let mut conflict = PackedEventConflict::route_arm(scope, arm);
    while len <= markers.len() {
        let Some(LocalConflict::RouteArm { scope, .. }) = conflict.to_conflict() else {
            return len;
        };
        if scope.is_none() {
            panic!("route commit scope missing");
        }
        len += 1;
        conflict = route_scope_conflict_for_commit(markers, view_len, scope);
    }
    panic!("route commit rows overflow");
}

pub(in crate::global::role_program::image_impl) const fn route_commit_conflict_at(
    markers: ScopeMarkerView<'_>,
    view_len: usize,
    scope: ScopeId,
    arm: u8,
    target: usize,
) -> PackedEventConflict {
    let arm = binary_route_arm_index(arm) as u8;
    let mut depth = 0usize;
    let mut conflict = PackedEventConflict::route_arm(scope, arm);
    while depth <= target && depth <= markers.len() {
        let Some(LocalConflict::RouteArm { scope, .. }) = conflict.to_conflict() else {
            panic!("route commit row missing");
        };
        if depth == target {
            return conflict;
        }
        conflict = route_scope_conflict_for_commit(markers, view_len, scope);
        depth += 1;
    }
    panic!("route commit row overflow");
}
