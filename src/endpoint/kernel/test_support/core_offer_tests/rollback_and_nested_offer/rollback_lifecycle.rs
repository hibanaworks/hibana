use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn pico_budget_offer_fixture_is_separate_from_large_host_fixture()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn dropped_branch_restores_original_binding_evidence_frame_label()
 {
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
                            channel: Channel::new(10),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
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
                        let branch = (worker)
                            .produce_branch(
                                selection,
                                resolved,
                                crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn selected_branch_decode_uses_original_evidence_channel()
 {
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
                            channel: Channel::new(11),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
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
                        let branch: MaterializedRouteBranch<'_> = (worker)
                            .produce_branch(
                                selection,
                                resolved,
                                crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn wire_recv_with_selected_binding_evidence_requeues_transport_without_staging_it()
 {
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
                                channel: Channel::new(21),
                            };
                            let selection = OfferScopeSelection {
                                scope_id: scope,
                                frontier_parallel_root: None,
                                offer_lane: 0,
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
                            let staged_transport = Payload::new(&[0x6b]);
                            let branch: MaterializedRouteBranch<'_> = (worker)
                                .produce_branch(
                                    selection,
                                    resolved,
                                    crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
                                    Some(LaneIngressEvidence::new(0, observed)),
                                    Some(worker.received_transport_frame(
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn finish_resolved_rebuffers_carried_other_arm_evidence()
 {
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
                            channel: Channel::new(3),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
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
                        let branch: MaterializedRouteBranch<'_> = (worker)
                            .produce_branch(
                                selection,
                                resolved,
                                crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn finish_resolved_does_not_drop_carried_other_arm_when_fresh_selected_evidence_exists()
 {
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
                            channel: Channel::new(5),
                        };
                        let carried_other_arm = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, fresh_selected);

                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
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
                        let branch: MaterializedRouteBranch<'_> = (worker)
                            .produce_branch(
                                selection,
                                resolved,
                                crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
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
