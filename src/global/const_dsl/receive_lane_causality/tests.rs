use super::{
    CausalRoles, RollBodyRange, receive_precedes_after_roll_reentry, receive_precedes_later_send,
};

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
        payload_schema: 0,
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
                merge_parallel_lanes(&mut events, 1, 2, 3, 1, 1);
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
