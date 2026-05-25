use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots()
 {
    run_offer_regression_test(
        "refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots",
        || {
            const OUTER_LEFT_LABEL: u8 = 0x61;
            const OUTER_RIGHT_LABEL: u8 = 0x62;
            const OUTER_LEFT_DATA_LABEL: u8 = 0x53;
            const INNER_LEFT_LABEL: u8 = 0x63;
            const INNER_RIGHT_LABEL: u8 = 0x64;
            const INNER_LEFT_DATA_LABEL: u8 = 0x54;
            const INNER_RIGHT_DATA_LABEL: u8 = 0x55;
            const INNER_REPLY_DATA_LABEL: u8 = 0x56;

            type InnerArm0 = SeqSteps<
                SendOnly<2, Role<0>, Role<0>, Msg<INNER_LEFT_LABEL, u8>>,
                SeqSteps<
                    SendOnly<2, Role<0>, Role<1>, Msg<INNER_LEFT_DATA_LABEL, u8>>,
                    SendOnly<2, Role<1>, Role<0>, Msg<INNER_REPLY_DATA_LABEL, u8>>,
                >,
            >;
            type InnerArm1 = SeqSteps<
                SendOnly<2, Role<0>, Role<0>, Msg<INNER_RIGHT_LABEL, u8>>,
                SendOnly<2, Role<0>, Role<1>, Msg<INNER_RIGHT_DATA_LABEL, u8>>,
            >;
            type InnerRouteSteps = RouteSteps<InnerArm0, InnerArm1>;
            type OuterLeftSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, Msg<OUTER_LEFT_LABEL, u8>>,
                SendOnly<0, Role<0>, Role<1>, Msg<OUTER_LEFT_DATA_LABEL, u8>>,
            >;
            type OuterRightSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, Msg<OUTER_RIGHT_LABEL, u8>>,
                InnerRouteSteps,
            >;
            type NestedSplitRouteSteps = RouteSteps<OuterLeftSteps, OuterRightSteps>;

            let inner_arm0_program: g::Program<InnerArm0> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<INNER_LEFT_LABEL, u8>, 2>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, Msg<INNER_LEFT_DATA_LABEL, u8>, 2>(),
                    g::send::<Role<1>, Role<0>, Msg<INNER_REPLY_DATA_LABEL, u8>, 2>(),
                ),
            );
            let inner_arm1_program: g::Program<InnerArm1> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<INNER_RIGHT_LABEL, u8>, 2>(),
                g::send::<Role<0>, Role<1>, Msg<INNER_RIGHT_DATA_LABEL, u8>, 2>(),
            );
            let inner_route_program: g::Program<InnerRouteSteps> =
                g::route(inner_arm0_program, inner_arm1_program);
            let outer_left_program: g::Program<OuterLeftSteps> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<OUTER_LEFT_LABEL, u8>, 0>(),
                g::send::<Role<0>, Role<1>, Msg<OUTER_LEFT_DATA_LABEL, u8>, 0>(),
            );
            let outer_right_program: g::Program<OuterRightSteps> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<OUTER_RIGHT_LABEL, u8>, 0>(),
                inner_route_program,
            );
            let nested_split_route_program: g::Program<NestedSplitRouteSteps> =
                g::route(outer_left_program, outer_right_program);

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1008);
                    let worker_program: RoleProgram<1> = project(&nested_split_route_program);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &worker_program,
                                NoBinding,
                            )
                            .expect("attach nested worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();

                    let outer_scope = worker.cursor.node_scope_id();
                    assert!(
                        !outer_scope.is_none(),
                        "worker must start at outer route scope"
                    );
                    let nested_scope = worker
                        .cursor
                        .seek_label_index(INNER_LEFT_DATA_LABEL)
                        .map(|idx| worker.cursor.node_scope_id_at(idx))
                        .expect("nested route recv label must exist");
                    let left_entry = worker.cursor.index();
                    let right_entry = worker
                        .route_scope_offer_entry_index(nested_scope)
                        .expect("nested route must retain an offer entry");

                    worker
                        .test_commit_route_arm(0, outer_scope, 1)
                        .expect("select outer right arm");
                    worker.set_cursor_index(right_entry);
                    RouteFrontierMachine::new(&mut *worker)
                        .align_cursor_to_selected_scope()
                        .expect("selected nested route must become current scope");
                    worker.refresh_lane_offer_state(0);
                    worker.refresh_lane_offer_state(2);

                    let left_info = worker.route_state.lane_offer_state(0);
                    let right_info = worker.route_state.lane_offer_state(2);
                    assert_eq!(left_info.scope, outer_scope);
                    assert_eq!(state_index_to_usize(left_info.entry), left_entry);
                    assert_eq!(right_info.scope, nested_scope);
                    assert_eq!(state_index_to_usize(right_info.entry), right_entry);
                    assert!(
                        worker.cursor.max_frontier_entries() >= 2,
                        "nested split fixture must retain two compiled frontier slots"
                    );
                    let active_entries = worker.global_active_entries();
                    assert_eq!(active_entries.occupancy_mask(), 0b11);
                    let (
                        _cached_key_slots,
                        _cached_offer_lane_words,
                        _cached_binding_lane_words,
                        cached_key,
                    ) = copied_frontier_observation_key_storage(
                        RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        ),
                        worker.cursor.max_frontier_entries(),
                        worker.cursor.logical_lane_count(),
                    );
                    let (_cached_observed_slots, mut cached_observed_entries) =
                        observed_entry_set_storage(worker.cursor.max_frontier_entries());
                    for entry_idx in [left_entry, right_entry] {
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let observed = worker
                            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                            .expect("cached observed state");
                        let (observed_bit, inserted) = cached_observed_entries
                            .insert_entry(entry_idx)
                            .expect("insert cached observed entry");
                        assert!(inserted);
                        cached_observed_entries.observe_with_frontier_mask(
                            observed_bit,
                            observed,
                            worker.offer_entry_frontier_mask(entry_idx, entry_state),
                        );
                    }
                    let left_bit = cached_observed_entries.entry_bit(left_entry);
                    let right_bit = cached_observed_entries.entry_bit(right_entry);
                    assert_eq!(left_bit, 1u8 << 0);
                    assert_eq!(right_bit, 1u8 << 1);
                    let cached_left_ready = cached_observed_entries.ready_mask & left_bit;
                    let cached_left_progress = cached_observed_entries.progress_mask & left_bit;
                    let cached_right_ready = cached_observed_entries.ready_mask & right_bit;
                    let cached_right_progress = cached_observed_entries.progress_mask & right_bit;
                    let inner_left_data_frame =
                        frame_label_for_cursor_label(&worker.cursor, INNER_LEFT_DATA_LABEL);

                    assert!(worker.binding_inbox.push_back(
                        2,
                        crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(inner_left_data_frame),
                            channel: crate::binding::Channel::new(7),
                            instance: 7,
                            has_fin: false,
                        },
                    ));
                    let observation_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );
                    let changed_slot_mask = worker
                        .cached_frontier_changed_entry_slot_mask(
                            ScopeId::none(),
                            false,
                            observation_key,
                            cached_key,
                        )
                        .expect("same active frontier must stay structurally reusable");
                    let expected_right = worker
                        .recompute_offer_entry_observed_state_non_consuming(right_entry)
                        .expect("expected right observed state");

                    assert_eq!(
                        changed_slot_mask, right_bit,
                        "lane-2 binding changes must invalidate only the secondary frontier slot"
                    );

                    let refreshed = worker
                        .refresh_frontier_observed_entries_from_cache(
                            ScopeId::none(),
                            false,
                            active_entries,
                            observation_key,
                            cached_key,
                            cached_observed_entries,
                        )
                        .expect("same active frontier should refresh changed entry slots in place");

                    assert_eq!(refreshed.entry_bit(left_entry), left_bit);
                    assert_eq!(refreshed.entry_bit(right_entry), right_bit);
                    assert_eq!(
                        refreshed.ready_mask & left_bit,
                        cached_left_ready,
                        "lane-2 updates must not rewrite slot 0 readiness"
                    );
                    assert_eq!(
                        refreshed.progress_mask & left_bit,
                        cached_left_progress,
                        "lane-2 updates must not rewrite slot 0 progress"
                    );
                    assert_eq!(
                        refreshed.ready_mask & right_bit != 0,
                        (expected_right.flags & OfferEntryObservedState::FLAG_READY) != 0
                    );
                    assert_eq!(
                        refreshed.progress_mask & right_bit != 0,
                        expected_right.has_progress_evidence()
                    );
                    assert!(
                        refreshed.ready_mask & right_bit != cached_right_ready
                            || refreshed.progress_mask & right_bit != cached_right_progress,
                        "slot 1 must refresh at least one observed bit from the changed lane-2 binding state"
                    );
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn offer_entry_reentry_prefers_first_ready_lane_candidate()
 {
    let current_scope = ScopeId::generic(11);
    let current_parallel_root = ScopeId::generic(7);
    let mut ready_entry_idx = None;
    let mut any_entry_idx = None;
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(20),
            entry_idx: 9,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, false, false),
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(21),
            entry_idx: 10,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(20),
            entry_idx: 9,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );

    assert_eq!(any_entry_idx, Some(9));
    assert_eq!(ready_entry_idx, Some(10));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_controller_without_evidence_yields_to_progress_sibling()
 {
    assert!(!current_entry_is_candidate(true, true, false, 1, true,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_controller_without_evidence_keeps_priority_without_progress_sibling()
 {
    assert!(current_entry_is_candidate(true, true, false, 1, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_controller_without_alternative_keeps_priority()
 {
    assert!(current_entry_is_candidate(true, true, false, 0, true,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn current_controller_with_evidence_keeps_priority()
 {
    assert!(current_entry_is_candidate(true, true, true, 1, true,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn controller_candidate_with_no_evidence_stays_blocked_when_current_has_offer_lanes()
 {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn controller_candidate_without_progress_stays_blocked_in_passive_frontier()
 {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_current_is_suppressed_only_by_controller_progress_sibling()
 {
    assert!(should_suppress_current_passive_without_evidence(
        FrontierKind::PassiveObserver,
        false,
        false,
        true,
    ));
    assert!(!should_suppress_current_passive_without_evidence(
        FrontierKind::PassiveObserver,
        false,
        false,
        false,
    ));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn evidence_less_non_current_candidate_requires_progress_or_unrunnable_current()
 {
    assert!(!candidate_participates_in_frontier_arbitration(
        10, 7, false, false,
    ));
    assert!(candidate_participates_in_frontier_arbitration(
        10, 7, false, true,
    ));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_recv_cursor_is_not_progress_evidence_for_sibling_preempt()
 {
    assert!(!candidate_has_progress_evidence(false, false, false));
    assert!(candidate_has_progress_evidence(true, false, false));
    assert!(candidate_has_progress_evidence(false, true, false));
    assert!(candidate_has_progress_evidence(false, false, true));
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn has_progress_controller_sibling(
    snapshot: FrontierSnapshot,
    scope_id: ScopeId,
    entry_idx: usize,
) -> bool {
    snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .is_some_and(|candidate| {
            candidate.scope_id != scope_id || candidate.entry_idx as usize != entry_idx
        })
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_frontier_detects_progress_controller_sibling()
 {
    let current_scope = ScopeId::generic(71);
    let controller_scope = ScopeId::generic(72);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: controller_scope,
        entry_idx: 53,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        63,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    assert!(has_progress_controller_sibling(snapshot, current_scope, 63));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_frontier_ignores_controller_without_progress_evidence()
 {
    let current_scope = ScopeId::generic(171);
    let controller_scope = ScopeId::generic(172);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: controller_scope,
        entry_idx: 53,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        63,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    assert!(!has_progress_controller_sibling(
        snapshot,
        current_scope,
        63
    ));
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_frontier_ignores_non_controller_sibling_for_controller_preemption()
 {
    let current_scope = ScopeId::generic(81);
    let sibling_scope = ScopeId::generic(82);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 59,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        63,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    assert!(!has_progress_controller_sibling(
        snapshot,
        current_scope,
        63
    ));
}
