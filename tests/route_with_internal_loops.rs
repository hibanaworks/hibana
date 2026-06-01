//! Regression test: Route arms with internal loops must have disjoint scope ordinals.
//!
//! This test verifies that binary route construction assigns disjoint ordinal ranges
//! to each arm, preventing scope parent mismatch panics when multiple arms contain
//! internal loops or nested routes.
//!
//! Before the fix in `hibana/src/global/program.rs`, this would panic with:
//! "scope parent mismatch for ordinal"
//!
//! The fix is in the binary route/par combinators: each arm and lane gets a
//! disjoint ordinal range during composition.

use hibana::g::{self, Msg};
use hibana::integration::cap::control::{LoopBreakKind, LoopContinueKind, RouteDecisionKind};
use hibana::integration::program::{RoleProgram, project};

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;

const ARM_A_CONTROL_LOGICAL: u8 = 120;
const ARM_B_CONTROL_LOGICAL: u8 = 121;

// Route arm marker kinds

const ROUTE_POLICY_ID: u16 = 0x1000;
fn client_program() -> RoleProgram<0> {
    let arm_a_loop_body = g::send::<0, 1, Msg<1, ()>, 0>();
    let arm_a_loop = g::route(
        g::seq(
            g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>()
                .policy::<{ ROUTE_POLICY_ID + 1 }>(),
            arm_a_loop_body,
        ),
        g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
            .policy::<{ ROUTE_POLICY_ID + 1 }>(),
    );
    let arm_a = g::seq(
        g::send::<0, 0, Msg<ARM_A_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        arm_a_loop,
    );

    let arm_b_loop_body = g::send::<0, 1, Msg<2, ()>, 0>();
    let arm_b_loop = g::route(
        g::seq(
            g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>()
                .policy::<{ ROUTE_POLICY_ID + 2 }>(),
            arm_b_loop_body,
        ),
        g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
            .policy::<{ ROUTE_POLICY_ID + 2 }>(),
    );
    let arm_b = g::seq(
        g::send::<0, 0, Msg<ARM_B_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        arm_b_loop,
    );
    let route_program = g::route(arm_a, arm_b);
    project(&route_program)
}

fn server_program() -> RoleProgram<1> {
    let arm_a_loop_body = g::send::<0, 1, Msg<1, ()>, 0>();
    let arm_a_loop = g::route(
        g::seq(
            g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>()
                .policy::<{ ROUTE_POLICY_ID + 1 }>(),
            arm_a_loop_body,
        ),
        g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
            .policy::<{ ROUTE_POLICY_ID + 1 }>(),
    );
    let arm_a = g::seq(
        g::send::<0, 0, Msg<ARM_A_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        arm_a_loop,
    );

    let arm_b_loop_body = g::send::<0, 1, Msg<2, ()>, 0>();
    let arm_b_loop = g::route(
        g::seq(
            g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>()
                .policy::<{ ROUTE_POLICY_ID + 2 }>(),
            arm_b_loop_body,
        ),
        g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
            .policy::<{ ROUTE_POLICY_ID + 2 }>(),
    );
    let arm_b = g::seq(
        g::send::<0, 0, Msg<ARM_B_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        arm_b_loop,
    );
    let route_program = g::route(arm_a, arm_b);
    project(&route_program)
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

/// Test that program construction succeeds without scope ordinal collision.
/// Before the fix, this would panic at const eval time or during projection.
#[test]
fn route_with_internal_loops_compiles() {
    let _ = client_program();
    let _ = server_program();
}
