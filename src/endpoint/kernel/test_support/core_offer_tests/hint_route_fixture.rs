const HINT_ROUTE_POLICY_ID: u16 = 601;
type HintLeftHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
        >,
        StepNil,
    >,
    HINT_ROUTE_POLICY_ID,
>;
type HintRightHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>,
        >,
        StepNil,
    >,
    HINT_ROUTE_POLICY_ID,
>;
#[allow(non_snake_case)]
fn HINT_LEFT_ARM()
-> g::Program<SeqSteps<HintLeftHead, StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>>> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    )
}

#[allow(non_snake_case)]
fn HINT_RIGHT_ARM()
-> g::Program<SeqSteps<HintRightHead, StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>>>
{
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>(),
    )
}
type HintRouteSteps = RouteSteps<
    SeqSteps<HintLeftHead, StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>>,
    SeqSteps<HintRightHead, StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>>,
>;
#[allow(non_snake_case)]
fn HINT_ROUTE_PROGRAM() -> g::Program<HintRouteSteps> {
    g::route(HINT_LEFT_ARM(), HINT_RIGHT_ARM())
}

#[allow(non_snake_case)]
fn HINT_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&HINT_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn HINT_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&HINT_ROUTE_PROGRAM())
}
type HintSplitLeftSteps = SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>;
type HintSplitRightSteps = SeqSteps<HintRightHead, SendOnly<2, Role<0>, Role<1>, Msg<101, u8>>>;
type HintSplitRouteSteps = RouteSteps<HintSplitLeftSteps, HintSplitRightSteps>;
#[allow(non_snake_case)]
fn HINT_SPLIT_LEFT_ARM() -> g::Program<HintSplitLeftSteps> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    )
}

#[allow(non_snake_case)]
fn HINT_SPLIT_RIGHT_ARM() -> g::Program<HintSplitRightSteps> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 2>(),
    )
}

#[allow(non_snake_case)]
fn HINT_SPLIT_ROUTE_PROGRAM() -> g::Program<HintSplitRouteSteps> {
    g::route(HINT_SPLIT_LEFT_ARM(), HINT_SPLIT_RIGHT_ARM())
}

#[allow(non_snake_case)]
fn HINT_SPLIT_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&HINT_SPLIT_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn HINT_SPLIT_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&HINT_SPLIT_ROUTE_PROGRAM())
}
const HINT_LEFT_DATA_LABEL: u8 = 100;
const HINT_RIGHT_DATA_LABEL: u8 = 101;
const HINT_LEFT_DATA_FRAME: u8 = 0;
const HINT_RIGHT_DATA_FRAME: u8 = 1;
type MultiSendRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>;
type MultiSendRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>;
type MultiSendLeftPayloadMsg = Msg<0x59, u8>;
type MultiSendRightFirstMsg = Msg<0x5a, u8>;
type MultiSendRightSecondMsg = Msg<0x5b, u8>;
type MultiSendRightPayloadSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<1>, MultiSendRightFirstMsg>,
    SendOnly<0, Role<0>, Role<1>, MultiSendRightSecondMsg>,
>;
type MultiSendLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, MultiSendRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, MultiSendLeftPayloadMsg>,
>;
type MultiSendRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, MultiSendRouteRightMsg>, MultiSendRightPayloadSteps>;
type MultiSendRouteSteps = BranchSteps<MultiSendLeftSteps, MultiSendRightSteps>;

struct FreshHintRouteResolverState {
    arm: Cell<u8>,
    calls: Cell<usize>,
}

