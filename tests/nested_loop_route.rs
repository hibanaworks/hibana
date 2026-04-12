use hibana::g::advanced::steps::{
    ParSteps, PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil,
};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::MintConfig;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;
type AckPolicyHead = PolicySteps<
    StepCons<
        SendStep<
            Role<2>,
            Role<2>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    10,
>;
type LossPolicyHead = PolicySteps<
    StepCons<
        SendStep<
            Role<2>,
            Role<2>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
    10,
>;
type AckBranchSteps = SeqSteps<
    AckPolicyHead,
    StepCons<
        SendStep<
            Role<2>,
            Role<2>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
>;
type LossBranchSteps = SeqSteps<
    LossPolicyHead,
    StepCons<
        SendStep<
            Role<2>,
            Role<2>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
>;
type AckLossRouteSteps = RouteSteps<AckBranchSteps, LossBranchSteps>;
type ContinuePolicyHead = PolicySteps<
    StepCons<
        SendStep<
            Role<2>,
            Role<2>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    11,
>;
type BreakPolicyHead = PolicySteps<
    StepCons<
        SendStep<
            Role<2>,
            Role<2>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
    11,
>;
type ContinueArmSteps = SeqSteps<
    ContinuePolicyHead,
    SeqSteps<StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>, AckLossRouteSteps>,
>;
type DecisionSteps = RouteSteps<ContinueArmSteps, BreakPolicyHead>;

const TICK: g::Program<StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>> =
    g::send::<Role<2>, Role<3>, Msg<1, ()>, 0>();
// Self-send for CanonicalControl within route arms
const ACK_BRANCH: g::Program<AckBranchSteps> = g::seq(
    g::send::<
        Role<2>,
        Role<2>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<10>(),
    g::send::<
        Role<2>,
        Role<2>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >(),
);
const LOSS_BRANCH: g::Program<LossBranchSteps> = g::seq(
    g::send::<
        Role<2>,
        Role<2>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        0,
    >()
    .policy::<10>(),
    g::send::<
        Role<2>,
        Role<2>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        0,
    >(),
);
// Inner route is local to Controller (2 → 2)
const ACK_LOSS_ROUTE: g::Program<AckLossRouteSteps> = g::route(ACK_BRANCH, LOSS_BRANCH);
// Self-send for loop continue/break
const CONTINUE_ARM: g::Program<ContinueArmSteps> = g::seq(
    g::send::<
        Role<2>,
        Role<2>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<11>(),
    g::seq(TICK, ACK_LOSS_ROUTE),
);
const BREAK_ARM: g::Program<BreakPolicyHead> = g::send::<
    Role<2>,
    Role<2>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<11>();
// Outer route is local to Controller (2 → 2)
const DECISION: g::Program<DecisionSteps> = g::route(CONTINUE_ARM, BREAK_ARM);

#[test]
fn nested_loop_scope_balanced() {
    let _role_program: RoleProgram<'_, 2, _, MintConfig> = project(&DECISION);

    const HANDSHAKE: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>> =
        g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>();
    const COMBINED: g::Program<
        ParSteps<StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>, DecisionSteps>,
    > = g::par(HANDSHAKE, DECISION);
    let _transport_program: RoleProgram<'_, 2, _, MintConfig> = project(&COMBINED);
}
