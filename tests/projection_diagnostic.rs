use hibana::g::{self, Msg, ProjectionProblem};
use hibana::runtime::program::{RoleProgram, project};

#[test]
fn endpoint_cannot_carry_itself_is_a_diagnostic_value() {
    type Grant = Msg<1, hibana::Endpoint<'static, 0>>;
    let program = g::send::<0, 1, Grant>();
    let diagnostic = g::diagnose(&program).unwrap();
    assert_eq!(
        diagnostic.problem,
        ProjectionProblem::EndpointIsItsOwnCarrier
    );
    assert_eq!(diagnostic.role, Some(0));
}

#[test]
fn parallel_transfer_and_use_is_a_diagnostic_value() {
    type Grant = Msg<1, hibana::Endpoint<'static, 10>>;
    let program = g::par(g::send::<0, 1, Grant>(), g::send::<10, 3, Msg<2, u32>>());
    let diagnostic = g::diagnose(&program).unwrap();
    assert_eq!(
        diagnostic.problem,
        ProjectionProblem::ParallelOwnershipConflict
    );
    assert_eq!(diagnostic.role, Some(10));
}

#[test]
fn conflicting_initial_holders_are_a_diagnostic_value() {
    type Grant = Msg<1, hibana::Endpoint<'static, 10>>;
    let program = g::seq(g::send::<0, 1, Grant>(), g::send::<2, 1, Grant>());
    let diagnostic = g::diagnose(&program).unwrap();
    assert_eq!(
        diagnostic.problem,
        ProjectionProblem::ConflictingInitialOwners
    );
    assert_eq!(diagnostic.role, Some(10));
}

#[test]
fn a_roll_that_does_not_return_ownership_is_a_diagnostic_value() {
    type Grant = Msg<1, hibana::Endpoint<'static, 10>>;
    let diagnostic = g::diagnose(&g::send::<0, 1, Grant>().roll()).unwrap();
    assert_eq!(diagnostic.problem, ProjectionProblem::RollChangesOwner);
    assert_eq!(diagnostic.role, Some(10));
}

#[test]
fn unrelated_senders_receive_separate_lanes() {
    let program = g::seq(
        g::send::<8, 9, Msg<168, u64>>(),
        g::send::<27, 9, Msg<190, u64>>(),
    );
    assert_eq!(g::diagnose(&program), None);
    let _: RoleProgram<9> = project(&program);
}

#[test]
fn real_handoff_accepts_and_still_projects() {
    let program = g::seq(
        g::send::<8, 9, Msg<168, u64>>(),
        g::seq(
            g::send::<9, 27, Msg<189, u64>>(),
            g::send::<27, 9, Msg<190, u64>>(),
        ),
    );
    assert_eq!(g::diagnose(&program), None);
    let _: RoleProgram<9> = project(&program);
}

#[test]
fn rolled_sender_changes_can_use_separate_lanes_or_a_causal_handoff() {
    let open = g::seq(
        g::send::<8, 9, Msg<168, u64>>(),
        g::seq(
            g::send::<9, 27, Msg<189, u64>>(),
            g::send::<27, 9, Msg<190, u64>>(),
        ),
    )
    .roll();
    assert_eq!(g::diagnose(&open), None);
    let _: RoleProgram<9> = project(&open);
    let closed = g::seq(
        g::seq(
            g::send::<8, 9, Msg<168, u64>>(),
            g::seq(
                g::send::<9, 27, Msg<189, u64>>(),
                g::send::<27, 9, Msg<190, u64>>(),
            ),
        ),
        g::send::<9, 8, Msg<171, u64>>(),
    )
    .roll();
    assert_eq!(g::diagnose(&closed), None);
    let _: RoleProgram<27> = project(&closed);
}

#[test]
fn passive_collector_needs_branch_information() {
    let program = g::route(
        g::send::<11, 10, Msg<1, u64>>(),
        g::seq(
            g::send::<11, 28, Msg<193, u64>>(),
            g::seq(
                g::send::<28, 11, Msg<194, u64>>(),
                g::send::<11, 10, Msg<2, u64>>(),
            ),
        ),
    );
    let d = g::diagnose(&program).unwrap();
    assert_eq!(d.problem, ProjectionProblem::MissingBranchKnowledge);
    assert_eq!(d.role, Some(28));
    assert_eq!(d.controller, Some(11));
    let arms = d.arms.unwrap();
    assert_eq!(
        (arms[0].start, arms[0].end, arms[1].start, arms[1].end),
        (0, 1, 1, 4)
    );
    assert_eq!(d.first, None);
    assert_eq!(d.second.unwrap().label, 193);
}

