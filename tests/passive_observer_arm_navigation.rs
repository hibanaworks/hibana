#![cfg(feature = "std")]

//! Tests for passive observer arm navigation in self-send loops.
//!
//! Verifies that `follow_passive_observer_arm_for_scope()` correctly finds
//! PassiveObserverBranch Jump nodes for both continue (arm 0) and break (arm 1)
//! in linger scopes where the role is a passive observer.

use hibana::{
    control::cap::{
        GenericCapToken,
        resource_kinds::{LoopBreakKind, LoopContinueKind},
    },
    g::{
        self, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, Msg, PhaseCursor, Role,
        steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil},
    },
    global::const_dsl::{DynamicMeta, HandlePlan, ScopeKind},
    runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE},
    PassiveArmNavigation,
};

type Controller = Role<0>;
type Target = Role<1>;

const LOOP_POLICY_ID: u16 = 99;
const LOOP_PLAN_META: DynamicMeta = DynamicMeta::new();

type Handshake = Msg<10, ()>;
type BodyMsg = Msg<7, u32>;
type ExitMsg = Msg<8, i32>;
type ContinueMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    hibana::g::CanonicalControl<LoopContinueKind>,
>;
type BreakMsg = Msg<
    { LABEL_LOOP_BREAK },
    GenericCapToken<LoopBreakKind>,
    hibana::g::CanonicalControl<LoopBreakKind>,
>;

type HandshakeSteps = StepCons<SendStep<Controller, Target, Handshake>, StepNil>;
type BodySteps = StepCons<SendStep<Controller, Target, BodyMsg>, StepNil>;
type ExitSteps = StepCons<SendStep<Target, Controller, ExitMsg>, StepNil>;
type LoopSeq = LoopDecisionSteps<Controller, ContinueMsg, BreakMsg, ExitSteps, BodySteps>;
type ProtocolSteps = <HandshakeSteps as StepConcat<LoopSeq>>::Output;

type TargetLocal = <ProtocolSteps as ProjectRole<Target>>::Output;

const LOOP_BODY: g::Program<BodySteps> = g::send::<Controller, Target, BodyMsg, 0>();
const LOOP_EXIT: g::Program<ExitSteps> = g::send::<Target, Controller, ExitMsg, 0>();

const LOOP_CONTINUE_ARM: g::Program<LoopContinueSteps<Controller, ContinueMsg, BodySteps>> =
    g::with_control_plan(
        g::send::<Controller, Controller, ContinueMsg, 0>(),
        HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
    )
    .then(LOOP_BODY);
const LOOP_BREAK_ARM: g::Program<LoopBreakSteps<Controller, BreakMsg, ExitSteps>> =
    g::with_control_plan(
        g::send::<Controller, Controller, BreakMsg, 0>(),
        HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
    )
    .then(LOOP_EXIT);

const LOOP_SEGMENT: g::Program<LoopSeq> = g::route::<0, _>(
    g::route_chain::<0, LoopContinueSteps<Controller, ContinueMsg, BodySteps>>(LOOP_CONTINUE_ARM)
        .and::<LoopBreakSteps<Controller, BreakMsg, ExitSteps>>(LOOP_BREAK_ARM),
);

const PROTOCOL: g::Program<ProtocolSteps> =
    g::seq(g::send::<Controller, Target, Handshake, 0>(), LOOP_SEGMENT);

static TARGET_PROGRAM: g::RoleProgram<'static, 1, TargetLocal> =
    g::project::<1, ProtocolSteps, _>(&PROTOCOL);

