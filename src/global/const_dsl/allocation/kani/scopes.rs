use super::atom;
use crate::global::const_dsl::{EffList, ReentryMark, ScopeEvent, ScopeId, ScopeKind};

#[kani::proof]
#[kani::unwind(32)]
fn route_scope_publication_is_closed_and_atomic() {
    let mut source = EffList::<6>::new_partitioned(2, 4, 0);
    let outer = ScopeId::new(ScopeKind::Route, 0);
    source.push_event_mut(atom(0, 1, 0));
    source.push_event_mut(atom(0, 1, 0));
    source.push_route_scope_mut(outer, 0, 1, 2, ReentryMark::SinglePass);

    let markers = source.scope_markers();
    assert!(markers.len() == 4);
    let first_enter = markers.at(0);
    let first_exit = markers.at(1);
    let second_enter = markers.at(2);
    let second_exit = markers.at(3);

    assert!(first_enter.scope_id.same(outer));
    assert!(first_enter.event == ScopeEvent::route_enter(2));
    assert!(first_enter.offset() == 0);
    assert!(first_enter.segment_end() == 1);
    assert!(first_enter.reentry == ReentryMark::SinglePass);
    assert!(first_exit.scope_id.same(outer));
    assert!(first_exit.event == ScopeEvent::Exit);
    assert!(first_exit.offset() == 1);
    assert!(first_exit.segment_end() == 1);
    assert!(first_exit.reentry == ReentryMark::SinglePass);
    assert!(second_enter.scope_id.same(outer));
    assert!(second_enter.event == ScopeEvent::route_arm_continuation());
    assert!(second_enter.offset() == 1);
    assert!(second_enter.segment_end() == 2);
    assert!(second_enter.reentry == ReentryMark::SinglePass);
    assert!(second_exit.scope_id.same(outer));
    assert!(second_exit.event == ScopeEvent::Exit);
    assert!(second_exit.offset() == 2);
    assert!(second_exit.segment_end() == 2);
    assert!(second_exit.reentry == ReentryMark::SinglePass);
    assert!(markers.is_first_enter(0));
    assert!(!markers.is_first_enter(2));
    assert!(
        super::super::super::scope_ranges::route_arm_ranges_from_first_enter(markers, 0)
            == [(0, 1), (1, 2)]
    );
}