impl FreshHintRouteResolverState {
    const fn new(arm: u8) -> Self {
        Self {
            arm: Cell::new(arm),
            calls: Cell::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }
}

fn fresh_hint_route_resolver(
    state: &FreshHintRouteResolverState,
    _ctx: crate::control::cluster::core::ResolverContext,
) -> Result<
    crate::control::cluster::core::RouteResolution,
    crate::control::cluster::core::ResolverError,
> {
    state.calls.set(state.calls.get().wrapping_add(1));
    Ok(crate::control::cluster::core::RouteResolution::Arm(
        state.arm.get(),
    ))
}

#[allow(non_snake_case)]
fn MULTI_SEND_ROUTE_PROGRAM() -> g::Program<MultiSendRouteSteps> {
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
fn MULTI_SEND_ROUTE_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&MULTI_SEND_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn MULTI_SEND_ROUTE_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&MULTI_SEND_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn ENTRY_ARM0_PROGRAM() -> g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
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
fn ENTRY_ARM1_PROGRAM() -> g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<86, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<87, u8>>, StepNil>,
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
type EntryRouteSteps = RouteSteps<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    >,
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<86, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<87, u8>>, StepNil>,
        >,
    >,
>;
#[allow(non_snake_case)]
fn ENTRY_ROUTE_PROGRAM() -> g::Program<EntryRouteSteps> {
    g::route(ENTRY_ARM0_PROGRAM(), ENTRY_ARM1_PROGRAM())
}

#[allow(non_snake_case)]
fn ENTRY_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&ENTRY_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn ENTRY_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&ENTRY_ROUTE_PROGRAM())
}

type NestedRouteSteps = RouteSteps<HintRouteSteps, EntryRouteSteps>;
#[allow(non_snake_case)]
fn NESTED_ROUTE_PROGRAM() -> g::Program<NestedRouteSteps> {
    g::route(HINT_ROUTE_PROGRAM(), ENTRY_ROUTE_PROGRAM())
}
const ENTRY_ARM0_SIGNAL_LABEL: u8 = 103;
const ENTRY_ARM0_SIGNAL_FRAME: u8 = 0;
const ENTRY_ARM1_SIGNAL_FRAME: u8 = 1;

