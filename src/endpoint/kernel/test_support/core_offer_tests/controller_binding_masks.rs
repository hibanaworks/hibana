#[test]
fn controller_local_ready_is_not_progress_evidence_for_sibling_preempt() {
    assert!(
        current_entry_is_candidate(true, true, false, 1, false),
        "controller local-ready only must not preempt without progress evidence"
    );
}

#[test]
fn frontier_arbitration_is_uniform_across_route_loop_parallel_observer() {
    let cases = [
        (ScopeId::none(), FrontierKind::Route),
        (ScopeId::none(), FrontierKind::Loop),
        (ScopeId::generic(101), FrontierKind::Parallel),
        (ScopeId::none(), FrontierKind::PassiveObserver),
    ];
    let mut idx = 0usize;
    while idx < cases.len() {
        let (parallel_root, frontier) = cases[idx];
        let current_scope = ScopeId::generic((110 + idx) as u16);
        let sibling_scope = ScopeId::generic((120 + idx) as u16);
        let mut candidates = frontier_candidates::<2>();
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: (70 + idx) as u16,
            parallel_root,
            frontier,
            flags: FrontierCandidate::pack_flags(false, false, false, true),
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: (80 + idx) as u16,
            parallel_root,
            frontier,
            flags: FrontierCandidate::pack_flags(true, true, true, true),
        };
        let snapshot = frontier_snapshot_fixture(
            current_scope,
            70 + idx,
            parallel_root,
            frontier,
            &mut candidates,
            2,
        );
        let picked = snapshot
            .select_yield_candidate(empty_frontier_visit_set())
            .expect("uniform frontier defer must pick progress-evidence-bearing sibling");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_eq!(picked.frontier, frontier);
        idx += 1;
    }
}

#[test]
fn dynamic_route_ignores_hint_evidence_for_authority() {
    run_offer_regression_test("dynamic_route_ignores_hint_evidence_for_authority", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_FRAME);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(904);
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

                    let scope = {
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");
                        assert!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    scope,
                                    0,
                                    HINT_LEFT_DATA_FRAME
                                )
                                .is_none(),
                            "dynamic route arm authority must not depend on first-recv dispatch"
                        );
                        scope
                    };

                    let worker = worker_slot.borrow_mut();
                    let mut cx = Context::from_waker(noop_waker_ref());
                    let branch = {
                        let mut offer = pin!(cursor_offer(worker));
                        match offer.as_mut().poll(&mut cx) {
                            Poll::Ready(Ok(next_branch)) => {
                                panic!(
                                    "dynamic route payload hint selected branch {} without controller authority",
                                    next_branch.label()
                                )
                            }
                            Poll::Ready(Err(err)) => {
                                panic!("offer should not fail before decision: {err:?}")
                            }
                            Poll::Pending => {}
                        }
                        let mut branch = None;
                        {
                            let controller = controller_slot.borrow_mut();
                            controller.port_for_lane(0).record_route_decision(scope, 0);
                        }
                        if branch.is_none() {
                            let mut attempts = 0usize;
                            while attempts < 4 {
                                match offer.as_mut().poll(&mut cx) {
                                    Poll::Ready(Ok(next_branch)) => {
                                        branch = Some(next_branch);
                                        break;
                                    }
                                    Poll::Ready(Err(err)) => {
                                        panic!(
                                            "offer should resolve via authoritative decision: {err:?}"
                                        );
                                    }
                                    Poll::Pending => {}
                                }
                                attempts += 1;
                            }
                        }
                        branch.expect("offer should become ready after authoritative decision")
                    };
                    assert_eq!(
                        branch_label(&branch),
                        HINT_LEFT_DATA_LABEL,
                        "resolved branch must follow authoritative arm, not hint-derived ACK"
                    );
                    worker.restore_materialized_route_branch(branch);
                    let branch = {
                        let mut offer = pin!(cursor_offer(worker));
                        poll_ready_ok(&mut cx, offer.as_mut(), "re-offer after preview drop")
                    };
                    assert_eq!(
                        branch_label(&branch),
                        HINT_LEFT_DATA_LABEL,
                        "dropping a preview branch must leave the same branch re-offerable"
                    );
                    worker.restore_materialized_route_branch(branch);
                    assert!(
                        worker.scope_has_ready_arm_evidence(scope),
                        "dropping a preview branch must not clear ready-arm evidence"
                    );
                    assert!(
                        worker.selected_arm_for_scope(scope).is_none(),
                        "dropping a preview branch must not commit route progress"
                    );
                });
            });
        });
    });
}

