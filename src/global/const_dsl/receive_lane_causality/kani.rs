use super::{
    CausalRoles, RollBodyRange, receive_precedes_after_roll_reentry, receive_precedes_later_send,
    validate_linear_receive_lane_causality, validate_receive_lane_causality,
    validate_structured_receive_lane_causality,
};
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::EffList,
};

fn event(from: u8, to: u8, lane: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane,
    }
}

fn interval_participants_match_roles<const OWNED: bool>() {
    let mut events = EffList::<3>::new();
    let mut references = [[0u16; 2]; 3];
    for pair in &mut references {
        events.push_event_mut(event(kani::any(), kani::any(), 0));
        *pair = [u16::from(kani::any::<u8>()), u16::from(kani::any::<u8>())];
    }
    let image = crate::g::SourceActorImage::<3, 0>::causal_fixture(&references, &[]);
    let index =
        super::participants::Participants::new(&events, if OWNED { Some(&image) } else { None });
    let facts = CausalRoles(kani::any());
    let left = usize::from(kani::any::<u8>() % 4);
    let right = usize::from(kani::any::<u8>() % 4);
    let start = left.min(right);
    let stop = left.max(right);
    let mut expected = true;
    for (event, pair) in references.iter().enumerate() {
        if start <= event && event < stop {
            let atom = events.atom_at(event);
            let (sender, receiver) = if OWNED {
                (pair[0] as u8, pair[1] as u8)
            } else {
                (atom.from, atom.to)
            };
            expected &= facts.contains(sender) && facts.contains(receiver);
        }
    }
    let covered = index.covered(start, stop, &facts);
    assert_eq!(covered, expected);
    kani::cover!(start == stop);
    kani::cover!(covered && start < stop);
    kani::cover!(!covered);
}

#[kani::proof]
#[kani::unwind(6)]
fn logical_interval_participants_are_exact() {
    interval_participants_match_roles::<false>();
}

#[kani::proof]
#[kani::unwind(6)]
fn physical_interval_participants_are_exact() {
    interval_participants_match_roles::<true>();
}

