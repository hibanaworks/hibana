//! Build a receive-safe refinement of the existing parallel lane classes.
use super::{BYTE_DOMAIN, CausalRoles, EffList};

pub(super) const fn refine<const E: usize>(
    events: &mut EffList<E>,
    #[cfg(hibana_profile)] counters: &mut crate::g::profiling::Counters,
) -> bool {
    // Full logical-sender / original-lane domain, used only by the compiler.
    let mut assignment = [u16::MAX; BYTE_DOMAIN * BYTE_DOMAIN];
    let mut lanes = CausalRoles::empty();
    let mut event = 0;
    while event < events.len() {
        #[cfg(hibana_profile)]
        {
            counters.refinement_event_visits += 1;
        }
        lanes.insert(events.atom_at(event).lane);
        event += 1;
    }
    let mut base = 0u16;
    while let Some(lane) = lanes.pop_first() {
        let mut receivers = [CausalRoles::empty(); BYTE_DOMAIN];
        let mut senders = CausalRoles::empty();
        let mut targets = CausalRoles::empty();
        event = 0;
        while event < events.len() {
            #[cfg(hibana_profile)]
            {
                counters.refinement_event_visits += 1;
            }
            let row = events.atom_at(event);
            if row.lane == lane {
                senders.insert(row.from);
                if row.from != row.to {
                    receivers[row.to as usize].insert(row.from);
                    targets.insert(row.to);
                }
            }
            event += 1;
        }
        let mut neighbors = [CausalRoles::empty(); BYTE_DOMAIN];
        while let Some(target) = targets.pop_first() {
            let members = receivers[target as usize];
            let mut pending = members;
            while let Some(sender) = pending.pop_first() {
                #[cfg(hibana_profile)]
                {
                    counters.refinement_neighbor_visits += 1;
                }
                neighbors[sender as usize] = neighbors[sender as usize].union(members);
            }
        }
        let mut colors = [u16::MAX; BYTE_DOMAIN];
        let mut width = 0;
        while let Some(sender) = senders.pop_first() {
            let mut forbidden = CausalRoles::empty();
            let mut pending = neighbors[sender as usize];
            while let Some(other) = pending.pop_first() {
                #[cfg(hibana_profile)]
                {
                    counters.refinement_neighbor_visits += 1;
                }
                let color = colors[other as usize];
                if color != u16::MAX {
                    forbidden.insert(color as u8);
                }
            }
            let mut color = 0u16;
            while color < BYTE_DOMAIN as u16 && forbidden.contains(color as u8) {
                color += 1;
            }
            if base + color >= BYTE_DOMAIN as u16 {
                // No source update has occurred. The causal allocator can
                // still reuse colors justified by actual source order.
                return false;
            }
            colors[sender as usize] = color;
            assignment[sender as usize * BYTE_DOMAIN + lane as usize] = base + color;
            if width <= color {
                width = color + 1;
            }
        }
        base += width;
    }
    event = 0;
    while event < events.len() {
        #[cfg(hibana_profile)]
        {
            counters.refinement_event_visits += 1;
        }
        let mut row = events.atom_at(event);
        let color = assignment[row.from as usize * BYTE_DOMAIN + row.lane as usize];
        if color >= BYTE_DOMAIN as u16 {
            crate::invariant();
        }
        row.lane = color as u8;
        events.replace_atom(event, row);
        event += 1;
    }
    true
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::eff::{EffAtom, EventOrigin, EventPayload};

    fn row(from: u8, to: u8, lane: u8) -> EffAtom {
        EffAtom {
            from,
            to,
            lane,
            label: 19,
            origin: EventOrigin::User,
            payload: EventPayload::Wire(42),
        }
    }

    #[test]
    fn refinement_preserves_parallel_separation_and_separates_receive_senders() {
        let roles = [0, 63, 128, 255];
        let mut cases = 0;
        for from in roles {
            for to in roles {
                for other_from in roles {
                    for other_to in roles {
                        for lane in [0, 1, 255] {
                            for other_lane in [0, 1, 255] {
                                let before =
                                    [row(from, to, lane), row(other_from, other_to, other_lane)];
                                let mut events =
                                    EffList::<2>::new().push(before[0]).push(before[1]);
                                #[cfg(hibana_profile)]
                                let mut counters = crate::g::profiling::Counters::new();
                                assert!(refine(
                                    &mut events,
                                    #[cfg(hibana_profile)]
                                    &mut counters
                                ));
                                let after = [events.atom_at(0), events.atom_at(1)];
                                for (old, new) in before.into_iter().zip(after) {
                                    assert_eq!(
                                        EffAtom {
                                            lane: old.lane,
                                            ..new
                                        },
                                        old
                                    );
                                }
                                if lane != other_lane
                                    || (from != to
                                        && other_from != other_to
                                        && to == other_to
                                        && from != other_from)
                                {
                                    assert_ne!(after[0].lane, after[1].lane);
                                }
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2304);
    }

    #[test]
    fn an_exhausted_refinement_leaves_every_source_row_unchanged() {
        let mut events = EffList::<257>::new().push(row(1, 0, 0)).push(row(2, 0, 0));
        for lane in 1..=255 {
            events.push_event_mut(row(255, 255, lane));
        }
        let before: std::vec::Vec<_> = (0..events.len()).map(|at| events.atom_at(at)).collect();
        #[cfg(hibana_profile)]
        let mut counters = crate::g::profiling::Counters::new();
        assert!(!refine(
            &mut events,
            #[cfg(hibana_profile)]
            &mut counters
        ));
        for (at, expected) in before.into_iter().enumerate() {
            assert_eq!(events.atom_at(at), expected);
        }
    }
}
