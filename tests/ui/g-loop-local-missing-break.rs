use hibana::control::cap::{
    resource_kinds::{LoopBreakKind, LoopContinueKind},
    GenericCapToken,
};
use hibana::g::{
    self, CanonicalControl, LocalSend, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps,
    LoopSteps, SendStep, StepCons, StepNil,
};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

type Controller = g::Role<0>;
type Target = g::Role<1>;

type BodySteps = LoopContinueSteps<Controller, Target, g::Msg<7, ()>>;
type LoopContinueArm = LoopContinueSteps<
    Controller,
    Target,
    g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
>;
type LoopBreakArm = LoopBreakSteps<
    Controller,
    Target,
    g::Msg<
        LABEL_LOOP_BREAK,
        GenericCapToken<LoopBreakKind>,
        CanonicalControl<LoopBreakKind>,
    >,
    StepNil,
>;
type LoopDecision = LoopDecisionSteps<
    Controller,
    Target,
    g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
    g::Msg<
        LABEL_LOOP_BREAK,
        GenericCapToken<LoopBreakKind>,
        CanonicalControl<LoopBreakKind>,
    >,
    StepNil,
>;

const BODY: g::Program<BodySteps> = g::send::<Controller, Target, g::Msg<7, ()>>();
const EXIT: g::Program<StepNil> = g::Program::empty();
const CONTINUE_ARM: g::Program<LoopContinueArm> =
    g::send::<Controller, Target, g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >>();
const BREAK_BASE: g::Program<
    StepCons<
        SendStep<
            Controller,
            Target,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
> = g::send::<Controller, Target, g::Msg<
    LABEL_LOOP_BREAK,
    GenericCapToken<LoopBreakKind>,
    CanonicalControl<LoopBreakKind>,
>>();
const BREAK_ARM: g::Program<LoopBreakArm> = g::seq(BREAK_BASE, EXIT);
const DECISION: g::Program<LoopDecision> = g::route::<0, 1, _>(
    g::route_chain::<0, LoopContinueArm>(CONTINUE_ARM)
        .and::<LoopBreakArm>(BREAK_ARM),
);

const LOOP: g::Program<
    LoopSteps<
        BodySteps,
        Controller,
        Target,
        g::Msg<
            LABEL_LOOP_CONTINUE,
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        g::Msg<
            LABEL_LOOP_BREAK,
            GenericCapToken<LoopBreakKind>,
            CanonicalControl<LoopBreakKind>,
        >,
        StepNil,
    >,
> = g::seq(DECISION, BODY);

// Forgetting to account for the loop break arm must fail: the actual projection
// includes both continue and break transitions.
type WrongControllerLocal = StepCons<
    LocalSend<Target, g::Msg<7, ()>>,
    StepCons<
        LocalSend<
            Target,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
>;

const CONTROLLER: g::RoleProgram<'static, 0, WrongControllerLocal> =
    g::project::<0, _, _>(&LOOP);

fn main() {
    let _ = CONTROLLER;
}