#[test]
fn different_route_controllers_are_not_reported_as_missing_observer_knowledge() {
    let program = g::route(
        g::send::<0, 1, Msg<1, u64>>(),
        g::send::<2, 1, Msg<2, u64>>(),
    );
    let d = g::diagnose(&program).unwrap();
    assert_eq!(d.problem, ProjectionProblem::RouteControllerMismatch);
    assert_eq!(d.controller, None);
    assert_eq!(d.role, None);
    assert!(d.arms.is_some());
}

#[test]
fn delegated_route_controller_mismatch_identifies_the_source_events() {
    type Loan<'r> = Msg<10, hibana::Endpoint<'r, 3>>;
    let program = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::route(
            g::send::<1, 2, Msg<11, ()>>(),
            g::send::<2, 1, Msg<12, ()>>(),
        ),
    );
    let d = g::diagnose(&program).unwrap();
    assert_eq!(d.problem, ProjectionProblem::RouteControllerMismatch);
    assert_eq!(d.controller, None);
    assert_eq!(d.role, None);
    assert!(d.scope.is_some());
    let arms = d.arms.unwrap();
    assert_eq!(
        (arms[0].start, arms[0].end, arms[1].start, arms[1].end),
        (1, 2, 2, 3)
    );
    let first = d.first.unwrap();
    let second = d.second.unwrap();
    assert_eq!(
        (first.index, first.from, first.to, first.label),
        (1, 1, 2, 11)
    );
    assert_eq!(
        (second.index, second.from, second.to, second.label),
        (2, 2, 1, 12)
    );
}

#[test]
fn a_delegated_controller_error_skips_an_earlier_actor_mergeable_choice() {
    type Loan<'r> = Msg<10, hibana::Endpoint<'r, 3>>;
    let prefix = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::route(
            g::send::<0, 1, Msg<11, ()>>(),
            g::send::<0, 3, Msg<12, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&prefix), None);
    let program = g::seq(
        prefix,
        g::route(
            g::send::<1, 2, Msg<13, ()>>(),
            g::send::<2, 1, Msg<14, ()>>(),
        )
        .resolve::<7>()
        .roll(),
    );
    let d = g::diagnose(&program).unwrap();
    assert_eq!(d.problem, ProjectionProblem::RouteControllerMismatch);
    assert_eq!(d.controller, None);
    assert_eq!(d.role, None);
    assert!(d.scope.is_some());
    let arms = d.arms.unwrap();
    assert_eq!(
        (arms[0].start, arms[0].end, arms[1].start, arms[1].end),
        (3, 4, 4, 5)
    );
    let first = d.first.unwrap();
    let second = d.second.unwrap();
    assert_eq!(
        (first.index, first.from, first.to, first.label),
        (3, 1, 2, 13)
    );
    assert_eq!(
        (second.index, second.from, second.to, second.label),
        (4, 2, 1, 14)
    );
}

#[test]
fn mutually_exclusive_senders_with_controller_handoff_are_accepted() {
    let program = g::route(
        g::seq(
            g::send::<0, 1, Msg<1, u64>>(),
            g::seq(
                g::send::<0, 2, Msg<2, u64>>(),
                g::send::<1, 3, Msg<3, u64>>(),
            ),
        ),
        g::seq(
            g::send::<0, 1, Msg<4, u64>>(),
            g::seq(
                g::send::<0, 2, Msg<5, u64>>(),
                g::send::<2, 3, Msg<6, u64>>(),
            ),
        ),
    );
    assert_eq!(g::diagnose(&program), None);
    let _: RoleProgram<3> = project(&program);
}

#[test]
fn explicit_branch_notification_repairs_the_passive_collector() {
    let program = g::route(
        g::seq(
            g::send::<11, 28, Msg<193, u64>>(),
            g::seq(
                g::send::<28, 11, Msg<194, u64>>(),
                g::send::<11, 10, Msg<2, u64>>(),
            ),
        ),
        g::seq(
            g::send::<11, 28, Msg<195, u64>>(),
            g::seq(
                g::send::<28, 11, Msg<194, u64>>(),
                g::send::<11, 10, Msg<1, u64>>(),
            ),
        ),
    );
    assert_eq!(g::diagnose(&program), None);
    let _: RoleProgram<28> = project(&program);
}
