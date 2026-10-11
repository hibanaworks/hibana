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

// Keep the original ordered full scan as the differential oracle. The window
// optimization must preserve every equal-offset candidate and its fold order.
fn passive_child_full_scan(
    markers: ScopeMarkerView<'_>,
    route: ScopeId,
    arm: u8,
) -> Option<ScopeId> {
    use super::{StructuredScopeRange, outermost_scope_range, route_arm_event_ranges_for_scope};
    use crate::global::const_dsl::ScopeKind;

    if arm > 1 || !matches!(route.kind(), Some(ScopeKind::Route)) {
        return None;
    }
    let ranges = route_arm_event_ranges_for_scope(markers, route)?;
    let (arm_start, arm_end) = ranges[arm as usize];
    let mut child: Option<StructuredScopeRange> = None;
    let mut index = 0;
    while index < markers.len() {
        let marker = markers.at(index);
        if markers.is_first_enter(index)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && !marker.scope_id.same(route)
            && marker.offset() == arm_start
        {
            let [_, (_, child_end)] =
                closed_route_arm_ranges_from_first_enter(markers, index).unwrap();
            if child_end <= arm_end {
                let candidate = StructuredScopeRange::new(marker.scope_id, arm_start, child_end);
                child = Some(match child {
                    Some(current) => outermost_scope_range(current, candidate),
                    None => candidate,
                });
            }
        }
        index += 1;
    }
    child.map(|value| value.scope())
}

#[test]
fn passive_child_window_matches_full_scan_across_sorted_ties_gaps_and_domains() {
    // Each bit chooses a gap or an equal offset. Sparse scope IDs deliberately
    // separate encoded identity from marker row position and route slot.
    for mask in 0usize..256 {
        let mut rows = [SourceRow::Empty; 8];
        let mut offset = 4;
        for (index, row) in rows.iter_mut().enumerate() {
            offset += 3 * ((mask >> index) & 1);
            *row = route_marker(
                offset,
                offset + 1 + 5 * (index % 6),
                offset + 2 + 5 * (index % 6),
                ScopeId::route((index * 7) as u16),
            );
        }
        let markers = ScopeMarkerView {
            rows: &rows,
            start: 0,
            len: rows.len(),
        };
        for ordinal in 0..=50 {
            for arm in [0, 1, 2, u8::MAX] {
                let scope = ScopeId::route(ordinal);
                assert_eq!(
                    passive_route_child_scope(markers, scope, arm),
                    passive_child_full_scan(markers, scope, arm),
                    "mask={mask}, ordinal={ordinal}, arm={arm}",
                );
            }
        }
        for scope in [
            ScopeId::none(),
            ScopeId::roll_scope(0),
            ScopeId::parallel(0),
        ] {
            assert_eq!(
                passive_route_child_scope(markers, scope, 0),
                passive_child_full_scan(markers, scope, 0),
            );
        }
    }
}

#[test]
fn passive_child_window_keeps_later_candidates_and_outermost_equal_range_ties() {
    let parent = ScopeId::route(0);
    let inner = ScopeId::route(5);
    let outer = ScopeId::route(2);
    // A nonzero arena start checks that lower-bound indices are view-relative.
    let rows = [
        SourceRow::Empty,
        route_marker(0, 1, 2, ScopeId::route(20)),
        route_marker(10, 18, 25, parent),
        route_marker(10, 12, 16, inner),
        route_marker(10, 13, 16, outer),
        route_marker(18, 19, 21, ScopeId::route(7)),
        route_marker(30, 31, 32, ScopeId::route(9)),
        SourceRow::Empty,
    ];
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 1,
        len: 6,
    };
    assert_eq!(passive_child_full_scan(markers, parent, 0), Some(outer));
    for arm in 0..=2 {
        assert_eq!(
            passive_route_child_scope(markers, parent, arm),
            passive_child_full_scan(markers, parent, arm),
        );
    }
    let empty = ScopeMarkerView {
        rows: &[],
        start: 0,
        len: 0,
    };
    assert_eq!(passive_route_child_scope(empty, parent, 0), None);
}

#[test]
fn passive_child_window_retains_duplicate_candidate_invariant_failure() {
    let parent = ScopeId::route(0);
    let duplicate = ScopeId::route(1);
    let rows = [
        route_marker(7, 15, 20, parent),
        route_marker(7, 9, 11, duplicate),
        route_marker(7, 9, 11, duplicate),
    ];
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 0,
        len: rows.len(),
    };
    let old = std::panic::catch_unwind(|| passive_child_full_scan(markers, parent, 0));
    let new = std::panic::catch_unwind(|| passive_route_child_scope(markers, parent, 0));
    assert!(old.is_err());
    assert!(new.is_err());
}
