use hibana::g::advanced::steps::{
    LoopDecisionSteps, ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil,
};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::MintConfig;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;

const TICK: g::Program<StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>> =
    g::send::<Role<2>, Role<3>, Msg<1, ()>, 0>();
// Self-send for CanonicalControl within route arms
const ACK_BRANCH: g::Program<
    SeqSteps<
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
    >,
> = g::seq(
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
const LOSS_BRANCH: g::Program<
    SeqSteps<
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
    >,
> = g::seq(
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
const ACK_LOSS_ROUTE: g::Program<
    <SeqSteps<
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
    > as StepConcat<
        SeqSteps<
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
        >,
    >>::Output,
> = g::route(ACK_BRANCH, LOSS_BRANCH);
// Self-send for loop continue/break
const CONTINUE_ARM: g::Program<
    SeqSteps<
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
        SeqSteps<
            StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>,
            <SeqSteps<
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
            > as StepConcat<
                SeqSteps<
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
                >,
            >>::Output,
        >,
    >,
> = g::seq(
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
const BREAK_ARM: g::Program<
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
> = g::send::<
    Role<2>,
    Role<2>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<11>();
// Outer route is local to Controller (2 → 2)
const DECISION: g::Program<
    LoopDecisionSteps<
        Role<2>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        StepNil,
        SeqSteps<
            StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>,
            <SeqSteps<
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
            > as StepConcat<
                SeqSteps<
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
                >,
            >>::Output,
        >,
    >,
> = g::route(CONTINUE_ARM, BREAK_ARM);

#[test]
fn nested_loop_scope_balanced() {
    let _role_program: RoleProgram<
        '_,
        2,
        <LoopDecisionSteps<
            Role<2>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
            SeqSteps<
                StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>,
                <SeqSteps<
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
                > as StepConcat<
                    SeqSteps<
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
                    >,
                >>::Output,
            >,
        > as ProjectRole<Role<2>>>::Output,
        MintConfig,
    > = project(&DECISION);

    const HANDSHAKE: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil>> =
        g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>();
    const COMBINED: g::Program<
        <StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil> as StepConcat<
            LoopDecisionSteps<
                Role<2>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                StepNil,
                SeqSteps<
                    StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>,
                    <SeqSteps<
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
                    > as StepConcat<
                        SeqSteps<
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
                        >,
                    >>::Output,
                >,
            >,
        >>::Output,
    > = g::par(HANDSHAKE, DECISION);
    let _transport_program: RoleProgram<
        '_,
        2,
        <<StepCons<SendStep<Role<0>, Role<1>, Msg<10, ()>>, StepNil> as StepConcat<
            LoopDecisionSteps<
                Role<2>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                StepNil,
                SeqSteps<
                    StepCons<SendStep<Role<2>, Role<3>, Msg<1, ()>>, StepNil>,
                    <SeqSteps<
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
                    > as StepConcat<
                        SeqSteps<
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
                        >,
                    >>::Output,
                >,
            >,
        >>::Output as ProjectRole<Role<2>>>::Output,
        MintConfig,
    > = project(&COMBINED);
}
