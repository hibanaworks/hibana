use super::*;

#[test]
fn binding_read_error_in_public_decode_preserves_binding_diagnostic() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            with_tls_mut(
                &FLOW_SHARED_SLOT,
                |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                |shared| {
                    shared.reset();
                    let shared_ref: &'static FlowBindingShared = shared;
                    let transport = FlowTransport::new(shared_ref);
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, transport.clone())
                        .expect("register rv");

                    register_route_resolvers_for_program(&cluster, rv_id, &controller_program());
                    register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                    let sid = SessionId::new(904);
                    with_tls_mut(
                        &CONTROLLER_BINDING_SLOT,
                        |ptr: *mut FlowBinding| unsafe {
                            ptr.write(FlowBinding::new(0, shared_ref))
                        },
                        |controller_binding| {
                            with_tls_mut(
                                &WORKER_BINDING_SLOT,
                                |ptr: *mut FlowBinding| unsafe {
                                    ptr.write(FlowBinding::new(1, shared_ref))
                                },
                                |worker_binding| {
                                    with_tls_mut(
                                        &CONTROLLER_ENDPOINT_SLOT,
                                        |ptr| unsafe {
                                            write_value(
                                                ptr,
                                                cluster
                                                    .rendezvous(rv_id)
                                                    .session(sid)
                                                    .role(&controller_program())
                                                    .enter(controller_binding)
                                                    .expect("attach controller"),
                                            );
                                        },
                                        |controller| {
                                            with_tls_mut(
                                                &WORKER_ENDPOINT_SLOT,
                                                |ptr| unsafe {
                                                    write_value(
                                                        ptr,
                                                        cluster
                                                            .rendezvous(rv_id)
                                                            .session(sid)
                                                            .role(&worker_program())
                                                            .enter(worker_binding)
                                                            .expect("attach worker"),
                                                    );
                                                },
                                                |worker| {
                                                    futures::executor::block_on(async move {
                                                        controller
                                                            .flow::<Msg<
                                                                { TEST_ROUTE_DECISION_LOGICAL },
                                                                (),
                                                                RouteDecisionKind,
                                                            >>(
                                                            )
                                                            .expect("control flow")
                                                            .send(&())
                                                            .await
                                                            .expect("send route control");

                                                        controller
                                                            .flow::<Msg<71, u32>>()
                                                            .expect("left data flow")
                                                            .send(&9999)
                                                            .await
                                                            .expect("send left data");

                                                        let worker_branch = worker
                                                            .offer()
                                                            .await
                                                            .expect("offer left arm");
                                                        assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LOGICAL_LABEL
                                                            );

                                                        shared_ref.state.with_mut(
                                                            FlowBindingSharedState::clear_payloads,
                                                        );

                                                        let err = worker_branch
                                                                .decode::<Msg<71, u32>>()
                                                                .await
                                                                .expect_err(
                                                                    "missing binding payload must fail decode",
                                                                );
                                                        let rendered = format!("{err:?}");
                                                        assert!(
                                                            rendered.contains("Binding")
                                                                && rendered
                                                                    .contains("ChannelUnavailable"),
                                                            "first decode failure must preserve binding error, got {rendered}"
                                                        );
                                                        assert!(
                                                            !rendered.contains("PhaseInvariant"),
                                                            "binding read errors must not be collapsed into PhaseInvariant, got {rendered}"
                                                        );

                                                        let continuation_err = match worker
                                                            .offer()
                                                            .await
                                                        {
                                                            Ok(_) => panic!(
                                                                "binding failure must poison the session"
                                                            ),
                                                            Err(error) => error,
                                                        };
                                                        let continuation_rendered =
                                                            format!("{continuation_err:?}");
                                                        assert!(
                                                            continuation_rendered
                                                                .contains("SessionFault")
                                                                && continuation_rendered
                                                                    .contains("TransportClosed"),
                                                            "poisoned continuation must report transport-class session fault, got {continuation_rendered}"
                                                        );
                                                    })
                                                },
                                            )
                                        },
                                    )
                                },
                            )
                        },
                    )
                },
            );
        })
    });
}

