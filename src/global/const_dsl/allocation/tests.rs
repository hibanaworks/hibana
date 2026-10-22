use super::merge_parallel_lanes;
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::EffList,
};

const fn atom(from: u8, to: u8, lane: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane,
    }
}

#[test]
fn parallel_lanes_reuse_colors_exactly_for_disjoint_endpoint_sets() {
    let mut disjoint = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(2, 3, 0));
    assert_eq!(merge_parallel_lanes(&mut disjoint, None, 0, 1, 2, 1, 1), 1);
    assert_eq!(disjoint.atom_at(1).lane, 0);

    let mut shared = EffList::<2>::new().push(atom(0, 1, 0)).push(atom(1, 2, 0));
    assert_eq!(merge_parallel_lanes(&mut shared, None, 0, 1, 2, 1, 1), 2);
    assert_eq!(shared.atom_at(1).lane, 1);
}

#[test]
fn parallel_lane_matching_reassigns_an_earlier_class_for_a_constrained_class() {
    let mut source = EffList::<4>::new()
        .push(atom(0, 1, 0))
        .push(atom(2, 3, 1))
        .push(atom(4, 5, 0))
        .push(atom(2, 6, 1));

    assert_eq!(merge_parallel_lanes(&mut source, None, 0, 2, 4, 2, 2), 2);
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

    assert_eq!(
        merge_parallel_lanes(&mut source, None, 0, 256, 258, 256, 2),
        256
    );
    assert_eq!(source.atom_at(256).lane, 1);
    assert_eq!(source.atom_at(257).lane, 0);
}
