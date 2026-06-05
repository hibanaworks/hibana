use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn materialized_branch_preserves_actual_binding_evidence_frame_label()
 {
    run_offer_regression_test(
        "materialized_branch_preserves_actual_binding_evidence_frame_label",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .register_rendezvous(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9060);
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
                            instance: 12,
                            channel: Channel::new(9),
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
                            resolved_hint_frame: None,
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
                            .expect("fresh transport mismatch must not discard selected branch")
                            .into();

                        assert_eq!(branch_label(&branch), ENTRY_ARM0_SIGNAL_LABEL);
                        assert_eq!(
                            branch.binding_evidence.into_option(),
                            Some(observed),
                            "materialization must keep the binding evidence frame label observed by demux"
                        );
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn produce_non_wire_recv_evidence_requeues_staged_transport_payload()
 {
    run_offer_regression_test(
        "produce_non_wire_recv_evidence_requeues_staged_transport_payload",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerBindingEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .register_rendezvous(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(9063);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_CONTROLLER_PROGRAM(),
                                        TestBinding::default(),
                                    )
                                    .expect("attach controller endpoint");
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

                            let controller = controller_slot.borrow_mut();
                            let worker_borrow = worker_slot.borrow_mut();
                            core::hint::black_box(&worker_borrow);
                            let scope = controller.cursor.node_scope_id();
                            assert!(
                                !scope.is_none(),
                                "controller must start at route controller scope"
                            );
                            let evidence = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
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
                                resolved_hint_frame: None,
                                route_decision_commit_evidence:
                                    RouteDecisionCommitEvidence::CachedOrDemux,
                            };
                            let staged_payload = Payload::new(&[0x6b]);

                            let branch: MaterializedRouteBranch<'_> = (controller)
                                .produce_branch(
                                    selection,
                                    resolved,
                                    crate::endpoint::kernel::offer::OfferScopeProfile::ControllerStatic,
                                    Some(LaneIngressEvidence::new(0, evidence)),
                                    Some(controller.received_transport_frame(
                                        0,
                                        staged_payload,
                                    )),
                                )
                                .expect("non-wire branch must rebuffer stray binding evidence")
                                .expect("non-wire branch must not discard selected branch")
                                .into();
                            assert!(
                                !branch_has_staged_payload(&branch),
                                "requeued transport payload must not remain staged on a non-wire branch"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "non-wire materialization must requeue staged transport payload once"
                            );
                            controller.restore_materialized_route_branch(branch);
                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "restoring the materialized branch must not requeue an already requeued payload"
                            );
                            assert_eq!(
                                controller.binding_inbox.take_matching_or_poll(
                                    &mut controller.binding,
                                    0,
                                    evidence.frame_label.raw(),
                                ),
                                Some(evidence),
                                "non-wire materialization must rebuffer stray binding evidence"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn produce_wire_recv_frame_mismatch_discards_without_requeue_or_progress()
 {
    run_offer_regression_test(
        "produce_wire_recv_frame_mismatch_discards_without_requeue_or_progress",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .register_rendezvous(config, transport)
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
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }

                            let controller_borrow = controller_slot.borrow_mut();
                            core::hint::black_box(&controller_borrow);
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            assert!(!scope.is_none(), "worker must start at route scope");
                            assert_ne!(
                                ENTRY_ARM0_SIGNAL_FRAME, ENTRY_ARM1_SIGNAL_FRAME,
                                "fixture must use distinct frame labels"
                            );
                            let selection = OfferScopeSelection {
                                scope_id: scope,
                                frontier_parallel_root: None,
                                offer_lane: 0,
                                at_route_offer_entry: false,
                            };
                            let resolved = ResolvedRouteDecision {
                                route_token: RouteDecisionToken::from_poll(
                                    Arm::new(0).expect("binary route arm"),
                                ),
                                selected_arm: 0,
                                resolved_hint_frame: Some(ResolvedFrameHint::staged_transport(
                                    0,
                                    ENTRY_ARM1_SIGNAL_FRAME,
                                )),
                                route_decision_commit_evidence:
                                    RouteDecisionCommitEvidence::CachedOrDemux,
                            };
                            let staged_payload = Payload::new(&[0x6d]);

                            let branch = (worker).produce_branch(
                                selection,
                                resolved,
                                crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
                                None,
                                Some(worker.received_transport_frame_with_label(
                                    0,
                                    ENTRY_ARM1_SIGNAL_FRAME,
                                    staged_payload,
                                )),
                            )
                            .expect("fresh transport mismatch must be runtime evidence");
                            assert!(
                                branch.is_none(),
                                "fresh transport mismatch must discard the frame and leave offer pending"
                            );
                            assert_eq!(
                                worker.cursor.node_scope_id(),
                                scope,
                                "fresh transport mismatch must not advance route cursor"
                            );
                            let saw_label_mismatch = OFFER_TEST_TAP.with(|tap| unsafe {
                                (&*tap.get()).iter().copied().any(|event| {
                                    event.id == crate::observe::ids::TRANSPORT_MISMATCH
                                        && event.causal_seq()
                                            == crate::observe::ids::TRANSPORT_MISMATCH_LABEL
                                })
                            });
                            assert!(
                                saw_label_mismatch,
                                "fresh transport label mismatch must be observable as TransportMismatch"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                0,
                                "discarded fresh mismatch must not requeue staged transport payload"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn reset_public_offer_state_restores_carried_binding_evidence()
 {
    run_offer_regression_test(
        "reset_public_offer_state_restores_carried_binding_evidence",
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
                            let rv_id = cluster_ref
                                .register_rendezvous(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(9065);
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
                            let top_level = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                                instance: 31,
                                channel: Channel::new(31),
                            };
                            let staged = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                                instance: 32,
                                channel: Channel::new(32),
                            };

                            worker.public_offer_state.install_carried_binding_evidence(
                                LaneIngressEvidence::new(0, top_level),
                            );
                            worker.public_offer_state.install_collecting_rollback_items(
                                Some(LaneIngressEvidence::new(0, staged)),
                                None,
                            );

                            worker.reset_public_offer_state();

                            assert_eq!(
                                worker.binding_inbox.take_matching_or_poll(
                                    &mut worker.binding,
                                    0,
                                    top_level.frame_label.raw(),
                                ),
                                Some(top_level),
                                "non-terminal offer reset must put back top-level carried binding evidence"
                            );
                            assert_eq!(
                                worker.binding_inbox.take_matching_or_poll(
                                    &mut worker.binding,
                                    0,
                                    staged.frame_label.raw(),
                                ),
                                Some(staged),
                                "non-terminal offer reset must put back run-stage binding evidence"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn reset_public_offer_state_restores_carried_transport_payloads()
 {
    run_offer_regression_test(
        "reset_public_offer_state_restores_carried_transport_payloads",
        || {
            static TOP_LEVEL_PAYLOAD: [u8; 1] = [0x6b];

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .register_rendezvous(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(9066);
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
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }
                            let controller_borrow = controller_slot.borrow_mut();
                            core::hint::black_box(&controller_borrow);
                            let worker = worker_slot.borrow_mut();

                            let top_level_frame = worker
                                .received_transport_frame(0, Payload::new(&TOP_LEVEL_PAYLOAD));
                            worker
                                .public_offer_state
                                .install_carried_transport_frame(top_level_frame);
                            worker
                                .public_offer_state
                                .install_resolving_rollback_items(None, None);

                            worker.reset_public_offer_state();

                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "non-terminal offer reset must requeue carried transport payload"
                            );
                        });
                    });
                });
            });
        },
    );
}