#[test]
fn binding_inbox_take_is_one_shot() {
    let evidence = IngressEvidence {
        frame_label: FrameLabel::new(7),
        instance: 3,
        has_fin: false,
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
fn binding_inbox_take_matching_skips_head_mismatch() {
    let head = IngressEvidence {
        frame_label: FrameLabel::new(7),
        instance: 3,
        has_fin: false,
        channel: Channel::new(1),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(9),
        instance: 4,
        has_fin: false,
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
fn binding_inbox_take_matching_scans_buffered_entries() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(5),
        instance: 3,
        has_fin: false,
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
fn binding_inbox_nonempty_mask_tracks_buffered_lanes() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        has_fin: false,
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
fn binding_inbox_frame_label_masks_track_buffered_frame_labels_exactly() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let third = IngressEvidence {
        frame_label: FrameLabel::new(207),
        instance: 3,
        has_fin: false,
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
fn binding_inbox_take_matching_mask_drops_buffered_loop_control_frames() {
    let loop_control = IngressEvidence {
        frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let deferred = IngressEvidence {
        frame_label: FrameLabel::new(33),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(55),
        instance: 3,
        has_fin: false,
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
fn binding_frame_mismatch_finds_later_matching_frame_label() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(11),
        instance: 1,
        has_fin: false,
        channel: Channel::new(21),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(12),
        instance: 2,
        has_fin: false,
        channel: Channel::new(22),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(13),
        instance: 3,
        has_fin: false,
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

#[test]
fn stage_transport_payload_copies_bytes() {
    let mut scratch = [0u8; 8];
    let src = [1u8, 2, 3, 4];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}

#[test]
fn stage_transport_payload_rejects_oversize() {
    let mut scratch = [0u8; 2];
    let src = [1u8, 2, 3];
    let err = stage_transport_payload(&mut scratch, &src).expect_err("oversize");
    assert!(matches!(err, RecvError::PhaseInvariant));
}

#[test]
fn offer_select_priority_is_deterministic() {
    assert_eq!(
        choose_offer_priority(true, 1, 1, 2),
        Some(OfferSelectPriority::CurrentOfferEntry)
    );
    assert_eq!(
        choose_offer_priority(false, 1, 2, 2),
        Some(OfferSelectPriority::DynamicControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 1, 2),
        Some(OfferSelectPriority::ControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 2, 1),
        Some(OfferSelectPriority::CandidateUnique)
    );
    assert_eq!(choose_offer_priority(false, 0, 2, 2), None);
}

#[test]
fn static_controller_current_is_not_preempted() {
    let selected = choose_offer_priority(true, 1, 1, 2);
    assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
}

#[test]
fn hint_filter_does_not_override_priority() {
    // Stage A applies filter; Stage B ordering is still fixed.
    let current_is_candidate_after_filter = true;
    let selected = choose_offer_priority(current_is_candidate_after_filter, 1, 1, 1);
    assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
}

#[test]
fn offer_priority_has_no_progress_override() {
    // Stage B priority is fixed and independent from progress signals.
    assert_eq!(
        choose_offer_priority(false, 1, 1, 1),
        Some(OfferSelectPriority::DynamicControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 1, 1),
        Some(OfferSelectPriority::ControllerUnique)
    );
}

#[test]
fn current_scope_selection_meta_non_route_defaults_do_not_block_current() {
    let meta = CurrentScopeSelectionMeta::EMPTY;
    assert!(!meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(!meta.is_controller());
}

#[test]
fn current_scope_selection_meta_route_entry_flags_roundtrip() {
    let meta = CurrentScopeSelectionMeta {
        flags: CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY
            | CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES
            | CurrentScopeSelectionMeta::FLAG_CONTROLLER,
    };
    assert!(meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(meta.is_controller());
}

#[test]
fn current_frontier_selection_state_loop_controller_without_evidence_is_exact() {
    let base = CurrentFrontierSelectionState {
        frontier: FrontierKind::Loop,
        parallel_root: ScopeId::none(),
        ready: true,
        has_progress_evidence: false,
        flags: CurrentFrontierSelectionState::FLAG_CONTROLLER,
    };
    assert!(base.loop_controller_without_evidence());
    assert!(
        !CurrentFrontierSelectionState {
            ready: false,
            ..base
        }
        .loop_controller_without_evidence()
    );
    assert!(
        !CurrentFrontierSelectionState {
            has_progress_evidence: true,
            ..base
        }
        .loop_controller_without_evidence()
    );
    assert!(!CurrentFrontierSelectionState { flags: 0, ..base }.loop_controller_without_evidence());
}

#[test]
fn current_frontier_selection_state_updates_only_current_candidate() {
    let mut state = CurrentFrontierSelectionState {
        frontier: FrontierKind::Parallel,
        parallel_root: ScopeId::generic(3),
        ready: false,
        has_progress_evidence: false,
        flags: 0,
    };
    state.observe_candidate(
        ScopeId::generic(11),
        7,
        FrontierCandidate {
            scope_id: ScopeId::generic(12),
            entry_idx: 9,
            parallel_root: ScopeId::generic(3),
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
    );
    assert!(!state.ready);
    assert!(!state.has_progress_evidence);

    state.observe_candidate(
        ScopeId::generic(11),
        7,
        FrontierCandidate {
            scope_id: ScopeId::generic(11),
            entry_idx: 7,
            parallel_root: ScopeId::generic(3),
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
    );
    assert!(state.ready);
    assert!(state.has_progress_evidence);
}

#[test]
fn scope_loop_meta_recvless_ready_requires_active_or_linger() {
    assert!(!ScopeLoopMeta::EMPTY.recvless_ready());
    assert!(
        ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE,
        }
        .recvless_ready()
    );
    assert!(
        ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        }
        .recvless_ready()
    );
    assert!(
        !ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE
                | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV
                | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        }
        .recvless_ready()
    );
}

#[test]
fn scope_loop_meta_loop_label_scope_and_arm_recv_bits_are_exact() {
    let meta = ScopeLoopMeta {
        flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
    };
    assert!(meta.loop_label_scope());
    assert!(!meta.arm_has_recv(0));
    assert!(meta.arm_has_recv(1));

    let linger = ScopeLoopMeta {
        flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
    };
    assert!(linger.loop_label_scope());
    assert!(linger.arm_has_recv(0));
    assert!(!linger.arm_has_recv(1));
    assert!(!ScopeLoopMeta::EMPTY.loop_label_scope());
}

#[test]
fn scope_frame_label_meta_current_recv_frame_label_and_arm_bits_are_exact() {
    let no_arm = ScopeFrameLabelMeta {
        recv_frame_label: 7,
        recv_arm: 1,
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL,
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert!(no_arm.matches_current_recv_frame_label(7));
    assert!(no_arm.matches_frame_hint(7));
    assert_eq!(no_arm.current_recv_arm_for_frame_label(7), None);
    let with_arm = ScopeFrameLabelMeta {
        arm_frame_label_masks: [FrameLabelMask::EMPTY, FrameLabelMask::from_frame_label(7)],
        flags: no_arm.flags | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM,
        ..no_arm
    };
    assert_eq!(with_arm.current_recv_arm_for_frame_label(7), Some(1));
    assert_eq!(with_arm.arm_for_frame_label(7), Some(1));
    assert!(!with_arm.matches_current_recv_frame_label(8));

    let high_frame = ScopeFrameLabelMeta {
        arm_frame_label_masks: [FrameLabelMask::EMPTY, FrameLabelMask::from_frame_label(200)],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::EMPTY,
            FrameLabelMask::from_frame_label(200),
        ],
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert!(high_frame.matches_frame_hint(200));
    assert_eq!(high_frame.arm_for_frame_label(200), Some(1));
    assert_eq!(high_frame.preferred_binding_frame_label(Some(1)), Some(200));
}

#[test]
fn scope_frame_label_meta_controller_frame_labels_map_to_binary_arms_exactly() {
    let meta = ScopeFrameLabelMeta {
        controller_frame_labels: [11, 13],
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(11),
            FrameLabelMask::from_frame_label(13),
        ],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(11),
            FrameLabelMask::from_frame_label(13),
        ],
        flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert_eq!(meta.controller_arm_for_frame_label(11), Some(0));
    assert_eq!(meta.controller_arm_for_frame_label(13), Some(1));
    assert_eq!(meta.controller_arm_for_frame_label(17), None);
    assert_eq!(meta.arm_for_frame_label(11), Some(0));
    assert_eq!(meta.arm_for_frame_label(13), Some(1));
}

#[test]
fn scope_frame_label_meta_dispatch_frame_labels_do_not_count_as_ready_evidence() {
    let mut meta = ScopeFrameLabelMeta::EMPTY;
    meta.record_dispatch_arm_frame_label(1, 29);

    assert!(meta.matches_frame_hint(29));
    assert_eq!(meta.arm_for_frame_label(29), Some(1));
    assert_eq!(meta.evidence_arm_for_frame_label(29), None);
}

#[test]
fn scope_frame_label_meta_binding_evidence_can_be_stricter_than_hint_evidence() {
    let meta = ScopeFrameLabelMeta {
        recv_frame_label: 41,
        recv_arm: 0,
        arm_frame_label_masks: [FrameLabelMask::from_frame_label(41), FrameLabelMask::EMPTY],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41),
            FrameLabelMask::EMPTY,
        ],
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert!(meta.matches_frame_hint(41));
    assert_eq!(meta.arm_for_frame_label(41), Some(0));
    assert_eq!(meta.evidence_arm_for_frame_label(41), Some(0));
    assert_eq!(meta.binding_evidence_arm_for_frame_label(41), None);
}

#[test]
fn scope_frame_label_meta_preferred_binding_frame_label_is_exact_only_for_singletons() {
    let meta = ScopeFrameLabelMeta {
        recv_frame_label: 41,
        recv_arm: 0,
        controller_frame_labels: [43, 47],
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert_eq!(meta.preferred_binding_frame_label(Some(0)), Some(43));
    assert_eq!(meta.preferred_binding_frame_label(Some(1)), Some(47));
    assert_eq!(meta.preferred_binding_frame_label(None), None);

    let singleton = ScopeFrameLabelMeta {
        controller_frame_labels: [53, 0],
        arm_frame_label_masks: [FrameLabelMask::from_frame_label(53), FrameLabelMask::EMPTY],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(53),
            FrameLabelMask::EMPTY,
        ],
        flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0,
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert_eq!(singleton.preferred_binding_frame_label(None), Some(53));
}

#[test]
fn scope_frame_label_meta_preferred_binding_frame_label_mask_respects_authoritative_arm() {
    let meta = ScopeFrameLabelMeta {
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(11) | FrameLabelMask::from_frame_label(13),
            FrameLabelMask::from_frame_label(17),
        ],
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert_eq!(
        meta.preferred_binding_frame_label_mask(Some(0)),
        FrameLabelMask::from_frame_label(11) | FrameLabelMask::from_frame_label(13)
    );
    assert_eq!(
        meta.preferred_binding_frame_label_mask(Some(1)),
        FrameLabelMask::from_frame_label(17)
    );
    assert_eq!(
        meta.preferred_binding_frame_label_mask(None),
        meta.frame_hint_mask()
    );
}

#[test]
fn scope_frame_label_meta_preferred_binding_frame_label_mask_keeps_current_recv_for_demux() {
    let meta = ScopeFrameLabelMeta {
        recv_frame_label: 41,
        recv_arm: 0,
        controller_frame_labels: [43, 47],
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert_eq!(
        meta.preferred_binding_frame_label_mask(Some(0)),
        FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43)
    );
}

#[test]
fn lane_offer_state_roundtrips_static_frontier_flags() {
    let state = LaneOfferState {
        scope: ScopeId::generic(5),
        entry: StateIndex::from_usize(11),
        parallel_root: ScopeId::generic(2),
        frontier: FrontierKind::Parallel,
        static_ready: true,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
    };
    assert!(state.is_controller());
    assert!(state.is_dynamic());
    assert!(state.static_ready());
    assert_eq!(state.frontier, FrontierKind::Parallel);
}

#[test]
fn refresh_lane_offer_state_caches_scope_frame_label_meta() {
    run_offer_regression_test(
        "refresh_lane_offer_state_caches_scope_frame_label_meta",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(997);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    worker.refresh_lane_offer_state(0);
                    let entry_idx =
                        state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                    let entry_state = worker
                        .offer_entry_state_snapshot(entry_idx)
                        .expect("offer entry state snapshot");
                    let cached = RouteFrontierMachine::offer_entry_frame_label_meta(
                        &worker, scope, entry_idx,
                    )
                    .expect("cached offer-entry label metadata");
                    let recv_meta = worker.cursor.try_recv_meta().expect("recv metadata");
                    assert_eq!(cached.scope_id(), scope);
                    assert_eq!(
                        cached.loop_meta().flags,
                        CursorEndpoint::<
                            1,
                            HintOnlyTransport,
                            DefaultLabelUniverse,
                            CounterClock,
                            crate::control::cap::mint::EpochTbl,
                            4,
                            crate::control::cap::mint::MintConfig,
                            NoBinding,
                        >::scope_loop_meta_at(
                            &worker.cursor,
                            &worker.control_semantics(),
                            scope,
                            entry_idx,
                        )
                        .flags
                    );
                    assert!(cached.matches_current_recv_frame_label(recv_meta.frame_label));
                    assert_eq!(
                        cached.current_recv_arm_for_frame_label(recv_meta.frame_label),
                        recv_meta.route_arm
                    );
                    assert_eq!(entry_state.scope_id, scope);
                    assert_eq!(
                        entry_state.frontier,
                        worker.route_state.lane_offer_state(0).frontier
                    );
                    assert!(entry_state.selection_meta.is_route_entry());
                    assert_eq!(
                        entry_state.selection_meta.is_controller(),
                        worker.route_state.lane_offer_state(0).is_controller()
                    );
                    assert_eq!(
                        entry_state.summary.frontier_mask,
                        worker.route_state.lane_offer_state(0).frontier.bit()
                    );
                    assert_eq!(
                        entry_state.summary.is_controller(),
                        worker.route_state.lane_offer_state(0).is_controller()
                    );
                    assert_eq!(
                        entry_state.summary.is_dynamic(),
                        worker.route_state.lane_offer_state(0).is_dynamic()
                    );
                    assert_eq!(
                        entry_state.summary.static_ready(),
                        worker.route_state.lane_offer_state(0).static_ready()
                    );
                    let observed = worker
                        .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                        .expect("observed state");
                    assert_eq!(
                        worker.offer_entry_observed_state_cached(entry_idx),
                        Some(observed)
                    );
                    assert_lane_set_eq(
                        worker.offer_lane_set_for_scope(scope),
                        worker.cursor.logical_lane_count(),
                        &[0],
                    );
                    assert_eq!(entry_state.lane_idx, 0);
                    assert_eq!(
                        worker
                            .offer_entry_lane_state(scope, entry_idx)
                            .map(|info| info.entry),
                        Some(worker.route_state.lane_offer_state(0).entry)
                    );
                    let materialization = worker
                        .offer_entry_materialization_meta(scope, entry_idx)
                        .expect("descriptor-derived materialization metadata");
                    assert_eq!(
                        materialization.arm_count,
                        worker.cursor.route_scope_arm_count(scope).unwrap_or(0)
                    );
                    let mut arm = 0u8;
                    while arm <= 1 {
                        let expected_controller_cross_role_recv = worker
                            .cursor
                            .controller_arm_entry_by_arm(scope, arm)
                            .and_then(|(entry, _)| {
                                worker.cursor.try_recv_meta_at(state_index_to_usize(entry))
                            })
                            .map(|recv_meta| recv_meta.peer != 1)
                            .unwrap_or(false);
                        assert_eq!(
                            materialization.controller_arm_entry(arm),
                            worker.cursor.controller_arm_entry_by_arm(scope, arm)
                        );
                        assert_eq!(
                            materialization.controller_arm_requires_ready_evidence(arm),
                            expected_controller_cross_role_recv
                        );
                        assert_eq!(
                            materialization.recv_entry(arm),
                            worker
                                .cursor
                                .route_scope_arm_recv_index(scope, arm)
                                .map(StateIndex::from_usize)
                        );
                        assert_eq!(
                            materialization.passive_arm_entry(arm),
                            worker
                                .cursor
                                .follow_passive_observer_arm_for_scope(scope, arm)
                                .map(|nav| match nav {
                                    PassiveArmNavigation::WithinArm { entry } => entry,
                                })
                        );
                        let mut lane_idx = 0usize;
                        while lane_idx < worker.cursor.logical_lane_count() {
                            let mut expected_binding_demux_lane = false;
                            if let Some((entry, _)) =
                                worker.cursor.controller_arm_entry_by_arm(scope, arm)
                                && let Some(recv_meta) =
                                    worker.cursor.try_recv_meta_at(state_index_to_usize(entry))
                                && recv_meta.lane as usize == lane_idx
                            {
                                expected_binding_demux_lane = true;
                            }
                            if let Some(entry) =
                                worker.cursor.route_scope_arm_recv_index(scope, arm)
                                && let Some(recv_meta) = worker.cursor.try_recv_meta_at(entry)
                                && recv_meta.lane as usize == lane_idx
                            {
                                expected_binding_demux_lane = true;
                            }
                            let mut dispatch_idx = 0usize;
                            while let Some((frame_label, lane, dispatch_arm, target)) = worker
                                .cursor
                                .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
                            {
                                if (dispatch_arm == arm || dispatch_arm == ARM_SHARED)
                                    && let Some(recv_meta) =
                                        worker.cursor.try_recv_meta_at(state_index_to_usize(target))
                                    && recv_meta.frame_label == frame_label
                                    && recv_meta.lane == lane
                                    && lane as usize == lane_idx
                                {
                                    expected_binding_demux_lane = true;
                                }
                                dispatch_idx += 1;
                            }
                            assert_eq!(
                                worker.binding_demux_contains_lane(scope, Some(arm), lane_idx),
                                expected_binding_demux_lane
                            );
                            lane_idx += 1;
                        }
                        if arm == 1 {
                            break;
                        }
                        arm += 1;
                    }
                    let mut dispatch_idx = 0usize;
                    while let Some((frame_label, lane, arm, target)) = worker
                        .cursor
                        .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
                    {
                        assert_eq!(
                            materialization
                                .first_recv_target_for_lane_frame_label(lane, frame_label),
                            Some((arm, target))
                        );
                        dispatch_idx += 1;
                    }
                    assert_eq!(materialization.first_recv_len as usize, dispatch_idx);
                });
            });
        },
    );
}
