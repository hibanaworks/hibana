use super::*;
use crate::eff::{EffAtom, EventOrigin, EventPayload};

fn atom(from: u8, to: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        lane: 0,
        origin: EventOrigin::User,
        payload: EventPayload::Wire(0),
    }
}

#[test]
fn one_direction_recolors_all_of_its_incompatible_receives_together() {
    let mut events = EffList::<64>::new().push(atom(0, 2));
    for _ in 0..32 {
        events.push_event_mut(atom(1, 2));
    }
    #[cfg(hibana_profile)]
    let mut counters = crate::g::profiling::Counters::new();
    assert!(assign_receive_lanes(
        &mut events,
        None,
        #[cfg(hibana_profile)]
        &mut counters,
    ));
    let lane = events.atom_at(1).lane;
    assert_ne!(lane, events.atom_at(0).lane);
    for event in 1..events.len() {
        assert_eq!(events.atom_at(event).lane, lane);
    }
    assert!(
        receive_lane_conflict_in::<64, 0>(
            &events,
            &FlowIndex::new::<0>(&events, None),
            None,
            ReceiveScan::From(None),
            None,
        )
        .is_none()
    );
    #[cfg(hibana_profile)]
    assert_eq!(counters.recolorings, 1);
}

#[test]
fn direction_candidates_match_every_enclosing_parallel_constraint() {
    use crate::global::const_dsl::{ReentryMark, ScopeId};
    for selected in 0..128 {
        let mut events = EffList::<64>::new_partitioned(8, 56, 0);
        for event in 0..8 {
            let mut row = if event % 2 == 0 {
                atom(0, 1)
            } else {
                atom(1, 2)
            };
            row.lane = (event % 2) as u8;
            events.push_event_mut(row);
        }
        if selected & 1 != 0 {
            events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 8);
        }
        let parallel = [(0, 4, 8), (1, 2, 3), (4, 6, 8)];
        for (bit, &(start, split, end)) in parallel.iter().enumerate() {
            if selected & (2 << bit) != 0 {
                events.push_parallel_scope_mut(ScopeId::parallel(bit as u16), start, split, end);
            }
        }
        if selected & 16 != 0 {
            events.push_route_scope_mut(ScopeId::route(0), 0, 1, 4, ReentryMark::SinglePass);
        }
        if selected & 32 != 0 {
            events.push_roll_scope_mut(ScopeId::roll_scope(1), 1, 3);
        }
        if selected & 64 != 0 {
            events.push_route_scope_mut(ScopeId::route(1), 4, 5, 6, ReentryMark::SinglePass);
        }
        let candidates = direction_members(
            &events,
            &FlowIndex::new::<0>(&events, None),
            &LogicalDirections::new(&events),
            (0, events.atom_at(0).lane, 1),
            &mut Participants::empty(events.len()),
        );
        for (event, &candidate) in candidates.iter().enumerate().take(events.len()) {
            let blocked = parallel
                .iter()
                .enumerate()
                .any(|(bit, &(start, split, end))| {
                    if selected & (2 << bit) == 0 || event < start || event >= end {
                        return false;
                    }
                    let mut sibling = if event < split {
                        split..end
                    } else {
                        start..split
                    };
                    sibling.any(|other| other % 2 == 1)
                });
            assert_eq!(candidate, event % 2 == 0 && !blocked);
        }
    }
}

#[test]
fn direction_candidates_use_physical_participants_of_delegated_roles() {
    use crate::global::const_dsl::ScopeId;
    let mut other = atom(2, 3);
    other.lane = 1;
    let mut events = EffList::<8>::new_partitioned(2, 6, 0)
        .push(atom(0, 1))
        .push(other);
    events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 1, 2);
    let image = SourceActorImage::<8, 0>::causal_fixture(&[[0, 1], [0, 3]], &[]);
    let candidates = direction_members(
        &events,
        &FlowIndex::new(&events, Some(&image)),
        &LogicalDirections::new(&events),
        (0, 0, 1),
        &mut Participants::empty(events.len()),
    );
    assert!(!candidates[0]);
    assert!(!candidates[1]);
}

#[test]
fn sparse_lane_classes_keep_the_full_wire_color_domain() {
    for old in 0..=u8::MAX {
        let mut first = atom(0, 2);
        first.lane = old;
        let mut second = atom(1, 2);
        second.lane = old;
        let mut events = EffList::<2>::new().push(first).push(second);
        let scopes = FlowIndex::new::<0>(&events, None);
        #[cfg(hibana_profile)]
        let mut counters = crate::g::profiling::Counters::new();
        assert_eq!(
            align_range(
                &mut events,
                &scopes,
                None,
                (1, 2),
                (0, 1),
                (old as u16 + 1, 256),
                None,
                #[cfg(hibana_profile)]
                &mut counters,
            ),
            Some(if old == 0 { 2 } else { 1 })
        );
        assert_eq!(events.atom_at(0).lane, old);
        assert_eq!(events.atom_at(1).lane, if old == 0 { 1 } else { 0 });
        assert!(
            receive_lane_conflict_in::<2, 0>(&events, &scopes, None, ReceiveScan::From(None), None)
                .is_none()
        );
    }
}

#[test]
fn a_lane_class_checks_its_last_receiver_before_certification() {
    let mut prefix = atom(0, 2);
    prefix.lane = 7;
    let other = atom(4, 2);
    let mut first = atom(1, 5);
    first.lane = 7;
    let mut last = atom(3, 2);
    last.lane = 7;
    let mut events = EffList::<4>::new()
        .push(prefix)
        .push(other)
        .push(first)
        .push(last);
    let scopes = FlowIndex::new::<0>(&events, None);
    #[cfg(hibana_profile)]
    let mut counters = crate::g::profiling::Counters::new();
    assert_eq!(
        align_range(
            &mut events,
            &scopes,
            None,
            (2, 4),
            (0, 3),
            (8, 8),
            None,
            #[cfg(hibana_profile)]
            &mut counters,
        ),
        Some(2)
    );
    // The first member has no receive competitor on color 0. Its last member
    // does: 4 -> 2 supplies no causal handoff to 3 -> 2. Both must use color 1.
    assert_eq!(events.atom_at(0).lane, 7);
    assert_eq!(events.atom_at(1).lane, 0);
    assert_eq!(events.atom_at(2).lane, 1);
    assert_eq!(events.atom_at(3).lane, 1);
    assert!(
        receive_lane_conflict_in::<4, 0>(&events, &scopes, None, ReceiveScan::From(None), None)
            .is_none()
    );
}
