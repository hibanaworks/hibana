use super::*;

#[test]
fn flow_preview_is_policy_free_until_send_consumes_it() {
    with_fixture(|_clock, tap_buf, slab| {
        reset_route_resolver_calls();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let tap_ptr = tap_buf.as_ptr();
            let tap_len = tap_buf.len();
            let tap_events = || unsafe { core::slice::from_raw_parts(tap_ptr, tap_len) };
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
                    let rv = cluster
                        .rendezvous(config, transport.clone())
                        .expect("register rv");

                    register_route_resolvers_for_program(&rv, &controller_program());

                    let sid = SessionId::new(900);
                    with_tls_mut(
                        &CONTROLLER_BINDING_SLOT,
                        |ptr: *mut FlowBinding| unsafe {
                            ptr.write(FlowBinding::new(0, shared_ref))
                        },
                        |controller_binding| {
                            with_tls_mut(
                                &CONTROLLER_ENDPOINT_SLOT,
                                |ptr| unsafe {
                                    write_value(
                                        ptr,
                                        rv.session(sid)
                                            .role(&controller_program())
                                            .enter_with_binding(controller_binding)
                                            .expect("attach controller"),
                                    );
                                },
                                |controller| {
                                    let decision_policy_calls_before = route_resolver_calls();
                                    let route_audit_before = count_policy_audit_ext_for_slot(
                                        tap_events(),
                                        SLOT_TAG_ROUTE,
                                    );

                                    let flow =
                                        controller
                                            .flow::<Msg<
                                                { TEST_ROUTE_DECISION_LOGICAL },
                                                (),
                                                RouteDecisionKind,
                                            >>()
                                            .expect("route control preview");
                                    drop(flow);

                                    assert_eq!(
                                        route_resolver_calls(),
                                        decision_policy_calls_before,
                                        "dropping flow preview must not invoke decision resolver",
                                    );
                                    assert_eq!(
                                        count_policy_audit_ext_for_slot(
                                            tap_events(),
                                            SLOT_TAG_ROUTE,
                                        ),
                                        route_audit_before,
                                        "dropping flow preview must not emit route-slot policy audit",
                                    );

                                    futures::executor::block_on(async {
                                        controller
                                            .flow::<Msg<
                                                { TEST_ROUTE_DECISION_LOGICAL },
                                                (),
                                                RouteDecisionKind,
                                            >>()
                                            .expect("route control preview for send")
                                            .send(&())
                                            .await
                                            .expect("send route control");
                                    });
                                },
                            )
                        },
                    )
                },
            );
        });
    });
}

#[test]
fn offer_decode_binding_consumes_evidence_once() {
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
                    let rv = cluster
                        .rendezvous(config, transport.clone())
                        .expect("register rv");

                    register_route_resolvers_for_program(&rv, &controller_program());
                    register_route_resolvers_for_program(&rv, &worker_program());

                    let sid = SessionId::new(901);
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
                                                rv.session(sid)
                                                    .role(&controller_program())
                                                    .enter_with_binding(controller_binding)
                                                    .expect("attach controller"),
                                            );
                                        },
                                        |controller| {
                                            with_tls_mut(
                                                &WORKER_ENDPOINT_SLOT,
                                                |ptr| unsafe {
                                                    write_value(
                                                        ptr,
                                                        rv.session(sid)
                                                            .role(&worker_program())
                                                            .enter_with_binding(worker_binding)
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
                                                            .send(&4444)
                                                            .await
                                                            .expect("send left data");

                                                        let worker_branch = worker
                                                            .offer()
                                                            .await
                                                            .expect("offer left arm");
                                                        assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as Message>::LOGICAL_LABEL
                                                            );
                                                        let data_value = worker_branch
                                                            .decode::<Msg<71, u32>>()
                                                            .await
                                                            .expect("decode left data");
                                                        assert_eq!(data_value, 4444);

                                                        controller
                                                            .flow::<Msg<73, u32>>()
                                                            .expect("tail flow")
                                                            .send(&55)
                                                            .await
                                                            .expect("send tail");

                                                        let tail = worker
                                                            .recv::<Msg<73, u32>>()
                                                            .await
                                                            .expect("recv tail after offer/decode");
                                                        assert_eq!(tail, 55);
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
