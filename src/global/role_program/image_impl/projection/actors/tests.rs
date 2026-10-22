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

#[test]
fn resident_actor_image_checks_both_sides_before_publication() {
    use super::super::super::{RoleImageRef, plan::RoleImageColumnCounts};
    use crate::global::{compiled::lowering::RoleCompiledCounts, role_program::RuntimeRoleFacts};
    type Loan = g::Msg<1, Endpoint<'static, 2>>;
    type Steps = g::Seq<g::Send<0, 1, Loan>, g::Send<2, 1, g::Msg<2, u32>>>;
    const SOURCE: g::ProgramSourceData<8> = g::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<64> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("resident actor fixture"),
        };
    static PROGRAM: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    const FACTS: RuntimeRoleFacts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 3,
        route_scope_count: 0,
        active_lane_count: 1,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    });
    const COLUMNS: super::super::super::super::RoleImageColumns = RoleImageColumnCounts {
        dependency_rows: 0,
        conflict_rows: 0,
        resident_boundaries: 0,
        lane_bits: 1,
        route_arm_lane_steps: 0,
        route_commit_rows: 0,
        roll_scopes: 0,
        roll_entry_views: 0,
        owned_receipts: 0,
    }
    .columns(FACTS);
    const fn local_bytes() -> [u8; 64] {
        let mut bytes = [0; 64];
        let mut row = 0;
        while row < 3 {
            bytes[row * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE] =
                if row == 0 { 0 } else { 1 };
            let mut field = 2;
            while field < 8 {
                bytes[row * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE + field] = 255;
                field += 1;
            }
            bytes[row * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE + 9] =
                if row == 1 { 2 } else { 4 };
            row += 1;
        }
        bytes[3 * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE + 3] = 1;
        let index = COLUMNS.operation_index().offset as usize;
        bytes[index] = 0;
        bytes[index + 2] = 2;
        bytes[index + 4] = 1;
        bytes
    }
    static LOCAL: [u8; 64] = local_bytes();
    static DUPLICATED_SEND: [u8; 64] = {
        let mut bytes = local_bytes();
        bytes[2 * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE + 9] = 2;
        bytes
    };
    static COLLAPSED_LOCAL: [u8; 64] = {
        let mut bytes = local_bytes();
        bytes[crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE + 9] = 0;
        bytes
    };
    let image = RoleImageRef::new(&PROGRAM, 1, FACTS, COLUMNS, &LOCAL);
    assert_eq!(image.local_step_count(), 3);
    assert!(image.local_step_node(1).unwrap().action().is_send());
    assert!(image.local_step_node(2).unwrap().action().is_recv());
    for malformed in [&DUPLICATED_SEND, &COLLAPSED_LOCAL] {
        assert!(
            std::panic::catch_unwind(|| RoleImageRef::new(&PROGRAM, 1, FACTS, COLUMNS, malformed))
                .is_err()
        );
    }
    assert!(
        std::panic::catch_unwind(|| RoleImageRef::new(&PROGRAM, 0, FACTS, COLUMNS, &LOCAL))
            .is_err()
    );
}
