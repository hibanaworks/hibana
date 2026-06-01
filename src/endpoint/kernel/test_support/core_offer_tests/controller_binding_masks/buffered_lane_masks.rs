use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_binding_for_offer_prefers_exact_label_for_ack_arm()
 {
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
                            .register_rendezvous(config, transport)
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
                                            channel: Channel::new(3),
                                        },
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                                            instance: 9,
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
                            state_index_to_usize(worker.decision_state.lane_offer_state(0).entry);
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane()
 {
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
                            .register_rendezvous(config, transport)
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm()
 {
    run_offer_regression_test(
        "poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .register_rendezvous(config, transport)
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
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_binding_for_offer_prefers_authoritative_arm_frame_label_mask_when_non_singleton()
 {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_authoritative_arm_frame_label_mask_when_non_singleton",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .register_rendezvous(config, transport)
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
                                            channel: Channel::new(5),
                                        },
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                            instance: 7,
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
                            state_index_to_usize(worker.decision_state.lane_offer_state(0).entry);
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
