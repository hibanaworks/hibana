use super::*;

#[test]
fn passive_route_decode_allows_tail_send_from_same_endpoint() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
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
                    .role(&routed_payload_with_tail_role1_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(right_route_resolver),
                    )
                    .expect("register role1 route resolver");

                let sid = SessionId::new(18);
                with_tls_mut(
                    &WORKER_ROLE0_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&routed_payload_with_tail_role0_worker_program())
                                .enter(NoBinding)
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
                                        .role(&routed_payload_with_tail_role1_controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                block_on_async(async {
                                    controller
                                        .flow::<Msg<
                                            ROUTE_RIGHT_CONTROL_LOGICAL,
                                            GenericCapToken<RouteRightKind>,
                                            RouteRightKind,
                                        >>()
                                        .expect("right route control must be available")
                                        .send(())
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
                                        .expect("passive offer must select routed payload");
                                    assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                    let payload = branch
                                        .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .await
                                        .expect("passive decode must commit route arm");
                                    assert_eq!(payload, 7);

                                    worker
                                        .flow::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                        .expect("tail flow must be available after route decode")
                                        .send(&1u8)
                                        .await
                                        .expect("tail send must progress after route decode");

                                    let ack = controller
                                        .recv::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                        .await
                                        .expect("controller must receive tail ack");
                                    assert_eq!(ack, 1);
                                });
                            },
                        );
                    },
                );

                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn split_kits_passive_role0_decodes_payload_after_local_resolver_decision() {
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
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |controller_kit| {
                with_resident_tls_ref(
                    &SESSION_SLOT_B,
                    |storage| unsafe { SessionKit::init_in_place(storage) },
                    |worker_kit| {
                        let controller_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (controller_tap, controller_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let worker_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
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
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role1 route resolver");
                        worker_kit
                            .rendezvous(worker_rv)
                            .role(&routed_payload_role0_worker_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role0 route resolver");

                        let sid = SessionId::new(13);
                        with_tls_mut(
                            &WORKER_ROLE0_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    worker_kit
                                        .rendezvous(worker_rv)
                                        .session(sid)
                                        .role(&routed_payload_role0_worker_program())
                                        .enter(NoBinding)
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
                                                .enter(NoBinding)
                                                .expect("controller endpoint"),
                                        );
                                    },
                                    |controller| {
                                        block_on_async(async {
                                            controller
                                                .flow::<Msg<
                                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                                    GenericCapToken<RouteRightKind>,
                                                    RouteRightKind,
                                                >>()
                                                .expect("right route control must be available")
                                                .send(())
                                                .await
                                                .expect(
                                                    "right route control self-send must resolve",
                                                );

                                            controller
                                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .expect(
                                                    "right route payload flow must be available",
                                                )
                                                .send(&9u8)
                                                .await
                                                .expect(
                                                    "right route payload send must cross transport",
                                                );

                                            let branch = worker.offer().await.expect(
                                                "split worker offer must select routed payload",
                                            );
                                            assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                            let payload = branch
                                                .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .await
                                                .expect(
                                                    "split worker decode must commit route arm",
                                                );
                                            assert_eq!(payload, 9);
                                        });
                                    },
                                );
                            },
                        );
                    },
                )
            },
        );
    });
}

#[test]
fn split_kits_passive_route_decode_allows_tail_send() {
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
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |controller_kit| {
                with_resident_tls_ref(
                    &SESSION_SLOT_B,
                    |storage| unsafe { SessionKit::init_in_place(storage) },
                    |worker_kit| {
                        let controller_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (controller_tap, controller_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let worker_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
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
                            .role(&routed_payload_with_tail_role1_controller_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role1 route resolver");
                        worker_kit
                            .rendezvous(worker_rv)
                            .role(&routed_payload_with_tail_role0_worker_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role0 route resolver");

                        let sid = SessionId::new(19);
                        with_tls_mut(
                            &WORKER_ROLE0_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    worker_kit
                                        .rendezvous(worker_rv)
                                        .session(sid)
                                        .role(&routed_payload_with_tail_role0_worker_program())
                                        .enter(NoBinding)
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
                                                .rendezvous(controller_rv).session(sid).role(&routed_payload_with_tail_role1_controller_program()).enter(NoBinding,)
                                                .expect("controller endpoint"),
                                        );
                                    },
                                    |controller| {
                                        block_on_async(async {
                                            controller
                                                .flow::<Msg<
                                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                                    GenericCapToken<RouteRightKind>,
                                                    RouteRightKind,
                                                >>()
                                                .expect("right route control must be available")
                                                .send(())
                                                .await
                                                .expect(
                                                    "right route control self-send must resolve",
                                                );

                                            controller
                                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .expect(
                                                    "right route payload flow must be available",
                                                )
                                                .send(&9u8)
                                                .await
                                                .expect(
                                                    "right route payload send must cross transport",
                                                );

                                            let branch = worker.offer().await.expect(
                                                "split worker offer must select routed payload",
                                            );
                                            assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                            let payload = branch
                                                .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .await
                                                .expect(
                                                    "split worker decode must commit route arm",
                                                );
                                            assert_eq!(payload, 9);

                                            worker
                                                .flow::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                                .expect(
                                                    "split tail flow must be available after route decode",
                                                )
                                                .send(&1u8)
                                                .await
                                                .expect(
                                                    "split tail send must progress after route decode",
                                                );

                                            let ack = controller
                                                .recv::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                                .await
                                                .expect("controller must receive split tail ack");
                                            assert_eq!(ack, 1);
                                        });
                                    },
                                );
                            },
                        );

                        assert!(transport.queue_is_empty());
                    },
                )
            },
        );
    });
}

