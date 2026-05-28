use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn lane_offer_state_roundtrips_static_frontier_flags()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn refresh_lane_offer_state_caches_scope_frame_label_meta()
 {
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
                    let cached =
                        crate::endpoint::kernel::CursorEndpoint::offer_entry_frame_label_meta(
                            &worker, scope, entry_idx,
                        )
                        .expect("cached offer-entry label metadata");
                    let recv_meta = worker.cursor.try_recv_meta().expect("recv metadata");
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
                            while let Some(dispatch) = worker
                                .cursor
                                .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
                            {
                                if (dispatch.arm() == arm || dispatch.arm() == ARM_SHARED)
                                    && let Some(recv_meta) = worker
                                        .cursor
                                        .try_recv_meta_at(state_index_to_usize(dispatch.target()))
                                    && recv_meta.frame_label == dispatch.frame_label()
                                    && recv_meta.lane == dispatch.lane()
                                    && dispatch.lane() as usize == lane_idx
                                {
                                    expected_binding_demux_lane = true;
                                }
                                dispatch_idx += 1;
                            }
                            assert_eq!(
                                worker.binding_demux_contains_lane(
                                    materialization,
                                    Some(arm),
                                    lane_idx,
                                ),
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
                    while let Some(dispatch) = worker
                        .cursor
                        .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
                    {
                        assert_eq!(
                            materialization.first_recv_target_for_lane_frame_label(
                                dispatch.lane(),
                                dispatch.frame_label()
                            ),
                            Some((dispatch.arm(), dispatch.target()))
                        );
                        dispatch_idx += 1;
                    }
                    assert_eq!(
                        materialization.first_recv_dispatch_len() as usize,
                        dispatch_idx
                    );
                });
            });
        },
    );
}
