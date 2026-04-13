use hibana::g::{self, Msg, Role};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::advanced::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

const LOOP_POLICY_ID: u16 = 10;
const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;

type LoopContinueHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopBreakHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopContinueArmSteps = SeqSteps<LoopContinueHead, StepNil>;
type LoopProgramSteps = RouteSteps<LoopContinueArmSteps, LoopBreakHead>;
type OuterLoopContinueArmSteps = SeqSteps<LoopContinueArmSteps, LoopProgramSteps>;
type NestedLoopProgramSteps = RouteSteps<OuterLoopContinueArmSteps, LoopBreakHead>;

const LOOP_CONTINUE_ARM: g::Program<LoopContinueArmSteps> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<LOOP_POLICY_ID>(),
    StepNil::PROGRAM,
);
const LOOP_BREAK_ARM: g::Program<LoopBreakHead> = g::send::<
    Role<0>,
    Role<0>,
    Msg<
        { LABEL_LOOP_BREAK },
        GenericCapToken<LoopBreakKind>,
        CanonicalControl<LoopBreakKind>,
    >,
    0,
>()
.policy::<LOOP_POLICY_ID>();
const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);
const OUTER_LOOP_CONTINUE_ARM: g::Program<OuterLoopContinueArmSteps> =
    g::seq(LOOP_CONTINUE_ARM, LOOP_PROGRAM);

const NESTED_LOOP_PROGRAM: g::Program<NestedLoopProgramSteps> =
    g::route(OUTER_LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

const CONTROLLER: RoleProgram<'static, 0, NestedLoopProgramSteps> = project(&NESTED_LOOP_PROGRAM);

fn main() {
    let _ = CONTROLLER;
}
