
#[test]
fn attach_endpoint_keeps_primary_lane_on_first_live_application_lane() {
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
fn selection_materialization_helpers_match_reference_lookup_logic() {
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
fn scope_arm_materialization_meta_caches_passive_recv_meta_exactly() {
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
fn align_cursor_to_selected_scope_skips_observation_for_single_active_entry() {
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
fn align_cursor_to_selected_scope_reuses_cached_multi_entry_observation() {
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
                            ScopeId::none(),
                            false,
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
fn align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes() {
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
                            ScopeId::none(),
                            false,
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
fn align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes() {
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
                            ScopeId::none(),
                            false,
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

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes() {
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
                            .add_rendezvous_from_config(config, transport)
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
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
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
                        worker.route_state.scope_evidence[unrelated_slot].ready_arm_mask =
                            ScopeEvidence::ARM0_READY;
                        worker.bump_scope_evidence_generation(unrelated_slot);

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect(
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
fn align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh() {
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
                            .add_rendezvous_from_config(config, transport)
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
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 31;

                        worker.refresh_lane_offer_state(2);

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
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
fn align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative() {
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
                        .add_rendezvous_from_config(config, transport)
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
                        worker.route_state.lane_offer_state(0).scope,
                        outer_scope,
                        "pre-align lane state intentionally still points at the ancestor route",
                    );

                    RouteFrontierMachine::new(worker)
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

#[test]
fn active_entry_set_orders_entries_by_representative_lane() {
    let (_entry_slots, mut entries) = active_entry_set_storage(3);
    assert!(entries.insert_entry(9, 4));
    assert!(entries.insert_entry(3, 1));
    assert!(entries.insert_entry(7, 1));
    assert_eq!(entries.entry_at(0), Some(3));
    assert_eq!(entries.entry_at(1), Some(7));
    assert_eq!(entries.entry_at(2), Some(9));

    assert!(entries.remove_entry(3));
    assert_eq!(entries.entry_at(0), Some(7));
    assert_eq!(entries.entry_at(1), Some(9));
    assert_eq!(entries.occupancy_mask(), 0b0000_0011);
}

#[test]
fn current_passive_without_evidence_keeps_priority_with_controller_present() {
    assert!(!current_entry_is_candidate(false, false, false, 0, false,));
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_passive_with_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, true, 1, false,));
}

#[test]
fn current_passive_without_controller_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_passive_observer_without_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_candidate_stays_selectable_without_route_lane_metadata() {
    assert!(current_entry_matches_after_filter(true, true, 43, None));
}

#[test]
fn current_candidate_respects_hint_filter() {
    assert!(!current_entry_matches_after_filter(
        true,
        true,
        43,
        Some(47)
    ));
}

#[test]
fn current_without_candidate_stays_blocked() {
    assert!(!current_entry_matches_after_filter(false, true, 43, None));
}

#[test]
fn current_without_offer_lanes_stays_blocked() {
    assert!(!current_entry_matches_after_filter(true, false, 43, None));
}

#[test]
fn offer_entry_observed_state_merges_static_summary_and_dynamic_evidence() {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        static_ready: true,
        flags: LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(41), summary, true, false, true);

    assert_eq!(observed.scope_id, ScopeId::generic(41));
    assert!(observed.matches_frontier(FrontierKind::Parallel));
    assert!(observed.is_controller());
    assert!(observed.is_dynamic());
    assert!(observed.has_progress_evidence());
    assert!(observed.has_ready_arm_evidence());
    assert!(observed.binding_ready());
    assert_ne!(observed.flags & OfferEntryObservedState::FLAG_READY, 0);
}

#[test]
fn cached_offer_entry_observed_state_preserves_arbitration_bits() {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(51), summary, true, false, true);
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(1);
    let (observed_bit, inserted) = observed_entries.insert_entry(17).expect("insert entry");
    assert!(inserted);
    observed_entries.observe(observed_bit, observed);

    let cached = cached_offer_entry_observed_state(
        ScopeId::generic(51),
        summary,
        observed_entries,
        observed_bit,
    );
    let original_candidate = offer_entry_frontier_candidate(
        ScopeId::generic(51),
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        observed,
    );
    let cached_candidate = offer_entry_frontier_candidate(
        ScopeId::generic(51),
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        cached,
    );

    assert!(cached.matches_frontier(FrontierKind::PassiveObserver));
    assert!(cached.is_controller());
    assert!(cached.is_dynamic());
    assert!(cached.has_progress_evidence());
    assert!(cached.has_ready_arm_evidence());
    assert!(cached.ready());
    assert_eq!(cached_candidate.scope_id, original_candidate.scope_id);
    assert_eq!(
        cached_candidate.parallel_root,
        original_candidate.parallel_root
    );
    assert_eq!(cached_candidate.frontier, original_candidate.frontier);
    assert_eq!(
        cached_candidate.is_controller(),
        original_candidate.is_controller()
    );
    assert_eq!(
        cached_candidate.is_dynamic(),
        original_candidate.is_dynamic()
    );
    assert_eq!(
        cached_candidate.has_evidence(),
        original_candidate.has_evidence()
    );
    assert_eq!(cached_candidate.ready(), original_candidate.ready());
}

#[test]
fn observed_entry_set_entry_bit_tracks_inserted_entries_exactly() {
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (first_bit, inserted_first) = observed_entries.insert_entry(17).expect("insert first");
    assert!(inserted_first);
    let (second_bit, inserted_second) = observed_entries.insert_entry(3).expect("insert second");
    assert!(inserted_second);
    let (reused_bit, inserted_reused) = observed_entries.insert_entry(17).expect("reuse first");
    assert!(!inserted_reused);
    assert_eq!(reused_bit, first_bit);
    assert_eq!(observed_entries.entry_bit(17), first_bit);
    assert_eq!(observed_entries.entry_bit(3), second_bit);
    assert_eq!(observed_entries.entry_bit(9), 0);
}
