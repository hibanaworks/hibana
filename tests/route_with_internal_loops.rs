//! Regression test: Route arms with internal loops must have disjoint scope ordinals.
//!
//! This test verifies that `RouteChainBuilder` correctly assigns disjoint ordinal ranges
//! to each arm, preventing scope parent mismatch panics when multiple arms contain
//! internal loops or nested routes.
//!
//! Before the fix in `hibana/src/global/program.rs`, this would panic with:
//! "scope parent mismatch for ordinal"
//!
//! The fix: `RouteChainBuilder` now tracks a `scope_cursor` and gives each arm
//! a disjoint ordinal range, similar to how `ParChainBuilder` handles parallel lanes.

use hibana::control::cap::GenericCapToken;
use hibana::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use hibana::g::{self, steps::*, Msg, Role, RoleProgram};
use hibana::global::const_dsl::{DynamicMeta, HandlePlan};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

// Roles
type Client = Role<0>;
type Server = Role<1>;

// Route arm marker labels (custom, not loop labels)
const LABEL_ARM_A: u8 = 64;
const LABEL_ARM_B: u8 = 65;

// Route arm marker kinds
hibana::impl_control_resource!(ArmAKind, handle: RouteDecision, name: "ArmA", label: LABEL_ARM_A);
hibana::impl_control_resource!(ArmBKind, handle: RouteDecision, name: "ArmB", label: LABEL_ARM_B);

// Route arm marker messages (self-send, CanonicalControl)
type ArmAMsg = Msg<LABEL_ARM_A, GenericCapToken<ArmAKind>, g::CanonicalControl<ArmAKind>>;
type ArmBMsg = Msg<LABEL_ARM_B, GenericCapToken<ArmBKind>, g::CanonicalControl<ArmBKind>>;

// Loop control messages
type LoopContMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    g::CanonicalControl<LoopContinueKind>,
>;
type LoopBreakMsg = Msg<
    { LABEL_LOOP_BREAK },
    GenericCapToken<LoopBreakKind>,
    g::CanonicalControl<LoopBreakKind>,
>;

// Data messages - different labels so passive observers can dispatch by label
type DataMsgA = Msg<1, ()>;
type DataMsgB = Msg<2, ()>;

// -----------------------------------------------------------------------------
// Arm A: self-send marker + internal loop
// -----------------------------------------------------------------------------

type ArmAMarkerStep = StepCons<SendStep<Client, Client, ArmAMsg, 0>, StepNil>;

// Loop body in arm A
type ArmALoopBodySteps = StepCons<SendStep<Client, Server, DataMsgA, 0>, StepNil>;

// Loop decision in arm A
type ArmALoopContArm = g::LoopContinueSteps<Client, LoopContMsg, ArmALoopBodySteps>;
type ArmALoopBreakArm = g::LoopBreakSteps<Client, LoopBreakMsg>;
type ArmALoopDecision =
    g::LoopDecisionSteps<Client, LoopContMsg, LoopBreakMsg, StepNil, ArmALoopBodySteps>;

// Full arm A steps: marker + loop
type ArmASteps = <ArmAMarkerStep as StepConcat<ArmALoopDecision>>::Output;

// -----------------------------------------------------------------------------
// Arm B: self-send marker + internal loop (same structure, different arm)
// -----------------------------------------------------------------------------

type ArmBMarkerStep = StepCons<SendStep<Client, Client, ArmBMsg, 0>, StepNil>;

// Loop body in arm B
type ArmBLoopBodySteps = StepCons<SendStep<Client, Server, DataMsgB, 0>, StepNil>;

// Loop decision in arm B
type ArmBLoopContArm = g::LoopContinueSteps<Client, LoopContMsg, ArmBLoopBodySteps>;
type ArmBLoopBreakArm = g::LoopBreakSteps<Client, LoopBreakMsg>;
type ArmBLoopDecision =
    g::LoopDecisionSteps<Client, LoopContMsg, LoopBreakMsg, StepNil, ArmBLoopBodySteps>;

// Full arm B steps: marker + loop
type ArmBSteps = <ArmBMarkerStep as StepConcat<ArmBLoopDecision>>::Output;

// -----------------------------------------------------------------------------
// Route: arm A | arm B (both arms have internal loops)
// -----------------------------------------------------------------------------

