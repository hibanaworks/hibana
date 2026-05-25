use crate::endpoint::kernel::core::offer_regression_tests::cases::*;

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn attach_endpoint_keeps_primary_lane_on_first_live_application_lane()
 {
    run_offer_regression_test(
        "attach_endpoint_keeps_primary_lane_on_first_live_application_lane",
        || {
            offer_fixture!(2048, clock, config);
            type LaneThreeWorkerSteps =
                StepCons<SendStep<Role<0>, Role<1>, Msg<0x66, u8>, 3>, StepNil>;
            let lane_three_program: g::Program<LaneThreeWorkerSteps> =
                g::send::<Role<0>, Role<1>, Msg<0x66, u8>, 3>();

            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(998);
                    let worker_program: RoleProgram<1> = project(&lane_three_program);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &worker_program,
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    assert_eq!(
                        worker.primary_lane, 3,
                        "primary lane must follow the first live application lane instead of falling back to lane 0",
                    );
                    assert!(
                        worker.ports[worker.primary_lane].is_some(),
                        "the live primary lane must hold the leased primary port"
                    );
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn selection_materialization_helpers_match_reference_lookup_logic()
 {
    run_offer_regression_test(
        "selection_materialization_helpers_match_reference_lookup_logic",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(999);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
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
                        let controller = controller_slot.borrow_mut();
                        let worker = worker_slot.borrow_mut();

                        controller.refresh_lane_offer_state(0);
                        let controller_scope = controller.cursor.node_scope_id();
                        let controller_selection = RouteFrontierMachine::new(&mut *controller)
                            .select_scope()
                            .expect("controller selection");
                        worker.refresh_lane_offer_state(0);
                        let worker_scope = worker.cursor.node_scope_id();
                        let worker_selection = RouteFrontierMachine::new(&mut *worker)
                            .select_scope()
                            .expect("worker selection");

                        let mut arm = 0u8;
                        while arm <= 1 {
                            assert_eq!(
                                controller.selection_arm_requires_materialization_ready_evidence(
                                    controller_selection,
                                    true,
                                    arm,
                                ),
                                controller.arm_requires_materialization_ready_evidence(
                                    controller_scope,
                                    arm
                                )
                            );
                            assert_eq!(
                                worker.selection_arm_requires_materialization_ready_evidence(
                                    worker_selection,
                                    false,
                                    arm,
                                ),
                                if worker_selection.at_route_offer_entry
                                    && worker
                                        .selection_materialization_meta(worker_selection)
                                        .passive_arm_entry(arm)
                                        .is_some()
                                {
                                    if worker
                                        .selection_materialization_meta(worker_selection)
                                        .arm_has_first_recv_dispatch(arm)
                                    {
                                        !worker.selection_arm_dispatch_materializes_without_ready_evidence(
                                            worker_selection,
                                            arm,
                                        )
                                    } else {
                                        false
                                    }
                                } else {
                                    worker.arm_requires_materialization_ready_evidence(
                                        worker_scope,
                                        arm,
                                    )
                                }
                            );
                            assert_eq!(
                                controller.selection_non_wire_loop_control_recv(
                                    controller_selection,
                                    true,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL,
                                ),
                                controller.is_non_wire_loop_control_recv(
                                    controller_scope,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL,
                                )
                            );
                            assert_eq!(
                                controller.selection_non_wire_loop_control_recv(
                                    controller_selection,
                                    true,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL,
                                ),
                                controller.is_non_wire_loop_control_recv(
                                    controller_scope,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL,
                                )
                            );
                            assert_eq!(
                                worker.selection_non_wire_loop_control_recv(
                                    worker_selection,
                                    false,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL,
                                ),
                                worker.is_non_wire_loop_control_recv(
                                    worker_scope,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL
                                )
                            );
                            assert_eq!(
                                worker.selection_non_wire_loop_control_recv(
                                    worker_selection,
                                    false,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL,
                                ),
                                worker.is_non_wire_loop_control_recv(
                                    worker_scope,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL
                                )
                            );
                            if arm == 1 {
                                break;
                            }
                            arm += 1;
                        }
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_arm_materialization_meta_caches_passive_recv_meta_exactly()
 {
    run_offer_regression_test(
        "scope_arm_materialization_meta_caches_passive_recv_meta_exactly",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(998);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &ENTRY_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    worker.refresh_lane_offer_state(0);
                    let offer_lane = worker.offer_lane_for_scope(scope);
                    let materialization_meta = worker.compute_scope_arm_materialization_meta(scope);
                    let passive_recv_meta = worker.compute_scope_passive_recv_meta(
                        materialization_meta,
                        scope,
                        offer_lane,
                    );
                    let region = worker
                        .cursor
                        .scope_region_by_id(scope)
                        .expect("scope region should exist");

                    let mut arm = 0u8;
                    while arm <= 1 {
                        let expected = worker
                            .cursor
                            .follow_passive_observer_arm_for_scope(scope, arm)
                            .map(|nav| match nav {
                                PassiveArmNavigation::WithinArm { entry } => entry,
                            })
                            .and_then(|entry| {
                                let target_idx = state_index_to_usize(entry);
                                if let Some(recv_meta) = worker.cursor.try_recv_meta_at(target_idx)
                                {
                                    return Some((target_idx, recv_meta));
                                }
                                if let Some(send_meta) = worker.cursor.try_send_meta_at(target_idx)
                                {
                                    return Some((
                                        target_idx,
                                        RecvMeta {
                                            eff_index: send_meta.eff_index,
                                            label: send_meta.label,
                                            frame_label: send_meta.frame_label,
                                            peer: send_meta.peer,
                                            resource: send_meta.resource,
                                            semantic: send_meta.semantic,
                                            is_control: send_meta.is_control,
                                            next: target_idx,
                                            scope,
                                            route_arm: Some(arm),
                                            is_choice_determinant: false,
                                            shot: send_meta.shot,
                                            policy: send_meta.policy(),
                                            lane: send_meta.lane,
                                        },
                                    ));
                                }
                                if worker.cursor.is_jump_at(target_idx) {
                                    let scope_end =
                                        worker.cursor.jump_target_at(target_idx).unwrap_or(0);
                                    if region.linger {
                                        let (controller_entry, synthetic_label) =
                                            materialization_meta.controller_arm_entry(arm)?;
                                        let synthetic_semantic = loop_control_semantic_kind(
                                            worker.cursor.control_semantic_at(
                                                state_index_to_usize(controller_entry),
                                            ),
                                        )
                                        .unwrap_or(ControlSemanticKind::RouteArm);
                                        return Some((
                                            scope_end,
                                            RecvMeta {
                                                eff_index: EffIndex::ZERO,
                                                label: synthetic_label,
                                                frame_label: 0,
                                                peer: 1,
                                                resource: None,
                                                semantic: synthetic_semantic,
                                                is_control: true,
                                                next: scope_end,
                                                scope,
                                                route_arm: Some(arm),
                                                is_choice_determinant: false,
                                                shot: None,
                                                policy: PolicyMode::static_mode(),
                                                lane: offer_lane,
                                            },
                                        ));
                                    }
                                    if let Some(recv_meta) =
                                        worker.cursor.try_recv_meta_at(scope_end)
                                    {
                                        return Some((scope_end, recv_meta));
                                    }
                                    if let Some(send_meta) =
                                        worker.cursor.try_send_meta_at(scope_end)
                                    {
                                        return Some((
                                            scope_end,
                                            RecvMeta {
                                                eff_index: send_meta.eff_index,
                                                label: send_meta.label,
                                                frame_label: send_meta.frame_label,
                                                peer: send_meta.peer,
                                                resource: send_meta.resource,
                                                semantic: send_meta.semantic,
                                                is_control: send_meta.is_control,
                                                next: scope_end,
                                                scope,
                                                route_arm: Some(arm),
                                                is_choice_determinant: false,
                                                shot: send_meta.shot,
                                                policy: send_meta.policy(),
                                                lane: send_meta.lane,
                                            },
                                        ));
                                    }
                                    return None;
                                }
                                if region.linger {
                                    let (controller_entry, synthetic_label) =
                                        materialization_meta.controller_arm_entry(arm)?;
                                    let synthetic_semantic = loop_control_semantic_kind(
                                        worker.cursor.control_semantic_at(state_index_to_usize(
                                            controller_entry,
                                        )),
                                    )
                                    .unwrap_or(ControlSemanticKind::RouteArm);
                                    return Some((
                                        target_idx,
                                        RecvMeta {
                                            eff_index: EffIndex::ZERO,
                                            label: synthetic_label,
                                            frame_label: 0,
                                            peer: 1,
                                            resource: None,
                                            semantic: synthetic_semantic,
                                            is_control: true,
                                            next: target_idx,
                                            scope,
                                            route_arm: Some(arm),
                                            is_choice_determinant: false,
                                            shot: None,
                                            policy: PolicyMode::static_mode(),
                                            lane: offer_lane,
                                        },
                                    ));
                                }
                                None
                            });
                        let cached = passive_recv_meta
                            .get(arm as usize)
                            .copied()
                            .and_then(|meta| meta.recv_meta());
                        assert_eq!(cached, expected);
                        if region.linger {
                            assert!(
                                materialization_meta.controller_arm_entry(arm).is_some(),
                                "passive linger route must retain controller arm facts for arm {arm}"
                            );
                            let cached_semantic = cached.map(|(_, meta)| meta.semantic);
                            let expected_semantic = materialization_meta
                                .controller_arm_entry(arm)
                                .and_then(|(entry, _)| {
                                    loop_control_semantic_kind(
                                        worker
                                            .cursor
                                            .control_semantic_at(state_index_to_usize(entry)),
                                    )
                                });
                            assert_eq!(cached_semantic, expected_semantic);
                        }
                        if arm == 1 {
                            break;
                        }
                        arm += 1;
                    }
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_skips_observation_for_single_active_entry()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_skips_observation_for_single_active_entry",
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
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(998);
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
                        assert!(
                            worker
                                .active_frontier_entries(None)
                                .contains_only(current_idx)
                        );
                        let observed_key = worker.cached_global_frontier_observation_key();
                        let observed_entries = worker.global_frontier_observed_entries();

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("single current entry should select directly");

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert!(
                            worker.cached_global_frontier_observation_key() == observed_key,
                            "single-active fast path must not rebuild cached observation key during align"
                        );
                        assert!(
                            worker
                                .global_frontier_observed_entries()
                                .entry_bit(current_idx)
                                == observed_entries.entry_bit(current_idx)
                                && worker.frontier_state.global_frontier_observed.progress_mask
                                    == observed_entries.progress_mask
                                && worker
                                    .frontier_state
                                    .global_frontier_observed
                                    .ready_arm_mask
                                    == observed_entries.ready_arm_mask
                                && worker.frontier_state.global_frontier_observed.ready_mask
                                    == observed_entries.ready_mask,
                            "single-active fast path must not rebuild observation during align"
                        );
                    });
                }
            );
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_reuses_cached_multi_entry_observation()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_reuses_cached_multi_entry_observation",
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
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(999);
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
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            FrontierObservationDomain::global(),
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 17;

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("fresh cached observation should be reused");

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 17,
                            "cache hit must not rebuild frontier observation"
                        );
                    });
                }
            );
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes",
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
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1000);
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
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            FrontierObservationDomain::global(),
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 23;

                        let unrelated = crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(91),
                            channel: crate::binding::Channel::new(7),
                            instance: 7,
                            has_fin: false,
                        };
                        assert!(worker.binding_inbox.push_back(2, unrelated));

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect(
                                "unrelated binding changes must not invalidate cached observation",
                            );

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 23,
                            "cache hit must survive unrelated-lane binding updates"
                        );
                    });
                }
            );
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes()
 {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes",
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
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1003);
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
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);

                        let first = crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(31),
                            channel: crate::binding::Channel::new(3),
                            instance: 3,
                            has_fin: false,
                        };
                        let second = crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(32),
                            channel: crate::binding::Channel::new(4),
                            instance: 4,
                            has_fin: false,
                        };
                        assert!(worker.binding_inbox.push_back(0, first));
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            FrontierObservationDomain::global(),
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 27;

                        assert!(worker.binding_inbox.push_back(0, second));

                        RouteFrontierMachine::new(&mut *worker).align_cursor_to_selected_scope().expect(
                            "relevant lane content-only changes must not invalidate cached observation",
                        );

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 27,
                            "cache hit must survive content-only updates on already-nonempty offer lanes"
                        );
                    });
                }
            );
        },
    );
}
