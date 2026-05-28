use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn record_route_decision_for_scope_lanes_refreshes_sibling_frontier_cache()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn take_binding_for_selected_arm_preserves_cached_other_arm_evidence()
 {
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
                            channel: Channel::new(5),
                        };
                        let cached_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn selected_arm_mask_preserves_actual_binding_evidence_frame_label()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn physical_frame_label_mismatch_is_phase_invariant_not_logical_label_mismatch()
 {
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
                            channel: Channel::new(12),
                        };
                        let mismatched = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                            instance: 15,
                            channel: Channel::new(13),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
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
                            route_decision_commit_evidence:
                                RouteDecisionCommitEvidence::CachedOrDemux,
                        };

                        let mut branch = (worker)
                            .produce_branch(
                                selection,
                                resolved,
                                crate::endpoint::kernel::offer::OfferScopeProfile::PassiveStatic,
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