#[test]
fn select_scope_prepass_keeps_pending_scope_evidence_non_consuming() {
    run_offer_regression_test(
        "select_scope_prepass_keeps_pending_scope_evidence_non_consuming",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_LEFT_DATA_FRAME);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9041);
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
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        {
                            let controller = controller_slot.borrow_mut();
                            controller.port_for_lane(0).record_route_decision(scope, 0);
                        }
                        let frame_label_meta =
                            endpoint_scope_frame_label_meta(worker, scope, ScopeLoopMeta::EMPTY);
                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let (_binding_ready, has_ack, has_ready_arm_evidence) = worker
                            .preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);
                        assert!(has_ack, "prepass may observe pending ACK authority");
                        assert!(
                            !has_ready_arm_evidence,
                            "pending demux hints must not be promoted to ready-arm evidence during prepass"
                        );

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("scope prepass should succeed without consuming evidence");
                        assert!(
                            worker.peek_scope_ack(scope).is_none(),
                            "prepass must not consume route ACK authority into scope evidence"
                        );
                        assert!(
                            worker.peek_scope_frame_hint(scope).is_none(),
                            "prepass must not consume route hints into scope evidence"
                        );
                        assert_eq!(
                            worker.scope_ready_arm_mask(scope),
                            0,
                            "prepass must not synthesize ready-arm evidence before selected-scope ingest"
                        );
                        assert_eq!(
                            worker.port_for_lane(0).peek_route_decision(scope, 1),
                            Some(0),
                            "authoritative route ACK must remain pending on the port after prepass"
                        );
                        assert!(
                            worker
                                .port_for_lane(0)
                                .has_route_hint_matching(|label| label == HINT_LEFT_DATA_FRAME),
                            "matching route hint must remain queued on the port after prepass"
                        );

                        with_lane_set_view(&[0], |offer_lanes| {
                            worker.ingest_scope_evidence_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                true,
                                frame_label_meta,
                            );
                        });

                        assert_eq!(
                            worker
                                .peek_scope_ack(scope)
                                .map(|token| token.arm().as_u8()),
                            Some(0),
                            "selected-scope ingest must materialize the pending ACK exactly once"
                        );
                        assert!(
                            worker.scope_has_ready_arm_evidence(scope),
                            "selected-scope ingest must materialize ready-arm evidence from the pending hint"
                        );
                        assert_eq!(
                            worker.port_for_lane(0).peek_route_decision(scope, 1),
                            None,
                            "selected-scope ingest must consume the pending ACK from the port"
                        );
                        assert!(
                            !worker
                                .port_for_lane(0)
                                .has_route_hint_matching(|label| label == HINT_LEFT_DATA_FRAME),
                            "selected-scope ingest must consume the route-observation hint after materializing ready-arm evidence"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope() {
    run_offer_regression_test(
        "preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9042);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let scope = {
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            assert!(!scope.is_none(), "worker must start at route scope");
                            scope
                        };

                        {
                            let controller = controller_slot.borrow_mut();
                            controller.port_for_lane(0).record_route_decision(scope, 0);
                        }
                        let worker = worker_slot.borrow_mut();
                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let (binding_ready, has_ack, has_ready_arm_evidence) = worker
                            .preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);

                        assert!(!binding_ready, "empty binding must remain not-ready");
                        assert!(has_ack, "pending route decision must count as ACK evidence");
                        assert!(
                            !has_ready_arm_evidence,
                            "ACK-only preview must not synthesize ready-arm evidence"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            0,
                            "binding probe must be skipped when ACK already supplies progress evidence"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn preview_offer_entry_evidence_defers_binding_poll_until_selected_scope() {
    run_offer_regression_test(
        "preview_offer_entry_evidence_defers_binding_poll_until_selected_scope",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9043);
                        let evidence = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::with_incoming(&[evidence]),
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
                        let (binding_ready, has_ack, has_ready_arm_evidence) = worker
                            .preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);

                        assert!(
                            !binding_ready,
                            "prepass must not probe binding to synthesize ready state"
                        );
                        assert!(
                            !has_ack,
                            "evidence-only prepass must not synthesize ACK authority"
                        );
                        assert!(
                            !has_ready_arm_evidence,
                            "evidence-only prepass must not synthesize ready-arm evidence"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            0,
                            "prepass must not touch binding before selected-scope demux"
                        );

                        let picked = worker.poll_binding_for_offer(
                            scope,
                            entry_state.lane_idx as usize,
                            RouteFrontierMachine::offer_entry_frame_label_meta(
                                &worker, scope, entry_idx,
                            )
                            .expect("descriptor-derived frame label metadata"),
                            worker
                                .offer_entry_materialization_meta(scope, entry_idx)
                                .expect("descriptor-derived materialization metadata"),
                        );
                        assert_eq!(
                            picked,
                            Some((0, evidence)),
                            "selected-scope poll must still resolve the deferred binding evidence"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            1,
                            "binding must be polled exactly once after scope selection"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn hint_or_evidence_never_writes_ack_authority() {
    run_offer_regression_test("hint_or_evidence_never_writes_ack_authority", || {
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
            TestBinding,
        >;
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_FRAME);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(905);
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
                                TestBinding::with_incoming(&[IngressEvidence {
                                    frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                    instance: 0,
                                    has_fin: false,
                                    channel: Channel::new(1),
                                }]),
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    let frame_label_meta =
                        endpoint_scope_frame_label_meta(worker, scope, ScopeLoopMeta::EMPTY);

                    with_lane_set_view(&[0], |offer_lanes| {
                        worker.ingest_scope_evidence_for_offer_lanes(
                            scope,
                            0,
                            offer_lanes,
                            true,
                            frame_label_meta,
                        );
                    });
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "dynamic hint ingest must not mint ack authority"
                    );

                    let mut binding_evidence = None;
                    worker.cache_binding_evidence_for_offer(
                        scope,
                        0,
                        frame_label_meta,
                        worker.offer_scope_materialization_meta(scope, 0),
                        &mut binding_evidence,
                    );
                    assert!(
                        binding_evidence.is_some(),
                        "binding evidence should still be staged for decode/demux"
                    );
                    let evidence = binding_evidence.expect("binding evidence should be available");
                    worker.ingest_binding_scope_evidence(
                        scope,
                        evidence.lane(),
                        evidence.frame_label(),
                        true,
                        frame_label_meta,
                    );
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "evidence must not mint ack authority for dynamic route"
                    );
                    assert_eq!(
                        worker.poll_arm_from_ready_mask(scope),
                        None,
                        "dynamic binding evidence must not materialize Poll authority"
                    );
                });
            });
        });
    });
}

