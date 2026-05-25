use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn reset_public_offer_state_restores_empty_carried_transport_payload()
 {
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn terminal_offer_clear_discards_carried_preview_state()
 {
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