#[test]
fn dynamic_route_passive_ignores_non_authoritative_binding_evidence() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            with_tls_mut(
                &FLOW_SHARED_SLOT,
                |ptr: *mut FlowBindingShared| unsafe { ptr.write(FlowBindingShared::new()) },
                |shared| {
                    shared.reset();
                    let shared_ref: &'static FlowBindingShared = shared;
                    let transport = FlowTransport::new(shared_ref);
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, transport.clone())
                        .expect("register rv");

                    register_route_resolvers_for_program(&cluster, rv_id, &controller_program());
                    register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                    let sid = SessionId::new(902);
                    with_tls_mut(
                        &CONTROLLER_BINDING_SLOT,
                        |ptr: *mut FlowBinding| unsafe {
                            ptr.write(FlowBinding::new(0, shared_ref))
                        },
                        |controller_binding| {
                            with_tls_mut(
                                &WORKER_BINDING_SLOT,
                                |ptr: *mut FlowBinding| unsafe {
                                    ptr.write(FlowBinding::new(1, shared_ref))
                                },
                                |worker_binding| {
                                    with_tls_mut(
                                        &CONTROLLER_ENDPOINT_SLOT,
                                        |ptr| unsafe {
                                            write_value(
                                                ptr,
                                                cluster
                                                    .rendezvous(rv_id)
                                                    .session(sid)
                                                    .role(&controller_program())
                                                    .enter(controller_binding)
                                                    .expect("attach controller"),
                                            );
                                        },
                                        |controller| {
                                            with_tls_mut(
                                                &WORKER_ENDPOINT_SLOT,
                                                |ptr| unsafe {
                                                    write_value(
                                                        ptr,
                                                        cluster
                                                            .rendezvous(rv_id)
                                                            .session(sid)
                                                            .role(&worker_program())
                                                            .enter(worker_binding)
                                                            .expect("attach worker"),
                                                    );
                                                },
                                                |worker| {
                                                    futures::executor::block_on(async move {
                                                        shared_ref.state.with_mut(|guard| {
                                                                guard.push_incoming(
                                                                    1,
                                                                    PendingInbound {
                                                                        lane: 0,
                                                                        evidence:
                                                                            IngressEvidence {
                                                                                frame_label: FrameLabel::new(FOREIGN_BINDING_FRAME),
                                                                                instance: 0,
                                                                                channel: Channel::new(999),
                                                                            },
                                                                    },
                                                                );
                                                            });

                                                        controller
                                                            .flow::<Msg<
                                                                { TEST_ROUTE_DECISION_LOGICAL },
                                                                (),
                                                                RouteDecisionKind,
                                                            >>(
                                                            )
                                                            .expect("control flow")
                                                            .send(&())
                                                            .await
                                                            .expect("send route control");

                                                        controller
                                                            .flow::<Msg<71, u32>>()
                                                            .expect("left data flow")
                                                            .send(&7777)
                                                            .await
                                                            .expect("send left data");

                                                        let worker_branch = worker
                                                            .offer()
                                                            .await
                                                            .expect("offer left arm");
                                                        assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LOGICAL_LABEL
                                                            );
                                                        let value = worker_branch
                                                            .decode::<Msg<71, u32>>()
                                                            .await
                                                            .expect("decode left data");
                                                        assert_eq!(value, 7777);

                                                        controller
                                                            .flow::<Msg<73, u32>>()
                                                            .expect("tail flow")
                                                            .send(&1)
                                                            .await
                                                            .expect("send tail");
                                                    })
                                                },
                                            )
                                        },
                                    )
                                },
                            )
                        },
                    )
                },
            );
        });
    });
}
