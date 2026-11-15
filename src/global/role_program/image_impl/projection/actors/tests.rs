use super::*;
use crate::global::compiled::{
    images::{ProgramImageBytes, ProgramImagePlan},
    lowering::CompiledProgramImage,
};
use crate::{Endpoint, g};
use std::vec::Vec;

fn program<Steps: g::ProgramShape>(_: g::Program<Steps>) -> CompiledProgramRef {
    let source = const { g::ProgramSourceData::<64>::lower::<Steps>() };
    let plan = ProgramImagePlan::from_source(&source);
    let summary = CompiledProgramImage::scan_const(source.eff_list());
    let bytes: &'static ProgramImageBytes<512> = &const {
        let source = g::ProgramSourceData::<64>::lower::<Steps>();
        let plan = ProgramImagePlan::from_source(&source);
        match ProgramImageBytes::from_source_if_fits(&source, plan.columns()) {
            Some(bytes) => bytes,
            None => panic!("actor event fixture storage"),
        }
    };
    bytes.compiled_ref(&summary, plan.columns())
}

fn rows(program: &CompiledProgramRef, actor: u8) -> Vec<(usize, RoleLocalDirection)> {
    let mut storage = [0xff; 8];
    let mut projection = ActorEventProjection::new(program, actor, &mut storage).unwrap();
    let mut rows = Vec::new();
    while let Some(row) = projection.next() {
        rows.push((row.event, row.direction));
    }
    assert!(projection.next().is_none());
    rows
}

#[test]
fn actor_projection_retains_logical_self_actions_and_distinct_facets() {
    use RoleLocalDirection::{Local, Recv, Send};
    type Loan = g::Msg<1, Endpoint<'static, 2>>;
    let program = program(g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<2, 2, g::Msg<2, ()>>(),
            g::send::<2, 1, g::Msg<3, u32>>(),
        ),
    ));
    assert_eq!(rows(&program, 0), [(0, Send)]);
    assert_eq!(
        rows(&program, 1),
        [(0, Recv), (1, Local), (2, Send), (2, Recv)]
    );
    assert!(rows(&program, 2).is_empty());
}

#[test]
fn conditional_membership_keeps_the_canonical_decision_for_later_admission() {
    use RoleLocalDirection::{Recv, Send};
    type Loan = g::Msg<1, Endpoint<'static, 255>>;
    type OtherLoan = g::Msg<2, Endpoint<'static, 255>>;
    let program = program(g::seq(
        g::route(g::send::<0, 1, Loan>(), g::send::<0, 2, OtherLoan>()).resolve::<701>(),
        g::send::<255, 3, g::Msg<3, u32>>(),
    ));
    assert_eq!(program.actor_decision_count(), 1);
    assert_eq!(program.actor_binding_at(2), Some([256, 3]));
    assert_eq!(rows(&program, 0), [(0, Send), (1, Send)]);
    assert_eq!(rows(&program, 1), [(0, Recv), (2, Send)]);
    assert_eq!(rows(&program, 2), [(1, Recv), (2, Send)]);
    assert_eq!(rows(&program, 3), [(2, Recv)]);
    assert!(rows(&program, 255).is_empty());
    // Membership is not a selected route or a current holder. Projection has
    // not rewritten or resolved the guarded continuation at event 2.
    assert_eq!(program.actor_binding_at(2), Some([256, 3]));
}

#[test]
fn independent_rolled_loans_keep_each_actors_operations() {
    use RoleLocalDirection::{Recv, Send};
    type Db = g::Msg<1, Endpoint<'static, 10>>;
    type Auth = g::Msg<2, Endpoint<'static, 11>>;
    type Work = g::Msg<3, u32>;
    type Result = g::Msg<4, u32>;
    let program = program(g::par(
        g::seq(
            g::send::<0, 1, Db>(),
            g::seq(
                g::send::<10, 3, Work>(),
                g::seq(g::send::<3, 10, Result>(), g::send::<1, 0, Db>()),
            ),
        )
        .roll(),
        g::seq(
            g::send::<0, 2, Auth>(),
            g::seq(
                g::send::<11, 3, Work>(),
                g::seq(g::send::<3, 11, Result>(), g::send::<2, 0, Auth>()),
            ),
        )
        .roll(),
    ));
    assert_eq!(
        rows(&program, 0),
        [(0, Send), (3, Recv), (4, Send), (7, Recv)]
    );
    assert_eq!(
        rows(&program, 1),
        [(0, Recv), (1, Send), (2, Recv), (3, Send)]
    );
    assert_eq!(
        rows(&program, 2),
        [(4, Recv), (5, Send), (6, Recv), (7, Send)]
    );
    assert_eq!(
        rows(&program, 3),
        [(1, Recv), (2, Send), (5, Recv), (6, Send)]
    );
    // The stream specifies storage order. It does not add dependency edges
    // between the two par arms or impose an execution schedule.
    assert_eq!(program.event_atom_at(1).lane, 0);
    assert_eq!(program.event_atom_at(5).lane, 1);
}

#[test]
fn ordinary_identity_binding_needs_no_decision_storage() {
    use RoleLocalDirection::{Local, Recv, Send};
    let program = program(g::seq(
        g::send::<0, 255, g::Msg<1, u32>>(),
        g::send::<255, 255, g::Msg<2, ()>>(),
    ));
    let mut empty = [];
    let mut projection = ActorEventProjection::new(&program, 255, &mut empty).unwrap();
    let first = projection.next().unwrap();
    assert_eq!((first.event, first.direction), (0, Recv));
    let second = projection.next().unwrap();
    assert_eq!((second.event, second.direction), (1, Local));
    assert!(projection.next().is_none());
    assert_eq!(rows(&program, 0), [(0, Send)]);
}

#[test]
fn insufficient_scratch_never_publishes_a_partial_projection() {
    type Loan = g::Msg<1, Endpoint<'static, 255>>;
    type OtherLoan = g::Msg<2, Endpoint<'static, 255>>;
    let program = program(g::seq(
        g::route(g::send::<0, 1, Loan>(), g::send::<0, 2, OtherLoan>()).resolve::<1>(),
        g::send::<255, 3, g::Msg<3, ()>>(),
    ));
    let mut empty = [];
    assert!(ActorEventProjection::new(&program, 1, &mut empty).is_none());
    let mut scratch = [0xff];
    for actor in [1, 3, 2, 255] {
        let mut projection = ActorEventProjection::new(&program, actor, &mut scratch).unwrap();
        while projection.next().is_some() {}
        assert_eq!(scratch[0], u8::from(actor == 1 || actor == 2));
    }
}
