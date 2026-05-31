use super::*;

#[test]
fn route_dynamic_self_send_send_path_requires_decision_resolver_match() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();

            let rv_id = cluster
                .add_rendezvous_from_config(config, transport.clone())
                .expect("register rendezvous");
            cluster
                .rendezvous(rv_id)
                .role(&controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(route_resolver),
                )
                .expect("register decision resolver");

            let sid = SessionId::new(7);

            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |_worker_endpoint| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid)
                                    .role(&controller_program())
                                    .enter()
                                    .expect("controller endpoint"),
                            );
                        },
                        |controller_cursor| {
                            set_route_allow(false);
                            block_on_async(async {
                                let first_flow = controller_cursor
                                    .flow::<Msg<
                                        { TEST_ROUTE_DECISION_LOGICAL },
                                        (),
                                        RouteDecisionKind,
                                    >>()
                                    .expect("self-send route flow should be available");
                                let err = first_flow
                                    .send(&())
                                    .await
                                    .expect_err("self-send route must obey decision resolver");
                                assert!(
                                    format!("{err:?}").contains("PolicyAbort"),
                                    "resolver rejection must surface as policy abort: {err:?}"
                                );
                            });
                        },
                    );
                },
            );

            set_route_allow(true);

            let sid2 = SessionId::new(8);

            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid2)
                            .role(&worker_program())
                            .enter()
                            .expect("worker endpoint (retry)"),
                    );
                },
                |_worker_endpoint| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid2)
                                    .role(&controller_program())
                                    .enter()
                                    .expect("controller endpoint (retry)"),
                            );
                        },
                        |controller_cursor| {
                            block_on_async(async {
                                let send_flow = controller_cursor
                                    .flow::<Msg<
                                        { TEST_ROUTE_DECISION_LOGICAL },
                                        (),
                                        RouteDecisionKind,
                                    >>()
                                    .expect("route should proceed when allowed");

                                send_flow.send(&()).await.expect("send route decision");
                            });
                        },
                    );
                },
            );

            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn route_dynamic_self_send_offer_resolves_without_controller_arm_entry() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();

            let rv_id = cluster
                .add_rendezvous_from_config(config, transport.clone())
                .expect("register rendezvous");
            cluster
                .rendezvous(rv_id)
                .role(&controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(route_resolver),
                )
                .expect("register decision resolver");

            set_route_allow(true);
            let sid = SessionId::new(9);

            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |_worker_endpoint| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid)
                                    .role(&controller_program())
                                    .enter()
                                    .expect("controller endpoint"),
                            );
                        },
                        |controller_cursor| {
                            block_on_async(async {
                                let branch = controller_cursor.offer().await.expect(
                                    "self-send route offer should resolve via decision policy",
                                );
                                assert_eq!(
                                    branch.label(),
                                    TEST_ROUTE_DECISION_LOGICAL,
                                    "self-send dynamic offer must resolve the selected arm without controller arm entries"
                                );
                            });
                        },
                    );
                },
            );

            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn passive_dynamic_offer_decodes_payload_selected_by_controller_route_frame() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();

            let rv_id = cluster
                .add_rendezvous_from_config(config, transport.clone())
                .expect("register rendezvous");
            cluster
                .rendezvous(rv_id)
                .role(&routed_payload_controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(right_route_resolver),
                )
                .expect("register controller decision resolver");

            let sid = SessionId::new(11);
            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&routed_payload_worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |worker| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid)
                                    .role(&routed_payload_controller_program())
                                    .enter()
                                    .expect("controller endpoint"),
                            );
                        },
                        |controller| {
                            block_on_async(async {
                                controller
                                    .flow::<Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>>()
                                    .expect("right route control must be available")
                                    .send(&())
                                    .await
                                    .expect("right route control self-send must resolve");

                                controller
                                    .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                    .expect("right route payload flow must be available")
                                    .send(&42u8)
                                    .await
                                    .expect("right route payload send must cross transport");

                                let branch = worker
                                    .offer()
                                    .await
                                    .expect("passive worker offer must select routed payload");
                                assert_eq!(
                                    branch.label(),
                                    ROUTE_RIGHT_PAYLOAD_LOGICAL,
                                    "passive branch label must come from projected payload frame"
                                );
                                let payload = branch
                                    .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                    .await
                                    .expect("passive branch decode must commit route arm");
                                assert_eq!(payload, 42);
                            });
                        },
                    );
                },
            );

            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn send_first_route_branch_decode_is_phase_invariant() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();

            let rv_id = cluster
                .add_rendezvous_from_config(config, transport.clone())
                .expect("register rendezvous");
            cluster
                .rendezvous(rv_id)
                .role(&send_first_route_controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(right_route_resolver),
                )
                .expect("register controller decision resolver");

            let sid = SessionId::new(111);
            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&send_first_route_worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |worker| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid)
                                    .role(&send_first_route_controller_program())
                                    .enter()
                                    .expect("controller endpoint"),
                            );
                        },
                        |controller| {
                            block_on_async(async {
                                controller
                                    .flow::<Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>>()
                                    .expect("right route control must be available")
                                    .send(&())
                                    .await
                                    .expect("right route control self-send must resolve");

                                let branch = worker
                                    .offer()
                                    .await
                                    .expect("worker offer must select send-first arm");
                                assert_eq!(
                                    branch.label(),
                                    ROUTE_SEND_FIRST_PAYLOAD_LOGICAL,
                                    "send-first branch must preview the first send label"
                                );

                                let err = branch
                                    .decode::<Msg<ROUTE_SEND_FIRST_PAYLOAD_LOGICAL, ()>>()
                                    .await
                                    .expect_err("send-first arm must not decode");
                                let rendered = format!("{err:?}");
                                assert!(
                                    rendered.contains("PhaseInvariant")
                                        && !rendered.contains("SessionFault"),
                                    "send-first decode must fail before synthetic progress: {rendered}"
                                );
                            });
                        },
                    );
                },
            );

            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn passive_role0_offer_decodes_payload_selected_by_role1_controller_route_frame() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();

            let rv_id = cluster
                .add_rendezvous_from_config(config, transport.clone())
                .expect("register rendezvous");
            cluster
                .rendezvous(rv_id)
                .role(&routed_payload_role1_controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(right_route_resolver),
                )
                .expect("register role1 decision resolver");

            let sid = SessionId::new(12);
            with_tls_mut(
                &WORKER_ROLE0_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&routed_payload_role0_worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |worker| {
                    with_tls_mut(
                        &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid)
                                    .role(&routed_payload_role1_controller_program())
                                    .enter()
                                    .expect("controller endpoint"),
                            );
                        },
                        |controller| {
                            block_on_async(async {
                                controller
                                    .flow::<Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>>()
                                    .expect("right route control must be available")
                                    .send(&())
                                    .await
                                    .expect("right route control self-send must resolve");

                                controller
                                    .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                    .expect("right route payload flow must be available")
                                    .send(&7u8)
                                    .await
                                    .expect("right route payload send must cross transport");

                                let branch = worker
                                    .offer()
                                    .await
                                    .expect("passive role0 offer must select routed payload");
                                assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                let payload = branch
                                    .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                    .await
                                    .expect("passive role0 decode must commit route arm");
                                assert_eq!(payload, 7);
                            });
                        },
                    );
                },
            );

            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn passive_dynamic_offer_without_route_evidence_waits_instead_of_faulting() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();

            let rv_id = cluster
                .add_rendezvous_from_config(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(17);

            with_tls_mut(
                &WORKER_ROLE0_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&routed_payload_with_tail_role0_worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |worker| {
                    let mut offer = Box::pin(worker.offer());
                    let waker = futures::task::noop_waker();
                    let mut cx = Context::from_waker(&waker);
                    match Future::poll(offer.as_mut(), &mut cx) {
                        Poll::Pending => {}
                        Poll::Ready(Ok(branch)) => {
                            panic!(
                                "passive dynamic offer selected branch {} without route evidence",
                                branch.label()
                            );
                        }
                        Poll::Ready(Err(error)) => {
                            panic!(
                                "passive dynamic offer must wait for route evidence, got {error:?}"
                            );
                        }
                    }
                },
            )
        });
    });
}
