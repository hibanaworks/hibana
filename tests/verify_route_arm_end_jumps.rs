#![cfg(feature = "std")]

//! Verify RouteArmEnd Jump generation for non-linger Route scopes.
//!
//! CFG-pure design: arm 0 ends with RouteArmEnd Jump → scope_end.
//! This eliminates sequential layout (arm 0 fall-through to arm 1) and
//! removes runtime arm repositioning logic from flow().
//!
//! For non-linger routes (controller view), the CFG is:
//! - arm 0 nodes → RouteArmEnd Jump → scope_end
//! - arm 1 nodes → scope_end (sequential, no Jump needed for final arm)

use hibana::control::cap::GenericCapToken;
use hibana::g::steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil};
use hibana::g::{self, Msg, PhaseCursor, Role};
use hibana::global::const_dsl::{DynamicMeta, HandlePlan, ScopeKind};
use hibana::global::typestate::JumpReason;

type Sender = Role<0>;
type Receiver = Role<1>;

const ROUTE_POLICY_ID: u16 = 500;
const ROUTE_META: DynamicMeta = DynamicMeta::new();

// Custom resource kinds with unique labels for each arm
hibana::impl_control_resource!(
    ArmADecisionKind,
    handle: RouteDecision,
    name: "ArmADecision",
    label: 50,
);
hibana::impl_control_resource!(
    ArmBDecisionKind,
    handle: RouteDecision,
    name: "ArmBDecision",
    label: 51,
);

// Route decision messages - self-send for controller with DIFFERENT labels
type ArmADecisionMsg = Msg<
    50,
    GenericCapToken<ArmADecisionKind>,
    g::CanonicalControl<ArmADecisionKind>,
>;
type ArmBDecisionMsg = Msg<
    51,
    GenericCapToken<ArmBDecisionKind>,
    g::CanonicalControl<ArmBDecisionKind>,
>;

// Data messages
type DataA = Msg<10, u32>;
type DataB = Msg<20, u32>;

// Arm definitions with self-send decision
type ArmASteps = StepCons<
    SendStep<Sender, Sender, ArmADecisionMsg>,
    StepCons<SendStep<Sender, Receiver, DataA>, StepNil>,
>;
type ArmBSteps = StepCons<
    SendStep<Sender, Sender, ArmBDecisionMsg>,
    StepCons<SendStep<Sender, Receiver, DataB>, StepNil>,
>;

// 2-arm route (NOT linger since no LoopContinue/LoopBreak)
type RouteSteps = <ArmASteps as StepConcat<ArmBSteps>>::Output;

// Build the program
const ARM_A: g::Program<ArmASteps> = g::with_control_plan(
    g::send::<Sender, Sender, ArmADecisionMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_META),
)
.then(g::send::<Sender, Receiver, DataA, 0>());

const ARM_B: g::Program<ArmBSteps> = g::with_control_plan(
    g::send::<Sender, Sender, ArmBDecisionMsg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_META),
)
.then(g::send::<Sender, Receiver, DataB, 0>());

const ROUTE_PROGRAM: g::Program<RouteSteps> =
    g::route::<0, _>(g::route_chain::<0, ArmASteps>(ARM_A).and::<ArmBSteps>(ARM_B));

type SenderLocal = <RouteSteps as ProjectRole<Sender>>::Output;

static SENDER_PROGRAM: g::RoleProgram<'static, 0, SenderLocal> =
    g::project::<0, RouteSteps, _>(&ROUTE_PROGRAM);

/// Verify non-linger Route scope structure.
#[test]
fn route_is_not_linger() {
    let cursor = PhaseCursor::new(&SENDER_PROGRAM);
    let scope_region = cursor.scope_region().expect("should have scope region");
    assert!(
        !scope_region.linger,
        "2-arm route without LoopContinue/LoopBreak should NOT be linger"
    );
    assert_eq!(cursor.scope_kind(), Some(ScopeKind::Route));
}

/// Verify arm 0 ends with RouteArmEnd Jump → scope_end.
///
/// CFG-pure design: arm 0 explicitly exits to scope_end via RouteArmEnd Jump,
/// NOT fall-through to arm 1. This eliminates runtime arm repositioning.
///
/// For non-linger routes (controller view):
/// - arm 0's last action node → RouteArmEnd Jump → scope_end
/// - arm 1's last node → scope_end (sequential, final arm doesn't need Jump)
#[test]
fn arm0_ends_with_route_arm_end_jump() {
    let cursor = PhaseCursor::new(&SENDER_PROGRAM);
    let scope_region = cursor.scope_region().expect("scope region");

    // Find RouteArmEnd Jump node in the scope
    let mut route_arm_end_jump_idx = None;
    for idx in scope_region.start..scope_region.end {
        let node = cursor.typestate_node(idx);
        if node.action().jump_reason() == Some(JumpReason::RouteArmEnd) {
            route_arm_end_jump_idx = Some(idx);
            break;
        }
    }

    let jump_idx = route_arm_end_jump_idx.expect("should find RouteArmEnd Jump node");
    let jump_node = cursor.typestate_node(jump_idx);

    // RouteArmEnd Jump should target scope_end
    assert_eq!(
        jump_node.next() as usize,
        scope_region.end,
        "RouteArmEnd Jump should target scope_end"
    );

    // Verify it's for arm 0
    assert_eq!(
        jump_node.route_arm(),
        Some(0),
        "RouteArmEnd Jump should be for arm 0"
    );
}

/// Verify final arm's last node flows to Terminate (sequential flow).
#[test]
fn final_arm_last_node_flows_to_terminate() {
    let cursor = PhaseCursor::new(&SENDER_PROGRAM);
    let scope_region = cursor.scope_region().expect("scope region");

    // Find arm 1's last node (DataB send)
    let mut arm1_last_idx = None;
    for idx in scope_region.start..scope_region.end {
        let node = cursor.typestate_node(idx);
        if node.route_arm() == Some(1) {
            arm1_last_idx = Some(idx);
        }
    }

    let arm1_last_idx = arm1_last_idx.expect("should find arm 1 nodes");
    let arm1_last_node = cursor.typestate_node(arm1_last_idx);

    // Final arm should flow naturally to scope_end (which is Terminate)
    assert_eq!(
        arm1_last_node.next() as usize,
        scope_region.end,
        "Final arm's last node should have next=scope_end (sequential flow)"
    );
}

/// Verify the typestate structure has expected properties.
#[test]
fn typestate_structure_is_valid() {
    let cursor = PhaseCursor::new(&SENDER_PROGRAM);
    let len = cursor.typestate_len();

    // Typestate should have at least 4 nodes:
    // - 2 decision nodes (arm A and B self-sends)
    // - 2 data nodes (DataA and DataB sends)
    // - potentially Jump nodes and Terminate
    assert!(len >= 4, "typestate should have at least 4 nodes, got {}", len);

    // Verify scope region exists
    let region = cursor.scope_region().expect("should have scope region");
    assert!(region.start < region.end, "scope region should be non-empty");
    assert!(!region.linger, "non-linger route expected");

    // Verify each arm has nodes with correct route_arm
    let mut arm0_count = 0;
    let mut arm1_count = 0;
    for idx in region.start..region.end {
        let node = cursor.typestate_node(idx);
        match node.route_arm() {
            Some(0) => arm0_count += 1,
            Some(1) => arm1_count += 1,
            _ => {}
        }
    }
    assert!(arm0_count > 0, "arm 0 should have at least one node");
    assert!(arm1_count > 0, "arm 1 should have at least one node");
}
