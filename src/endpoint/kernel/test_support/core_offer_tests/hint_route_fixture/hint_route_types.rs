use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const HINT_ROUTE_POLICY_ID:
    u16 = 601;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintLeftHead =
    PolicySteps<
        SendOnly<0, Role<0>, Role<0>, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>>,
        HINT_ROUTE_POLICY_ID,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintRightHead =
    PolicySteps<
        SendOnly<0, Role<0>, Role<0>, Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteDecisionKind>>,
        HINT_ROUTE_POLICY_ID,
    >;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_LEFT_ARM()
-> g::Program<SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                (),
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_RIGHT_ARM()
-> g::Program<SeqSteps<HintRightHead, SendOnly<0, Role<0>, Role<1>, Msg<101, u8>>>> {
    g::seq(
        g::send::<Role<0>, Role<0>, Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteDecisionKind>, 0>()
            .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>(),
    )
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintRouteSteps =
    RouteSteps<
        SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>,
        SeqSteps<HintRightHead, SendOnly<0, Role<0>, Role<1>, Msg<101, u8>>>,
    >;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_ROUTE_PROGRAM()
-> g::Program<HintRouteSteps> {
    g::route(HINT_LEFT_ARM(), HINT_RIGHT_ARM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&HINT_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&HINT_ROUTE_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintSplitLeftSteps =
    SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintSplitRightSteps =
    SeqSteps<HintRightHead, SendOnly<2, Role<0>, Role<1>, Msg<101, u8>>>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintSplitRouteSteps =
    RouteSteps<HintSplitLeftSteps, HintSplitRightSteps>;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_SPLIT_LEFT_ARM()
-> g::Program<HintSplitLeftSteps> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                (),
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_SPLIT_RIGHT_ARM()
-> g::Program<HintSplitRightSteps> {
    g::seq(
        g::send::<Role<0>, Role<0>, Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteDecisionKind>, 0>()
            .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 2>(),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_SPLIT_ROUTE_PROGRAM()
-> g::Program<HintSplitRouteSteps> {
    g::route(HINT_SPLIT_LEFT_ARM(), HINT_SPLIT_RIGHT_ARM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_SPLIT_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&HINT_SPLIT_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn HINT_SPLIT_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&HINT_SPLIT_ROUTE_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const HINT_LEFT_DATA_LABEL:
    u8 = 100;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const HINT_RIGHT_DATA_LABEL:
    u8 = 101;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const HINT_LEFT_DATA_FRAME:
    u8 = 0;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const HINT_RIGHT_DATA_FRAME:
    u8 = 1;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteHintRightKind>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendLeftPayloadMsg =
    Msg<0x59, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRightFirstMsg =
    Msg<0x5a, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRightSecondMsg =
    Msg<0x5b, u8>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRightPayloadSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<1>, MultiSendRightFirstMsg>,
        SendOnly<0, Role<0>, Role<1>, MultiSendRightSecondMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendLeftSteps =
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, MultiSendRouteLeftMsg>,
        SendOnly<0, Role<0>, Role<1>, MultiSendLeftPayloadMsg>,
    >;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, MultiSendRouteRightMsg>, MultiSendRightPayloadSteps>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type MultiSendRouteSteps =
    BranchSteps<MultiSendLeftSteps, MultiSendRightSteps>;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct FreshHintRouteResolverState
{
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) arm: Cell<u8>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) calls: Cell<usize>,
}

impl FreshHintRouteResolverState {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn new(
        arm: u8,
    ) -> Self {
        Self {
            arm: Cell::new(arm),
            calls: Cell::new(0),
        }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn calls(&self) -> usize {
        self.calls.get()
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn fresh_hint_route_resolver(
    state: &FreshHintRouteResolverState,
) -> Result<
    crate::control::cluster::core::DecisionResolution,
    crate::control::cluster::core::ResolverError,
> {
    state.calls.set(state.calls.get().wrapping_add(1));
    let arm = if state.arm.get() == 0 {
        crate::control::cluster::core::DecisionArm::Left
    } else {
        crate::control::cluster::core::DecisionArm::Right
    };
    Ok(crate::control::cluster::core::DecisionResolution::Arm(arm))
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn MULTI_SEND_ROUTE_PROGRAM()
-> g::Program<MultiSendRouteSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, MultiSendRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, MultiSendLeftPayloadMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, MultiSendRouteRightMsg, 0>(),
            g::seq(
                g::send::<Role<0>, Role<1>, MultiSendRightFirstMsg, 0>(),
                g::send::<Role<0>, Role<1>, MultiSendRightSecondMsg, 0>(),
            ),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn MULTI_SEND_ROUTE_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&MULTI_SEND_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn MULTI_SEND_ROUTE_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&MULTI_SEND_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ENTRY_ARM0_PROGRAM()
-> g::Program<
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, Msg<102, u8>>,
        SeqSteps<
            SendOnly<0, Role<0>, Role<1>, Msg<103, u8>>,
            SendOnly<0, Role<1>, Role<0>, Msg<104, u8>>,
        >,
    >,
> {
    g::seq(
        g::send::<Role<0>, Role<0>, Msg<102, u8>, 0>(),
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>(),
            g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ENTRY_ARM1_PROGRAM()
-> g::Program<
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, Msg<105, u8>>,
        SeqSteps<
            SendOnly<0, Role<0>, Role<1>, Msg<86, u8>>,
            SendOnly<0, Role<1>, Role<0>, Msg<87, u8>>,
        >,
    >,
> {
    g::seq(
        g::send::<Role<0>, Role<0>, Msg<105, u8>, 0>(),
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<86, u8>, 0>(),
            g::send::<Role<1>, Role<0>, Msg<87, u8>, 0>(),
        ),
    )
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type EntryRouteSteps =
    RouteSteps<
        SeqSteps<
            SendOnly<0, Role<0>, Role<0>, Msg<102, u8>>,
            SeqSteps<
                SendOnly<0, Role<0>, Role<1>, Msg<103, u8>>,
                SendOnly<0, Role<1>, Role<0>, Msg<104, u8>>,
            >,
        >,
        SeqSteps<
            SendOnly<0, Role<0>, Role<0>, Msg<105, u8>>,
            SeqSteps<
                SendOnly<0, Role<0>, Role<1>, Msg<86, u8>>,
                SendOnly<0, Role<1>, Role<0>, Msg<87, u8>>,
            >,
        >,
    >;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ENTRY_ROUTE_PROGRAM()
-> g::Program<EntryRouteSteps> {
    g::route(ENTRY_ARM0_PROGRAM(), ENTRY_ARM1_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ENTRY_CONTROLLER_PROGRAM()
-> RoleProgram<0> {
    project(&ENTRY_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ENTRY_WORKER_PROGRAM()
-> RoleProgram<1> {
    project(&ENTRY_ROUTE_PROGRAM())
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type NestedRouteSteps =
    RouteSteps<HintRouteSteps, EntryRouteSteps>;
#[allow(non_snake_case)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn NESTED_ROUTE_PROGRAM()
-> g::Program<NestedRouteSteps> {
    g::route(HINT_ROUTE_PROGRAM(), ENTRY_ROUTE_PROGRAM())
}
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const ENTRY_ARM0_SIGNAL_LABEL: u8 = 103;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const ENTRY_ARM0_SIGNAL_FRAME: u8 = 0;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const ENTRY_ARM1_SIGNAL_FRAME: u8 = 1;

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_inbox_take_is_one_shot()
 {
    let evidence = IngressEvidence {
        frame_label: FrameLabel::new(7),
        instance: 3,
        channel: Channel::new(1),
    };
    let mut binding = TestBinding::with_incoming(&[evidence]);
    with_test_binding_inbox::<1, _>(|inbox| {
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(evidence));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), None);

        inbox.put_back(0, evidence);
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(evidence));
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_inbox_take_matching_skips_head_mismatch()
 {
    let head = IngressEvidence {
        frame_label: FrameLabel::new(7),
        instance: 3,
        channel: Channel::new(1),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(9),
        instance: 4,
        channel: Channel::new(2),
    };
    let mut binding = TestBinding::with_incoming(&[head, expected]);
    with_test_binding_inbox::<1, _>(|inbox| {
        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.frame_label.raw());
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(head));
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_inbox_take_matching_scans_buffered_entries()
 {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        channel: Channel::new(12),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(5),
        instance: 3,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::default();
    with_test_binding_inbox::<1, _>(|inbox| {
        assert!(inbox.push_back(0, first));
        assert!(inbox.push_back(0, second));
        assert!(inbox.push_back(0, expected));

        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.frame_label.raw());
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_inbox_nonempty_mask_tracks_buffered_lanes()
 {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        channel: Channel::new(12),
    };
    let mut binding = TestBinding::default();
    with_test_binding_inbox::<3, _>(|inbox| {
        assert_nonempty_lanes_eq(inbox, 3, &[]);

        assert!(inbox.push_back(0, first));
        assert_nonempty_lanes_eq(inbox, 3, &[0]);

        assert!(inbox.push_back(2, second));
        assert_nonempty_lanes_eq(inbox, 3, &[0, 2]);

        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_nonempty_lanes_eq(inbox, 3, &[2]);

        assert_eq!(
            inbox.take_matching_or_poll(&mut binding, 2, second.frame_label.raw()),
            Some(second)
        );
        assert_nonempty_lanes_eq(inbox, 3, &[]);
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_inbox_frame_label_masks_track_buffered_frame_labels_exactly()
 {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        channel: Channel::new(12),
    };
    let third = IngressEvidence {
        frame_label: FrameLabel::new(207),
        instance: 3,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::default();
    with_test_binding_inbox::<3, _>(|inbox| {
        assert!(inbox.push_back(0, first));
        assert!(inbox.push_back(0, second));
        assert!(inbox.push_back(2, third));
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(0),
            FrameLabelMask::from_frame_label(first.frame_label.raw())
                | FrameLabelMask::from_frame_label(second.frame_label.raw())
        );
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(2),
            FrameLabelMask::from_frame_label(third.frame_label.raw())
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(first.frame_label.raw()),
            &[0],
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(second.frame_label.raw()),
            &[0],
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(third.frame_label.raw()),
            &[2],
        );

        assert_eq!(
            inbox.take_matching_or_poll(&mut binding, 0, second.frame_label.raw()),
            Some(second)
        );
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(0),
            FrameLabelMask::from_frame_label(first.frame_label.raw())
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(second.frame_label.raw()),
            &[],
        );
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(0),
            FrameLabelMask::EMPTY
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(first.frame_label.raw()),
            &[],
        );
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_inbox_take_matching_mask_drops_buffered_loop_control_frames()
 {
    let loop_control = IngressEvidence {
        frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
        instance: 1,
        channel: Channel::new(11),
    };
    let deferred = IngressEvidence {
        frame_label: FrameLabel::new(33),
        instance: 2,
        channel: Channel::new(12),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(55),
        instance: 3,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::with_incoming(&[expected]);
    with_test_binding_inbox::<1, _>(|inbox| {
        assert!(inbox.push_back(0, loop_control));
        assert!(inbox.push_back(0, deferred));

        let picked = inbox.take_matching_mask_or_poll(
            &mut binding,
            0,
            FrameLabelMask::from_frame_label(expected.frame_label.raw()),
            FrameLabelMask::from_frame_label(TEST_LOOP_CONTINUE_FRAME)
                | FrameLabelMask::from_frame_label(TEST_LOOP_BREAK_FRAME),
            |frame_label| {
                matches!(
                    frame_label,
                    TEST_LOOP_CONTINUE_FRAME | TEST_LOOP_BREAK_FRAME
                )
            },
        );
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(deferred));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), None);
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn binding_frame_mismatch_finds_later_matching_frame_label()
 {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(11),
        instance: 1,
        channel: Channel::new(21),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(12),
        instance: 2,
        channel: Channel::new(22),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(13),
        instance: 3,
        channel: Channel::new(23),
    };
    let mut binding = TestBinding::with_incoming(&[first, second, expected]);
    with_test_binding_inbox::<1, _>(|inbox| {
        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.frame_label.raw());
        assert_eq!(
            picked,
            Some(expected),
            "scan must continue past mismatched head entries"
        );
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
    });
}
