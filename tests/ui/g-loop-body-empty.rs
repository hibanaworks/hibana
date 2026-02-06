use hibana::control::{
    cap::GenericCapToken,
    resource_kinds::{LoopBreakKind, LoopContinueKind},
};
use hibana::g::{
    self, CanonicalControl, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, LoopSteps,
    SendStep, StepCons, StepNil,
};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

type Controller = g::Role<0>;
type Target = g::Role<1>;

type BodySteps = StepNil;
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

const BODY: g::Program<BodySteps> = g::Program::empty();
const EXIT: g::Program<StepNil> = g::Program::empty();
const CONTINUE_ARM: g::Program<LoopContinueArm> = g::send::<
    Controller,
    Target,
    g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
>();
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

fn main() {
    let _ = LOOP;
}