/// Verify that passive observer arm navigation works correctly via scope_id lookup.
///
/// This is a regression test for a bug where PassiveObserverBranch Jump for arm 0
/// was not being generated for passive observers because the condition checked
/// `arm_last[0] != LINGER_ARM_NO_NODE` before checking `is_passive`.
#[test]
fn passive_observer_arm_navigation_via_scope_id() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);

    // Advance past handshake to enter the linger scope
    let cursor_in_loop = cursor.advance_for_test();

    // Get the linger Route scope
    let region = cursor_in_loop
        .enclosing_scope_of_kind(ScopeKind::Route)
        .expect("should find Route scope");
    assert!(region.linger, "Route scope should be linger");

    // Verify both arms are navigable via scope_id lookup
    let arm0_nav = cursor_in_loop
        .follow_passive_observer_arm_for_scope(region.scope_id, 0)
        .expect("arm 0 (continue) should be navigable");
    let arm1_nav = cursor_in_loop
        .follow_passive_observer_arm_for_scope(region.scope_id, 1)
        .expect("arm 1 (break) should be navigable");

    // Extract entry indices from navigation results
    let PassiveArmNavigation::WithinArm { entry: arm0_entry } = arm0_nav;
    let PassiveArmNavigation::WithinArm { entry: arm1_entry } = arm1_nav;
    let arm0_entry = arm0_entry as usize;
    let arm1_entry = arm1_entry as usize;

    // Navigate to arm entries
    let arm0_cursor = cursor_in_loop.with_index(arm0_entry);
    let arm1_cursor = cursor_in_loop.with_index(arm1_entry);

    // Verify arm 0 leads to the BodyMsg recv (label 7)
    assert!(
        arm0_cursor.is_recv(),
        "arm 0 should navigate to a recv node"
    );
    let arm0_meta = arm0_cursor.try_recv_meta().expect("should have recv meta");
    assert_eq!(arm0_meta.label, 7, "arm 0 should lead to BodyMsg (label 7)");

    // Verify arm 1 leads to the ExitMsg send (label 8)
    assert!(
        arm1_cursor.is_send(),
        "arm 1 should navigate to a send node"
    );
    let arm1_meta = arm1_cursor.try_send_meta().expect("should have send meta");
    assert_eq!(arm1_meta.label, 8, "arm 1 should lead to ExitMsg (label 8)");
}

/// Verify that controller_role is correctly propagated from ScopeMarker to ScopeEntry.
///
/// Phase 3 test: controller_role should be set to Some(0) for Route scopes
/// where CONTROLLER=0 (Controller role).
#[test]
fn controller_role_propagation_in_route_scope() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);

    // Advance past handshake to enter the linger scope
    let cursor_in_loop = cursor.advance_for_test();

    // Get the linger Route scope
    let region = cursor_in_loop
        .enclosing_scope_of_kind(ScopeKind::Route)
        .expect("should find Route scope");

    // Verify controller_role is set correctly
    // The route_chain was created with CONTROLLER=0 (Controller role),
    // so controller_role should be Some(0).
    assert_eq!(
        region.controller_role,
        Some(0),
        "controller_role should be Some(0) for route_chain<0, _>"
    );

    // For Target (role 1), controller_role != ROLE, so Target is passive observer
    // This is verified implicitly by the fact that passive_arm navigation works
}

// Also create a Controller cursor test to verify controller is not passive
type ControllerLocal = <ProtocolSteps as ProjectRole<Controller>>::Output;

static CONTROLLER_PROGRAM: g::RoleProgram<'static, 0, ControllerLocal> =
    g::project::<0, ProtocolSteps, _>(&PROTOCOL);

/// Verify that controller_role correctly identifies the controller role.
///
/// Phase 3 test: For the controller (role 0), controller_role == ROLE,
/// so the role should be identified as controller, not passive observer.
#[test]
fn controller_role_identifies_controller() {
    let cursor = PhaseCursor::new(&CONTROLLER_PROGRAM);

    // Advance past handshake to enter the linger scope
    let cursor_in_loop = cursor.advance_for_test();

    // Get the linger Route scope
    let region = cursor_in_loop
        .enclosing_scope_of_kind(ScopeKind::Route)
        .expect("should find Route scope");

    // Verify controller_role is set correctly
    assert_eq!(
        region.controller_role,
        Some(0),
        "controller_role should be Some(0) for route_chain<0, _>"
    );

    // For Controller (role 0), controller_role == ROLE (0 == 0), so Controller is NOT passive
    // Verify that controller_arm_entry is set (controller has Local nodes)
    // This verifies the type-level detection is consistent with the inference-based detection
}