#[test]
fn in_place_split_kits_one_endpoint_allow_route_tail_send() {
    with_fixture(|_clock, controller_tap, slab| {
        let worker_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let (controller_slab, worker_slab) = slab.split_at_mut(512 * 1024);
        let controller_storage = Box::leak(Box::new(MaybeUninit::<EmbeddedTestKit>::uninit()));
        let worker_storage = Box::leak(Box::new(MaybeUninit::<EmbeddedTestKit>::uninit()));
        let controller_kit = unsafe { EmbeddedTestKit::init_in_place(controller_storage) };
        let worker_kit = unsafe { EmbeddedTestKit::init_in_place(worker_storage) };
        let transport = TestTransport::default();
        let controller_config =
            Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
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
            .expect("register in-place controller rendezvous");
        let worker_rv = worker_kit
            .add_rendezvous_from_config(worker_config, transport.clone())
            .expect("register in-place worker rendezvous");
        controller_kit
            .rendezvous(controller_rv)
            .role(&routed_payload_with_tail_role1_controller_program())
            .set_resolver::<ROUTE_POLICY_ID>(hibana::integration::policy::ResolverRef::route_fn(
                right_route_resolver,
            ))
            .expect("register in-place role1 route resolver");
        worker_kit
            .rendezvous(worker_rv)
            .role(&routed_payload_with_tail_role0_worker_program())
            .set_resolver::<ROUTE_POLICY_ID>(hibana::integration::policy::ResolverRef::route_fn(
                right_route_resolver,
            ))
            .expect("register in-place role0 route resolver");

        let sid = SessionId::new(20);
        with_tls_mut(
            &WORKER_ROLE0_ENDPOINT_SLOT,
            |ptr| unsafe {
                write_value(
                    ptr,
                    worker_kit
                        .rendezvous(worker_rv)
                        .session(sid)
                        .role(&routed_payload_with_tail_role0_worker_program())
                        .enter(NoBinding)
                        .expect("in-place worker endpoint"),
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
                                .role(&routed_payload_with_tail_role1_controller_program())
                                .enter(NoBinding)
                                .expect("in-place controller endpoint"),
                        );
                    },
                    |controller| {
                        block_on_async(async {
                            controller
                                .flow::<Msg<
                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                    GenericCapToken<RouteRightKind>,
                                    RouteRightKind,
                                >>()
                                .expect("right route control must be available")
                                .send(())
                                .await
                                .expect("right route control self-send must resolve");

                            controller
                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                .expect("right route payload flow must be available")
                                .send(&11u8)
                                .await
                                .expect("right route payload send must cross transport");

                            let branch = worker
                                .offer()
                                .await
                                .expect("in-place worker offer must select routed payload");
                            assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                            let payload = branch
                                .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                .await
                                .expect("in-place worker decode must commit route arm");
                            assert_eq!(payload, 11);

                            worker
                                .flow::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                .expect("in-place tail flow must be available after route decode")
                                .send(&1u8)
                                .await
                                .expect("in-place tail send must progress after route decode");

                            let ack = controller
                                .recv::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                .await
                                .expect("controller must receive in-place tail ack");
                            assert_eq!(ack, 1);
                        });
                    },
                );
            },
        );

        assert!(transport.queue_is_empty());
    });
}
