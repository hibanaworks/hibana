use hibana::control::cap::GenericCapToken;
use hibana::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use hibana::g::{self, Msg, Role, RoleProgram};
use hibana::global::const_dsl::{DynamicMeta, HandlePlan};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

type Controller = Role<2>;
type ObserverA = Role<3>;
#[allow(dead_code)]
type ObserverB = Role<4>;

type TickMsg = Msg<1, ()>;
// Self-send CanonicalControl messages for loop decisions
type AckMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    g::CanonicalControl<LoopContinueKind>,
>;
type LossMsg =
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, g::CanonicalControl<LoopBreakKind>>;
type ContinueMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    g::CanonicalControl<LoopContinueKind>,
>;
type BreakMsg =
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, g::CanonicalControl<LoopBreakKind>>;

type TickSteps =
    g::steps::StepCons<g::steps::SendStep<Controller, ObserverA, TickMsg>, g::steps::StepNil>;
// AckBranch and LossBranch now use self-send for CanonicalControl
type AckBranch = g::steps::StepCons<
    g::steps::SendStep<Controller, Controller, AckMsg>,
    g::steps::StepCons<g::steps::SendStep<Controller, Controller, AckMsg>, g::steps::StepNil>,
>;
type LossBranch = g::steps::StepCons<
    g::steps::SendStep<Controller, Controller, LossMsg>,
    g::steps::StepCons<g::steps::SendStep<Controller, Controller, LossMsg>, g::steps::StepNil>,
>;
type AckLossRoute = <AckBranch as g::steps::StepConcat<LossBranch>>::Output;
type BodySteps = <TickSteps as g::steps::StepConcat<AckLossRoute>>::Output;
// Loop continue/break steps are now self-send (no Target param)
type ContinueArm = g::LoopContinueSteps<Controller, ContinueMsg, BodySteps>;
type BreakArm = g::LoopBreakSteps<Controller, BreakMsg>;
type Decision =
    g::LoopDecisionSteps<Controller, ContinueMsg, BreakMsg, g::steps::StepNil, BodySteps>;
type Steps = g::LoopSteps<BodySteps, Controller, ContinueMsg, BreakMsg, g::steps::StepNil>;

const TICK: g::Program<TickSteps> = g::send::<Controller, ObserverA, TickMsg, 0>();
// Self-send for CanonicalControl within route arms
const ACK_BRANCH: g::Program<AckBranch> = g::with_control_plan(
    g::send::<Controller, Controller, AckMsg, 0>(),
    HandlePlan::dynamic(10, DynamicMeta::new()),
)
.then(g::send::<Controller, Controller, AckMsg, 0>());
const LOSS_BRANCH: g::Program<LossBranch> = g::with_control_plan(
    g::send::<Controller, Controller, LossMsg, 0>(),
    HandlePlan::dynamic(10, DynamicMeta::new()),
)
.then(g::send::<Controller, Controller, LossMsg, 0>());
// Inner route is local to Controller (2 → 2)
const ACK_LOSS_ROUTE: g::Program<AckLossRoute> =
    g::route::<2, _>(g::route_chain::<2, AckBranch>(ACK_BRANCH).and::<LossBranch>(LOSS_BRANCH));
// Self-send for loop continue/break
const CONTINUE_ARM: g::Program<ContinueArm> = g::with_control_plan(
    g::send::<Controller, Controller, ContinueMsg, 0>(),
    HandlePlan::dynamic(11, DynamicMeta::new()),
)
.then(TICK)
.then(ACK_LOSS_ROUTE);
const BREAK_ARM: g::Program<BreakArm> = g::with_control_plan(
    g::send::<Controller, Controller, BreakMsg, 0>(),
    HandlePlan::dynamic(11, DynamicMeta::new()),
)
.then(g::Program::empty());
// Outer route is local to Controller (2 → 2)
const DECISION: g::Program<Decision> =
    g::route::<2, _>(g::route_chain::<2, ContinueArm>(CONTINUE_ARM).and::<BreakArm>(BREAK_ARM));
const PROGRAM: g::Program<Steps> = DECISION;

#[test]
fn nested_loop_scope_balanced() {
    let _role_program: RoleProgram<'static, 2, <Steps as g::steps::ProjectRole<Role<2>>>::Output> =
        g::project::<2, Steps, _>(&PROGRAM);

    type HandshakeSteps =
        g::steps::StepCons<g::steps::SendStep<Role<0>, Role<1>, Msg<10, ()>, 0>, g::steps::StepNil>;
    const HANDSHAKE: g::Program<HandshakeSteps> = g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>();
    type CombinedSteps = <HandshakeSteps as g::steps::StepConcat<Steps>>::Output;
    const COMBINED: g::Program<CombinedSteps> =
        g::par(g::par_chain::<HandshakeSteps>(HANDSHAKE).and::<Steps>(PROGRAM));
    let _transport_program: RoleProgram<
        'static,
        2,
        <CombinedSteps as g::steps::ProjectRole<Role<2>>>::Output,
    > = g::project::<2, CombinedSteps, _>(&COMBINED);
}
