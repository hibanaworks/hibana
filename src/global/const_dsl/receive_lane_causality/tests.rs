use super::{
    CausalRoles, FlowIndex, RollBodyRange, receive_precedes_after_roll_reentry,
    receive_precedes_later_send,
};
use std::vec::Vec;

#[test]
fn direction_index_matches_full_scans_for_every_subrange() {
    check_direction_indexes::<4>();
    check_direction_indexes::<64>();
    check_direction_indexes::<128>();
}

fn check_direction_indexes<const E: usize>() {
    let mut events = EffList::<E>::new();
    for (from, to, lane) in [
        (255, 128, 0),
        (1, 128, 255),
        (0, 128, 0),
        (255, 128, 0),
        (128, 128, 0),
    ]
    .into_iter()
    .take(E)
    {
        let mut atom = event(from, to);
        atom.lane = lane;
        events.push_event_mut(atom);
    }
    let mut storage = [super::Direction::EMPTY; E];
    let index = super::ReceiveDirectionIndex::new(&events, &mut storage);
    assert!(index.has_changes());
    for start in (0..=events.len()).rev().chain(0..=events.len()) {
        for end in start..=events.len() {
            for query in 0..events.len() {
                let seed = events.atom_at(query);
                for cutoff in start..=end {
                    let expected = (cutoff..end)
                        .rfind(|&candidate| {
                            let atom = events.atom_at(candidate);
                            atom.from != atom.to
                                && atom.to == seed.to
                                && atom.lane == seed.lane
                                && atom.from != seed.from
                        })
                        .map(|event| event + 1);
                    assert_eq!(index.last_change(seed, (cutoff, end)), expected);
                }
            }
        }
    }
    let logical = super::LogicalDirections::new(&events);
    for lane in [0, 128, 255] {
        for position in 0..events.len() {
            let mut atom = events.atom_at(position);
            atom.lane = lane;
            events.replace_atom(position, atom);
        }
        for query in 0..events.len() {
            let seed = events.atom_at(query);
            let mut next = logical.first(query);
            for position in 0..events.len() {
                let atom = events.atom_at(position);
                if atom.from == seed.from && atom.to == seed.to {
                    assert_eq!(next, position as u16);
                    next = logical.next(next);
                }
            }
            assert_eq!(next, u16::MAX);
        }
    }
}

