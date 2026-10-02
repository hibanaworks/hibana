use super::super::{EffList, ReentryMark, ScopeEvent, ScopeId};
use crate::eff::{EffAtom, EventOrigin};

#[test]
fn boundary_seek_preserves_floors_and_every_equal_offset_marker() {
    use super::super::source_arena::{ScopeMarker, ScopeMarkerView, SourceRow};
    for mask in 0usize..256 {
        let mut rows = [SourceRow::Empty; 8];
        let mut offset = 0;
        for (index, row) in rows.iter_mut().enumerate() {
            offset += (mask >> index) & 1;
            *row = SourceRow::Scope(ScopeMarker::new(
                offset,
                offset + 1,
                ScopeId::roll_scope(0),
                ScopeEvent::roll_enter(),
                ReentryMark::SinglePass,
            ));
        }
        let markers = ScopeMarkerView {
            rows: &rows,
            start: 0,
            len: rows.len(),
        };
        for floor in 0..=rows.len() {
            for query in 0..=9 {
                let expected = (floor..rows.len())
                    .find(|&index| markers.at(index).offset() >= query)
                    .unwrap_or(rows.len());
                assert_eq!(markers.offset_lower_bound(query, floor), expected);
            }
        }
    }
}

const fn atom() -> EffAtom {
    EffAtom {
        from: 0,
        to: 1,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    }
}

#[test]
fn equal_boundary_scope_markers_keep_canonical_nesting_order() {
    let parent = ScopeId::parallel(0);
    let left_child = ScopeId::roll_scope(1);
    let right_child = ScopeId::roll_scope(2);
    let mut source = EffList::<11>::new_partitioned(2, 9, 0);

    source.push_event_mut(atom());
    source.push_event_mut(atom());
    source.push_roll_scope_mut(left_child, 0, 1);
    source.push_roll_scope_mut(right_child, 1, 2);
    source.push_parallel_scope_mut(parent, 0, 1, 2);

    let markers = source.scope_markers();
    assert_eq!(markers.len(), 7);
    assert!(markers.at(0).scope_id.same(parent));
    assert!(markers.at(1).scope_id.same(left_child));
    assert!(markers.at(0).event.is_primary_enter());
    assert!(markers.at(1).event.is_primary_enter());

    assert!(markers.at(2).scope_id.same(left_child));
    assert!(matches!(markers.at(2).event, ScopeEvent::Exit));
    assert!(markers.at(3).scope_id.same(right_child));
    assert!(markers.at(3).event.is_primary_enter());
    assert!(markers.at(4).scope_id.same(parent));
    assert!(matches!(markers.at(4).event, ScopeEvent::Split));

    assert!(markers.at(5).scope_id.same(right_child));
    assert!(markers.at(6).scope_id.same(parent));
    assert!(matches!(markers.at(5).event, ScopeEvent::Exit));
    assert!(matches!(markers.at(6).event, ScopeEvent::Exit));
}

#[test]
fn scope_marker_view_has_one_first_enter_authority_per_scope() {
    let route = ScopeId::route(0);
    let mut source = EffList::<6>::new_partitioned(2, 4, 0);

    source.push_event_mut(atom());
    source.push_event_mut(atom());
    source.push_route_scope_mut(route, 0, 1, 2, ReentryMark::SinglePass);

    let markers = source.scope_markers();
    let first = markers.first_enter_index(route).expect("first route enter");
    assert!(markers.is_first_enter(first));
    let mut first_count = 0usize;
    for index in 0..markers.len() {
        if markers.is_first_enter(index) {
            first_count += 1;
        }
    }
    assert_eq!(first_count, 1);
}

#[test]
#[should_panic(expected = "source arena partition exceeds bucket")]
fn source_arena_rejects_partition_beyond_bucket() {
    let _ = EffList::<1>::new_partitioned(1, 1, 0);
}
