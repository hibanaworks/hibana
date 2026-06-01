use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn dropping_pending_decode_future_preserves_preview_branch_state()
 {
    run_offer_regression_test(
        "dropping_pending_decode_future_preserves_preview_branch_state",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport =
                                HintPendingTransport::new(pending_state, HINT_LEFT_DATA_FRAME);
                            let rv_id = cluster_ref
                                .register_rendezvous(config, transport)
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
                                        NoBinding,
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
                            let before_idx = worker.cursor.index();

                            let mut cx = Context::from_waker(noop_waker_ref());
                            let branch = {
                                let mut offer = pin!(cursor_offer(worker));
                                poll_ready_ok(&mut cx, offer.as_mut(), "preview branch offer")
                            };
                            assert_eq!(
                                branch_label(&branch),
                                HINT_LEFT_DATA_LABEL,
                                "offer must preview the hinted recv branch before decode"
                            );
                            let preview_ready_mask = worker.scope_ready_arm_mask(scope);
                            let preview_ack = worker.peek_scope_ack(scope);

                            {
                                let mut decode =
                                    pin!(CursorDecode::<Msg<100, u8>>::run(worker, branch));
                                assert!(
                                    matches!(decode.as_mut().poll(&mut cx), Poll::Pending),
                                    "decode should wait on transport recv before commit"
                                );
                                drop(decode);
                            }

                            assert_eq!(
                                worker.cursor.index(),
                                before_idx,
                                "dropping a pending decode future must not advance the cursor"
                            );
                            assert_eq!(
                                worker.peek_scope_ack(scope),
                                preview_ack,
                                "dropping a pending decode future must not consume ACK authority"
                            );
                            assert_eq!(
                                worker.scope_ready_arm_mask(scope),
                                preview_ready_mask,
                                "dropping a pending decode future must not clear ready-arm evidence"
                            );
                            assert!(
                                worker.selected_arm_for_scope(scope).is_none(),
                                "dropping a pending decode future must not commit route progress"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn restoring_public_preview_branch_clears_cached_arm_slot()
 {
    run_offer_regression_test(
        "restoring_public_preview_branch_clears_cached_arm_slot",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport =
                                HintPendingTransport::new(pending_state, HINT_LEFT_DATA_FRAME);
                            let rv_id = cluster_ref
                                .register_rendezvous(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(906);
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
                                scope
                            };

                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 0);
                            }

                            let worker = worker_slot.borrow_mut();
                            let mut cx = Context::from_waker(noop_waker_ref());

                            assert!(worker.init_public_offer_state());
                            let label = match worker.poll_public_offer(&mut cx) {
                                Poll::Ready(Ok(label)) => label,
                                Poll::Ready(Err(err)) => {
                                    panic!("public offer must materialize preview branch: {err:?}")
                                }
                                Poll::Pending => {
                                    panic!(
                                        "public offer must not pend once the hinted arm is ready"
                                    )
                                }
                            };
                            assert_eq!(
                                label, HINT_LEFT_DATA_LABEL,
                                "public offer must cache the hinted preview branch"
                            );
                            assert!(
                                worker.public_route_branch.is_some(),
                                "public offer must park the materialized branch until decode or drop"
                            );

                            worker.restore_public_route_branch();

                            assert!(
                                worker.public_route_branch.is_none(),
                                "restoring the preview branch must clear the cached public arm slot"
                            );

                            assert!(worker.init_public_offer_state());
                            let label = match worker.poll_public_offer(&mut cx) {
                                Poll::Ready(Ok(label)) => label,
                                Poll::Ready(Err(err)) => panic!(
                                    "re-offer after restore must rematerialize the branch: {err:?}"
                                ),
                                Poll::Pending => {
                                    panic!("re-offer after restore must not pend")
                                }
                            };
                            assert_eq!(
                                label, HINT_LEFT_DATA_LABEL,
                                "re-offer after restore must rematerialize the same branch from restored state"
                            );
                            assert!(
                                worker.public_route_branch.is_some(),
                                "re-offer after restore must park a fresh preview branch"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart()
 {
    run_offer_regression_test(
        "static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart",
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
                            let sid = SessionId::new(1201);
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
                            let controller = controller_slot.borrow_mut();
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            controller.port_for_lane(0).record_route_decision(scope, 1);

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            let mut offer = pin!(cursor_offer(worker));
                            match offer.as_mut().poll(&mut cx) {
                                Poll::Ready(Ok(branch)) => {
                                    panic!(
                                        "offer must not materialize before transport ingress: {}",
                                        branch_label(&branch)
                                    )
                                }
                                Poll::Ready(Err(err)) => {
                                    panic!("offer must wait for transport ingress: {err:?}")
                                }
                                Poll::Pending => {}
                            }
                            assert_eq!(
                                transport_probe.poll_count(),
                                1,
                                "known static passive arm must park on transport once instead of frontier-restarting"
                            );
                        });
                    });
                });
            });
        },
    );
}