#[test]
fn poll_binding_for_offer_prefers_exact_label_for_ack_arm() {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_exact_label_for_ack_arm",
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
                TestBinding,
            >;
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9044);
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
                                    TestBinding::with_incoming(&[
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                            instance: 7,
                                            has_fin: false,
                                            channel: Channel::new(3),
                                        },
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                                            instance: 9,
                                            has_fin: false,
                                            channel: Channel::new(5),
                                        },
                                    ]),
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
                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label(Some(1)),
                            Some(HINT_RIGHT_DATA_FRAME)
                        );
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let picked = worker.poll_binding_for_offer(
                            scope,
                            entry_state.lane_idx as usize,
                            frame_label_meta,
                            worker
                                .offer_entry_materialization_meta(scope, entry_idx)
                                .expect("descriptor-derived materialization metadata"),
                        );
                        assert_eq!(
                            picked
                                .map(|(lane_idx, evidence)| (lane_idx, evidence.frame_label.raw())),
                            Some((0, HINT_RIGHT_DATA_FRAME)),
                            "authoritative arm should narrow binding demux to the exact matching label"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_LEFT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred.map(|evidence| evidence.frame_label.raw()),
                            Some(HINT_LEFT_DATA_FRAME),
                            "non-authoritative arm evidence must remain buffered"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane() {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane",
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
                TestBinding,
            >;
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9046);
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
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let buffered = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        worker.binding_inbox.put_back(2, buffered);

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [0, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                worker.offer_scope_materialization_meta(scope, 0),
                            )
                        });
                        assert_eq!(
                            picked,
                            Some((2, buffered)),
                            "buffered matching lane should be selected before probing empty poll lane"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            0,
                            "buffered cross-lane hit should not poll unrelated empty lanes first"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm() {
    run_offer_regression_test(
        "poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9047);
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
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
                            has_fin: false,
                            channel: Channel::new(7),
                        };
                        worker.binding_inbox.put_back(0, loop_mismatch);
                        worker.binding_inbox.put_back(2, matching);

                        let extra_frame = 99;
                        let frame_label_meta = ScopeFrameLabelMeta {
                            recv_frame_label: extra_frame,
                            recv_arm: 1,
                            controller_frame_labels: [0, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
                                | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(
                            worker.binding_inbox.take_matching_or_poll(
                                &mut worker.binding,
                                0,
                                TEST_LOOP_CONTINUE_FRAME,
                            ),
                            Some(loop_mismatch),
                            "authoritative arm demux must not scan unrelated loop-control lane"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_prefers_authoritative_arm_frame_label_mask_when_non_singleton() {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_authoritative_arm_frame_label_mask_when_non_singleton",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9045);
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
                                    TestBinding::with_incoming(&[
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                                            instance: 9,
                                            has_fin: false,
                                            channel: Channel::new(5),
                                        },
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                            instance: 7,
                                            has_fin: false,
                                            channel: Channel::new(3),
                                        },
                                    ]),
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
                        let extra_frame = 99;
                        let frame_label_meta = ScopeFrameLabelMeta {
                            recv_frame_label: extra_frame,
                            recv_arm: 0,
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
                                | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label(Some(0)),
                            None
                        );
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label_mask(Some(0)),
                            FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                | FrameLabelMask::from_frame_label(extra_frame)
                        );
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(0).expect("binary route arm")),
                        );

                        let picked = worker.poll_binding_for_offer(
                            scope,
                            entry_state.lane_idx as usize,
                            frame_label_meta,
                            worker
                                .offer_entry_materialization_meta(scope, entry_idx)
                                .expect("descriptor-derived materialization metadata"),
                        );
                        assert_eq!(
                            picked
                                .map(|(lane_idx, evidence)| (lane_idx, evidence.frame_label.raw())),
                            Some((0, HINT_LEFT_DATA_FRAME)),
                            "authoritative arm mask should skip buffered labels from the other arm"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred.map(|evidence| evidence.frame_label.raw()),
                            Some(HINT_RIGHT_DATA_FRAME),
                            "non-authoritative arm evidence must remain buffered after mask match"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_uses_frame_label_mask_to_skip_other_arm_lanes_without_authority() {
    run_offer_regression_test(
        "poll_binding_for_offer_uses_frame_label_mask_to_skip_other_arm_lanes_without_authority",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9048);
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
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
                            has_fin: false,
                            channel: Channel::new(7),
                        };
                        worker.binding_inbox.put_back(0, loop_mismatch);
                        worker.binding_inbox.put_back(2, matching);

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);
                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(
                            worker.binding_inbox.take_matching_or_poll(
                                &mut worker.binding,
                                0,
                                TEST_LOOP_CONTINUE_FRAME,
                            ),
                            Some(loop_mismatch),
                            "no-authority demux should still restrict scans to lanes implied by the label mask"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_buffered_match_skips_drop_only_preferred_lane_for_non_singleton_mask() {
    run_offer_regression_test(
        "poll_binding_for_offer_buffered_match_skips_drop_only_preferred_lane_for_non_singleton_mask",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9050);
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
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
                            has_fin: false,
                            channel: Channel::new(7),
                        };
                        worker.binding_inbox.put_back(0, loop_mismatch);
                        worker.binding_inbox.put_back(2, matching);

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);
                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(
                            worker.binding_inbox.take_matching_or_poll(
                                &mut worker.binding,
                                0,
                                TEST_LOOP_CONTINUE_FRAME,
                            ),
                            Some(loop_mismatch),
                            "buffered matching lane should win before scanning drop-only preferred lane"
                        );
                    });
                });
            });
        },
    );
}
