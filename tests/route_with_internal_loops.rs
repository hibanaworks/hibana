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

use hibana::g::advanced::steps::{
    LoopBreakSteps, LoopDecisionSteps, ProjectRole, SendStep, SeqSteps, StepConcat, StepCons,
    StepNil,
};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;

// Route arm marker labels (custom, not loop labels)
const LABEL_ARM_A: u8 = 64;
const LABEL_ARM_B: u8 = 65;

// Route arm marker kinds
hibana::impl_control_resource!(ArmAKind, handle: RouteDecision, name: "ArmA", label: LABEL_ARM_A);
hibana::impl_control_resource!(ArmBKind, handle: RouteDecision, name: "ArmB", label: LABEL_ARM_B);

// -----------------------------------------------------------------------------
// Programs
// -----------------------------------------------------------------------------

const ROUTE_POLICY_ID: u16 = 0x1000;

// Arm A: marker + loop
const ARM_A_LOOP_BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>();

const ARM_A_LOOP_CONT: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<{ ROUTE_POLICY_ID + 1 }>(),
    ARM_A_LOOP_BODY,
);

const ARM_A_LOOP_BREAK: g::Program<
    LoopBreakSteps<
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    >,
> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<{ ROUTE_POLICY_ID + 1 }>();

const ARM_A_LOOP: g::Program<
    LoopDecisionSteps<
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        StepNil,
        StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
    >,
> = g::route(ARM_A_LOOP_CONT, ARM_A_LOOP_BREAK);

const ARM_A: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, CanonicalControl<ArmAKind>>,
                0,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
        >,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, CanonicalControl<ArmAKind>>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>(),
    ARM_A_LOOP,
);

// Arm B: marker + loop
const ARM_B_LOOP_BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<2, ()>, 0>();

const ARM_B_LOOP_CONT: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<{ ROUTE_POLICY_ID + 2 }>(),
    ARM_B_LOOP_BODY,
);

const ARM_B_LOOP_BREAK: g::Program<
    LoopBreakSteps<
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    >,
> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<{ ROUTE_POLICY_ID + 2 }>();

const ARM_B_LOOP: g::Program<
    LoopDecisionSteps<
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        StepNil,
        StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>,
    >,
> = g::route(ARM_B_LOOP_CONT, ARM_B_LOOP_BREAK);

const ARM_B: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, CanonicalControl<ArmBKind>>,
                0,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>,
        >,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, CanonicalControl<ArmBKind>>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>(),
    ARM_B_LOOP,
);

// Route with both arms (this is the key test - both arms have internal loops)
// Passive observers can distinguish arms by recv label (functional dispatch).
const ROUTE_PROGRAM: g::Program<
    <SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, CanonicalControl<ArmAKind>>,
                0,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, CanonicalControl<ArmBKind>>,
                    0,
                >,
                StepNil,
            >,
            LoopDecisionSteps<
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                StepNil,
                StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>,
            >,
        >,
    >>::Output,
> = g::route(ARM_A, ARM_B);

// Role projections
static CLIENT_PROGRAM: RoleProgram<
    'static,
    0,
    <<SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, CanonicalControl<ArmAKind>>,
                0,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, CanonicalControl<ArmBKind>>,
                    0,
                >,
                StepNil,
            >,
            LoopDecisionSteps<
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                StepNil,
                StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>,
            >,
        >,
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&ROUTE_PROGRAM);
static SERVER_PROGRAM: RoleProgram<
    'static,
    1,
    <<SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, CanonicalControl<ArmAKind>>,
                0,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, CanonicalControl<ArmBKind>>,
                    0,
                >,
                StepNil,
            >,
            LoopDecisionSteps<
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                StepNil,
                StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>,
            >,
        >,
    >>::Output as ProjectRole<Role<1>>>::Output,
> = project(&ROUTE_PROGRAM);

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

/// Test that program construction succeeds without scope ordinal collision.
/// Before the fix, this would panic at const eval time or during projection.
#[test]
fn route_with_internal_loops_compiles() {
    // If we get here, the programs compiled successfully
    let _ = CLIENT_PROGRAM.eff_list();
    let _ = SERVER_PROGRAM.eff_list();
}

/// Verify that scope budgets are reasonable (arms didn't collide).
#[test]
fn route_scope_budget_is_sane() {
    // The route itself has a scope budget
    let budget = CLIENT_PROGRAM.eff_list().scope_budget();
    // Each arm has a loop (scope budget ~1), plus the route scope itself
    // With disjoint allocation, we expect: 1 (route) + arm_a_budget + arm_b_budget
    // Arm A: marker (0) + loop (1) = 1
    // Arm B: marker (0) + loop (1) = 1
    // Total: 1 + 1 + 1 = 3 minimum
    assert!(budget >= 3, "scope budget {} is too small", budget);
}

/// Verify that the EffList contains the expected number of atoms.
#[test]
fn route_eff_list_structure() {
    let eff = CLIENT_PROGRAM.eff_list();
    // Each arm has: 1 marker + (1 loop_cont + 1 body) + (1 loop_break) = 4 atoms per arm
    // Total: 8 atoms minimum
    assert!(eff.len() >= 8, "eff list len {} is too small", eff.len());
}