#[kani::proof]
#[kani::unwind(5)]
fn a_relay_between_known_participants_preserves_all_causal_facts() {
    let mut facts = CausalRoles(kani::any());
    let before = facts;
    let atom = event(kani::any(), kani::any(), 0);
    if facts.contains(atom.from) && facts.contains(atom.to) {
        facts.handoff(atom);
        for word in 0..facts.0.len() {
            assert_eq!(facts.0[word], before.0[word]);
        }
        kani::cover!(atom.from != atom.to);
        kani::cover!(atom.from == atom.to);
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn direction_index_is_the_last_matching_receive_in_every_subrange() {
    let mut events = EffList::<4>::new();
    for _ in 0..4 {
        events.push_event_mut(event(kani::any(), kani::any(), kani::any()));
    }
    let earlier = event(kani::any(), kani::any(), kani::any());
    let first = usize::from(kani::any::<u8>() % 5);
    let second = usize::from(kani::any::<u8>() % 5);
    let range = super::FlowRange {
        start: first.min(second),
        end: first.max(second),
        marker_floor: 0,
    };
    let mut expected = None;
    for index in 0..4 {
        let atom = events.atom_at(index);
        if range.start <= index
            && index < range.end
            && atom.from != atom.to
            && earlier.to == atom.to
            && earlier.lane == atom.lane
            && earlier.from != atom.from
        {
            expected = Some(index + 1);
        }
    }
    let directions = super::ReceiveDirectionIndex::new(&events, range.start, range.end);
    assert_eq!(directions.last_change(earlier, range.start), expected);
    kani::cover!(expected.is_none());
    kani::cover!(expected == Some(1));
    kani::cover!(expected == Some(4));
    kani::cover!(range.start == range.end);
}

#[kani::proof]
#[kani::unwind(5)]
fn boundary_seek_matches_the_first_qualifying_marker_for_all_compact_offsets() {
    use super::super::{
        ReentryMark, ScopeEvent, ScopeId,
        source_arena::{ScopeMarker, ScopeMarkerView, SourceRow},
    };
    let mut offsets = [0u16; 4];
    for offset in &mut offsets {
        *offset = kani::any();
    }
    for _ in 0..4 {
        for index in 1..4 {
            if offsets[index - 1] > offsets[index] {
                offsets.swap(index - 1, index);
            }
        }
    }
    let mut rows = [SourceRow::Empty; 4];
    for index in 0..4 {
        let offset = offsets[index] as usize;
        rows[index] = SourceRow::Scope(ScopeMarker::new(
            offset,
            offset,
            ScopeId::roll_scope(0),
            ScopeEvent::Exit,
            ReentryMark::SinglePass,
        ));
    }
    let markers = ScopeMarkerView {
        rows: &rows,
        start: 0,
        len: 4,
    };
    let floor = usize::from(kani::any::<u8>() % 5);
    let query = usize::from(kani::any::<u16>());
    let mut expected = 4;
    for index in 0..4 {
        if expected == 4 && floor <= index && usize::from(offsets[index]) >= query {
            expected = index;
        }
    }
    assert_eq!(markers.offset_lower_bound(query, floor), expected);
    let mut indexed = EffList::<8>::new_partitioned(4, 4, 0);
    for _ in 0..4 {
        indexed.push_event_mut(event(0, 0, 0));
    }
    for index in 0..4 {
        indexed.rows[4 + index] = rows[index];
    }
    indexed.scope_marker_len = 4;
    let scope_offsets = super::FlowIndex::new::<0>(&indexed, None);
    let source_event = usize::from(kani::any::<u8>() % 4);
    assert_eq!(
        floor.max(scope_offsets.first[source_event] as usize),
        markers.offset_lower_bound(source_event, floor)
    );
    kani::cover!(floor == 4);
    kani::cover!(floor < 4 && expected == floor);
    kani::cover!(floor < expected && expected < 4);
    kani::cover!(offsets[0] == offsets[1] && expected == 0);
}

fn distinct_role_permutation() -> (u8, u8, u8) {
    match kani::any::<u8>() % 6 {
        0 => (0, 1, 2),
        1 => (0, 2, 1),
        2 => (1, 0, 2),
        3 => (1, 2, 0),
        4 => (2, 0, 1),
        _ => (2, 1, 0),
    }
}

fn distinct_roles() -> (u8, u8, u8) {
    let earlier_source = kani::any::<u8>();
    let receiver = kani::any::<u8>();
    let later_source = kani::any::<u8>();
    if earlier_source != receiver && earlier_source != later_source && receiver != later_source {
        (earlier_source, receiver, later_source)
    } else {
        (0, 1, 2)
    }
}

#[kani::proof]
fn causal_role_facts_and_joins_are_role_exact() {
    let first_role = kani::any::<u8>();
    let second_role = kani::any::<u8>();
    let query_role = kani::any::<u8>();
    let mut left = CausalRoles::empty();
    left.insert(first_role);
    let mut right = CausalRoles::empty();
    right.insert(second_role);
    assert_eq!(left.contains(query_role), query_role == first_role);
    assert_eq!(
        left.union(right).contains(query_role),
        query_role == first_role || query_role == second_role
    );
    assert_eq!(
        left.intersect(right).contains(query_role),
        query_role == first_role && query_role == second_role
    );
}

fn three_event_pairwise_safe(events: &EffList<4>) -> bool {
    let mut pairwise_safe = true;
    for earlier in 0..3 {
        for later in earlier + 1..3 {
            let first = events.atom_at(earlier);
            let next = events.atom_at(later);
            if first.from != first.to
                && next.from != next.to
                && first.to == next.to
                && first.lane == next.lane
                && first.from != next.from
            {
                pairwise_safe &= receive_precedes_later_send(events, earlier, later);
            }
        }
    }
    pairwise_safe
}

// Separate universal obligations keep the solver's equations independent.
// Both retain all u8 senders, receivers and lanes, with the same unwind bound.
#[kani::proof]
#[kani::unwind(5)]
fn three_event_linear_scan_matches_pairwise_checker() {
    let events = EffList::<4>::new()
        .push(event(kani::any(), kani::any(), kani::any()))
        .push(event(kani::any(), kani::any(), kani::any()))
        .push(event(kani::any(), kani::any(), kani::any()));
    assert_eq!(
        validate_linear_receive_lane_causality(&events),
        three_event_pairwise_safe(&events)
    );
}

#[kani::proof]
#[kani::unwind(5)]
fn three_event_structured_scan_matches_pairwise_checker() {
    let events = EffList::<4>::new()
        .push(event(kani::any(), kani::any(), kani::any()))
        .push(event(kani::any(), kani::any(), kani::any()))
        .push(event(kani::any(), kani::any(), kani::any()));
    assert_eq!(
        validate_structured_receive_lane_causality(&events),
        three_event_pairwise_safe(&events)
    );
}

#[kani::proof]
#[kani::unwind(5)]
fn three_event_causal_handoff_accepts_every_valid_role_assignment() {
    let (earlier_source, receiver, later_source) = distinct_roles();
    let lane = kani::any();

    let events = EffList::<4>::new()
        .push(event(earlier_source, receiver, lane))
        .push(event(receiver, later_source, lane))
        .push(event(later_source, receiver, lane));

    assert!(validate_receive_lane_causality(&events));
}

#[kani::proof]
#[kani::unwind(5)]
fn sender_change_without_causal_handoff_is_rejected() {
    let (earlier_source, receiver, later_source) = distinct_roles();
    let lane = kani::any();

    let events = EffList::<4>::new()
        .push(event(earlier_source, receiver, lane))
        .push(event(later_source, receiver, lane));

    assert!(!validate_receive_lane_causality(&events));
}

#[kani::proof]
fn roll_reentry_causal_closure_rejects_open_cycle() {
    let (first_source, receiver, last_source) = distinct_role_permutation();

    let events = EffList::<4>::new()
        .push(event(first_source, receiver, 0))
        .push(event(receiver, last_source, 0))
        .push(event(last_source, receiver, 0));

    assert!(!receive_precedes_after_roll_reentry(
        &events,
        RollBodyRange::new(0, 3),
        2,
        0,
    ));
}

#[kani::proof]
fn roll_reentry_causal_closure_accepts_closed_cycle() {
    let (first_source, receiver, last_source) = distinct_role_permutation();

    let events = EffList::<4>::new()
        .push(event(first_source, receiver, 0))
        .push(event(receiver, last_source, 0))
        .push(event(last_source, receiver, 0))
        .push(event(receiver, first_source, 0));

    assert!(receive_precedes_after_roll_reentry(
        &events,
        RollBodyRange::new(0, 4),
        2,
        0,
    ));
}
