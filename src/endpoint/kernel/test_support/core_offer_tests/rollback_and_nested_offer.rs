#[test]
fn pico_budget_offer_fixture_is_separate_from_large_host_fixture() {
    assert!(
        PICO_OFFER_FIXTURE_SLAB_CAPACITY < LARGE_HOST_OFFER_FIXTURE_SLAB_CAPACITY,
        "Pico-equivalent fixture budget must stay distinct from large host stress storage"
    );
    assert!(
        4096 <= PICO_OFFER_FIXTURE_SLAB_CAPACITY,
        "Pico-equivalent fixture must still cover common offer regressions"
    );
}

#[test]
fn dropped_branch_restores_original_binding_evidence_frame_label() {
    run_offer_regression_test(
        "dropped_branch_restores_original_binding_evidence_frame_label",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9061);
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
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 13,
                            has_fin: false,
                            channel: Channel::new(10),
                        };
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
                            poll_route_decision_authority: false,
                        };
                        let branch = RouteFrontierMachine::new(worker)
                            .produce_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, observed)),
                                None,
                            )
                            .expect("produce selected branch")
                            .into();

                        worker.restore_materialized_route_branch(branch);
                        let restored = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            observed.frame_label.raw(),
                        );
                        assert_eq!(
                            restored,
                            Some(observed),
                            "branch restore must rebuffer the original observed evidence frame label"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn selected_branch_decode_uses_original_evidence_channel() {
    run_offer_regression_test(
        "selected_branch_decode_uses_original_evidence_channel",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9062);
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
                                    TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 14,
                            has_fin: false,
                            channel: Channel::new(11),
                        };
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
                            poll_route_decision_authority: false,
                        };
                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .produce_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, observed)),
                                None,
                            )
                            .expect("produce selected branch")
                            .into();

                        assert_eq!(
                            branch.binding_evidence.into_option(),
                            Some(observed),
                            "decode preview must retain the original demux evidence identity"
                        );
                        let mut cx = Context::from_waker(noop_waker_ref());
                        let decoded = {
                            let mut decode =
                                pin!(CursorDecode::<Msg<103, u8>>::run(worker, branch));
                            poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "selected branch binding decode",
                            )
                        };
                        assert_eq!(decoded, 42);
                        assert_eq!(
                            worker.binding.last_recv_channel(),
                            Some(observed.channel),
                            "decode must use the original observed binding channel"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn wire_recv_with_selected_binding_evidence_requeues_transport_without_staging_it() {
    run_offer_regression_test(
        "wire_recv_with_selected_binding_evidence_requeues_transport_without_staging_it",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerBindingEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(9064);
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
                                        TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                    )
                                    .expect("attach worker endpoint");
                            }
                            let controller_borrow = controller_slot.borrow_mut();
                            core::hint::black_box(&controller_borrow);
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            assert!(!scope.is_none(), "worker must start at route scope");

                            let observed = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                                instance: 16,
                                has_fin: false,
                                channel: Channel::new(21),
                            };
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
                                poll_route_decision_authority: false,
                            };
                            let staged_transport = Payload::new(&[0x6b]);
                            let branch: MaterializedRouteBranch<'_> =
                                RouteFrontierMachine::new(worker)
                                    .produce_branch(
                                        selection,
                                        resolved,
                                        false,
                                        Some(LaneIngressEvidence::new(0, observed)),
                                        Some(lane_port::ReceivedFrame::synthetic_for_test(
                                            0,
                                            staged_transport,
                                        )),
                                    )
                                    .expect("produce selected branch")
                                    .into();

                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "selected binding evidence must requeue competing transport bytes"
                            );
                            assert!(
                                !branch_has_transport_payload(&branch),
                                "requeued transport bytes must not remain staged on the branch"
                            );
                            assert_eq!(
                                branch.binding_evidence.into_option(),
                                Some(observed),
                                "binding evidence must remain the selected payload source"
                            );
                            let mut cx = Context::from_waker(noop_waker_ref());
                            let decoded = {
                                let mut decode =
                                    pin!(CursorDecode::<Msg<103, u8>>::run(worker, branch));
                                poll_ready_ok(
                                    &mut cx,
                                    decode.as_mut(),
                                    "selected binding decode with requeued transport",
                                )
                            };
                            assert_eq!(
                                decoded, 42,
                                "decode must consume binding payload, not requeued transport bytes"
                            );
                            assert_eq!(
                                worker.binding.last_recv_channel(),
                                Some(observed.channel),
                                "decode must call the selected binding channel"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn finish_resolved_rebuffers_carried_other_arm_evidence() {
    run_offer_regression_test(
        "finish_resolved_rebuffers_carried_other_arm_evidence",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9056);
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
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let carried_other_arm = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
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
                            poll_route_decision_authority: false,
                        };
                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .produce_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, carried_other_arm)),
                                None,
                            )
                            .expect("produce selected branch")
                            .into();

                        assert_eq!(branch_label(&branch), HINT_LEFT_DATA_LABEL);
                        assert!(
                            branch.binding_evidence.into_option().is_none(),
                            "selected branch must not carry other-arm demux evidence"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred,
                            Some(carried_other_arm),
                            "other-arm evidence must be rebuffered for its own branch"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn finish_resolved_does_not_drop_carried_other_arm_when_fresh_selected_evidence_exists() {
    run_offer_regression_test(
        "finish_resolved_does_not_drop_carried_other_arm_when_fresh_selected_evidence_exists",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9057);
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
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let fresh_selected = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                            instance: 9,
                            has_fin: true,
                            channel: Channel::new(5),
                        };
                        let carried_other_arm = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, fresh_selected);

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
                            poll_route_decision_authority: false,
                        };
                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .produce_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, carried_other_arm)),
                                None,
                            )
                            .expect("produce selected branch")
                            .into();

                        assert_eq!(branch.binding_evidence.into_option(), Some(fresh_selected));
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred,
                            Some(carried_other_arm),
                            "fresh selected evidence must not cause carried other-arm evidence to be dropped"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn authoritative_arm_decode_never_reads_other_arm_binding_channel() {
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
                            poll_route_decision_authority: false,
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
fn selected_route_arm_keeps_later_same_lane_sends_available() {
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
fn static_passive_binding_label_materializes_poll() {
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
fn static_passive_staged_transport_hint_materializes_poll() {
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

#[test]
fn nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes() {
    run_offer_regression_test(
        "nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(909);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &NESTED_STATIC_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &NESTED_STATIC_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        let outer_scope = worker.cursor.node_scope_id();
                        let middle_scope = worker
                            .cursor
                            .passive_arm_scope_by_arm(outer_scope, 1)
                            .expect("outer right arm should enter middle route");
                        assert_ne!(
                            middle_scope, outer_scope,
                            "passive arm navigation must descend to a child route, not recurse on the same scope"
                        );
                        let inner_scope = worker
                            .cursor
                            .passive_arm_scope_by_arm(middle_scope, 0)
                            .expect("middle left arm should enter inner route");
                        assert_ne!(
                            inner_scope, middle_scope,
                            "nested passive arm navigation must keep descending"
                        );
                        let nested_leaf_frame = frame_label_for_cursor_label(&worker.cursor, 0x51);

                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    outer_scope,
                                    0,
                                    nested_leaf_frame
                                )
                                .map(|(arm, _)| arm),
                            Some(1),
                            "outer scope must resolve the leaf reply through first-recv dispatch"
                        );
                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    middle_scope,
                                    0,
                                    nested_leaf_frame
                                )
                                .map(|(arm, _)| arm),
                            Some(0),
                            "middle scope must resolve the leaf reply through first-recv dispatch"
                        );

                        for (scope, expected_arm) in
                            [(outer_scope, 1u8), (middle_scope, 0u8), (inner_scope, 0u8)]
                        {
                            let frame_label_meta = endpoint_scope_frame_label_meta(
                                worker,
                                scope,
                                ScopeLoopMeta::EMPTY,
                            );
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
                                0,
                                nested_leaf_frame,
                                false,
                                frame_label_meta,
                            );
                            assert_eq!(
                                worker.poll_arm_from_ready_mask(scope),
                                Some(Arm::new(expected_arm).expect("binary route arm")),
                                "exact nested leaf ingress must materialize Poll for scope {scope:?}"
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
fn dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit() {
    run_offer_regression_test(
        "dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit",
        || {
            type EntryArm0SignalMsg = Msg<{ ENTRY_ARM0_SIGNAL_LABEL }, u8>;
            type EntryArm0ReplyMsg = Msg<104, u8>;
            type DynamicParentLeftSteps =
                SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>;
            type DynamicParentRightBodySteps = SeqSteps<
                SendOnly<0, Role<0>, Role<1>, EntryArm0SignalMsg>,
                SendOnly<0, Role<1>, Role<0>, EntryArm0ReplyMsg>,
            >;
            type DynamicParentRightSteps = SeqSteps<HintRightHead, DynamicParentRightBodySteps>;
            type DynamicParentEntrySteps =
                BranchSteps<DynamicParentLeftSteps, DynamicParentRightSteps>;
            static DYNAMIC_DECODE_PAYLOAD: [u8; 1] = [0x5a];
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

            let program: g::Program<DynamicParentEntrySteps> = g::route(
                HINT_LEFT_ARM(),
                g::seq(
                    g::send::<
                        Role<0>,
                        Role<0>,
                        Msg<
                            ROUTE_HINT_RIGHT_LABEL,
                            GenericCapToken<RouteHintRightKind>,
                            RouteHintRightKind,
                        >,
                        0,
                    >()
                    .policy::<HINT_ROUTE_POLICY_ID>(),
                    g::seq(
                        g::send::<Role<0>, Role<1>, EntryArm0SignalMsg, 0>(),
                        g::send::<Role<1>, Role<0>, EntryArm0ReplyMsg, 0>(),
                    ),
                ),
            );
            let controller_program: RoleProgram<0> = project(&program);
            let worker_program: RoleProgram<1> = project(&program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(ControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(913);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &controller_program,
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
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
                        let parent_scope = worker.cursor.node_scope_id();
                        let entry_arm0_signal_frame =
                            frame_label_for_cursor_label(&worker.cursor, ENTRY_ARM0_SIGNAL_LABEL);
                        assert!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    parent_scope,
                                    0,
                                    entry_arm0_signal_frame,
                                )
                                .is_none(),
                            "dynamic parent route must not expose static Poll dispatch"
                        );
                        let (parent_arm, target_idx) = (0..worker.cursor.local_steps_len())
                            .find_map(|idx| {
                                let recv_meta = worker.cursor.try_recv_meta_at(idx)?;
                                if recv_meta.label == ENTRY_ARM0_SIGNAL_LABEL
                                    && recv_meta.scope == parent_scope
                                {
                                    Some((recv_meta.route_arm?, idx))
                                } else {
                                    None
                                }
                            })
                            .expect("dynamic parent right arm should contain the staged recv");
                        assert_eq!(parent_arm, 1);
                        worker.set_cursor_index(target_idx);
                        let recv_meta = worker
                            .cursor
                            .try_recv_meta()
                            .expect("cursor must point at recv");
                        assert_eq!(recv_meta.label, ENTRY_ARM0_SIGNAL_LABEL);
                        let before_cursor = worker.cursor.index();
                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            None,
                            "dynamic parent must start without route authority"
                        );

                        let branch = MaterializedRouteBranch {
                            label: ENTRY_ARM0_SIGNAL_LABEL,
                            binding_evidence: PackedIngressEvidence::EMPTY,
                            binding_evidence_lane: u8::MAX,
                            staged_payload: Some(StagedPayload::transport_for_test(
                                recv_meta.lane,
                                Payload::new(&DYNAMIC_DECODE_PAYLOAD),
                            )),
                            branch_meta: BranchMeta {
                                scope_id: parent_scope,
                                selected_arm: parent_arm,
                                lane_wire: recv_meta.lane,
                                eff_index: recv_meta.eff_index,
                                frame_label: recv_meta.frame_label,
                                kind: BranchKind::WireRecv,
                                route_source: RouteDecisionSource::Poll,
                                poll_route_decision_authority: false,
                            },
                        };
                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut decode =
                                pin!(CursorDecode::<EntryArm0SignalMsg>::run(worker, branch));
                            match decode.as_mut().poll(&mut cx) {
                                Poll::Ready(Err(RecvError::PhaseInvariant)) => {}
                                Poll::Ready(Ok(_)) => panic!(
                                    "decode must not commit a dynamic linger parent from child frame discriminator"
                                ),
                                Poll::Ready(Err(err)) => {
                                    panic!("decode failed with unexpected error: {err:?}")
                                }
                                Poll::Pending => panic!("staged decode unexpectedly pending"),
                            }
                        }

                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            None,
                            "dynamic parent must remain unselected after failed decode commit"
                        );
                        assert_eq!(
                            worker.cursor.index(),
                            before_cursor,
                            "decode commit failure must not publish cursor progress"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "decode failure must not mint ACK authority for the dynamic parent"
                        );
                    });
                });
            });
        },
    );
}