#[test]
fn causal_columns_cross_word_boundaries_in_routes_parallel_and_rolls() {
    use super::{ReceiveRelations, receive_pair_is_causal};
    let mut checked = 0;
    for all_seeds in [false, true] {
        for start in [60, 63, 64, 65, 124, 127, 128] {
            for shape in 0..6 {
                let count = start + 4;
                let mut events = EffList::<256>::new_partitioned(count, 12, 0);
                for row in 0..count {
                    let (from, to) = if all_seeds {
                        [
                            (0, 128),
                            (128, 255),
                            (255, 0),
                            (0, 255),
                            (255, 128),
                            (128, 0),
                        ][row % 6]
                    } else {
                        [(0, 128), (128, 255), (255, 128), (128, 0)][row % 4]
                    };
                    events.push_event_mut(event(from, to));
                }
                match shape {
                    0 => {}
                    1 => events.push_route_scope_mut(
                        ScopeId::route(0),
                        start,
                        start + 2,
                        count,
                        ReentryMark::SinglePass,
                    ),
                    2 => events.push_parallel_scope_mut(
                        ScopeId::parallel(0),
                        start,
                        start + 2,
                        count,
                    ),
                    3 => events.push_roll_scope_mut(ScopeId::roll_scope(0), start, count),
                    4 => {
                        events.push_route_scope_mut(
                            ScopeId::route(1),
                            start,
                            start + 2,
                            count,
                            ReentryMark::SinglePass,
                        );
                        events.push_roll_scope_mut(ScopeId::roll_scope(0), start, count);
                    }
                    _ => {
                        events.push_roll_scope_mut(ScopeId::roll_scope(1), start, start + 2);
                        events.push_parallel_scope_mut(
                            ScopeId::parallel(0),
                            start,
                            start + 2,
                            count,
                        );
                        events.push_roll_scope_mut(ScopeId::roll_scope(0), start, count);
                    }
                }
                let scopes = FlowIndex::new::<0>(&events, None);
                let columns = ReceiveRelations::<256, 4>::new::<0>(
                    &events,
                    &scopes,
                    None,
                    &super::ReceiveSeeds::new(&events),
                );
                let targets = [
                    0,
                    start,
                    start + 1,
                    start + 2,
                    start + 3,
                    63,
                    64,
                    65,
                    127,
                    128,
                    129,
                ];
                for before in targets.into_iter().filter(|&row| row < count) {
                    for after in targets.into_iter().filter(|&row| row < count) {
                        assert_eq!(
                            receive_pair_is_causal::<256, 0>(
                                &events,
                                &scopes,
                                None,
                                before,
                                after,
                                Some(columns.as_ref()),
                            ),
                            receive_pair_is_causal::<256, 0>(
                                &events, &scopes, None, before, after, None,
                            ),
                            "start={start} shape={shape} before={before} after={after}",
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 6_000);
}

#[test]
fn reentry_boundaries_match_nested_shared_and_adjacent_rolls() {
    for regions in [
        &[][..],
        &[(1, 5)][..],
        &[(0, 6), (1, 5)][..],
        &[(0, 3), (3, 6)][..],
        &[(0, 6), (0, 3), (0, 3)][..],
    ] {
        let mut events = EffList::<32>::new_partitioned(6, regions.len() * 2, 0);
        for row in 0..6 {
            events.push_event_mut(event((row % 3) as u8, ((row + 1) % 3) as u8));
        }
        for (scope, &(start, end)) in regions.iter().enumerate() {
            events.push_roll_scope_mut(ScopeId::roll_scope(scope as u16), start, end);
        }
        let index = FlowIndex::new::<0>(&events, None);
        for first in 0..6 {
            for second in 0..6 {
                let (body, tail) = (first.min(second), first.max(second));
                let expected = regions
                    .iter()
                    .any(|&(start, end)| start <= body && body < end && end <= tail);
                assert_eq!(index.reentry_can_cross(first, second), expected);
            }
        }
    }
}

#[test]
fn receive_lists_match_full_source_scans_before_and_after_recoloring() {
    let roles = [0, 128, 255];
    let mut checked = 0;
    for encoding in 0usize..9usize.pow(4) {
        let mut events = EffList::<4>::new();
        let mut word = encoding;
        for _ in 0..4 {
            let pair = word % 9;
            word /= 9;
            events.push_event_mut(event(roles[pair / 3], roles[pair % 3]));
        }
        let index = FlowIndex::new::<0>(&events, None);
        for recolored in [false, true] {
            if recolored {
                for occurrence in 0..events.len() {
                    let mut atom = events.atom_at(occurrence);
                    atom.lane = occurrence as u8;
                    events.replace_atom(occurrence, atom);
                }
            }
            for receiver in 0..=u8::MAX {
                let expected: Vec<_> = (0..events.len())
                    .filter(|&event| {
                        let atom = events.atom_at(event);
                        atom.from != atom.to && atom.to == receiver
                    })
                    .collect();
                let mut actual = Vec::new();
                let mut next = index.first_receive_to(receiver);
                while next != u16::MAX {
                    assert!(actual.len() < events.len(), "cyclic receive list");
                    actual.push(next as usize);
                    next = index.next_receive_to(next);
                }
                assert_eq!(actual, expected);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 3_359_232);
    let empty = EffList::<0>::new();
    let index = FlowIndex::new::<0>(&empty, None);
    for receiver in 0..=u8::MAX {
        assert_eq!(index.first_receive_to(receiver), u16::MAX);
    }
}

#[test]
fn bounded_obligations_match_full_scans_across_routes_parallel_and_reentry() {
    use super::{
        CausalFlow, FlowGoal, FlowRange, validate_roll_body_receive_lane_causality,
        validate_structured_receive_lane_causality,
    };
    for encoding in 0usize..9usize.pow(4) {
        for shape in 0..4 {
            let mut remaining = encoding;
            let mut events = EffList::<16>::new_partitioned(4, 8, 0);
            for _ in 0..4 {
                let pair = remaining % 9;
                remaining /= 9;
                events.push_event_mut(event((pair / 3) as u8, (pair % 3) as u8));
            }
            match shape {
                0 => {}
                1 => {
                    events.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::SinglePass)
                }
                2 => events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 2, 3),
                3 => {
                    events.push_route_scope_mut(
                        ScopeId::route(0),
                        1,
                        2,
                        3,
                        ReentryMark::SinglePass,
                    );
                    events.push_route_scope_mut(
                        ScopeId::route(1),
                        0,
                        3,
                        4,
                        ReentryMark::SinglePass,
                    );
                }
                _ => unreachable!(),
            }
            let indexed = super::receive_lane_conflict_in::<16, 0>(
                &events,
                &FlowIndex::new::<0>(&events, None),
                None,
                super::ReceiveScan::From(None),
                None,
            );
            assert_eq!(
                indexed.is_none(),
                super::validate_receive_lane_causality(&events),
                "logical source encoding={encoding} shape={shape}"
            );
            let range = FlowRange {
                start: 0,
                end: 4,
                marker_floor: 0,
            };
            let mut ordinary = true;
            let mut reentry = true;
            for earlier in 0..4 {
                let atom = events.atom_at(earlier);
                if atom.from == atom.to {
                    continue;
                }
                let flow = CausalFlow {
                    eff_list: &events,
                    scope_offsets: &FlowIndex::new::<0>(&events, None),
                    earlier,
                    goal: FlowGoal::ReceiveLane(atom, usize::MAX),
                    body_start: 0,
                    iteration_start: 0,
                };
                ordinary &= flow.advance(range, CausalRoles::empty()).is_some();
                let closure = CausalFlow {
                    goal: FlowGoal::Closure,
                    ..flow
                };
                let facts = closure.advance(range, CausalRoles::empty()).unwrap();
                let next = CausalFlow {
                    iteration_start: 4,
                    ..flow
                };
                reentry &= next.advance(range, facts).is_some();
            }
            assert_eq!(
                validate_structured_receive_lane_causality(&events),
                ordinary,
                "encoding={encoding} shape={shape}"
            );
            if shape == 0 {
                assert_eq!(
                    super::validate_linear_receive_lane_causality(&events),
                    ordinary,
                    "linear encoding={encoding}"
                );
            }
            assert_eq!(
                validate_roll_body_receive_lane_causality(&events, RollBodyRange::new(0, 4)),
                reentry,
                "reentry encoding={encoding} shape={shape}"
            );
        }
    }
}
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{EffList, ReentryMark, ScopeId},
};

fn event(from: u8, to: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    }
}

#[test]
fn role_facts_cover_the_exact_wire_domain_and_join_algebra() {
    for role in 0..=u8::MAX {
        let mut singleton = CausalRoles::empty();
        singleton.insert(role);
        for query in 0..=u8::MAX {
            assert_eq!(singleton.contains(query), role == query);
        }
        assert!(singleton.intersect(singleton).contains(role));
        assert!(!singleton.intersect(CausalRoles::empty()).contains(role));
        assert!(singleton.union(CausalRoles::empty()).contains(role));
    }
    assert_eq!(core::mem::size_of::<CausalRoles>(), 32);
}

#[test]
fn both_route_arms_transfer_authority_to_a_common_reply() {
    let mut events = EffList::<8>::new_partitioned(4, 4, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(1, 2))
        .push(event(2, 1));
    events.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::SinglePass);
    assert!(receive_precedes_later_send(&events, 0, 3));
}

#[test]
fn one_route_arm_cannot_supply_a_must_fact() {
    let mut events = EffList::<8>::new_partitioned(4, 4, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(3, 4))
        .push(event(2, 1));
    events.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::SinglePass);
    assert!(!receive_precedes_later_send(&events, 0, 3));
}

#[test]
fn nested_route_intersects_every_arm_instead_of_combining_partial_chains() {
    let mut events = EffList::<16>::new_partitioned(5, 8, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(1, 2))
        .push(event(1, 2))
        .push(event(2, 1));
    events.push_route_scope_mut(ScopeId::route(1), 1, 2, 3, ReentryMark::SinglePass);
    events.push_route_scope_mut(ScopeId::route(0), 1, 3, 4, ReentryMark::SinglePass);
    assert!(receive_precedes_later_send(&events, 0, 4));
    let mut missing = EffList::<16>::new_partitioned(5, 8, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(3, 4))
        .push(event(1, 2))
        .push(event(2, 1));
    missing.push_route_scope_mut(ScopeId::route(1), 1, 2, 3, ReentryMark::SinglePass);
    missing.push_route_scope_mut(ScopeId::route(0), 1, 3, 4, ReentryMark::SinglePass);
    assert!(!receive_precedes_later_send(&missing, 0, 4));
}

#[test]
fn parallel_arms_cannot_relay_each_others_outgoing_facts() {
    let mut events = EffList::<8>::new_partitioned(4, 4, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(2, 3))
        .push(event(3, 1));
    events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 2, 3);
    assert!(!receive_precedes_later_send(&events, 0, 3));
    assert!(!receive_precedes_later_send(&events, 1, 2));
}

