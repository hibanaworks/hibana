use hibana::g::{self, Msg, ProjectionProblem};
use hibana::runtime::program::{RoleProgram, project};

#[test]
fn lane_conflict_reports_actual_receiver_and_logical_messages() {
    let program = g::seq(
        g::send::<8, 9, Msg<168, u64>>(),
        g::send::<27, 9, Msg<190, u64>>(),
    );
    let d = g::diagnose(&program).unwrap();
    assert_eq!(d.problem, ProjectionProblem::ReceiveLaneCausality);
    assert_eq!(d.role, Some(9));
    assert_eq!(d.scope, None);
    let a = d.first.unwrap();
    let b = d.second.unwrap();
    assert_eq!((a.index, a.from, a.to, a.label), (0, 8, 9, 168));
    assert_eq!((b.index, b.from, b.to, b.label), (1, 27, 9, 190));
    assert!(d.to_string().contains("event#1(27->9 label=190 lane=0)"));
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
fn roll_requires_handoff_back_to_next_iteration_sender() {
    let open = g::seq(
        g::send::<8, 9, Msg<168, u64>>(),
        g::seq(
            g::send::<9, 27, Msg<189, u64>>(),
            g::send::<27, 9, Msg<190, u64>>(),
        ),
    )
    .roll();
    let d = g::diagnose(&open).unwrap();
    assert_eq!(d.problem, ProjectionProblem::ReceiveLaneCausality);
    assert_eq!(d.role, Some(9));
    assert!(d.scope.is_some());
    assert_eq!(d.first.unwrap().from, 27);
    assert_eq!(d.second.unwrap().from, 8);
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
