use super::separate_roll_frame_domains;
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{EffList, ScopeId},
};

fn atom(from: u8, to: u8, lane: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        lane,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
    }
}

fn separate_rolls<const E: usize>(count: usize) -> EffList<E> {
    let mut source = EffList::new_partitioned(count, count * 2, 0);
    for _ in 0..count {
        source.push_event_mut(atom(0, 1, 0));
    }
    for event in 0..count {
        source.push_roll_scope_mut(ScopeId::roll_scope(event as u16), event, event + 1);
    }
    source
}

#[test]
fn ordered_nonrolled_occurrences_keep_wire_reuse_beyond_256_events() {
    let mut source = EffList::<257>::new();
    for _ in 0..257 {
        source.push_event_mut(atom(0, 1, 0));
    }
    separate_roll_frame_domains(&mut source);
    for event in 0..257 {
        assert_eq!(source.frame_label_at(event), 0);
    }
}

#[test]
fn all_256_elastic_colors_are_available_without_wrapping() {
    let mut source = separate_rolls::<768>(256);
    separate_roll_frame_domains(&mut source);
    for event in 0..256 {
        assert_eq!(source.frame_label_at(event), event as u8);
    }
}

#[test]
#[should_panic(expected = "elastic roll frame domains exceed wire color capacity")]
fn a_257th_distinct_elastic_domain_fails_instead_of_aliasing() {
    let mut source = separate_rolls::<771>(257);
    separate_roll_frame_domains(&mut source);
}

#[test]
fn nested_and_coextensive_rolls_use_full_ancestry_not_only_span() {
    let mut source = EffList::<9>::new_partitioned(3, 6, 0);
    for _ in 0..3 {
        source.push_event_mut(atom(0, 1, 0));
    }
    source.push_roll_scope_mut(ScopeId::roll_scope(2), 1, 2);
    source.push_roll_scope_mut(ScopeId::roll_scope(1), 1, 2);
    source.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 3);
    separate_roll_frame_domains(&mut source);
    assert_eq!(source.frame_label_at(0), source.frame_label_at(2));
    assert_ne!(source.frame_label_at(0), source.frame_label_at(1));
}

#[test]
fn receiver_source_lane_and_original_route_color_remain_separate() {
    let mut source = EffList::<7>::new_partitioned(5, 2, 0);
    source.push_event_mut(atom(0, 1, 0));
    source.push_event_mut(atom(0, 1, 0));
    source.set_frame_label(1, 1);
    source.push_event_mut(atom(2, 1, 0));
    source.push_event_mut(atom(0, 2, 0));
    source.push_event_mut(atom(0, 1, 1));
    source.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 5);
    separate_roll_frame_domains(&mut source);
    assert_ne!(source.frame_label_at(0), source.frame_label_at(1));
    for event in [2, 3, 4] {
        assert_eq!(source.frame_label_at(event), 0);
    }
}
