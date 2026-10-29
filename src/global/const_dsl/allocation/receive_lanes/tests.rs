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
        let candidates = direction_members(&events, None, (events.atom_at(0), 1));
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
