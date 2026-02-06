use hibana::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use hibana::g::{
    self, CanonicalControl, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, Msg, PhaseCursor,
    Role,
    steps::{ProjectRole, SendStep, StepCons, StepNil},
};
use hibana::global::const_dsl::{DynamicMeta, HandlePlan, ScopeKind};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

// Roles
type Sender = Role<0>;
type Receiver = Role<1>;

// Messages
const DATA_LABEL: u8 = 1;
type DataMsg = Msg<DATA_LABEL, u32>;
type LoopContinueMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    hibana::control::cap::GenericCapToken<LoopContinueKind>,
    CanonicalControl<LoopContinueKind>,
>;
type LoopBreakMsg = Msg<
    { LABEL_LOOP_BREAK },
    hibana::control::cap::GenericCapToken<LoopBreakKind>,
    CanonicalControl<LoopBreakKind>,
>;

// Program construction (control-first loop semantics)
// LoopContinue/BreakMsg are self-send (Sender → Sender)
type BodySteps = StepCons<SendStep<Sender, Receiver, DataMsg>, StepNil>;
type LoopContinueArm = LoopContinueSteps<Sender, LoopContinueMsg, BodySteps>;
type LoopBreakArm = LoopBreakSteps<Sender, LoopBreakMsg, StepNil>;
type LoopDecision = LoopDecisionSteps<Sender, LoopContinueMsg, LoopBreakMsg, StepNil, BodySteps>;

const LOOP_BODY: g::Program<BodySteps> = g::send::<Sender, Receiver, DataMsg, 0>();
const LOOP_POLICY_ID: u16 = 120;
const LOOP_PLAN_META: DynamicMeta = DynamicMeta::new();
// Self-send for canonical control: Sender → Sender
const LOOP_BREAK_ARM: g::Program<LoopBreakArm> = g::with_control_plan(
    g::send::<Sender, Sender, LoopBreakMsg, 0>(),
    HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
)
.then(g::Program::empty());
const LOOP_CONTINUE_ARM: g::Program<LoopContinueArm> = g::with_control_plan(
    g::send::<Sender, Sender, LoopContinueMsg, 0>(),
    HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
)
.then(LOOP_BODY);
// Route is local to Sender (0 → 0)
const LOOP_DECISION: g::Program<LoopDecision> = g::route::<0, _>(
    g::route_chain::<0, LoopContinueArm>(LOOP_CONTINUE_ARM).and::<LoopBreakArm>(LOOP_BREAK_ARM),
);
const PROGRAM: g::Program<LoopDecision> = LOOP_DECISION;

type SenderLocal = <LoopDecision as ProjectRole<Sender>>::Output;
static SENDER_PROGRAM: g::RoleProgram<'static, 0, SenderLocal> =
    g::project::<0, LoopDecision, _>(&PROGRAM);

#[test]
fn loop_scope_is_route() {
    let cursor = PhaseCursor::new(&SENDER_PROGRAM);
    assert_eq!(cursor.scope_kind(), Some(ScopeKind::Route));
    let scope_id = cursor.scope_id().expect("route scope id available");
    assert_eq!(scope_id.kind(), ScopeKind::Route);
}

#[test]
fn continue_branch_exposes_body_then_rewinds() {
    let decision = PhaseCursor::new(&SENDER_PROGRAM);
    assert_eq!(decision.label(), Some(LABEL_LOOP_CONTINUE));

    let mut continue_cursor = decision
        .seek_label(LABEL_LOOP_CONTINUE)
        .expect("continue branch cursor");
    assert_eq!(continue_cursor.label(), Some(LABEL_LOOP_CONTINUE));

    continue_cursor = continue_cursor.advance_for_test();
    assert_eq!(continue_cursor.label(), Some(DATA_LABEL));

    let rewind = decision
        .seek_label(LABEL_LOOP_CONTINUE)
        .expect("rewind to continue");
    assert_eq!(rewind.label(), Some(LABEL_LOOP_CONTINUE));
}

#[test]
fn break_branch_skips_body() {
    let decision = PhaseCursor::new(&SENDER_PROGRAM);
    let mut break_cursor = decision
        .seek_label(LABEL_LOOP_BREAK)
        .expect("break branch cursor");
    assert_eq!(break_cursor.label(), Some(LABEL_LOOP_BREAK));

    break_cursor = break_cursor.advance_for_test();
    // After break we exit the loop entirely, so no DATA_LABEL send is available.
    assert_ne!(break_cursor.label(), Some(DATA_LABEL));
}

#[test]
fn role_projection_matches_expected_sequence() {
    let mut cursor = PhaseCursor::new(&SENDER_PROGRAM);
    assert_eq!(cursor.label(), Some(LABEL_LOOP_CONTINUE));

    cursor = cursor.advance_for_test();
    assert_eq!(cursor.label(), Some(DATA_LABEL));

    // After DATA, the next node is a LoopContinue Jump that loops back to start.
    // To reach LoopBreak, use seek_label since LoopContinue Jump loops back.
    cursor = cursor
        .seek_label(LABEL_LOOP_BREAK)
        .expect("LoopBreak should be reachable via seek_label");
    assert_eq!(cursor.label(), Some(LABEL_LOOP_BREAK));
}

#[test]
fn control_plan_records_loop_scope() {
    let plans: Vec<_> = SENDER_PROGRAM.control_plans().collect();
    assert!(
        plans.len() >= 2,
        "loop continue/break plans should be present"
    );
    for info in plans {
        let scope = info.scope_id;
        assert!(
            !scope.is_none(),
            "loop control plan should expose a scope id"
        );
        assert_eq!(scope.kind(), ScopeKind::Route, "loop scope kind matches");
    }
}
