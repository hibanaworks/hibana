use super::{
    EffAtom, EffList, Obligation, Paths, RollBodyRange, SourceActorImage, SourceContext,
    SourceScope,
    fixtures::{PARALLEL, ROUTE, prefix_source, receive_conflict},
    owned_receive_lane_conflict,
};
use crate::global::const_dsl::{ReentryMark, ScopeId};

fn event(from: u8, to: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        lane: 0,
        origin: crate::eff::EventOrigin::User,
        payload: crate::eff::EventPayload::Wire(0),
    }
}

#[test]
fn a_known_sender_still_teaches_an_unknown_parallel_receiver() {
    let mut events = EffList::<8>::new_partitioned(5, 3, 0)
        .push(event(4, 0))
        .push(event(0, 0))
        .push(event(0, 1))
        .push(event(0, 0))
        .push(event(1, 0));
    events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 3, 4);
    let actors =
        SourceActorImage::<8, 0>::causal_fixture(&[[4, 0], [0, 0], [0, 1], [0, 0], [1, 0]], &[]);
    assert!(receive_conflict(&events, &actors));
}

#[test]
fn a_known_receiver_does_not_authorize_an_unknown_parallel_sender() {
    let mut events = EffList::<8>::new_partitioned(4, 3, 0)
        .push(event(4, 0))
        .push(event(1, 0))
        .push(event(0, 0))
        .push(event(0, 0));
    events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 3, 4);
    let actors = SourceActorImage::<8, 0>::causal_fixture(&[[4, 0], [1, 0], [0, 0], [0, 0]], &[]);
    assert!(!receive_conflict(&events, &actors));
}

#[test]
fn source_scans_preserve_route_correlation_and_parallel_independence() {
    fn check<const PAR: bool, const SWAP: bool>() {
        let events = if PAR { &PARALLEL } else { &ROUTE };
        let actors = &const {
            let scope = SourceScope {
                start: 1,
                split: 2,
                stop: 3,
            };
            let branches = if SWAP { [2, 1] } else { [1, 2] };
            SourceActorImage::<8, 1>::causal_fixture(
                &[
                    [5, 0],
                    [0, 1],
                    [if PAR { 1 } else { 0 }, 2],
                    [if PAR { 2 } else { 256 }, 0],
                ],
                &[(scope, branches)],
            )
        };
        assert_eq!(
            owned_receive_lane_conflict(events, actors).is_none(),
            !PAR && !SWAP
        );
    }
    check::<false, false>();
    check::<false, true>();
    check::<true, false>();
    check::<true, true>();
}

