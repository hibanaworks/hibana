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

#[path = "support/route_control_kinds.rs"]
mod route_control_kinds;

use hibana::g::advanced::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
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
type ArmAKind = route_control_kinds::RouteControl<LABEL_ARM_A, 0>;
type ArmBKind = route_control_kinds::RouteControl<LABEL_ARM_B, 0>;

// -----------------------------------------------------------------------------
// Programs
// -----------------------------------------------------------------------------

const ROUTE_POLICY_ID: u16 = 0x1000;
type ArmALoopContinueHead = PolicySteps<
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
    { ROUTE_POLICY_ID + 1 },
>;
type ArmALoopBreakHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >,
        StepNil,
    >,
    { ROUTE_POLICY_ID + 1 },
>;
type ArmALoopSteps = RouteSteps<
    SeqSteps<ArmALoopContinueHead, StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>>,
    ArmALoopBreakHead,
>;
type ArmAMarkerHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, CanonicalControl<ArmAKind>>,
            0,
        >,
        StepNil,
    >,
    ROUTE_POLICY_ID,
>;
type RouteArmASteps = SeqSteps<ArmAMarkerHead, ArmALoopSteps>;
type ArmBLoopContinueHead = PolicySteps<
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
    { ROUTE_POLICY_ID + 2 },
>;
type ArmBLoopBreakHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >,
        StepNil,
    >,
    { ROUTE_POLICY_ID + 2 },
>;
type ArmBLoopSteps = RouteSteps<
    SeqSteps<ArmBLoopContinueHead, StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>>,
    ArmBLoopBreakHead,
>;
type ArmBMarkerHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, CanonicalControl<ArmBKind>>,
            0,
        >,
        StepNil,
    >,
    ROUTE_POLICY_ID,
>;
type RouteArmBSteps = SeqSteps<ArmBMarkerHead, ArmBLoopSteps>;
type RouteProgramSteps = RouteSteps<RouteArmASteps, RouteArmBSteps>;

// Arm A: marker + loop
const ARM_A_LOOP_BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>();

const ARM_A_LOOP_CONT: g::Program<
    SeqSteps<ArmALoopContinueHead, StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>>,
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

const ARM_A_LOOP_BREAK: g::Program<ArmALoopBreakHead> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<{ ROUTE_POLICY_ID + 1 }>();

const ARM_A_LOOP: g::Program<ArmALoopSteps> = g::route(ARM_A_LOOP_CONT, ARM_A_LOOP_BREAK);

const ARM_A: g::Program<RouteArmASteps> = g::seq(
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
    SeqSteps<ArmBLoopContinueHead, StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>>,
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

const ARM_B_LOOP_BREAK: g::Program<ArmBLoopBreakHead> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<{ ROUTE_POLICY_ID + 2 }>();

const ARM_B_LOOP: g::Program<ArmBLoopSteps> = g::route(ARM_B_LOOP_CONT, ARM_B_LOOP_BREAK);

const ARM_B: g::Program<RouteArmBSteps> = g::seq(
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
const ROUTE_PROGRAM: g::Program<RouteProgramSteps> = g::route(ARM_A, ARM_B);

// Role projections
static CLIENT_PROGRAM: RoleProgram<'static, 0, RouteProgramSteps> = project(&ROUTE_PROGRAM);
static SERVER_PROGRAM: RoleProgram<'static, 1, RouteProgramSteps> = project(&ROUTE_PROGRAM);

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

/// Test that program construction succeeds without scope ordinal collision.
/// Before the fix, this would panic at const eval time or during projection.
#[test]
fn route_with_internal_loops_compiles() {
    let _ = &CLIENT_PROGRAM;
    let _ = &SERVER_PROGRAM;
}
