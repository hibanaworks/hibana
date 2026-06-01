use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
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
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .register_rendezvous(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1001);
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

                        if crate::eff::meta::MAX_EFF_NODES < 2 {
                            return;
                        }

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key =
                            crate::endpoint::kernel::CursorEndpoint::frontier_observation_key(
                                &worker,
                                FrontierObservationDomain::global(),
                            );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 29;

                        let current_scope_slot = worker
                            .scope_slot_for_route(worker.cursor.node_scope_id())
                            .expect("current node scope should be a route scope");
                        if worker.cursor.route_scope_count() < 2 {
                            return;
                        }
                        let unrelated_slot = if current_scope_slot == 0 { 1 } else { 0 };
                        worker.decision_state.scope_evidence[unrelated_slot].ready_arm_mask =
                            ScopeEvidence::ARM0_READY;
                        worker.bump_scope_evidence_generation(unrelated_slot);

                        (&mut *worker).align_cursor_to_selected_scope().expect(
                            "unrelated scope evidence must not invalidate cached observation",
                        );

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 29,
                            "cache hit must survive unrelated-scope evidence updates"
                        );
                    });
                }
            );
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
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
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .register_rendezvous(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1002);
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
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key =
                            crate::endpoint::kernel::CursorEndpoint::frontier_observation_key(
                                &worker,
                                FrontierObservationDomain::global(),
                            );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 31;

                        worker.refresh_lane_offer_state(2);

                        (&mut *worker).align_cursor_to_selected_scope()
                            .expect("unrelated lane frontier refresh must not invalidate cached observation");

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 31,
                            "cache hit must survive unrelated-lane frontier refresh"
                        );
                    });
                }
            );
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative",
        || {
            offer_fixture!(2048, clock, config);
            let nested_program = NESTED_ROUTE_PROGRAM();
            let worker_program = project(&nested_program);
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .register_rendezvous(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1004);
                    type WorkerEndpoint = CursorEndpoint<
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
                    let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                                rv_id,
                                sid,
                                &worker_program,
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker =
                        unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                    let nested_scope = worker
                        .cursor
                        .seek_label_index(ENTRY_ARM0_SIGNAL_LABEL)
                        .map(|idx| worker.cursor.node_scope_id_at(idx))
                        .expect("nested route recv label must exist");

                    worker.refresh_lane_offer_state(0);
                    let outer_scope = worker.cursor.node_scope_id();
                    let outer_entry = worker.cursor.index();
                    let nested_entry = worker
                        .route_scope_offer_entry_index(nested_scope)
                        .expect("nested route must have offer entry");

                    assert_ne!(outer_entry, nested_entry);
                    worker
                        .test_commit_route_arm(0, outer_scope, 1)
                        .expect("select outer nested arm");
                    worker
                        .test_commit_route_arm(0, nested_scope, 0)
                        .expect("select nested arm");
                    worker.set_cursor_index(nested_entry);

                    assert_eq!(
                        worker.cursor.node_scope_id(),
                        nested_scope,
                        "cursor must already be positioned at the descended nested route",
                    );
                    assert_eq!(
                        worker.current_offer_scope_id(),
                        nested_scope,
                        "selected nested route must become the current offer scope",
                    );
                    assert_eq!(
                        worker.decision_state.lane_offer_state(0).scope,
                        outer_scope,
                        "pre-align lane state intentionally still points at the ancestor route",
                    );

                    (worker)
                        .align_cursor_to_selected_scope()
                        .expect("selected nested route entry should remain authoritative");

                    assert_eq!(
                        worker.cursor.index(),
                        nested_entry,
                        "align must not bounce a selected nested route entry back to the ancestor scope",
                    );
                    assert_eq!(worker.current_offer_scope_id(), nested_scope);
                    unsafe {
                        core::ptr::drop_in_place(worker);
                    }
                }
            );
        },
    );
}
