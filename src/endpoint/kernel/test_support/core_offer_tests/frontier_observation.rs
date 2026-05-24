fn observed_entries_with_ready_current_only(
    current_idx: usize,
) -> (std::vec::Vec<FrontierObservationSlot>, ObservedEntrySet) {
    observed_entry_set_from_states(&[(
        current_idx,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY,
        },
    )])
}

#[test]
fn refresh_cached_frontier_observation_entry_updates_stable_slot_in_place() {
    run_offer_regression_test(
        "refresh_cached_frontier_observation_entry_updates_stable_slot_in_place",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1013);
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

                    worker.refresh_lane_offer_state(0);
                    let current_idx = worker.cursor.index();
                    let mut summary = worker.compute_offer_entry_static_summary(current_idx);
                    summary.flags &= !OfferEntryStaticSummary::FLAG_STATIC_READY;
                    worker
                        .route_state
                        .lane_offer_state_mut(0)
                        .expect("lane 0 offer state")
                        .static_ready = false;

                    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(1);
                    let (observed_bit, inserted) = observed_entries
                        .insert_entry(current_idx)
                        .expect("insert current entry");
                    assert!(inserted);
                    observed_entries.observe(
                        observed_bit,
                        offer_entry_observed_state(
                            worker
                                .offer_entry_state_snapshot(current_idx)
                                .expect("offer entry state snapshot")
                                .scope_id,
                            summary,
                            false,
                            false,
                            false,
                        ),
                    );
                    overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                    let stored_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );
                    overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                    worker.frontier_state.frontier_observation_epoch = 41;
                    assert_eq!(
                        worker.frontier_state.global_frontier_observed.ready_mask & observed_bit,
                        0
                    );

                    worker
                        .route_state
                        .lane_offer_state_mut(0)
                        .expect("lane 0 offer state")
                        .static_ready = true;
                    let updated_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );
                    assert!(
                        worker
                            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                            .is_none(),
                        "summary fingerprint change should invalidate the stale cached key before patching",
                    );

                    assert!(
                        worker.refresh_cached_frontier_observation_entry(
                            ScopeId::none(),
                            false,
                            current_idx
                        ),
                        "stable active-entry slot should patch the cached frontier observation in place",
                    );
                    assert!(
                        worker.cached_global_frontier_observation_key() == updated_key,
                        "targeted patch should publish the refreshed observation under the new key",
                    );
                    let current_bit = worker
                        .global_frontier_observed_entries()
                        .entry_bit(current_idx);
                    assert_ne!(current_bit, 0);
                    assert_ne!(
                        worker.frontier_state.global_frontier_observed.ready_mask & current_bit,
                        0,
                        "patched observation should reflect the updated static ready bit",
                    );
                    assert!(
                        worker.frontier_state.frontier_observation_epoch > 41,
                        "targeted patch should publish a fresh frontier observation epoch",
                    );
                });
            });
        },
    );
}

#[test]
fn observed_entry_set_move_entry_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    let (fake_bit, inserted_fake) = observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake entry");
    assert!(inserted_fake);
    observed_entries.observe(
        fake_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    );

    assert!(observed_entries.move_entry_slot(fake_entry_idx, 0));
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 1u8 << 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 1);
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Parallel),
        1 << 0
    );
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Route),
        1u8 << 1
    );
}

#[test]
fn observed_entry_set_insert_observation_at_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );

    assert!(observed_entries.insert_observation_at_slot(
        fake_entry_idx,
        0,
        FrontierObservationSlot {
            entry: StateIndex::new(fake_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 1u8 << 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 1);
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Parallel),
        1 << 0
    );
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Route),
        1u8 << 1
    );
}

#[test]
fn observed_entry_set_remove_observation_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    assert!(observed_entries.insert_observation_at_slot(
        fake_entry_idx,
        0,
        FrontierObservationSlot {
            entry: StateIndex::new(fake_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));

    assert!(observed_entries.remove_observation(fake_entry_idx));
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 0);
    assert_eq!(observed_entries.controller_mask, 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 0);
    assert_eq!(observed_entries.ready_mask, 1u8 << 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Parallel), 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Route), 1 << 0);
}