type RouteSteps = <ArmASteps as StepConcat<ArmBSteps>>::Output;

// -----------------------------------------------------------------------------
// Programs
// -----------------------------------------------------------------------------

const ROUTE_POLICY_ID: u16 = 0x1000;

// Arm A: marker + loop
const ARM_A_LOOP_BODY: g::Program<ArmALoopBodySteps> = g::send::<Client, Server, DataMsgA, 0>();

const ARM_A_LOOP_CONT: g::Program<ArmALoopContArm> = g::with_control_plan(
    g::send::<Client, Client, LoopContMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID + 1, DynamicMeta::new()),
)
.then(ARM_A_LOOP_BODY);

const ARM_A_LOOP_BREAK: g::Program<ArmALoopBreakArm> = g::with_control_plan(
    g::send::<Client, Client, LoopBreakMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID + 1, DynamicMeta::new()),
);

const ARM_A_LOOP: g::Program<ArmALoopDecision> = g::route::<0, _>(
    g::route_chain::<0, ArmALoopContArm>(ARM_A_LOOP_CONT).and::<ArmALoopBreakArm>(ARM_A_LOOP_BREAK),
);

const ARM_A: g::Program<ArmASteps> = g::with_control_plan(
    g::send::<Client, Client, ArmAMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, DynamicMeta::new()),
)
.then(ARM_A_LOOP);

// Arm B: marker + loop
const ARM_B_LOOP_BODY: g::Program<ArmBLoopBodySteps> = g::send::<Client, Server, DataMsgB, 0>();

const ARM_B_LOOP_CONT: g::Program<ArmBLoopContArm> = g::with_control_plan(
    g::send::<Client, Client, LoopContMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID + 2, DynamicMeta::new()),
)
.then(ARM_B_LOOP_BODY);

const ARM_B_LOOP_BREAK: g::Program<ArmBLoopBreakArm> = g::with_control_plan(
    g::send::<Client, Client, LoopBreakMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID + 2, DynamicMeta::new()),
);

const ARM_B_LOOP: g::Program<ArmBLoopDecision> = g::route::<0, _>(
    g::route_chain::<0, ArmBLoopContArm>(ARM_B_LOOP_CONT).and::<ArmBLoopBreakArm>(ARM_B_LOOP_BREAK),
);

const ARM_B: g::Program<ArmBSteps> = g::with_control_plan(
    g::send::<Client, Client, ArmBMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, DynamicMeta::new()),
)
.then(ARM_B_LOOP);

// Route with both arms (this is the key test - both arms have internal loops)
// Passive observers can distinguish arms by recv label (functional dispatch).
const ROUTE_PROGRAM: g::Program<RouteSteps> = g::route::<0, _>(
    g::route_chain::<0, ArmASteps>(ARM_A).and::<ArmBSteps>(ARM_B),
);

// Role projections
type ClientRouteSteps = <RouteSteps as ProjectRole<Client>>::Output;
type ServerRouteSteps = <RouteSteps as ProjectRole<Server>>::Output;

static CLIENT_PROGRAM: RoleProgram<'static, 0, ClientRouteSteps> =
    g::project::<0, RouteSteps, _>(&ROUTE_PROGRAM);
static SERVER_PROGRAM: RoleProgram<'static, 1, ServerRouteSteps> =
    g::project::<1, RouteSteps, _>(&ROUTE_PROGRAM);

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

/// Test that program construction succeeds without scope ordinal collision.
/// Before the fix, this would panic at const eval time or during projection.
#[test]
fn route_with_internal_loops_compiles() {
    // If we get here, the programs compiled successfully
    let _ = &*CLIENT_PROGRAM;
    let _ = &*SERVER_PROGRAM;
}

/// Verify that scope budgets are reasonable (arms didn't collide).
#[test]
fn route_scope_budget_is_sane() {
    // The route itself has a scope budget
    let budget = ROUTE_PROGRAM.scope_budget();
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
    let eff = ROUTE_PROGRAM.eff_list();
    // Each arm has: 1 marker + (1 loop_cont + 1 body) + (1 loop_break) = 4 atoms per arm
    // Total: 8 atoms minimum
    assert!(eff.len() >= 8, "eff list len {} is too small", eff.len());
}
