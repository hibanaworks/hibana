#![cfg(feature = "std")]

//! Tests for Target (passive observer) role projection in loop protocols.
//!
//! Verifies that:
//! - seek_label() can find all expected labels
//! - PassiveObserverBranch Jump nodes exist for passive observer navigation
//! - scope_region is correctly set for loop scopes

use hibana::g::{
    self, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, Msg, PhaseCursor, Role,
    steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil},
};
use hibana::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use hibana::control::cap::GenericCapToken;
use hibana::global::const_dsl::{DynamicMeta, HandlePlan, ScopeKind};
use hibana::global::typestate::JumpReason;
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

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

/// Verify Target can find the Handshake label via seek_label.
#[test]
fn target_can_seek_handshake_label() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    let found = cursor.seek_label(10);
    assert!(found.is_some(), "Target should find Handshake label 10");
    let found = found.unwrap();
    assert!(found.is_recv(), "Handshake should be a recv for Target");
}

/// Verify Target can find BodyMsg label inside the loop scope.
#[test]
fn target_can_seek_body_msg_label() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    let found = cursor.seek_label(7);
    assert!(found.is_some(), "Target should find BodyMsg label 7");
    let found = found.unwrap();
    assert!(found.is_recv(), "BodyMsg should be a recv for Target");
}

/// Verify Target can find ExitMsg label inside the loop scope.
#[test]
fn target_can_seek_exit_msg_label() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    let found = cursor.seek_label(8);
    assert!(found.is_some(), "Target should find ExitMsg label 8");
    let found = found.unwrap();
    assert!(found.is_send(), "ExitMsg should be a send for Target");
}

/// Verify the loop scope is marked as linger.
#[test]
fn target_loop_scope_is_linger() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    // Advance past handshake to enter loop scope
    let c1 = cursor.advance_for_test();
    let scope_region = c1.scope_region();
    assert!(scope_region.is_some(), "should have scope region inside loop");
    let region = scope_region.unwrap();
    assert!(region.linger, "loop scope should be linger");
    assert_eq!(region.kind, ScopeKind::Route, "loop scope kind should be Route");
}

/// Verify PassiveObserverBranch Jump nodes exist for passive observer.
#[test]
fn target_has_passive_observer_branch_jumps() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    let len = cursor.typestate_len();

    let mut found_passive_jump = false;
    for idx in 0..len {
        let node = cursor.typestate_node(idx);
        if node.action().is_jump() {
            if let Some(JumpReason::PassiveObserverBranch) = node.action().jump_reason() {
                found_passive_jump = true;
                break;
            }
        }
    }
    assert!(
        found_passive_jump,
        "Target typestate should have PassiveObserverBranch Jump nodes for passive observer navigation"
    );
}

/// Verify scope_region is accessible after advancing into the loop.
#[test]
fn target_scope_region_accessible_in_loop() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    // Advance past handshake to enter loop scope
    let c1 = cursor.advance_for_test();
    let scope_region = c1.scope_region();
    assert!(scope_region.is_some(), "scope_region should be accessible");
    let region = scope_region.unwrap();
    assert!(region.start < region.end, "scope region should be non-empty");
}

/// Verify passive_arm_entry is set for both arms.
#[test]
fn target_passive_arm_entry_both_arms() {
    let cursor = PhaseCursor::new(&TARGET_PROGRAM);
    // Advance past handshake to enter loop scope
    let c1 = cursor.advance_for_test();
    let scope_region = c1.scope_region();
    assert!(scope_region.is_some(), "should have scope region inside loop");
    let region = scope_region.unwrap();

    // Check passive_arm_entry for both arms
    let record = cursor.scope_entry_for_test(region.scope_id);
    assert!(record.is_some(), "scope record should exist");
    let record = record.unwrap();

    assert_ne!(
        record.passive_arm_entry[0], u16::MAX,
        "passive_arm_entry[0] should be set (arm 0 has Recv BodyMsg)"
    );
    assert_ne!(
        record.passive_arm_entry[1], u16::MAX,
        "passive_arm_entry[1] should be set (arm 1 has Send ExitMsg)"
    );
}
