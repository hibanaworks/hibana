//! These tests invoke receive-order validation independently of route
//! projectability, selector validation and the runtime. No source-text checks.
use super::*;
use crate::global::const_dsl::{
    validate_owned_receive_lane_causality, validate_receive_lane_causality,
};

mod schedules;

fn check<Steps: ProgramShape>(_: g::Program<Steps>, wire: bool, owned: bool) {
    let source = const { crate::g::source::ProgramSourceData::<128>::lower::<Steps>() };
    assert_eq!(validate_receive_lane_causality(source.eff_list()), wire);
    assert_eq!(
        validate_owned_receive_lane_causality(source.eff_list(), source.actors().unwrap()),
        owned
    );
    assert_eq!(schedules::check(&Steps::SOURCE_NODE, &source), owned);
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