#[test]
fn observed_entry_set_replace_entry_at_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let old_entry_idx = 23usize;
    let new_entry_idx = 29usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    assert!(observed_entries.insert_observation_at_slot(
        old_entry_idx,
        1,
        FrontierObservationSlot {
            entry: StateIndex::new(old_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));

    assert!(observed_entries.replace_entry_at_slot(
        old_entry_idx,
        new_entry_idx,
        FrontierObservationSlot {
            entry: StateIndex::new(new_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(9),
            frontier_mask: FrontierKind::Loop.bit(),
            flags: OfferEntryObservedState::FLAG_READY_ARM | OfferEntryObservedState::FLAG_DYNAMIC,
        },
    ));
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(old_entry_idx), 0);
    assert_eq!(observed_entries.entry_bit(new_entry_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 0);
    assert_eq!(observed_entries.dynamic_controller_mask, 1u8 << 1);
    assert_eq!(observed_entries.progress_mask, 1u8 << 0);
    assert_eq!(observed_entries.ready_arm_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Parallel), 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Loop), 1u8 << 1);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Route), 1 << 0);
}

#[test]
fn frontier_observation_structural_entry_detection_is_exact() {
    with_active_entry_set_storage(2, |cached_entries| {
        assert!(cached_entries.insert_entry(11, 0));
        assert!(cached_entries.insert_entry(17, 0));

        with_frontier_observation_key_storage(2, 1, |cached_key| {
            cached_key.set_active_entries_from(*cached_entries);

            with_active_entry_set_storage(3, |inserted_entries| {
                inserted_entries.copy_from(*cached_entries);
                assert!(inserted_entries.insert_entry(23, 0));
                assert_eq!(
                    CursorEndpoint::<
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        EpochTbl,
                        4,
                    >::structural_inserted_entry_idx(
                        *inserted_entries, *cached_key
                    ),
                    Some(23)
                );

                with_frontier_observation_key_storage(3, 1, |inserted_key| {
                    inserted_key.set_active_entries_from(*inserted_entries);
                    assert_eq!(
                        CursorEndpoint::<
                            1,
                            HintOnlyTransport,
                            DefaultLabelUniverse,
                            CounterClock,
                            EpochTbl,
                            4,
                        >::structural_removed_entry_idx(
                            *cached_entries, *inserted_key
                        ),
                        Some(23)
                    );
                });
            });

            with_active_entry_set_storage(2, |replaced_entries| {
                assert!(replaced_entries.insert_entry(11, 0));
                assert!(replaced_entries.insert_entry(19, 0));
                assert_eq!(
                    CursorEndpoint::<
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        EpochTbl,
                        4,
                    >::structural_replaced_entry_idx(
                        *replaced_entries, *cached_key
                    ),
                    Some(19)
                );
            });
        });
    });

    with_active_entry_set_storage(2, |shifted_entries| {
        assert!(shifted_entries.insert_entry(17, 0));
        assert!(shifted_entries.insert_entry(11, 1));
        with_active_entry_set_storage(2, |shifted_cached_entries| {
            assert!(shifted_cached_entries.insert_entry(11, 0));
            assert!(shifted_cached_entries.insert_entry(17, 1));
            with_frontier_observation_key_storage(2, 1, |shifted_cached_key| {
                shifted_cached_key.set_active_entries_from(*shifted_cached_entries);
                assert_eq!(
                    CursorEndpoint::<
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        EpochTbl,
                        4,
                    >::structural_shifted_entry_idx(
                        *shifted_entries, *shifted_cached_key
                    ),
                    Some(17)
                );
            });
        });
    });
}

#[test]
fn cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes() {
    run_offer_regression_test(
        "cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1013);
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

                    assert!(worker.cursor.logical_lane_count() > 2);
                    worker.refresh_lane_offer_state(0);
                    let current_idx = worker.cursor.index();
                    let (_active_slots, active_entries) =
                        active_entry_set_from_pairs(&[(current_idx, 0)]);
                    overwrite_global_active_entries_fixture(&mut *worker, active_entries);

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
                    let unrelated = crate::binding::IngressEvidence {
                        frame_label: FrameLabel::new(91),
                        channel: crate::binding::Channel::new(7),
                        instance: 7,
                        has_fin: false,
                    };
                    assert!(worker.binding_inbox.push_back(2, unrelated));
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
                        .expect("active frontier is unchanged");

                    assert_eq!(
                        changed_slot_mask, 0,
                        "route changes on non-representative offer lanes must not invalidate the entry"
                    );
                });
            });
        },
    );
}

