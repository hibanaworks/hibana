use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn stage_transport_payload_copies_bytes()
 {
    let mut scratch = [0u8; 8];
    let src = [1u8, 2, 3, 4];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn stage_transport_payload_rejects_oversize()
 {
    let mut scratch = [0u8; 2];
    let src = [1u8, 2, 3];
    let err = stage_transport_payload(&mut scratch, &src).expect_err("oversize");
    assert!(matches!(err, RecvError::PhaseInvariant));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_scope_selection_meta_non_route_defaults_do_not_block_current()
 {
    let meta = CurrentScopeSelectionMeta::EMPTY;
    assert!(!meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(!meta.is_controller());
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_scope_selection_meta_route_entry_flags_roundtrip()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_frontier_selection_state_loop_controller_without_evidence_is_exact()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_frontier_selection_state_updates_only_current_candidate()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_loop_meta_recvless_ready_requires_active_or_linger()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_loop_meta_loop_label_scope_and_arm_recv_bits_are_exact()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_current_recv_frame_label_and_arm_bits_are_exact()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_controller_frame_labels_map_to_binary_arms_exactly()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_dispatch_frame_labels_do_not_count_as_ready_evidence()
 {
    let mut meta = ScopeFrameLabelMeta::EMPTY;
    meta.record_dispatch_arm_frame_label(1, 29);

    assert!(meta.matches_frame_hint(29));
    assert_eq!(meta.arm_for_frame_label(29), Some(1));
    assert_eq!(meta.evidence_arm_for_frame_label(29), None);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_binding_evidence_can_be_stricter_than_hint_evidence()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_preferred_binding_frame_label_is_exact_only_for_singletons()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_preferred_binding_frame_label_mask_respects_authoritative_arm()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_frame_label_meta_preferred_binding_frame_label_mask_keeps_current_recv_for_demux()
 {
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
