//! These tests invoke receive-order validation independently of route
//! projectability, selector validation and the runtime. No source-text checks.
use super::*;
use crate::g::source::{ProgramSourceData, SourceLowering};
use crate::global::const_dsl::{
    ReentryMark, owned_receive_lane_conflict, validate_receive_lane_causality,
};

mod schedules;

fn check_shared_lane_conflict<Steps: ProgramShape>(
    program: g::Program<Steps>,
    expected: (usize, usize, Option<u16>),
) {
    let source = const { ProgramSourceData::<128>::lower::<Steps>() };
    let actors = source.actors().unwrap();
    let mut lowering = SourceLowering::new(
        Steps::EVENT_COUNT,
        Steps::SCOPE_MARKER_COUNT,
        Steps::RESOLVER_MARKER_COUNT,
        Some(actors),
    );
    lowering.emit(&Steps::SOURCE_NODE, ReentryMark::SinglePass);
    assert_eq!(
        crate::global::const_dsl::owned_receive_lane_conflict(&lowering.eff, actors),
        Some(expected),
    );
    assert!(source.receive_causal());
    assert_eq!(g::diagnose(&program), None);
    let _ = crate::runtime::program::project::<2>(&program);
}

#[test]
fn separate_lanes_repair_a_delegated_receive_without_a_causal_handoff() {
    type Keys = g::Msg<240, Endpoint<'static, 36>>;
    check_shared_lane_conflict(
        g::seq(
            g::send::<3, 2, g::Msg<160, ()>>(),
            g::seq(
                g::send::<1, 2, Keys>(),
                g::seq(
                    g::send::<36, 1, g::Msg<241, ()>>(),
                    g::send::<1, 36, g::Msg<249, ()>>(),
                ),
            ),
        ),
        (0, 1, None),
    );
}

#[test]
fn separate_lanes_repair_a_delegated_receive_between_roll_visits() {
    type Keys = g::Msg<240, Endpoint<'static, 36>>;
    check_shared_lane_conflict(
        g::seq(
            g::send::<1, 2, Keys>(),
            g::seq(
                g::send::<36, 1, g::Msg<241, ()>>(),
                g::seq(
                    g::send::<1, 36, g::Msg<249, ()>>(),
                    g::seq(
                        g::send::<2, 1, Keys>(),
                        g::seq(
                            g::send::<2, 3, g::Msg<10, ()>>(),
                            g::send::<3, 2, g::Msg<11, ()>>(),
                        ),
                    ),
                ),
            ),
        )
        .roll(),
        (5, 0, Some(0)),
    );
}

fn check<Steps: ProgramShape>(_: g::Program<Steps>, wire: bool, owned: bool) {
    let source = const { crate::g::source::ProgramSourceData::<128>::lower::<Steps>() };
    let mut lowering = SourceLowering::new(
        Steps::EVENT_COUNT,
        Steps::SCOPE_MARKER_COUNT,
        Steps::RESOLVER_MARKER_COUNT,
        source.actors(),
    );
    lowering.emit(
        &Steps::SOURCE_NODE,
        crate::global::const_dsl::ReentryMark::SinglePass,
    );
    // Test the stated shared-lane hazards before route coloring. The allocator
    // may make a safe program by assigning those receives different lanes.
    let uncolored = ProgramSourceData {
        receive_causal: owned_receive_lane_conflict(&lowering.eff, source.actors().unwrap())
            .is_none(),
        eff: lowering.eff,
        actors: source.actors(),
    };
    check_source::<Steps>(&uncolored, wire, owned);
    let wire = validate_receive_lane_causality(source.eff_list());
    let owned = owned_receive_lane_conflict(source.eff_list(), source.actors().unwrap()).is_none();
    assert_eq!(source.receive_causal(), owned);
    check_source::<Steps>(&source, wire, owned);
}

fn check_source<Steps: ProgramShape>(source: &ProgramSourceData<128>, wire: bool, owned: bool) {
    assert_eq!(validate_receive_lane_causality(source.eff_list()), wire);
    assert_eq!(
        owned_receive_lane_conflict(source.eff_list(), source.actors().unwrap()).is_none(),
        owned
    );
    assert_eq!(schedules::check(&Steps::SOURCE_NODE, source), owned);
    for actors in [None, source.actors()] {
        let mut pairwise = true;
        for first in 0..source.eff_list().len() {
            for second in 0..source.eff_list().len() {
                if source.eff_list().atom_at(first).lane == source.eff_list().atom_at(second).lane {
                    pairwise &= crate::global::const_dsl::receive_pair_is_causal(
                        source.eff_list(),
                        &crate::global::const_dsl::ScopeOffsets::new(source.eff_list()),
                        actors,
                        first,
                        second,
                    );
                }
            }
        }
        let expected = match actors {
            None => wire,
            Some(_) => owned,
        };
        assert_eq!(
            pairwise, expected,
            "point and bulk receive queries disagree"
        );
    }
}

#[test]
fn a_return_through_the_current_holder_proves_the_previous_reply_was_consumed() {
    check(
        g::seq(
            g::send::<SERVICE, P, GrantDb>(),
            g::seq(
                g::send::<P, A, GrantDb>(),
                g::seq(
                    g::send::<DB, SERVICE, Work>(),
                    g::seq(
                        g::send::<SERVICE, DB, ResultMessage>(),
                        g::seq(g::send::<A, P, GrantDb>(), g::send::<P, SERVICE, GrantDb>()),
                    ),
                ),
            ),
        ),
        false,
        true,
    );
}