#[test]
fn endpoint_inside_parallel_arm_sees_only_its_own_prefix() {
    let mut events = EffList::<8>::new_partitioned(3, 3, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(2, 1));
    events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 2, 3);
    assert!(!receive_precedes_later_send(&events, 0, 2));
}

#[test]
fn route_choices_are_independent_across_roll_iterations() {
    let mut events = EffList::<8>::new_partitioned(4, 4, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(1, 2))
        .push(event(2, 1));
    events.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::Reentrant);
    let body = RollBodyRange::new(0, 4);
    assert!(!receive_precedes_after_roll_reentry(&events, body, 3, 0));
    let mut closed = EffList::<16>::new_partitioned(5, 8, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(1, 2))
        .push(event(2, 1))
        .push(event(1, 0));
    closed.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::Reentrant);
    let body = RollBodyRange::new(0, 5);
    assert!(receive_precedes_after_roll_reentry(&closed, body, 3, 0));
}

/// The same normalized rows exercise bulk validation and endpoint queries.
/// Export actual Rust decisions as kernel-checked Lean obligations; the
/// generated certificate is finite evidence, not a universal Rust refinement.
#[test]
#[ignore = "exports the kernel-checked causal correspondence artifact"]
fn export_causal_flow_for_lean() {
    use crate::global::const_dsl::{merge_parallel_lanes, validate_receive_lane_causality};
    use std::{fmt::Write, format, fs, path::Path, string::String};
    let mut output = String::from("import Hibana.IterationErasure\nopen Hibana\n");
    let mut count = 0;
    for [a, b, c] in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        for shape in 0..6 {
            let event_count = if shape == 3 || shape == 4 { 5 } else { 4 };
            let marker_count = match shape {
                2 => 3,
                3 => 8,
                4 | 5 => 6,
                _ => 4,
            };
            let mut events = EffList::<32>::new_partitioned(event_count, marker_count, 0)
                .push(event(a, b))
                .push(event(b, c));
            let send = |from, to| format!("(.send {from} {to} 0 0)");
            let first = send(a, b);
            let handoff = send(b, c);
            let reply = send(c, b);
            let joined =
                format!("(.seq {first} (.seq (.route .intrinsic {handoff} {handoff}) {reply}))");
            let choreo = match shape {
                0 => {
                    events = events.push(event(b, c)).push(event(c, b));
                    joined
                }
                1 => {
                    events = events.push(event(3, 4)).push(event(c, b));
                    format!(
                        "(.seq {first} (.seq (.route (.dynamic 7) {handoff} {}) {reply}))",
                        send(3, 4)
                    )
                }
                2 => {
                    events = events.push(event(c, 3)).push(event(3, b));
                    format!(
                        "(.seq {first} (.seq (.par {handoff} {}) {}))",
                        send(c, 3),
                        send(3, b)
                    )
                }
                3 => {
                    events = events.push(event(b, c)).push(event(b, c)).push(event(c, b));
                    format!(
                        "(.seq {first} (.seq (.route .intrinsic (.route .intrinsic {handoff} {handoff}) {handoff}) {reply}))"
                    )
                }
                4 => {
                    events = events.push(event(b, c)).push(event(c, b)).push(event(b, a));
                    format!("(.roll (.seq {joined} {}))", send(b, a))
                }
                5 => {
                    events = events.push(event(b, c)).push(event(c, b));
                    format!("(.roll {joined})")
                }
                _ => unreachable!(),
            };
            if shape == 2 {
                events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 2, 3);
                merge_parallel_lanes(&mut events, None, 1, 2, 3, 1, 1);
            } else if shape == 3 {
                events.push_route_scope_mut(ScopeId::route(1), 1, 2, 3, ReentryMark::SinglePass);
                events.push_route_scope_mut(ScopeId::route(0), 1, 3, 4, ReentryMark::SinglePass);
            } else {
                events.push_route_scope_mut(
                    ScopeId::route(1),
                    1,
                    2,
                    3,
                    if shape >= 4 {
                        ReentryMark::Reentrant
                    } else {
                        ReentryMark::SinglePass
                    },
                );
                if shape >= 4 {
                    events.push_roll_scope_mut(
                        ScopeId::new(crate::global::const_dsl::ScopeKind::Roll, 0),
                        0,
                        event_count,
                    );
                }
            }
            let accepted = validate_receive_lane_causality(&events);
            writeln!(output, "theorem causal_case_{count:03} :\n  (({choreo} : Choreo).checkReceiveLaneCausality 5 &&\n    ({choreo} : Choreo).checkRollReceiveLaneCausality 5) = {accepted} := by decide").unwrap();
            count += 1;
        }
    }
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("CausalGenerated.lean"), output).unwrap();
    assert_eq!(count, 36);
}

