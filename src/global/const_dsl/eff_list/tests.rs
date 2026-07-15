use super::super::{EffList, ReentryMark, ScopeEvent, ScopeId};
use crate::eff::{EffAtom, EffStruct, EventOrigin};

const fn atom() -> EffStruct {
    EffStruct::atom(EffAtom {
        from: 0,
        to: 1,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    })
}

#[test]
fn equal_boundary_scope_markers_keep_canonical_nesting_order() {
    let parent = ScopeId::parallel(0);
    let left_child = ScopeId::roll_scope(1);
    let right_child = ScopeId::roll_scope(2);
    let mut source = EffList::<11>::new_partitioned(2, 9, 0);

    source.push_scope_enter_reentry_mut(0, parent, ReentryMark::SinglePass);
    source.push_scope_enter_reentry_mut(0, left_child, ReentryMark::SinglePass);
    source.push_event_mut(atom());
    source.close_scope_segment_mut(left_child, 0, 1);
    source.push_scope_exit_mut(1, left_child);
    source.push_scope_split_mut(1, parent);
    source.push_scope_enter_reentry_mut(1, right_child, ReentryMark::SinglePass);
    source.push_event_mut(atom());
    source.close_scope_segment_mut(right_child, 1, 2);
    source.push_scope_exit_mut(2, right_child);
    source.close_scope_segment_mut(parent, 0, 2);
    source.push_scope_exit_mut(2, parent);

    let markers = source.scope_markers();
    assert_eq!(markers.len(), 7);
    assert!(markers.at(0).scope_id.same(parent));
    assert!(markers.at(1).scope_id.same(left_child));
    assert!(matches!(markers.at(0).event, ScopeEvent::Enter));
    assert!(matches!(markers.at(1).event, ScopeEvent::Enter));

    assert!(markers.at(2).scope_id.same(left_child));
    assert!(matches!(markers.at(2).event, ScopeEvent::Exit));
    assert!(markers.at(3).scope_id.same(right_child));
    assert!(matches!(markers.at(3).event, ScopeEvent::Enter));
    assert!(markers.at(4).scope_id.same(parent));
    assert!(matches!(markers.at(4).event, ScopeEvent::Split));

    assert!(markers.at(5).scope_id.same(right_child));
    assert!(markers.at(6).scope_id.same(parent));
    assert!(matches!(markers.at(5).event, ScopeEvent::Exit));
    assert!(matches!(markers.at(6).event, ScopeEvent::Exit));
}

#[test]
#[should_panic(expected = "source arena partition exceeds bucket")]
fn source_arena_rejects_partition_beyond_bucket() {
    let _ = EffList::<1>::new_partitioned(1, 1, 0);
}
