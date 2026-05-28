use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type OfferHintCluster =
    SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type OfferHintControllerEndpoint =
    CursorEndpoint<
        'static,
        0,
        HintOnlyTransport,
        DefaultLabelUniverse,
        CounterClock,
        crate::control::cap::mint::EpochTbl,
        4,
        crate::control::cap::mint::MintConfig,
        NoBinding,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type OfferHintWorkerEndpoint =
    CursorEndpoint<
        'static,
        1,
        HintOnlyTransport,
        DefaultLabelUniverse,
        CounterClock,
        crate::control::cap::mint::EpochTbl,
        4,
        crate::control::cap::mint::MintConfig,
        NoBinding,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type OfferHintWorkerBindingEndpoint =
    CursorEndpoint<
        'static,
        1,
        HintOnlyTransport,
        DefaultLabelUniverse,
        CounterClock,
        crate::control::cap::mint::EpochTbl,
        4,
        crate::control::cap::mint::MintConfig,
        TestBinding,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type OfferHintLaneAwareWorkerEndpoint =
    CursorEndpoint<
        'static,
        1,
        HintOnlyTransport,
        DefaultLabelUniverse,
        CounterClock,
        crate::control::cap::mint::EpochTbl,
        4,
        crate::control::cap::mint::MintConfig,
        LaneAwareTestBinding,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightOuterLeftMsg =
    Msg<0x50, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightMiddleLeftMsg =
    Msg<0x51, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightThirdLeftMsg =
    Msg<0x52, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightFinalLeftMsg =
    Msg<0x53, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightFinalRightMsg =
    Msg<0x55, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const DEEP_RIGHT_FINAL_RIGHT_FRAME: u8 = 4;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightStaticRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightStaticRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteHintRightKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightFinalDecisionLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, DeepRightFinalLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightFinalDecisionRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>,
        SendOnly<0, Role<0>, Role<1>, DeepRightFinalRightMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightFinalDecisionSteps =
    BranchSteps<DeepRightFinalDecisionLeftSteps, DeepRightFinalDecisionRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightThirdLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, DeepRightThirdLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightThirdRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>,
        DeepRightFinalDecisionSteps,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightThirdSteps =
    BranchSteps<DeepRightThirdLeftSteps, DeepRightThirdRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightMiddleLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, DeepRightMiddleLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightMiddleRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>, DeepRightThirdSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightMiddleSteps =
    BranchSteps<DeepRightMiddleLeftSteps, DeepRightMiddleRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightOuterLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, DeepRightOuterLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightOuterRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>, DeepRightMiddleSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type DeepRightProgramSteps =
    BranchSteps<DeepRightOuterLeftSteps, DeepRightOuterRightSteps>;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn DEEP_RIGHT_FINAL_DECISION()
-> g::Program<DeepRightFinalDecisionSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightFinalLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightFinalRightMsg, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn DEEP_RIGHT_THIRD()
-> g::Program<DeepRightThirdSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightThirdLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            DEEP_RIGHT_FINAL_DECISION(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn DEEP_RIGHT_MIDDLE()
-> g::Program<DeepRightMiddleSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightMiddleLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            DEEP_RIGHT_THIRD(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn DEEP_RIGHT_PROGRAM()
-> g::Program<DeepRightProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightOuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            DEEP_RIGHT_MIDDLE(),
        ),
    )
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticOuterLeftMsg =
    Msg<0x50, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticLeafLeftMsg =
    Msg<0x51, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticLeafRightMsg =
    Msg<0x52, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticMiddleRightMsg =
    Msg<0x53, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteHintRightKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticInnerLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedStaticLeafLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticInnerRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedStaticLeafRightMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticInnerSteps =
    BranchSteps<NestedStaticInnerLeftSteps, NestedStaticInnerRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticMiddleLeftSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>, NestedStaticInnerSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticMiddleRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedStaticMiddleRightMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticMiddleSteps =
    BranchSteps<NestedStaticMiddleLeftSteps, NestedStaticMiddleRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticOuterLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedStaticOuterLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticOuterRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>, NestedStaticMiddleSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedStaticProgramSteps =
    BranchSteps<NestedStaticOuterLeftSteps, NestedStaticOuterRightSteps>;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_STATIC_INNER()
