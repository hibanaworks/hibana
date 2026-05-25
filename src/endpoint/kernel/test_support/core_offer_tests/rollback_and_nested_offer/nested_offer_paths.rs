use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn authoritative_arm_decode_never_reads_other_arm_binding_channel()
 {
    run_offer_regression_test(
        "authoritative_arm_decode_never_reads_other_arm_binding_channel",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9058);
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
                                    TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let selected_evidence = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let other_arm_evidence = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, selected_evidence);
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                            route_decision_commit_evidence:
                                RouteDecisionCommitEvidence::CachedOrDemux,
                        };
                        let branch = RouteFrontierMachine::new(worker)
                            .produce_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, other_arm_evidence)),
                                None,
                            )
                            .expect("produce selected branch")
                            .into();

                        let mut cx = Context::from_waker(noop_waker_ref());
                        let decoded = {
                            let mut decode =
                                pin!(CursorDecode::<Msg<100, u8>>::run(worker, branch));
                            poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "selected branch binding decode",
                            )
                        };
                        assert_eq!(decoded, 42);
                        assert_eq!(
                            worker.binding.last_recv_channel(),
                            Some(selected_evidence.channel),
                            "decode must use the selected arm binding channel, not carried other-arm evidence"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(deferred, Some(other_arm_evidence));
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn selected_route_arm_keeps_later_same_lane_sends_available()
 {
    run_offer_regression_test(
        "selected_route_arm_keeps_later_same_lane_sends_available",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9055);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &MULTI_SEND_ROUTE_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &MULTI_SEND_ROUTE_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }

                        let controller = controller_slot.borrow_mut();
                        let mut cx = Context::from_waker(noop_waker_ref());

                        {
                            let mut route_right =
                                pin!(CursorSend::<MultiSendRouteRightMsg>::run(controller, ()));
                            let _ =
                                poll_ready_ok(&mut cx, route_right.as_mut(), "route-right send");
                        }

                        assert!(
                            controller.preview_flow::<MultiSendRightFirstMsg>().is_ok(),
                            "first payload send must remain available after choosing the route arm"
                        );
                        {
                            let mut first_payload =
                                pin!(CursorSend::<MultiSendRightFirstMsg>::run(controller, &1));
                            let _ = poll_ready_ok(
                                &mut cx,
                                first_payload.as_mut(),
                                "first payload send",
                            );
                        }

                        assert!(
                            controller.preview_flow::<MultiSendRightSecondMsg>().is_ok(),
                            "later payload send on the same route arm must remain available after the first send"
                        );
                        {
                            let mut second_payload =
                                pin!(CursorSend::<MultiSendRightSecondMsg>::run(controller, &2));
                            let _ = poll_ready_ok(
                                &mut cx,
                                second_payload.as_mut(),
                                "second payload send",
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn static_passive_binding_label_materializes_poll()
 {
    run_offer_regression_test("static_passive_binding_label_materializes_poll", || {
        let entry_route_program = ENTRY_ROUTE_PROGRAM();
        let entry_controller_program = project(&entry_route_program);
        let entry_worker_program = project(&entry_route_program);
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            let transport = HintOnlyTransport::new(HINT_NONE);
            let rv_id = cluster_ref
                .add_rendezvous_from_config(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(906);
            type ControllerEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
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
            let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
            let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
            unsafe {
                cluster_ref
                    .attach_endpoint_into::<0, _, _, _>(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                        rv_id,
                        sid,
                        &entry_controller_program,
                        NoBinding,
                    )
                    .expect("attach controller endpoint");
                cluster_ref
                    .attach_endpoint_into::<1, _, _, _>(
                        worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                        rv_id,
                        sid,
                        &entry_worker_program,
                        TestBinding::with_incoming(&[IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 0,
                            has_fin: false,
                            channel: Channel::new(1),
                        }]),
                    )
                    .expect("attach worker endpoint");
            }
            let controller =
                unsafe { &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>() };
            core::hint::black_box(&controller);
            let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
            let scope = worker.cursor.node_scope_id();
            assert!(!scope.is_none(), "worker must start at route scope");
            assert!(
                worker
                    .cursor
                    .first_recv_target_for_lane_frame_label(scope, 0, ENTRY_ARM0_SIGNAL_FRAME)
                    .is_some(),
                "test requires a static passive recv dispatch target"
            );

            let frame_label_meta =
                endpoint_scope_frame_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);

            let mut binding_evidence = None;
            worker.cache_binding_evidence_for_offer(
                scope,
                0,
                frame_label_meta,
                worker.offer_scope_materialization_meta(scope, 0),
                &mut binding_evidence,
            );
            let evidence = binding_evidence.expect("binding evidence should be staged for poll");
            with_lane_set_view(&[0], |offer_lanes| {
                worker.ingest_scope_evidence_for_offer_lanes(
                    scope,
                    0,
                    offer_lanes,
                    false,
                    frame_label_meta,
                );
            });
            worker.ingest_binding_scope_evidence(
                scope,
                evidence.lane(),
                evidence.frame_label(),
                false,
                frame_label_meta,
            );

            assert!(
                worker.peek_scope_ack(scope).is_none(),
                "binding-backed static dispatch must not mint ack authority"
            );
            let resolved_label = worker.take_scope_frame_hint(scope);
            assert_eq!(
                resolved_label,
                Some(evidence.frame_label()),
                "binding-backed poll should still preserve the resolved ingress label"
            );
            assert_eq!(
                worker.poll_arm_from_ready_mask(scope),
                Some(Arm::new(0).expect("binary route arm")),
                "exact binding ingress on a static passive route must materialize Poll authority"
            );

            unsafe {
                core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                core::ptr::drop_in_place(
                    controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                );
            }
        });
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn static_passive_staged_transport_hint_materializes_poll()
 {
    run_offer_regression_test(
        "static_passive_staged_transport_hint_materializes_poll",
        || {
            let entry_route_program = ENTRY_ROUTE_PROGRAM();
            let entry_controller_program = project(&entry_route_program);
            let entry_worker_program = project(&entry_route_program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                let transport = HintOnlyTransport::new(ENTRY_ARM0_SIGNAL_FRAME);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(907);
                type ControllerEndpoint = CursorEndpoint<
                    'static,
                    0,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
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
                let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                            rv_id,
                            sid,
                            &entry_controller_program,
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                            rv_id,
                            sid,
                            &entry_worker_program,
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let controller =
                    unsafe { &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>() };
                core::hint::black_box(&controller);
                let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                let scope = worker.cursor.node_scope_id();
                assert!(!scope.is_none(), "worker must start at route scope");
                assert!(
                    worker
                        .cursor
                        .first_recv_target_for_lane_frame_label(scope, 0, ENTRY_ARM0_SIGNAL_FRAME)
                        .is_some(),
                    "test requires a static passive recv dispatch target"
                );

                let frame_label_meta =
                    endpoint_scope_frame_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
                with_lane_set_view(&[0], |offer_lanes| {
                    worker.ingest_scope_evidence_for_offer_lanes(
                        scope,
                        0,
                        offer_lanes,
                        false,
                        frame_label_meta,
                    );
                });

                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    None,
                    "transport hint alone must remain non-authoritative until ingress is staged"
                );
                assert!(
                    worker.peek_scope_ack(scope).is_none(),
                    "transport-backed static dispatch must not mint ack authority"
                );
                let resolved_label = worker.take_scope_frame_hint(scope);
                assert_eq!(
                    resolved_label,
                    Some(ENTRY_ARM0_SIGNAL_FRAME),
                    "transport-backed poll should still preserve the resolved ingress label"
                );
                worker.mark_scope_ready_arm_from_frame_label(
                    scope,
                    0,
                    resolved_label.expect("transport hint must resolve"),
                    frame_label_meta,
                );
                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    Some(Arm::new(0).expect("binary route arm")),
                    "staged exact transport ingress on a static passive route must materialize Poll authority"
                );

                unsafe {
                    core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                    core::ptr::drop_in_place(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                    );
                }
            });
        },
    );
}
