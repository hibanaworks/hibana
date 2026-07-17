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

#[kani::proof]
#[kani::unwind(4)]
fn normalized_route_primary_preserves_all_valid_compact_bounds() {
    let split: u16 = kani::any();
    let end: u16 = kani::any();
    kani::assume(split != 0 && split < end);

    let scope = ScopeId::route(0);
    let rows = [route_marker(0, split as usize, end as usize, scope)];
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 0,
        len: rows.len(),
    };

    assert!(
        closed_route_arm_ranges_from_first_enter(markers, 0)
            == Some([(0, split as usize), (split as usize, end as usize)])
    );
}

#[kani::proof]
#[kani::should_panic]
fn normalized_route_primary_rejects_an_empty_arm() {
    let scope = ScopeId::route(0);
    let _ = route_marker(0, 1, 1, scope);
}

#[kani::proof]
#[kani::unwind(6)]
fn nested_route_topology_authorities_are_exact_and_coherent() {
    let outer = ScopeId::route(0);
    let inner = ScopeId::route(1);
    let rows = [route_marker(0, 4, 6, outer), route_marker(0, 2, 4, inner)];
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 0,
        len: rows.len(),
    };

    assert!(structured_scope_event_range(markers, inner) == Some((0, 4)));
    assert!(passive_route_child_scope(markers, outer, 0) == Some(inner));
    assert!(route_parent_arm_for_scope(markers, inner) == Some((outer, 0)));
    assert!(route_scope_slot_for_scope(markers, outer) == Some(0));
    assert!(route_scope_slot_for_scope(markers, inner) == Some(1));
}