-> g::Program<NestedStaticInnerSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticLeafLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticLeafRightMsg, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_STATIC_MIDDLE()
-> g::Program<NestedStaticMiddleSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
            NESTED_STATIC_INNER(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticMiddleRightMsg, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_STATIC_PROGRAM()
-> g::Program<NestedStaticProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticOuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
            NESTED_STATIC_MIDDLE(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_STATIC_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&NESTED_STATIC_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_STATIC_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&NESTED_STATIC_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedContinueMsg =
    Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), crate::control::cap::resource_kinds::LoopContinueKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedBreakMsg =
    Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), crate::control::cap::resource_kinds::LoopBreakKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteHintRightKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedInnerLeftMsg =
    Msg<90, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedInnerRightMsg =
    Msg<91, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedInnerLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, LoopContinueScopedInnerLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedInnerRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
        SendOnly<0, Role<0>, Role<1>, LoopContinueScopedInnerRightMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedInnerRouteSteps =
    BranchSteps<LoopContinueScopedInnerLeftSteps, LoopContinueScopedInnerRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedContinueArmSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedContinueMsg>,
        LoopContinueScopedInnerRouteSteps,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinueScopedProgramSteps =
    BranchSteps<
        LoopContinueScopedContinueArmSteps,
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedBreakMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopSemanticsProgramSteps =
    BranchSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedContinueMsg>,
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedBreakMsg>,
    >;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_SEMANTICS_PROGRAM()
-> g::Program<LoopSemanticsProgramSteps> {
    g::route(
        g::send::<Role<0>, Role<0>, LoopContinueScopedContinueMsg, 0>(),
        g::send::<Role<0>, Role<0>, LoopContinueScopedBreakMsg, 0>(),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_SEMANTICS_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&LOOP_SEMANTICS_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_CONTINUE_SCOPED_PROGRAM()
-> g::Program<LoopContinueScopedProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedContinueMsg, 0>(),
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinueScopedInnerLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinueScopedInnerRightMsg, 0>(),
                ),
            ),
        ),
        g::send::<Role<0>, Role<0>, LoopContinueScopedBreakMsg, 0>(),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_CONTINUE_SCOPED_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&LOOP_CONTINUE_SCOPED_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL: u8 = 0x51;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinuePassiveOuterLeftMsg =
    Msg<90, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinuePassiveRightReplyMsg =
    Msg<{ LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL }, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinuePassiveInnerLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, LoopContinuePassiveOuterLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinuePassiveInnerRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
        SendOnly<0, Role<0>, Role<1>, LoopContinuePassiveRightReplyMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinuePassiveInnerRouteSteps =
    BranchSteps<LoopContinuePassiveInnerLeftSteps, LoopContinuePassiveInnerRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type LoopContinuePassiveProgramSteps =
    BranchSteps<
        SeqSteps<
            SendOnly<0, Role<0>, Role<0>, LoopContinueScopedContinueMsg>,
            LoopContinuePassiveInnerRouteSteps,
        >,
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedBreakMsg>,
    >;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_CONTINUE_PASSIVE_PROGRAM()
-> g::Program<LoopContinuePassiveProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedContinueMsg, 0>(),
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinuePassiveOuterLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinuePassiveRightReplyMsg, 0>(),
                ),
            ),
        ),
        g::send::<Role<0>, Role<0>, LoopContinueScopedBreakMsg, 0>(),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_CONTINUE_PASSIVE_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&LOOP_CONTINUE_PASSIVE_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn LOOP_CONTINUE_PASSIVE_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&LOOP_CONTINUE_PASSIVE_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchOuterLeftMsg =
    Msg<0x10, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchLeafLeftMsg =
    Msg<0x51, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchLeafRightMsg =
    Msg<0x52, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchInnerLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedDispatchLeafLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchInnerRightSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedDispatchLeafRightMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchInnerSteps =
    BranchSteps<NestedDispatchInnerLeftSteps, NestedDispatchInnerRightSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchOuterLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, NestedDispatchOuterLeftMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedDispatchProgramSteps =
    BranchSteps<
        NestedDispatchOuterLeftSteps,
        SeqSteps<
            SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
            NestedDispatchInnerSteps,
        >,
    >;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_DISPATCH_PROGRAM()
-> g::Program<NestedDispatchProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedDispatchOuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, NestedDispatchLeafLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
                    g::send::<Role<0>, Role<1>, NestedDispatchLeafRightMsg, 0>(),
                ),
            ),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_DISPATCH_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&NESTED_DISPATCH_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_DISPATCH_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&NESTED_DISPATCH_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type PendingOfferCluster =
    SessionCluster<'static, PendingTransport, DefaultLabelUniverse, CounterClock, 4>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintPendingOfferCluster =
    SessionCluster<'static, HintPendingTransport, DefaultLabelUniverse, CounterClock, 4>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type FreshHintPendingOfferCluster =
    SessionCluster<'static, FreshHintPendingTransport, DefaultLabelUniverse, CounterClock, 4>;
