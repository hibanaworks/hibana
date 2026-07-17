use super::{
    closed_route_arm_ranges_from_first_enter, passive_route_child_scope,
    route_parent_arm_for_scope, route_scope_slot_for_scope, structured_scope_event_range,
};
use crate::global::const_dsl::{ReentryMark, ScopeEvent, ScopeId, ScopeMarker, ScopeMarkerView};

use super::super::source_arena::SourceRow;

const fn route_marker(offset: usize, split: usize, end: usize, scope: ScopeId) -> SourceRow {
    SourceRow::Scope(ScopeMarker::new(
        offset,
        split,
        scope,
        ScopeEvent::route_enter(end),
        ReentryMark::SinglePass,
    ))
}

#[test]
fn normalized_route_primary_carries_exact_contiguous_arms() {
    let scope = ScopeId::route(0);
    let rows = [route_marker(0, 2, 5, scope)];
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 0,
        len: rows.len(),
    };

    assert_eq!(
        closed_route_arm_ranges_from_first_enter(markers, 0),
        Some([(0, 2), (2, 5)])
    );
}

#[test]
#[should_panic(expected = "route right arm must be non-empty")]
fn normalized_route_primary_rejects_an_empty_right_arm() {
    let scope = ScopeId::route(0);
    let _ = route_marker(0, 2, 2, scope);
}

#[test]
#[should_panic(expected = "scope segment must be non-empty")]
fn normalized_route_primary_rejects_an_empty_left_arm() {
    let scope = ScopeId::route(0);
    let _ = route_marker(0, 0, 2, scope);
}

#[test]
fn nested_route_topology_has_one_range_parent_child_and_slot_authority() {
    let outer = ScopeId::route(0);
    let inner = ScopeId::route(1);
    let rows = [route_marker(0, 4, 6, outer), route_marker(0, 2, 4, inner)];
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 0,
        len: rows.len(),
    };

    assert_eq!(structured_scope_event_range(markers, outer), Some((0, 6)));
    assert_eq!(structured_scope_event_range(markers, inner), Some((0, 4)));
    assert_eq!(passive_route_child_scope(markers, outer, 0), Some(inner));
    assert_eq!(passive_route_child_scope(markers, outer, 1), None);
    assert_eq!(route_parent_arm_for_scope(markers, inner), Some((outer, 0)));
    assert_eq!(route_scope_slot_for_scope(markers, outer), Some(0));
    assert_eq!(route_scope_slot_for_scope(markers, inner), Some(1));
}