#[test]
#[ignore = "16,777,216 production queries; run explicitly with checked release settings"]
fn parallel_query_covers_every_u8_actor_triple() {
    let mut checked = 0u64;
    for receiver in 0..=u8::MAX {
        for first in 0..=u8::MAX {
            for second in 0..=u8::MAX {
                let actors = SourceActorImage::<8, 0>::causal_fixture(
                    &[
                        [0, receiver.into()],
                        [receiver.into(), first.into()],
                        [first.into(), second.into()],
                        [second.into(), receiver.into()],
                    ],
                    &[],
                );
                assert_eq!(
                    receive_conflict(&PARALLEL, &actors),
                    second == receiver || second == first || first == receiver,
                    "receiver={receiver} first={first} second={second}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 256u64.pow(3));
    std::eprintln!("exhaustive parallel queries checked={checked}");
}

#[test]
#[ignore = "134,217,728 production queries; run explicitly with checked release settings"]
fn prefix_queries_cover_every_u8_actor_triple_and_visit() {
    const PLAIN: EffList<12> = prefix_source(false);
    const ROLLED: EffList<12> = prefix_source(true);
    let empty = SourceContext::new(&[]);
    let paths = Paths {
        shared: &empty,
        current: &empty,
        next: &empty,
    };
    let mut checked = 0u64;
    for receiver in 0..=u8::MAX {
        for other in 0..=u8::MAX {
            for later in 0..=u8::MAX {
                let actors = SourceActorImage::<12, 0>::causal_fixture(
                    &[
                        [0, 0],
                        [0, receiver.into()],
                        [receiver.into(), other.into()],
                        [later.into(), receiver.into()],
                    ],
                    &[],
                );
                for (events, repeated) in
                    [(&PLAIN, None), (&ROLLED, Some(RollBodyRange::new(0, 4)))]
                {
                    for earlier in [1, 2] {
                        let query = Obligation {
                            events,
                            actors: Some(&actors),
                            earlier,
                            end: 4,
                            repeated,
                        };
                        let pruned = query.check(paths, Some(3));
                        let full = query.check_from(paths, Some(3), 0);
                        let expected = match (earlier, repeated) {
                            (1, None) => later == receiver,
                            (2, None) => later == other,
                            (1, Some(_)) => later == receiver || later == other,
                            (2, Some(_)) => later == other || (other == 0 && later == receiver),
                            _ => unreachable!(),
                        };
                        assert_eq!(
                            pruned,
                            expected,
                            "pruned receiver={receiver} other={other} later={later} earlier={earlier} repeated={}",
                            repeated.is_some()
                        );
                        assert_eq!(
                            full,
                            expected,
                            "full receiver={receiver} other={other} later={later} earlier={earlier} repeated={}",
                            repeated.is_some()
                        );
                        checked += 2;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 8 * 256u64.pow(3));
    std::eprintln!("exhaustive pruned/full queries checked={checked}");
}

#[test]
fn pruned_prefixes_match_full_scans_for_small_structured_sources() {
    let empty = SourceContext::new(&[]);
    let paths = Paths {
        shared: &empty,
        current: &empty,
        next: &empty,
    };
    let mut checked = 0u64;
    // The five equality classes of a fixed three-role actor assignment.
    for actors in [[0u16, 0, 0], [0, 0, 1], [0, 1, 0], [0, 1, 1], [0, 1, 2]] {
        for encoding in 0usize..9usize.pow(4) {
            for shape in 0..8 {
                let mut events = EffList::<24>::new_partitioned(4, 16, 0);
                let mut remaining = encoding;
                let mut references = [[0u16; 2]; 4];
                for pair in &mut references {
                    let value = remaining % 9;
                    remaining /= 9;
                    let from = (value / 3) as u8;
                    let to = (value % 3) as u8;
                    events.push_event_mut(event(from, to));
                    *pair = [actors[from as usize], actors[to as usize]];
                }
                let repeated = match shape {
                    0 => None,
                    1 => {
                        events.push_route_scope_mut(
                            ScopeId::route(0),
                            0,
                            2,
                            3,
                            ReentryMark::SinglePass,
                        );
                        None
                    }
                    2 => {
                        events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 2, 3);
                        None
                    }
                    3 => {
                        events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 4);
                        Some((0, 4))
                    }
                    4 => {
                        events.push_roll_scope_mut(ScopeId::roll_scope(1), 0, 2);
                        events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 2, 3);
                        Some((0, 2))
                    }
                    5 => {
                        events.push_parallel_scope_mut(ScopeId::parallel(1), 0, 1, 2);
                        events.push_route_scope_mut(
                            ScopeId::route(0),
                            0,
                            2,
                            3,
                            ReentryMark::SinglePass,
                        );
                        None
                    }
                    6 => {
                        events.push_roll_scope_mut(ScopeId::roll_scope(1), 2, 4);
                        events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 2, 4);
                        Some((2, 4))
                    }
                    _ => {
                        events.push_route_scope_mut(
                            ScopeId::route(2),
                            0,
                            1,
                            2,
                            ReentryMark::SinglePass,
                        );
                        events.push_parallel_scope_mut(ScopeId::parallel(1), 0, 2, 3);
                        events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 4);
                        Some((0, 4))
                    }
                };
                let image = SourceActorImage::<24, 0>::causal_fixture(&references, &[]);
                for earlier in 0..4 {
                    for end in earlier + 1..=4 {
                        let obligation = Obligation {
                            events: &events,
                            actors: Some(&image),
                            earlier,
                            end,
                            repeated: None,
                        };
                        for target in [None, Some(end - 1)] {
                            assert_eq!(
                                obligation.check(paths, target),
                                obligation.check_from(paths, target, 0),
                                "encoding={encoding} shape={shape} earlier={earlier} end={end}"
                            );
                            checked += 1;
                        }
                    }
                }
                if let Some((start, stop)) = repeated {
                    for earlier in start..stop {
                        for end in start + 1..=stop {
                            let obligation = Obligation {
                                events: &events,
                                actors: Some(&image),
                                earlier,
                                end,
                                repeated: Some(RollBodyRange::new(start, stop)),
                            };
                            for target in [None, Some(end - 1)] {
                                assert_eq!(
                                    obligation.check(paths, target),
                                    obligation.check_from(paths, target, start),
                                    "rolled encoding={encoding} shape={shape} earlier={earlier} end={end}"
                                );
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    std::eprintln!("pruned/full causal queries checked={checked}");
}

#[test]
fn dropping_an_enclosing_parallel_fork_changes_the_causal_answer() {
    let mut events = EffList::<8>::new_partitioned(4, 3, 0)
        .push(event(0, 0))
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(2, 1));
    events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 2, 3);
    let image = SourceActorImage::<8, 0>::causal_fixture(&[[0, 0], [0, 1], [1, 2], [2, 1]], &[]);
    let empty = SourceContext::new(&[]);
    let paths = Paths {
        shared: &empty,
        current: &empty,
        next: &empty,
    };
    let obligation = Obligation {
        events: &events,
        actors: Some(&image),
        earlier: 1,
        end: 4,
        repeated: None,
    };
    assert!(!obligation.check(paths, Some(3)));
    assert!(!obligation.check_from(paths, Some(3), 0));
    assert!(
        obligation.check_from(paths, Some(3), 1),
        "a sequentialized sibling must change this answer"
    );
}

/// Enumerate the complete three-actor u8 domain in both route correlations.
/// The CI gate runs this explicitly in release with overflow/debug checks on.
#[test]
#[ignore = "33,554,432 production queries; run explicitly with the checked release profile"]
fn route_query_covers_every_u8_actor_triple_and_correlation() {
    let scope = SourceScope {
        start: 1,
        split: 2,
        stop: 3,
    };
    let mut checked = 0u64;
    for receiver in 0..=u8::MAX {
        for first in 0..=u8::MAX {
            for second in 0..=u8::MAX {
                for swap in [false, true] {
                    let holders = if swap {
                        [u16::from(second), u16::from(first)]
                    } else {
                        [u16::from(first), u16::from(second)]
                    };
                    let actors = SourceActorImage::<8, 1>::causal_fixture(
                        &[
                            [0, receiver.into()],
                            [receiver.into(), first.into()],
                            [receiver.into(), second.into()],
                            [256, receiver.into()],
                        ],
                        &[(scope, holders)],
                    );
                    assert_eq!(
                        receive_conflict(&ROUTE, &actors),
                        !swap || first == second,
                        "receiver={receiver} first={first} second={second} swap={swap}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 2 * 256u64.pow(3));
    std::eprintln!("exhaustive route queries checked={checked}");
}
