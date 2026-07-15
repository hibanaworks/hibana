use super::{
    binary_route_arm_index, dependency_conflict_for_scope, first_enter_for_scope, route_arm_ranges,
    same_scope,
};
use crate::global::{
    const_dsl::{ScopeEvent, ScopeId, ScopeKind, ScopeMarkerView},
    typestate::{LocalConflict, PackedEventConflict},
};

pub(in crate::global::role_program::image_impl) const fn route_scope_slot_for_scope(
    markers: ScopeMarkerView<'_>,
    route: ScopeId,
) -> Option<usize> {
    if route.is_none() {
        return None;
    }
    let mut slot = 0usize;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if first_enter_for_scope(markers, idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if same_scope(marker.scope_id, route) {
                return Some(slot);
            }
            slot += 1;
        }
        idx += 1;
    }
    None
}

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

pub(in crate::global::role_program::image_impl) const fn passive_arm_child_scope(
    markers: ScopeMarkerView<'_>,
    view_len: usize,
    route: ScopeId,
    arm: u8,
    arm_start: usize,
    arm_end: usize,
) -> Option<ScopeId> {
    let arm = binary_route_arm_index(arm) as u8;
    let mut child = ScopeId::none();
    let mut child_span = 0usize;
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && !same_scope(marker.scope_id, route)
            && marker.offset() == arm_start
            && let LocalConflict::RouteArm {
                scope: parent,
                arm: parent_arm,
            } = dependency_conflict_for_scope(markers, view_len, marker.scope_id)
            && same_scope(parent, route)
            && parent_arm == arm
        {
            let Some(ranges) = route_arm_ranges(markers, marker.scope_id) else {
                crate::invariant();
            };
            let end = if ranges[0].1 > ranges[1].1 {
                ranges[0].1
            } else {
                ranges[1].1
            };
            if end > arm_start && end <= arm_end {
                let span = end - arm_start;
                if child.is_none() || span > child_span {
                    child = marker.scope_id;
                    child_span = span;
                }
            }
        }
        idx += 1;
    }
    if child.is_none() { None } else { Some(child) }
}
