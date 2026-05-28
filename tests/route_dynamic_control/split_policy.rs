use super::*;

#[test]
fn split_kits_passive_dynamic_route_does_not_use_payload_label_as_authority() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    with_fixture(|_clock, tap_buf, slab| {
        let _ = tap_buf;
        let controller_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let worker_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let (controller_slab, worker_slab) = slab.split_at_mut(512 * 1024);
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |controller_kit| {
            with_resident_tls_ref(&SESSION_SLOT_B, |worker_kit| {
                let controller_config = Config::<
                    hibana::integration::runtime::DefaultLabelUniverse,
                    _,
                >::from_resources(
                    (controller_tap, controller_slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
                let worker_config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (worker_tap, worker_slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let controller_rv = controller_kit
                    .add_rendezvous_from_config(controller_config, transport.clone())
                    .expect("register controller rendezvous");
                let worker_rv = worker_kit
                    .add_rendezvous_from_config(worker_config, transport.clone())
                    .expect("register worker rendezvous");
                controller_kit
                    .rendezvous(controller_rv)
                    .role(&routed_payload_role1_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::decision_fn(right_route_resolver),
                    )
                    .expect("register role1 decision resolver");

                let sid = SessionId::new(14);
                with_tls_mut(
                    &WORKER_ROLE0_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            worker_kit
                                .rendezvous(worker_rv)
                                .session(sid)
                                .role(&routed_payload_role0_worker_program())
                                .enter(None)
                                .expect("worker endpoint"),
                        );
                    },
                    |worker| {
                        with_tls_mut(
                            &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    controller_kit
                                        .rendezvous(controller_rv)
                                        .session(sid)
                                        .role(&routed_payload_role1_controller_program())
                                        .enter(None)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                block_on_async(async {
                                    controller
                                        .flow::<Msg<
                                            ROUTE_RIGHT_CONTROL_LOGICAL,
                                            (),
                                            RouteRightKind,
                                        >>()
                                        .expect("right route control must be available")
                                        .send(&())
                                        .await
                                        .expect("right route control self-send must resolve");

                                    controller
                                        .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .expect("right route payload flow must be available")
                                        .send(&11u8)
                                        .await
                                        .expect("right route payload send must cross transport");
                                });

                                let mut offer = Box::pin(worker.offer());
                                let waker = futures::task::noop_waker();
                                let mut cx = Context::from_waker(&waker);
                                match Pin::as_mut(&mut offer).poll(&mut cx) {
                                    Poll::Ready(Ok(branch)) => panic!(
                                        "dynamic route selected branch {} from payload label without passive resolver authority",
                                        branch.label()
                                    ),
                                    Poll::Ready(Err(_)) | Poll::Pending => {}
                                }
                            },
                        );
                    },
                );
            })
        });
    });
}

#[test]
fn route_head_policy_ignores_later_arm_dynamic_controls_on_enter() {
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
                .role(&route_tail_controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(route_resolver),
                )
                .expect("register decision resolver");
            set_route_allow(true);

            let sid = SessionId::new(10);
            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .rendezvous(rv_id)
                            .session(sid)
                            .role(&route_tail_worker_program())
                            .enter(None)
                            .expect("worker endpoint"),
                    );
                },
                |_worker| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                cluster
                                    .rendezvous(rv_id)
                                    .session(sid)
                                    .role(&route_tail_controller_program())
                                    .enter(None)
                                    .expect("controller endpoint"),
                            );
                        },
                        |controller| {
                            let route_flow = controller
                                .flow::<Msg<
                                    { TEST_ROUTE_DECISION_LOGICAL },
                                    (),
                                    RouteDecisionKind,
                                >>()
                                .expect("route flow should remain available after enter");
                            drop(route_flow);
                        },
                    );
                },
            );

            set_route_allow(false);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn route_send_aborts_when_decision_policy_input_changes_after_preview() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            with_tls_mut(
                &POLICY_INPUT_SLOT,
                |ptr: *mut Cell<u32>| unsafe { ptr.write(Cell::new(0)) },
                |policy_input0| {
                    let policy_input: &'static Cell<u32> = policy_input0;
                    with_tls_mut(
                        &POLICY_BINDING_SLOT,
                        |ptr: *mut PolicyInputBinding| unsafe {
                            ptr.write(PolicyInputBinding::new(policy_input))
                        },
                        |controller_binding| {
                            let config = Config::<
                                hibana::integration::runtime::DefaultLabelUniverse,
                                _,
                            >::from_resources(
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
                                    hibana::integration::policy::ResolverRef::decision_fn(
                                        decision_policy_input_resolver,
                                    ),
                                )
                                .expect("register decision resolver");

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
                                            .enter(None)
                                            .expect("worker endpoint"),
                                    );
                                },
                                |_worker| {
                                    with_tls_mut(
                                        &CONTROLLER_ENDPOINT_SLOT,
                                        |ptr| unsafe {
                                            write_value(
                                                ptr,
                                                cluster
                                                    .rendezvous(rv_id)
                                                    .session(sid)
                                                    .role(&controller_program())
                                                    .enter(Some(controller_binding))
                                                    .expect("controller endpoint"),
                                            );
                                        },
                                        |controller| {
                                            block_on_async(async {
                                                let send_flow = controller
                                                    .flow::<Msg<
                                                        { TEST_ROUTE_DECISION_LOGICAL },
                                                        (),
                                                        RouteDecisionKind,
                                                    >>(
                                                    )
                                                    .expect("route should select left arm");

                                                policy_input.set(1);

                                                let err = send_flow
                                                    .send(&())
                                                    .await
                                                    .expect_err(
                                                        "send must reject stale preview when decision policy changes",
                                                    );
                                                assert!(
                                                    format!("{err:?}").contains("PolicyAbort"),
                                                    "policy drift must abort the dynamic decision send: {err:?}"
                                                );
                                            });
                                        },
                                    );
                                },
                            );
                            assert!(transport_queue_is_empty(&transport));
                        },
                    )
                },
            );
        });
    });
}

