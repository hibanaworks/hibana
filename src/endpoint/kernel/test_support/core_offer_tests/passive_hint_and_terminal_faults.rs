#[test]
fn parked_passive_offer_does_not_drain_hint_from_same_lane() {
    run_offer_regression_test(
        "parked_passive_offer_does_not_drain_hint_from_same_lane",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            pending_state
                                .panic_on_hint_drain_while_recv_parked
                                .set(true);
                            let transport = HintPendingTransport::new(
                                pending_state,
                                <Msg<86, u8> as MessageSpec>::LOGICAL_LABEL,
                            );
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1203);
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

                            assert!(
                                matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                "first offer poll must park on transport recv"
                            );
                            assert!(
                                matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                "second offer poll must continue parked recv without draining hints"
                            );
                            transport_probe.assert_no_hint_drain_while_recv_parked();
                            assert_eq!(
                                transport_probe.poll_count(),
                                2,
                                "second offer poll must re-poll the same parked recv future"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn passive_dynamic_offer_consumes_fresh_hint_staged_by_ready_recv() {
    run_offer_regression_test(
        "passive_dynamic_offer_consumes_fresh_hint_staged_by_ready_recv",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, FreshHintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(FreshHintPendingWorkerEndpoint, worker_slot, {
                    with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                        with_offer_value_slot!(FreshHintRouteResolverState, deferred_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            deferred_state_slot.store(FreshHintRouteResolverState::new(1));
                            let resolver_state: &'static FreshHintRouteResolverState =
                                unsafe { &*deferred_state_slot.ptr() };
                            let transport = FreshHintPendingTransport::new(
                                pending_state,
                                HINT_RIGHT_DATA_FRAME,
                            );
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1205);
                            let worker_program = HINT_WORKER_PROGRAM();
                            cluster_ref
                                .set_resolver::<HINT_ROUTE_POLICY_ID, 1>(
                                    rv_id,
                                    &worker_program,
                                    crate::control::cluster::core::ResolverRef::route_state(
                                        resolver_state,
                                        fresh_hint_route_resolver,
                                    ),
                                )
                                .expect("register passive fresh-hint route resolver");
                            unsafe {
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
                            let scope = worker.cursor.node_scope_id();
                            assert!(!scope.is_none(), "worker must start at route scope");

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            let branch = {
                                let mut offer = pin!(cursor_offer(worker));
                                assert!(
                                    matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                    "first passive offer poll must park before transport payload arrives"
                                );
                                assert_eq!(
                                    transport_probe.poll_count(),
                                    1,
                                    "first passive offer poll must park exactly one recv"
                                );
                                assert_eq!(
                                    transport_probe.hint_drain_count(),
                                    0,
                                    "no route hint exists before poll_recv stages fresh receive state"
                                );

                                pending_state.ready.set(true);
                                unsafe {
                                    if let Some(waker) = (&mut *pending_state.waker.get()).take() {
                                        waker.wake();
                                    }
                                }

                                poll_ready_ok(
                                    &mut cx,
                                    offer.as_mut(),
                                    "passive offer after fresh recv hint",
                                )
                            };
                            assert_eq!(
                                resolver_state.calls(),
                                1,
                                "fresh frame hint must not bypass the dynamic route resolver"
                            );
                            assert_eq!(
                                branch_label(&branch),
                                HINT_RIGHT_DATA_LABEL,
                                "fresh frame hint staged by poll_recv must materialize the resolver-selected matching arm"
                            );
                            assert!(
                                branch_has_transport_payload(&branch),
                                "offer must keep the payload staged for RouteBranch::decode"
                            );
                            assert_eq!(
                                transport_probe.hint_drain_count(),
                                1,
                                "offer must consume the fresh hint staged by the ready poll_recv"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                0,
                                "selected payload must not be requeued before decode"
                            );

                            let mut decode =
                                pin!(CursorDecode::<Msg<101, u8>>::run(worker, branch));
                            let decoded =
                                poll_ready_ok(&mut cx, decode.as_mut(), "fresh-hint branch decode");
                            assert_eq!(decoded, 0x5a);
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn passive_dynamic_offer_does_not_use_fresh_hint_as_route_authority() {
    run_offer_regression_test(
        "passive_dynamic_offer_does_not_use_fresh_hint_as_route_authority",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, FreshHintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(FreshHintPendingWorkerEndpoint, worker_slot, {
                    with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                        with_offer_value_slot!(FreshHintRouteResolverState, deferred_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            deferred_state_slot.store(FreshHintRouteResolverState::new(0));
                            let resolver_state: &'static FreshHintRouteResolverState =
                                unsafe { &*deferred_state_slot.ptr() };
                            let transport = FreshHintPendingTransport::new(
                                pending_state,
                                HINT_RIGHT_DATA_FRAME,
                            );
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1206);
                            let worker_program = HINT_WORKER_PROGRAM();
                            cluster_ref
                                .set_resolver::<HINT_ROUTE_POLICY_ID, 1>(
                                    rv_id,
                                    &worker_program,
                                    crate::control::cluster::core::ResolverRef::route_state(
                                        resolver_state,
                                        fresh_hint_route_resolver,
                                    ),
                                )
                                .expect("register passive fresh-hint route resolver");
                            unsafe {
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
                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            {
                                let mut offer = pin!(cursor_offer(worker));
                                assert!(
                                    matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                    "first passive offer poll must park before transport payload arrives"
                                );

                                pending_state.ready.set(true);
                                unsafe {
                                    if let Some(waker) = (&mut *pending_state.waker.get()).take() {
                                        waker.wake();
                                    }
                                }

                                match offer.as_mut().poll(&mut cx) {
                                    Poll::Ready(Ok(branch)) => {
                                        panic!(
                                            "fresh frame hint selected branch {} without matching resolver arm",
                                            branch_label(&branch)
                                        );
                                    }
                                    Poll::Ready(Err(_)) | Poll::Pending => {}
                                }
                                assert_eq!(
                                    transport_probe.requeue_count(),
                                    0,
                                    "resolver/frame mismatch is terminal and must not roll back staged transport payload"
                                );
                            };
                            assert_eq!(
                                resolver_state.calls(),
                                1,
                                "fresh frame hint must reach resolver authority before any dynamic branch can materialize"
                            );
                            assert_eq!(
                                transport_probe.hint_drain_count(),
                                1,
                                "offer should consume the fresh hint only as demux/materialization evidence"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                0,
                                "dropping after a terminal resolver/frame mismatch must not requeue staged payload"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn decode_branch_commit_clears_stale_route_hints_on_selected_lane() {
    run_offer_regression_test(
        "decode_branch_commit_clears_stale_route_hints_on_selected_lane",
        || {
            const STALE_FRAME_HINT: u8 = 77;
            let stale_frame_hint_mask = FrameLabelMask::from_frame_label(STALE_FRAME_HINT);

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1204);
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

                        let controller = controller_slot.borrow_mut();
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        controller.port_for_lane(0).record_route_decision(scope, 1);

                        let offer_lane = worker.port_for_lane(0).lane();
                        assert!(
                            !worker
                                .port_for_lane(0)
                                .route_table()
                                .has_pending_frame_hint_for_lane(offer_lane, stale_frame_hint_mask),
                            "test must start without stale route hints on the selected lane"
                        );
                        worker
                            .port_for_lane(0)
                            .route_table()
                            .update_pending_frame_hint_mask_for_lane(
                                offer_lane,
                                FrameLabelMask::EMPTY,
                                stale_frame_hint_mask,
                            );
                        assert!(
                            worker
                                .port_for_lane(0)
                                .route_table()
                                .has_pending_frame_hint_for_lane(offer_lane, stale_frame_hint_mask),
                            "stale route hint must be staged before route-branch commit"
                        );

                        let mut cx = Context::from_waker(noop_waker_ref());
                        let branch = {
                            let mut offer = pin!(cursor_offer(worker));
                            poll_ready_ok(&mut cx, offer.as_mut(), "offer selected right branch")
                        };
                        assert_eq!(
                            branch_label(&branch),
                            HINT_RIGHT_DATA_LABEL,
                            "offer must still materialize the selected right branch"
                        );

                        {
                            let mut decode = pin!(
                                CursorDecode::<Msg<HINT_RIGHT_DATA_LABEL, u8>>::run(worker, branch)
                            );
                            let _ = poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "decode right branch and commit route preview",
                            );
                        }
                        assert!(
                            !worker
                                .port_for_lane(0)
                                .route_table()
                                .has_pending_frame_hint_for_lane(offer_lane, stale_frame_hint_mask),
                            "decode commit must clear offer-scoped route hints on the selected lane"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn nested_dispatch_arm_counts_as_recv_for_known_passive_route() {
    run_offer_regression_test(
        "nested_dispatch_arm_counts_as_recv_for_known_passive_route",
        || {
            #[inline(never)]
            fn assert_known_passive_route_waits_for_ingress(
                cx: &mut Context<'_>,
                worker_slot: &mut OfferValueSlotGuard<'_, PendingWorkerEndpoint>,
                transport_probe: &PendingTransport,
            ) {
                let worker = worker_slot.borrow_mut();
                let scope = worker.cursor.node_scope_id();
                assert!(
                    worker.arm_has_recv(scope, 1),
                    "nested first-recv dispatch must count as recv-bearing arm"
                );

                let mut offer = pin!(cursor_offer(worker));
                match offer.as_mut().poll(cx) {
                    Poll::Ready(Ok(branch)) => panic!(
                        "known passive route with nested dispatch recv must wait for wire ingress, got {}",
                        branch_label(&branch)
                    ),
                    Poll::Ready(Err(err)) => {
                        panic!(
                            "known passive route with nested dispatch recv must not fail: {err:?}"
                        )
                    }
                    Poll::Pending => {}
                }
                assert_eq!(
                    transport_probe.poll_count(),
                    1,
                    "known passive route with nested dispatch recv must still poll transport once"
                );
            }

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
                            let sid = SessionId::new(1202);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &NESTED_DISPATCH_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &NESTED_DISPATCH_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }

                            let scope = {
                                let worker = worker_slot.borrow_mut();
                                worker.cursor.node_scope_id()
                            };
                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 1);
                            }

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            assert_known_passive_route_waits_for_ingress(
                                &mut cx,
                                worker_slot,
                                &transport_probe,
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn scope_local_label_mapping_never_uses_global_scan() {
    run_offer_regression_test("scope_local_label_mapping_never_uses_global_scan", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(992);
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
                    let controller_borrow = controller_slot.borrow_mut();
                    core::hint::black_box(&controller_borrow);
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    let foreign_frame_label = (1u8..=u8::MAX).find(|frame_label| {
                        !matches!(
                            *frame_label,
                            TEST_LOOP_CONTINUE_FRAME | TEST_LOOP_BREAK_FRAME
                        ) && worker
                            .cursor
                            .first_recv_target_for_lane_frame_label(scope, 0, *frame_label)
                            .is_none()
                            && worker
                                .cursor
                                .find_arm_for_recv_lane_frame_label(0, *frame_label)
                                .is_some()
                    });
                    let Some(foreign_frame_label) = foreign_frame_label else {
                        // FIRST-recv dispatch can fully cover this scope; no entry-only
                        // entry-only dispatch evidence remains to probe.
                        return;
                    };

                    let frame_label_meta =
                        endpoint_scope_frame_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
                    worker.ingest_binding_scope_evidence(
                        scope,
                        0,
                        foreign_frame_label,
                        false,
                        frame_label_meta,
                    );

                    assert!(
                        !worker.scope_has_ready_arm_evidence(scope),
                        "foreign frame label {} must not become scope-local arm-ready evidence: hint={} arm={:?} evidence={:?} ready_mask=0b{:02b} controller={}",
                        foreign_frame_label,
                        frame_label_meta.matches_frame_hint(foreign_frame_label),
                        frame_label_meta.arm_for_frame_label(foreign_frame_label),
                        frame_label_meta.evidence_arm_for_frame_label(foreign_frame_label),
                        worker.scope_ready_arm_mask(scope),
                        worker.cursor.is_route_controller(scope)
                    );
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "foreign label must not mint route authority"
                    );
                });
            });
        });
    });
}

#[test]
fn payload_staging_is_selected_scope_lane_stable() {
    let mut scratch = [0u8; 8];
    let src = [9u8, 8, 7, 6];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}