#[test]
fn refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots() {
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
fn offer_entry_reentry_prefers_first_ready_lane_candidate() {
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
fn current_controller_without_evidence_yields_to_progress_sibling() {
    assert!(!current_entry_is_candidate(true, true, false, 1, true,));
}

#[test]
fn current_controller_without_evidence_keeps_priority_without_progress_sibling() {
    assert!(current_entry_is_candidate(true, true, false, 1, false,));
}

#[test]
fn current_controller_without_alternative_keeps_priority() {
    assert!(current_entry_is_candidate(true, true, false, 0, true,));
}

#[test]
fn current_controller_with_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, true, true, 1, true,));
}

#[test]
fn controller_candidate_with_no_evidence_stays_blocked_when_current_has_offer_lanes() {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
fn controller_candidate_without_progress_stays_blocked_in_passive_frontier() {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
fn passive_current_is_suppressed_only_by_controller_progress_sibling() {
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
fn evidence_less_non_current_candidate_requires_progress_or_unrunnable_current() {
    assert!(!candidate_participates_in_frontier_arbitration(
        10, 7, false, false,
    ));
    assert!(candidate_participates_in_frontier_arbitration(
        10, 7, false, true,
    ));
}

#[test]
fn passive_recv_cursor_is_not_progress_evidence_for_sibling_preempt() {
    assert!(!candidate_has_progress_evidence(false, false, false));
    assert!(candidate_has_progress_evidence(true, false, false));
    assert!(candidate_has_progress_evidence(false, true, false));
    assert!(candidate_has_progress_evidence(false, false, true));
}

fn has_progress_controller_sibling(
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
fn passive_frontier_detects_progress_controller_sibling() {
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
fn passive_frontier_ignores_controller_without_progress_evidence() {
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
fn passive_frontier_ignores_non_controller_sibling_for_controller_preemption() {
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

#[test]
fn frontier_yield_ping_pong_is_bounded() {
    let mut visited_slots = frontier_visit_slots::<2>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    let scope_a = ScopeId::generic(31);
    let scope_b = ScopeId::generic(32);
    visited.record(scope_a);
    visited.record(scope_b);
    visited.record(scope_a);
    assert!(visited.contains(scope_a));
    assert!(visited.contains(scope_b));
    assert_eq!(visited.len, 2);
}

#[test]
fn route_defer_yields_to_sibling_scope() {
    let current_scope = ScopeId::generic(41);
    let sibling_scope = ScopeId::generic(42);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 10,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 12,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        10,
        ScopeId::none(),
        FrontierKind::Route,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("route frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
fn loop_defer_yields_to_sibling_scope() {
    let current_scope = ScopeId::generic(51);
    let sibling_scope = ScopeId::generic(52);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        20,
        ScopeId::none(),
        FrontierKind::Loop,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("loop frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Loop);
}

#[test]
fn defer_yields_across_frontier_in_same_parallel_root() {
    let root = ScopeId::generic(55);
    let current_scope = ScopeId::generic(56);
    let sibling_scope = ScopeId::generic(57);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: root,
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        20,
        root,
        FrontierKind::Loop,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("defer must yield to progress sibling in same parallel root");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
fn parallel_frontier_prefers_ready_lane_before_phase_join() {
    let current_scope = ScopeId::generic(61);
    let root = ScopeId::generic(60);
    let ready_scope = ScopeId::generic(62);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 30,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: ScopeId::generic(63),
        entry_idx: 31,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(false, false, false, false),
    };
    candidates[2] = FrontierCandidate {
        scope_id: ready_scope,
        entry_idx: 32,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(false, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        30,
        root,
        FrontierKind::Parallel,
        &mut candidates,
        3,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("parallel frontier must choose progress sibling");
    assert_eq!(picked.scope_id, ready_scope);
    assert_eq!(picked.entry_idx, 32);
}

#[test]
fn passive_observer_defer_follow_is_progressive() {
    let current_scope = ScopeId::generic(71);
    let sibling_scope = ScopeId::generic(72);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 40,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 44,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        40,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    let mut visited_slots = frontier_visit_slots::<1>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    visited.record(current_scope);
    let picked = snapshot
        .select_yield_candidate(visited)
        .expect("passive observer defer must progress to sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_ne!(picked.scope_id, current_scope);
}

#[test]
fn passive_observer_defer_stops_without_progress_evidence() {
    let root = ScopeId::generic(73);
    let current_scope = ScopeId::generic(74);
    let sibling_scope = ScopeId::generic(75);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 50,
        parallel_root: root,
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 53,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        50,
        root,
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    let mut visited_slots = frontier_visit_slots::<1>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    visited.record(current_scope);
    assert_eq!(snapshot.select_yield_candidate(visited), None);
}
