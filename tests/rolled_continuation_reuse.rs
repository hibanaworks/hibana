//! Elastic reentry must not share an outbound selector with a continuation.
use hibana::g::{self, Msg, ProjectionProblem};

#[test]
fn inbound_boundary_does_not_disambiguate_later_outbound_reentry() {
    let protocol = g::seq(
        g::send::<0, 1, Msg<3, ()>>().roll(),
        g::seq(
            g::send::<1, 0, Msg<24, ()>>(),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn a_role_erases_unrelated_prefixes_before_reentry() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<0, 1, Msg<20, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn a_role_erases_an_unrelated_prefix_before_an_inbound_boundary() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<3, 2, Msg<24, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn distinct_reentry_contracts_remain_projectable() {
    let protocol = g::seq(
        g::send::<0, 1, Msg<3, ()>>().roll(),
        g::seq(
            g::send::<1, 0, Msg<24, ()>>(),
            g::send::<0, 1, Msg<13, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn ordinary_sequential_reuse_remains_projectable() {
    let protocol = g::seq(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<3, ()>>());
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn an_inbound_head_distinguishes_later_same_label_replies() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<1, 0, Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<0, 1, Msg<20, ()>>(),
            g::send::<1, 0, Msg<3, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn exclusive_route_arms_can_reuse_an_outbound_selector() {
    let protocol = g::route(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<0, 1, Msg<3, ()>>().roll(),
        ),
        g::seq(
            g::send::<0, 1, Msg<20, ()>>(),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn a_parallel_join_does_not_hide_a_competing_continuation() {
    let protocol = g::seq(
        g::par(
            g::send::<0, 1, Msg<3, ()>>().roll(),
            g::send::<2, 3, Msg<10, ()>>(),
        ),
        g::seq(
            g::send::<1, 0, Msg<24, ()>>(),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn independent_parallel_rolls_remain_projectable() {
    let protocol = g::par(
        g::seq(
            g::send::<0, 1, Msg<3, ()>>().roll(),
            g::send::<1, 0, Msg<24, ()>>(),
        ),
        g::send::<2, 3, Msg<3, ()>>().roll(),
    );
    assert_eq!(g::diagnose(&protocol), None);
}
