use hibana::control::cap::{
    resource_kinds::{LoopBreakKind, LoopContinueKind},
    GenericCapToken,
};
use hibana::g::{
    self, CanonicalControl, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, LoopSteps,
    SendStep, StepCons, StepNil,
};
use hibana::runtime::consts::LABEL_LOOP_BREAK;

type Controller = g::Role<0>;
type Target = g::Role<1>;

fn main() {
    type BodySteps = LoopContinueSteps<Controller, Target, g::Msg<7, ()>>;
    const BODY: g::Program<BodySteps> = g::send::<Controller, Target, g::Msg<7, ()>>();
    const EXIT: g::Program<StepNil> = g::Program::empty();
    const CONTINUE_ARM: g::Program<
        LoopContinueSteps<
            Controller,
            Target,
            g::Msg<
                99,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
    > = g::send::<Controller, Target, g::Msg<
        99,
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
    const BREAK_ARM: g::Program<
        LoopBreakSteps<
            Controller,
            Target,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        >,
    > = g::seq(BREAK_BASE, EXIT);
    const DECISION: g::Program<
        LoopDecisionSteps<
            Controller,
            Target,
            g::Msg<
                99,
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
    > = g::route::<0, 1, _>(
        g::route_chain::<0, LoopContinueSteps<
            Controller,
            Target,
            g::Msg<
                99,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >>(CONTINUE_ARM)
            .and::<LoopBreakSteps<
                Controller,
                Target,
                g::Msg<
                    LABEL_LOOP_BREAK,
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                StepNil,
            >>(BREAK_ARM),
    );

    const _INVALID: g::Program<
        LoopSteps<
            BodySteps,
            Controller,
            Target,
            g::Msg<
                99,
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
}