#[test]
fn diagnostic_witness_matches_acceptance_for_bounded_structured_sources() {
    use super::{receive_lane_conflict, validate_receive_lane_causality};
    for encoding in 0usize..9usize.pow(4) {
        for shape in 0..5 {
            let mut remaining = encoding;
            let mut events = EffList::<20>::new_partitioned(4, 12, 0);
            for _ in 0..4 {
                let pair = remaining % 9;
                remaining /= 9;
                events.push_event_mut(event((pair / 3) as u8, (pair % 3) as u8));
            }
            match shape {
                0 => {}
                1 => {
                    events.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::SinglePass)
                }
                2 => events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 2, 3),
                3 => events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 4),
                4 => {
                    events.push_route_scope_mut(
                        ScopeId::route(1),
                        1,
                        2,
                        3,
                        ReentryMark::SinglePass,
                    );
                    events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 4);
                }
                _ => unreachable!(),
            }
            let valid = validate_receive_lane_causality(&events);
            let witness = receive_lane_conflict(&events);
            assert_eq!(
                witness.is_none(),
                valid,
                "encoding={encoding} shape={shape}"
            );
            if let Some((first, second, _)) = witness {
                let a = events.atom_at(first);
                let b = events.atom_at(second);
                assert_eq!(a.to, b.to);
                assert_eq!(a.lane, b.lane);
                assert_ne!(a.from, b.from);
                assert_ne!(a.from, a.to);
                assert_ne!(b.from, b.to);
            }
        }
    }
}

