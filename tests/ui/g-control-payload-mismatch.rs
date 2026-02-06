use hibana::g::{
    self, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, LoopSteps, SendStep, StepCons,
    StepNil,
};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

type Controller = g::Role<0>;
type Target = g::Role<1>;

type BodySteps = LoopContinueSteps<Controller, Target, g::Msg<7, ()>>;
type LoopContinueArm = LoopContinueSteps<Controller, Target, g::Msg<LABEL_LOOP_CONTINUE, ()>>;
type LoopBreakArm =
    LoopBreakSteps<Controller, Target, g::Msg<LABEL_LOOP_BREAK, ()>, StepNil>;
type LoopDecision = LoopDecisionSteps<
    Controller,
    Target,
    g::Msg<LABEL_LOOP_CONTINUE, ()>,
    g::Msg<LABEL_LOOP_BREAK, ()>,
    StepNil,
>;

const BODY: g::Program<BodySteps> = g::send::<Controller, Target, g::Msg<7, ()>>();
const EXIT: g::Program<StepNil> = g::Program::empty();
const CONTINUE_ARM: g::Program<LoopContinueArm> =
    g::send::<Controller, Target, g::Msg<LABEL_LOOP_CONTINUE, ()>>();
const BREAK_BASE: g::Program<
    StepCons<
        SendStep<Controller, Target, g::Msg<LABEL_LOOP_BREAK, ()>>,
        StepNil,
    >,
> = g::send::<Controller, Target, g::Msg<LABEL_LOOP_BREAK, ()>>();
const BREAK_ARM: g::Program<LoopBreakArm> = g::seq(BREAK_BASE, EXIT);
const DECISION: g::Program<LoopDecision> = g::route::<0, 1, _>(
    g::route_chain::<0, LoopContinueArm>(CONTINUE_ARM)
        .and::<LoopBreakArm>(BREAK_ARM),
);

const _: g::Program<
    LoopSteps<
        BodySteps,
        Controller,
        Target,
        g::Msg<LABEL_LOOP_CONTINUE, ()>,
        g::Msg<LABEL_LOOP_BREAK, ()>,
        StepNil,
    >,
> = g::seq(DECISION, BODY);

fn main() {}
