use hibana::control::cap::{
    resource_kinds::{LoopBreakKind, LoopContinueKind},
    GenericCapToken,
};
use hibana::g::{
    self, CanonicalControl, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, Msg, Role,
    SendStep, StepCons, StepNil,
};
use hibana::global::const_dsl::{DynamicMeta, HandlePlan};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

type Controller = Role<0>;
type Target = Role<1>;

const DATA_LABEL: u8 = 7;
const LOOP_POLICY_ID: u16 = 910;
const LOOP_PLAN_META: DynamicMeta = DynamicMeta::new();

type BodySteps = StepCons<SendStep<Controller, Target, Msg<DATA_LABEL, ()>, 0>, StepNil>;
const BODY: g::Program<BodySteps> = g::send::<Controller, Target, Msg<DATA_LABEL, ()>, 0>();
const EXIT: g::Program<StepNil> = g::Program::empty();

type ContinueMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    CanonicalControl<LoopContinueKind>,
>;

type BreakMsg = Msg<
    { LABEL_LOOP_BREAK },
    GenericCapToken<LoopBreakKind>,
    CanonicalControl<LoopBreakKind>,
>;

// Self-send CanonicalControl: Controller → Controller (no Target param)
type LoopDecision = LoopDecisionSteps<
    Controller,
    ContinueMsg,
    BreakMsg,
    StepNil,
    BodySteps,
>;

// Route is local to Controller (0 → 0) for self-send loop control
const LOOP: g::Program<LoopDecision> = g::route::<0, _>(
    g::route_chain::<0, LoopContinueSteps<Controller, ContinueMsg, BodySteps>>(
        g::with_control_plan(
            g::send::<Controller, Controller, ContinueMsg, 0>(),
            HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
        )
        .then(BODY),
    )
    .and::<LoopBreakSteps<Controller, BreakMsg, StepNil>>(
        g::with_control_plan(
            g::send::<Controller, Controller, BreakMsg, 0>(),
            HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_PLAN_META),
        )
        .then(EXIT),
    ),
);

fn main() {
    let _ = LOOP;
}