#[test]
fn enclosing_parallel_paths_match_full_marker_scans_for_every_interval() {
    use super::super::parallel_arm_ranges_from_enter;
    let mut events = EffList::<32>::new_partitioned(6, 26, 0);
    for id in 0..6 {
        events.push_event_mut(event(id % 3, (id + 1) % 3));
    }
    events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 6);
    events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 3, 6);
    events.push_route_scope_mut(ScopeId::route(0), 0, 1, 3, ReentryMark::SinglePass);
    events.push_roll_scope_mut(ScopeId::roll_scope(1), 1, 3);
    events.push_parallel_scope_mut(ScopeId::parallel(1), 3, 4, 6);
    events.push_roll_scope_mut(ScopeId::roll_scope(2), 4, 6);
    let index = FlowIndex::new::<0>(&events, None);
    for start in 0..events.len() {
        for end in start + 1..=events.len() {
            let mut expected = std::vec::Vec::new();
            let markers = events.scope_markers();
            for marker in 0..markers.len() {
                let entry = markers.at(marker);
                if entry.event.is_primary_enter()
                    && matches!(entry.scope_id.kind(), Some(super::ScopeKind::Parallel))
                {
                    let (left, split, _, stop) =
                        parallel_arm_ranges_from_enter(markers, marker).unwrap();
                    if left <= start && end <= split {
                        expected.push((split, stop));
                    } else if split <= start && end <= stop {
                        expected.push((left, split));
                    }
                }
            }
            let mut actual = std::vec::Vec::new();
            let mut scope = index.enclosing_scope(start);
            let mut visited = 0;
            while scope != u16::MAX {
                visited += 1;
                assert!(visited <= markers.len(), "scope parent cycle");
                if let Some(sibling) = index.parallel_sibling(scope, start, end) {
                    actual.push(sibling);
                }
                scope = index.parent_scope(scope);
            }
            actual.sort_unstable();
            expected.sort_unstable();
            assert_eq!(actual, expected, "interval={start}..{end}");
        }
    }
}
