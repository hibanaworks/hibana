use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entries_with_ready_current_only(
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn refresh_cached_frontier_observation_entry_updates_stable_slot_in_place()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_move_entry_slot_remaps_masks_exactly()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_insert_observation_at_slot_remaps_masks_exactly()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_remove_observation_remaps_masks_exactly()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_replace_entry_at_slot_remaps_masks_exactly()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_observation_structural_entry_detection_is_exact()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes()
 {
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