#[test]
fn two_logical_senders_on_one_actor_still_need_a_receive_handoff() {
    check(
        g::seq(
            g::send::<P, A, GrantDb>(),
            g::seq(
                g::send::<DB, SERVICE, Work>(),
                g::send::<A, SERVICE, Work>(),
            ),
        ),
        false,
        false,
    );
    check(
        g::seq(
            g::send::<P, A, GrantDb>(),
            g::seq(
                g::send::<DB, SERVICE, Work>(),
                g::seq(
                    g::send::<SERVICE, DB, ResultMessage>(),
                    g::send::<A, SERVICE, Work>(),
                ),
            ),
        ),
        false,
        true,
    );
}

#[test]
fn a_choice_correlates_its_relay_with_the_holder_receiving_it() {
    check(
        g::seq(
            g::send::<5, SERVICE, Work>(),
            g::seq(
                g::route(
                    g::send::<SERVICE, A, GrantDb>(),
                    g::send::<SERVICE, B, GrantDb>(),
                )
                .resolve::<7>(),
                g::send::<DB, SERVICE, Work>(),
            ),
        ),
        false,
        true,
    );
}

#[test]
fn teaching_a_possible_holder_is_insufficient_for_another_selected_holder() {
    check(
        g::seq(
            g::send::<5, SERVICE, Work>(),
            g::seq(
                g::route(
                    g::seq(g::send::<P, A, GrantDb>(), g::send::<SERVICE, A, Work>()),
                    g::seq(g::send::<P, B, GrantDb>(), g::send::<SERVICE, A, Work>()),
                )
                .resolve::<7>(),
                g::send::<DB, SERVICE, Work>(),
            ),
        ),
        false,
        false,
    );
}

#[test]
fn a_parallel_siblings_receive_cannot_order_another_arms_send() {
    check(
        g::seq(
            g::send::<P, B, GrantDb>(),
            g::seq(
                g::send::<5, SERVICE, Work>(),
                g::seq(
                    g::par(g::send::<SERVICE, A, Work>(), g::send::<A, 4, Work>()),
                    g::send::<4, SERVICE, Work>(),
                ),
            ),
        ),
        false,
        false,
    );
}

#[test]
fn each_rolled_visit_may_choose_a_different_holder_without_reusing_knowledge() {
    check(
        g::route(
            g::seq(
                g::send::<P, A, GrantDb>(),
                g::seq(
                    g::send::<A, SERVICE, Work>(),
                    g::seq(
                        g::send::<SERVICE, DB, ResultMessage>(),
                        g::seq(g::send::<DB, SERVICE, Work>(), g::send::<A, P, GrantDb>()),
                    ),
                ),
            ),
            g::seq(
                g::send::<P, B, GrantDb>(),
                g::seq(
                    g::send::<B, SERVICE, Work>(),
                    g::seq(
                        g::send::<SERVICE, DB, ResultMessage>(),
                        g::seq(g::send::<DB, SERVICE, Work>(), g::send::<B, P, GrantDb>()),
                    ),
                ),
            ),
        )
        .resolve::<7>()
        .roll(),
        false,
        false,
    );
    check(
        g::route(
            g::seq(
                g::send::<P, A, GrantDb>(),
                g::seq(
                    g::send::<A, SERVICE, Work>(),
                    g::seq(
                        g::send::<SERVICE, DB, ResultMessage>(),
                        g::seq(
                            g::send::<DB, SERVICE, Work>(),
                            g::seq(
                                g::send::<SERVICE, DB, ResultMessage>(),
                                g::send::<A, P, GrantDb>(),
                            ),
                        ),
                    ),
                ),
            ),
            g::seq(
                g::send::<P, B, GrantDb>(),
                g::seq(
                    g::send::<B, SERVICE, Work>(),
                    g::seq(
                        g::send::<SERVICE, DB, ResultMessage>(),
                        g::seq(
                            g::send::<DB, SERVICE, Work>(),
                            g::seq(
                                g::send::<SERVICE, DB, ResultMessage>(),
                                g::send::<B, P, GrantDb>(),
                            ),
                        ),
                    ),
                ),
            ),
        )
        .resolve::<7>()
        .roll(),
        false,
        true,
    );
}

#[test]
fn a_choice_outside_roll_keeps_the_same_holder_in_both_visits() {
    check(
        g::seq(
            g::route(
                g::seq(g::send::<P, A, GrantDb>(), g::send::<P, A, GrantAuth>()),
                g::seq(g::send::<P, B, GrantDb>(), g::send::<P, B, GrantAuth>()),
            )
            .resolve::<7>(),
            g::seq(
                g::send::<DB, SERVICE, Work>(),
                g::seq(
                    g::send::<SERVICE, DB, ResultMessage>(),
                    g::seq(
                        g::send::<AUTH, SERVICE, Work>(),
                        g::send::<SERVICE, AUTH, ResultMessage>(),
                    ),
                ),
            )
            .roll(),
        ),
        false,
        true,
    );
}

#[test]
fn an_unselected_earlier_receive_does_not_seed_a_different_path() {
    check(
        g::seq(
            g::route(
                g::seq(
                    g::send::<5, SERVICE, Work>(),
                    g::seq(
                        g::send::<P, A, GrantDb>(),
                        g::send::<SERVICE, DB, ResultMessage>(),
                    ),
                ),
                g::seq(
                    g::send::<P, B, GrantDb>(),
                    g::send::<SERVICE, DB, ResultMessage>(),
                ),
            )
            .resolve::<7>(),
            g::send::<DB, SERVICE, Work>(),
        ),
        true,
        true,
    );
}
