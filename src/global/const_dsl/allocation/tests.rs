use super::{color_roll_frame_labels, merge_parallel_lanes, merge_route_frame_labels};
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{EffList, ReentryMark, ScopeId, ScopeKind},
};

const fn atom(from: u8, to: u8, lane: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane,
    }
}

#[test]
fn ordered_occurrences_reuse_one_frame_label_beyond_the_wire_domain() {
    let mut source = EffList::<257>::new();
    for _ in 0..257 {
        source.push_event_mut(atom(0, 1, 0));
    }

    for idx in 0..source.len() {
        assert_eq!(source.frame_label_at(idx), 0);
    }
}

#[test]
fn route_arms_separate_only_competing_receive_lane_classes() {
    let mut competing = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(0, 1, 0));
    merge_route_frame_labels(&mut competing, 0, 1, 2);
    assert_eq!(competing.frame_label_at(0), 0);
    assert_eq!(competing.frame_label_at(1), 1);

    let mut distinct_source = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(2, 1, 0));
    merge_route_frame_labels(&mut distinct_source, 0, 1, 2);
    assert_eq!(distinct_source.frame_label_at(0), 0);
    assert_eq!(distinct_source.frame_label_at(1), 0);

    let mut independent = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(0, 1, 1));
    merge_route_frame_labels(&mut independent, 0, 1, 2);
    assert_eq!(independent.frame_label_at(0), 0);
    assert_eq!(independent.frame_label_at(1), 0);
}

#[test]
fn parallel_lanes_reuse_colors_exactly_for_disjoint_endpoint_sets() {
    let mut disjoint = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(2, 3, 0));
    assert_eq!(merge_parallel_lanes(&mut disjoint, 0, 1, 2, 1, 1), 1);
    assert_eq!(disjoint.atom_at(1).lane, 0);

    let mut shared = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(1, 2, 0));
    assert_eq!(merge_parallel_lanes(&mut shared, 0, 1, 2, 1, 1), 2);
    assert_eq!(shared.atom_at(1).lane, 1);
}

#[test]
fn parallel_lane_matching_reassigns_an_earlier_class_for_a_constrained_class() {
    let mut source = EffList::<4>::new()
        .push(atom(0, 1, 0))
        .push(atom(2, 3, 1))
        .push(atom(4, 5, 0))
        .push(atom(2, 6, 1));

    assert_eq!(merge_parallel_lanes(&mut source, 0, 2, 4, 2, 2), 2);
    assert_eq!(source.atom_at(2).lane, 1);
    assert_eq!(source.atom_at(3).lane, 0);
}

#[test]
fn parallel_lane_matching_avoids_a_false_two_hundred_fifty_seventh_lane() {
    let mut source = EffList::<258>::new().push(atom(0, 1, 0));
    for lane in 1..=u8::MAX {
        source.push_event_mut(atom(2, 3, lane));
    }
    source.push_event_mut(atom(4, 5, 0));
    source.push_event_mut(atom(2, 6, 1));

    assert_eq!(merge_parallel_lanes(&mut source, 0, 256, 258, 256, 2), 256);
    assert_eq!(source.atom_at(256).lane, 1);
    assert_eq!(source.atom_at(257).lane, 0);
}

#[test]
#[should_panic(expected = "route inbound occurrence coloring exceeds wire domain")]
fn route_rejects_an_unavoidable_two_hundred_fifty_seventh_color() {
    let mut source = EffList::<257>::new();
    for label in 0..=u8::MAX {
        source.push_event_mut(atom(0, 1, 0));
        source.set_frame_label(label as usize, label);
    }
    source.push_event_mut(atom(0, 1, 0));

    merge_route_frame_labels(&mut source, 0, 256, 257);
}

#[test]
fn local_effects_do_not_consume_wire_frame_colors() {
    let mut source = EffList::<257>::new();
    for label in 0..=u8::MAX {
        source.push_event_mut(atom(0, 0, 0));
        source.set_frame_label(label as usize, label);
    }
    source.push_event_mut(atom(0, 0, 0));

    merge_route_frame_labels(&mut source, 0, 256, 257);
    color_roll_frame_labels(&mut source, 0, 257);

    for idx in 0..source.len() {
        assert_eq!(source.frame_label_at(idx), (idx as u8));
    }
}

fn nested_roll_source(prefix_from: u8, nested_from: u8) -> EffList<14> {
    let mut source = EffList::<14>::new_partitioned(4, 10, 0);
    let roll = ScopeId::new(ScopeKind::Roll, 0);
    let outer = ScopeId::new(ScopeKind::Route, 1);
    let inner = ScopeId::new(ScopeKind::Route, 2);

    source.push_event_mut(atom(9, 1, 0));
    source.push_event_mut(atom(prefix_from, 1, 0));
    source.push_event_mut(atom(nested_from, 1, 0));
    source.push_event_mut(atom(nested_from, 1, 0));
    source.push_route_scope_mut(inner, 2, 3, 4, ReentryMark::Reentrant);
    merge_route_frame_labels(&mut source, 2, 3, 4);
    source.push_route_scope_mut(outer, 0, 1, 4, ReentryMark::Reentrant);
    merge_route_frame_labels(&mut source, 0, 1, 4);
    source.push_roll_scope_mut(roll, 0, 4);
    color_roll_frame_labels(&mut source, 0, 4);
    source
}

#[test]
fn roll_coloring_separates_route_paths_for_the_complete_inbound_key() {
    let same_source = nested_roll_source(0, 0);
    assert_eq!(same_source.frame_label_at(0), 0);
    assert_eq!(same_source.frame_label_at(1), 0);
    assert_eq!(same_source.frame_label_at(2), 1);
    assert_eq!(same_source.frame_label_at(3), 2);

    let distinct_source = nested_roll_source(0, 2);
    assert_eq!(distinct_source.frame_label_at(1), 0);
    assert_eq!(distinct_source.frame_label_at(2), 0);
}

#[test]
fn roll_coloring_consumes_only_atomically_published_scopes() {
    let mut source = EffList::<4>::new_partitioned(2, 2, 0);
    let roll = ScopeId::roll_scope(1);
    source.push_event_mut(atom(0, 1, 0));
    source.push_event_mut(atom(0, 1, 0));
    source.push_roll_scope_mut(roll, 0, 2);

    color_roll_frame_labels(&mut source, 0, 2);

    assert_eq!(source.frame_label_at(0), 0);
    assert_eq!(source.frame_label_at(1), 0);
}
