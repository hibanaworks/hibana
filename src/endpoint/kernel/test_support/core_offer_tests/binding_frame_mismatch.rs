#[test]
fn poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask() {
    run_offer_regression_test(
        "poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintLaneAwareWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9052);
                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_WORKER_PROGRAM(),
                                    LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

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
                        assert_eq!(
                            picked, None,
                            "generic mask path must not probe unbuffered cross-lane bindings before the selected lane"
                        );
                        assert_eq!(worker.binding.poll_count_for_lane(0), 1);
                        assert_eq!(worker.binding.poll_count_for_lane(2), 0);

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                2,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(worker.binding.poll_count_for_lane(2), 1);
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded() {
    run_offer_regression_test(
        "poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintLaneAwareWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9053);
                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 11,
                            has_fin: false,
                            channel: Channel::new(6),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_WORKER_PROGRAM(),
                                    LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );
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
                        assert_eq!(worker.binding.poll_count_for_lane(0), 0);
                        assert_eq!(worker.binding.poll_count_for_lane(2), 1);
                    });
                });
            });
        },
    );
}

#[test]
fn record_route_decision_for_scope_lanes_refreshes_sibling_frontier_cache() {
    run_offer_regression_test(
        "record_route_decision_for_scope_lanes_refreshes_sibling_frontier_cache",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintLaneAwareWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9054);
                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 12,
                            has_fin: false,
                            channel: Channel::new(6),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_WORKER_PROGRAM(),
                                    LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut offer = pin!(cursor_offer(worker));
                            assert!(
                                matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                "unresolved split-lane route must cache a pending frontier observation before the decision arrives"
                            );
                        }

                        worker.record_route_decision_for_scope_lanes(scope, 1, 0);
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let mut offer = pin!(cursor_offer(worker));
                        let branch =
                            poll_ready_ok(&mut cx, offer.as_mut(), "split-lane sibling offer");
                        assert_eq!(
                            branch_label(&branch),
                            HINT_RIGHT_DATA_LABEL,
                            "broadcast route decisions must invalidate sibling-lane frontier caches immediately"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn take_binding_for_selected_arm_preserves_cached_other_arm_evidence() {
    run_offer_regression_test(
        "take_binding_for_selected_arm_preserves_cached_other_arm_evidence",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9049);
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

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                            instance: 9,
                            has_fin: true,
                            channel: Channel::new(5),
                        };
                        let cached_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, matching);
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
                        let mut binding_evidence = Some(cached_mismatch);

                        let selected = worker.take_binding_for_selected_arm(
                            0,
                            0,
                            frame_label_meta,
                            &mut binding_evidence,
                        );
                        assert_eq!(
                            selected,
                            Some(matching),
                            "selected-arm helper must preserve the matched ingress evidence exactly"
                        );
                        assert!(
                            binding_evidence.is_none(),
                            "cached mismatch should be re-buffered, not left staged"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred,
                            Some(cached_mismatch),
                            "selected-arm demux must preserve cached other-arm evidences"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn selected_arm_mask_preserves_actual_binding_evidence_frame_label() {
    run_offer_regression_test(
        "selected_arm_mask_preserves_actual_binding_evidence_frame_label",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9059);
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
                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                            instance: 11,
                            has_fin: true,
                            channel: Channel::new(8),
                        };
                        let frame_label_meta = ScopeFrameLabelMeta {
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(observed.frame_label.raw()),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(observed.frame_label.raw()),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label(Some(0)),
                            None,
                            "fixture must exercise the non-singleton selected-arm mask path"
                        );
                        let mut binding_evidence = Some(observed);

                        let selected = worker.take_binding_for_selected_arm(
                            0,
                            0,
                            frame_label_meta,
                            &mut binding_evidence,
                        );

                        assert_eq!(
                            selected,
                            Some(observed),
                            "selected-arm demux must not rewrite the observed evidence frame label"
                        );
                        assert!(
                            binding_evidence.is_none(),
                            "matched evidence should be consumed by the selected branch"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn physical_frame_label_mismatch_is_phase_invariant_not_logical_label_mismatch() {
    run_offer_regression_test(
        "physical_frame_label_mismatch_is_phase_invariant_not_logical_label_mismatch",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
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

                        let selected = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 15,
                            has_fin: false,
                            channel: Channel::new(12),
                        };
                        let mismatched = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                            instance: 15,
                            has_fin: false,
                            channel: Channel::new(13),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        assert_ne!(
                            mismatched.frame_label.raw(),
                            ENTRY_ARM0_SIGNAL_FRAME,
                            "fixture must exercise evidence frame label != branch frame label"
                        );
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                            poll_route_decision_authority: false,
                        };

                        let mut branch = RouteFrontierMachine::new(worker)
                            .produce_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, selected)),
                                None,
                            )
                            .expect("produce selected branch")
                            .into();
                        assert_eq!(branch_label(&branch), ENTRY_ARM0_SIGNAL_LABEL);
                        branch.binding_evidence =
                            PackedIngressEvidence::from_option(Some(mismatched));

                        let mut cx = Context::from_waker(noop_waker_ref());
                        let err = {
                            let mut decode =
                                pin!(CursorDecode::<Msg<103, u8>>::run(worker, branch));
                            match decode.as_mut().poll(&mut cx) {
                                Poll::Ready(Err(err)) => err,
                                Poll::Ready(Ok(_)) => {
                                    panic!(
                                        "mismatched binding evidence frame label must fail closed"
                                    )
                                }
                                Poll::Pending => {
                                    panic!(
                                        "mismatched binding evidence frame label must not await I/O"
                                    )
                                }
                            }
                        };
                        assert!(
                            matches!(err, RecvError::PhaseInvariant),
                            "expected physical frame-label mismatch to be phase invariant, got {err:?}"
                        );
                        assert_eq!(
                            worker.binding.last_recv_channel(),
                            None,
                            "frame-label mismatch must be rejected before reading the evidence channel"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn materialized_branch_preserves_actual_binding_evidence_frame_label() {
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
fn produce_non_wire_recv_evidence_requeues_staged_transport_payload() {
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
                                poll_route_decision_authority: false,
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
fn produce_wire_recv_frame_mismatch_is_terminal_without_requeue() {
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
                                poll_route_decision_authority: false,
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
fn reset_public_offer_state_restores_carried_binding_evidence() {
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
fn reset_public_offer_state_restores_carried_transport_payloads() {
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

#[test]
fn reset_public_offer_state_restores_empty_carried_transport_payload() {
    run_offer_regression_test(
        "reset_public_offer_state_restores_empty_carried_transport_payload",
        || {
            static EMPTY_PAYLOAD: [u8; 0] = [];

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
                            let sid = SessionId::new(9068);
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
                                    Payload::new(&EMPTY_PAYLOAD),
                                );
                            worker
                                .public_offer_state
                                .stage_collect_rollback_items_for_test(
                                    None,
                                    Some((0, Payload::new(&EMPTY_PAYLOAD))),
                                );

                            worker.reset_public_offer_state();

                            assert_eq!(
                                transport_probe.requeue_count(),
                                2,
                                "empty transport payload is still a consumed frame and must be requeued on non-terminal offer reset"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn terminal_offer_clear_discards_carried_preview_state() {
    run_offer_regression_test(
        "terminal_offer_clear_discards_carried_preview_state",
        || {
            static TERMINAL_PAYLOAD: [u8; 1] = [0x6d];

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
                            let sid = SessionId::new(9067);
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
                            let evidence = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                                instance: 33,
                                has_fin: false,
                                channel: Channel::new(33),
                            };

                            worker
                                .public_offer_state
                                .stage_carried_binding_evidence_for_test(LaneIngressEvidence::new(
                                    0, evidence,
                                ));
                            worker
                                .public_offer_state
                                .stage_carried_transport_payload_for_test(
                                    0,
                                    Payload::new(&TERMINAL_PAYLOAD),
                                );

                            worker.terminal_clear_public_offer_state();

                            assert_eq!(
                                worker.binding_inbox.take_matching_or_poll(
                                    &mut worker.binding,
                                    0,
                                    evidence.frame_label.raw(),
                                ),
                                None,
                                "terminal offer clear must discard preview binding evidence"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                0,
                                "terminal offer clear must not restore preview transport bytes"
                            );
                        });
                    });
                });
            });
        },
    );
}
