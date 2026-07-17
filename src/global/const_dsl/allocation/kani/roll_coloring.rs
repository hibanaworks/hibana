use super::super::{color_roll_frame_labels, merge_route_frame_labels};
use super::atom;
use crate::global::const_dsl::{EffList, ReentryMark, ScopeEvent, ScopeId, ScopeKind};

#[kani::proof]
#[kani::unwind(160)]
fn nested_roll_frame_coloring_uses_the_complete_inbound_key() {
    let same_source: bool = kani::any();
    let prefix_from = 0;
    let nested_from = if same_source { 0 } else { 1 };

    let mut source = EffList::<14>::new_partitioned(4, 10, 0);
    let roll = ScopeId::new(ScopeKind::Roll, 0);
    let outer = ScopeId::new(ScopeKind::Route, 1);
    let inner = ScopeId::new(ScopeKind::Route, 2);
    source.push_event_mut(atom(2, 3, 0));
    source.push_event_mut(atom(prefix_from, 3, 0));
    source.push_event_mut(atom(nested_from, 3, 0));
    source.push_event_mut(atom(nested_from, 3, 0));
    source.push_route_scope_mut(inner, 2, 3, 4, ReentryMark::Reentrant);
    merge_route_frame_labels(&mut source, 2, 3, 4);
    source.push_route_scope_mut(outer, 0, 1, 4, ReentryMark::Reentrant);
    merge_route_frame_labels(&mut source, 0, 1, 4);
    source.push_roll_scope_mut(roll, 0, 4);
    color_roll_frame_labels(&mut source, 0, 4);

    assert!(source.frame_label_at(1) == 0);
    assert!(source.frame_label_at(2) == if same_source { 1 } else { 0 });
}

#[kani::proof]
#[kani::unwind(24)]
fn local_effect_frame_labels_are_erased_from_the_wire_coloring_domain() {
    let role: u8 = kani::any();
    let left_label: u8 = kani::any();
    let right_label: u8 = kani::any();
    let mut source = EffList::<2>::new()
        .push(atom(role, role, 0))
        .push(atom(role, role, 0));
    source.set_frame_label(0, left_label);
    source.set_frame_label(1, right_label);

    merge_route_frame_labels(&mut source, 0, 1, 2);
    color_roll_frame_labels(&mut source, 0, 2);

    assert!(source.frame_label_at(0) == left_label);
    assert!(source.frame_label_at(1) == right_label);
}

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