/// Test that self-send loop control type definitions compile correctly.
///
/// Loop and route controls share the same decision resolver contract: a
/// `.policy()` annotation makes the resolver authoritative, while an unannotated
/// route remains static.
#[test]
fn loop_dynamic_resolver_policy_abort_and_success() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            with_tls_mut(
                &POLICY_INPUT_SLOT,
                |ptr: *mut Cell<u32>| unsafe { ptr.write(Cell::new(0)) },
                |policy_input0| {
                    let policy_input: &'static Cell<u32> = policy_input0;

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
                        .role(&loop_controller_program())
                        .set_resolver::<LOOP_POLICY_ID>(
                            hibana::integration::policy::ResolverRef::decision_fn(
                                decision_policy_input_resolver,
                            ),
                        )
                        .expect("register loop decision resolver");

                    policy_input.set(0);
                    with_tls_mut(
                        &POLICY_BINDING_SLOT,
                        |ptr: *mut PolicyInputBinding| unsafe {
                            ptr.write(PolicyInputBinding::new(policy_input))
                        },
                        |controller_binding| {
                            with_tls_mut(
                                &CONTROLLER_ENDPOINT_SLOT,
                                |ptr| unsafe {
                                    write_value(
                                        ptr,
                                        cluster
                                            .rendezvous(rv_id)
                                            .session(SessionId::new(30))
                                            .role(&loop_controller_program())
                                            .enter(Some(controller_binding))
                                            .expect("continue endpoint"),
                                    );
                                },
                                |controller| {
                                    block_on_async(async {
                                        controller
                                            .flow::<Msg<
                                                { TEST_LOOP_CONTINUE_LOGICAL },
                                                (),
                                                LoopContinueKind,
                                            >>()
                                            .expect("continue flow must be available")
                                            .send(&())
                                            .await
                                            .expect("continue must match left decision arm");
                                    });
                                },
                            );
                        },
                    );

                    policy_input.set(0);
                    with_tls_mut(
                        &POLICY_BINDING_SLOT,
                        |ptr: *mut PolicyInputBinding| unsafe {
                            ptr.write(PolicyInputBinding::new(policy_input))
                        },
                        |controller_binding| {
                            with_tls_mut(
                                &CONTROLLER_ENDPOINT_SLOT,
                                |ptr| unsafe {
                                    write_value(
                                        ptr,
                                        cluster
                                            .rendezvous(rv_id)
                                            .session(SessionId::new(31))
                                            .role(&loop_controller_program())
                                            .enter(Some(controller_binding))
                                            .expect("break mismatch endpoint"),
                                    );
                                },
                                |controller| {
                                    block_on_async(async {
                                        let err = controller
                                            .flow::<Msg<
                                                { TEST_LOOP_BREAK_LOGICAL },
                                                (),
                                                LoopBreakKind,
                                            >>()
                                            .expect("break flow must be available")
                                            .send(&())
                                            .await
                                            .expect_err(
                                                "break must abort when resolver selects continue",
                                            );
                                        assert!(
                                            format!("{err:?}").contains("PolicyAbort"),
                                            "loop decision mismatch must surface as policy abort: {err:?}"
                                        );
                                    });
                                },
                            );
                        },
                    );

                    policy_input.set(1);
                    with_tls_mut(
                        &POLICY_BINDING_SLOT,
                        |ptr: *mut PolicyInputBinding| unsafe {
                            ptr.write(PolicyInputBinding::new(policy_input))
                        },
                        |controller_binding| {
                            with_tls_mut(
                                &CONTROLLER_ENDPOINT_SLOT,
                                |ptr| unsafe {
                                    write_value(
                                        ptr,
                                        cluster
                                            .rendezvous(rv_id)
                                            .session(SessionId::new(32))
                                            .role(&loop_controller_program())
                                            .enter(Some(controller_binding))
                                            .expect("break endpoint"),
                                    );
                                },
                                |controller| {
                                    block_on_async(async {
                                        controller
                                            .flow::<Msg<
                                                { TEST_LOOP_BREAK_LOGICAL },
                                                (),
                                                LoopBreakKind,
                                            >>()
                                            .expect("break flow must be available")
                                            .send(&())
                                            .await
                                            .expect("break must match right decision arm");
                                    });
                                },
                            );
                        },
                    );
                    assert!(transport_queue_is_empty(&transport));
                },
            );
        });
    });
}

/// Test nested routes with flow().send(&()) pattern.
///
/// With self-send local control (Controller → Controller), all route decisions
/// are local to the Controller role. Worker doesn't participate in route control.
#[test]
fn nested_loop_dynamic_send_and_offer() {
    with_fixture(|_clock, tap_buf, slab| {
        let config =
            Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                (tap_buf, slab),
                hibana::integration::runtime::CounterClock::new(),
            );
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv_id = cluster
                .add_rendezvous_from_config(config, transport.clone())
                .expect("register rendezvous");
            assert_ne!(rv_id.raw(), 0);

            let controller_program = nested_loop_controller_program();
            drop(controller_program);
        });

        assert!(transport_queue_is_empty(&transport));
    });
}
