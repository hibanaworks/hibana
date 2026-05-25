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
                            .add_rendezvous_from_config(config, transport)
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
                            has_fin: false,
                            channel: Channel::new(9),
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
                            route_decision_commit_evidence:
                                RouteDecisionCommitEvidence::CachedOrDemux,
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
                                .add_rendezvous_from_config(config, transport)
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
                                route_decision_commit_evidence:
                                    RouteDecisionCommitEvidence::CachedOrDemux,
                            };
                            let staged_payload = Payload::new(&[0x6b]);

                            let branch: MaterializedRouteBranch<'_> =
                                RouteFrontierMachine::new(controller)
                                    .produce_branch(
                                        selection,
                                        resolved,
                                        true,
                                        Some(LaneIngressEvidence::new(0, evidence)),
                                        Some(lane_port::ReceivedFrame::synthetic_for_test(
                                            0,
                                            staged_payload,
                                        )),
                                    )
                                    .expect("non-wire branch must rebuffer stray binding evidence")
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn produce_wire_recv_frame_mismatch_is_terminal_without_requeue()
 {
    run_offer_regression_test(
        "produce_wire_recv_frame_mismatch_is_terminal_without_requeue",
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
                                offer_lane_idx: 0,
                                at_route_offer_entry: false,
                            };
                            let resolved = ResolvedRouteDecision {
                                route_token: RouteDecisionToken::from_poll(
                                    Arm::new(0).expect("binary route arm"),
                                ),
                                selected_arm: 0,
                                resolved_hint_frame_label: Some(ENTRY_ARM1_SIGNAL_FRAME),
                                route_decision_commit_evidence:
                                    RouteDecisionCommitEvidence::CachedOrDemux,
                            };
                            let staged_payload = Payload::new(&[0x6d]);

                            let err = match RouteFrontierMachine::new(worker).produce_branch(
                                selection,
                                resolved,
                                false,
                                None,
                                Some(lane_port::ReceivedFrame::synthetic_for_test(
                                    0,
                                    staged_payload,
                                )),
                            ) {
                                Ok(_) => panic!("frame mismatch must fail closed"),
                                Err(err) => err,
                            };
                            assert!(
                                matches!(err, RecvError::PhaseInvariant),
                                "frame mismatch must be phase invariant, got {err:?}"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                0,
                                "terminal frame mismatch must not rollback staged transport payload before poisoning"
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
                                .add_rendezvous_from_config(config, transport)
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
                                has_fin: false,
                                channel: Channel::new(31),
                            };
                            let staged = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                                instance: 32,
                                has_fin: false,
                                channel: Channel::new(32),
                            };

                            worker
                                .public_offer_state
                                .stage_carried_binding_evidence_for_test(LaneIngressEvidence::new(
                                    0, top_level,
                                ));
                            worker
                                .public_offer_state
                                .stage_collect_rollback_items_for_test(
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
            static STAGED_PAYLOAD: [u8; 1] = [0x6c];

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
                                .add_rendezvous_from_config(config, transport)
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

                            worker
                                .public_offer_state
                                .stage_carried_transport_payload_for_test(
                                    0,
                                    Payload::new(&TOP_LEVEL_PAYLOAD),
                                );
                            worker
                                .public_offer_state
                                .stage_resolve_rollback_items_for_test(
                                    None,
                                    Some((0, Payload::new(&STAGED_PAYLOAD))),
                                );

                            worker.reset_public_offer_state();

                            assert_eq!(
                                transport_probe.requeue_count(),
                                2,
                                "non-terminal offer reset must requeue every carried transport payload"
                            );
                        });
                    });
                });
            });
        },
    );
}
