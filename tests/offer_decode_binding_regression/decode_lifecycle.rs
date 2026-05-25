use super::*;

#[test]
fn drop_public_preview_branch_preserves_offer_progression() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
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
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );
                        register_route_resolvers_for_program(&cluster, rv_id, &worker_program());

                        let sid = SessionId::new(903);
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
                                                                    GenericCapToken<
                                                                        RouteDecisionKind,
                                                                    >,
                                                                    RouteDecisionKind,
                                                                >>(
                                                                )
                                                                .expect("control flow")
                                                                .send(())
                                                                .await
                                                                .expect("send route control");

                                                            controller
                                                                .flow::<Msg<71, u32>>()
                                                                .expect("left data flow")
                                                                .send(&4444)
                                                                .await
                                                                .expect("send left data");

                                                            let endpoint_rx_audit_before =
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                );
                                                            let drain_calls_before = shared_ref
                                                                .state
                                                                .with(|state| state.drain_calls);

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect("offer left arm");
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LOGICAL_LABEL
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "offer preview must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "offer preview must not flush transport events for EndpointRx policy",
                                                            );
                                                            drop(worker_branch);
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "dropping preview branch must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "dropping preview branch must not flush transport events",
                                                            );

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect(
                                                                    "re-offer left arm after dropped preview",
                                                                );
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LOGICAL_LABEL
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "re-offer preview must stay policy-free until decode",
                                                            );
                                                            let data_value = worker_branch
                                                                .decode::<Msg<71, u32>>()
                                                                .await
                                                                .expect(
                                                                    "decode left data after dropped preview",
                                                                );
                                                            assert_eq!(data_value, 4444);
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before + 1,
                                                                "decode consume path must emit EndpointRx policy audit once",
                                                            );
                                                            assert!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls)
                                                                    > drain_calls_before,
                                                                "decode consume path must own transport-event flushing",
                                                            );

                                                            controller
                                                                .flow::<Msg<73, u32>>()
                                                                .expect("tail flow")
                                                                .send(&55)
                                                                .await
                                                                .expect("send tail");

                                                            let tail = worker
                                                                .recv::<Msg<73, u32>>()
                                                                .await
                                                                .expect(
                                                                    "recv tail after dropped preview branch",
                                                                );
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
            },
        );
    });
}

#[test]
fn codec_error_in_public_decode_poisons_same_generation() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
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
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, transport.clone())
                            .expect("register rv");

                        register_route_resolvers_for_program(
                            &cluster,
                            rv_id,
                            &controller_program(),
                        );
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
                                                                    GenericCapToken<
                                                                        RouteDecisionKind,
                                                                    >,
                                                                    RouteDecisionKind,
                                                                >>(
                                                                )
                                                                .expect("control flow")
                                                                .send(())
                                                                .await
                                                                .expect("send route control");

                                                            controller
                                                                .flow::<Msg<71, u32>>()
                                                                .expect("left data flow")
                                                                .send(&4444)
                                                                .await
                                                                .expect("send left data");

                                                            let endpoint_rx_audit_before =
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                );
                                                            let drain_calls_before = shared_ref
                                                                .state
                                                                .with(|state| state.drain_calls);

                                                            let worker_branch = worker
                                                                .offer()
                                                                .await
                                                                .expect("offer left arm");
                                                            assert_eq!(
                                                                worker_branch.label(),
                                                                <Msg<71, u32> as MessageSpec>::LOGICAL_LABEL
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "offer preview must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "offer preview must not flush transport events",
                                                            );
                                                            type WrongDecodeMsg = Msg<71, u64>;
                                                            let decode_line = line!() + 2;
                                                            let decode_future = worker_branch
                                                                .decode::<WrongDecodeMsg>(
                                                            );
                                                            let err = decode_future
                                                                .await
                                                                .expect_err(
                                                                    "codec mismatch must terminally fault the generation",
                                                                );
                                                            assert_eq!(err.operation(), "decode");
                                                            assert!(
                                                                err.file().ends_with(
                                                                    "tests/offer_decode_binding_regression/decode_lifecycle.rs"
                                                                )
                                                            );
                                                            assert_eq!(err.line(), decode_line);
                                                            let rendered = format!("{err:?}");
                                                            assert!(
                                                                rendered.contains("Codec"),
                                                                "expected original codec fault evidence, got {rendered}"
                                                            );
                                                            assert!(
                                                                !rendered.contains("SessionFault"),
                                                                "first decode fault must not be replaced by session poison, got {rendered}"
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "codec error must not emit EndpointRx policy audit",
                                                            );
                                                            assert_eq!(
                                                                shared_ref
                                                                    .state
                                                                    .with(|state| state.drain_calls),
                                                                drain_calls_before,
                                                                "codec error must not flush transport events for EndpointRx policy",
                                                            );

                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "codec error must not emit EndpointRx policy audit before poisoning",
                                                            );
                                                            let continuation_err = match worker
                                                                .offer()
                                                                .await
                                                            {
                                                                Ok(_) => panic!(
                                                                    "poisoned generation must not re-offer the route arm"
                                                                ),
                                                                Err(error) => error,
                                                            };
                                                            assert_eq!(
                                                                continuation_err.operation(),
                                                                "offer"
                                                            );
                                                            let continuation_rendered =
                                                                format!("{continuation_err:?}");
                                                            assert!(
                                                                continuation_rendered
                                                                    .contains("SessionFault")
                                                                    && continuation_rendered
                                                                        .contains("DecodeFailed"),
                                                                "same generation must preserve original fault evidence, got {continuation_rendered}"
                                                            );
                                                            assert_eq!(
                                                                count_policy_audit_ext_for_slot(
                                                                    tap_events(),
                                                                    SLOT_TAG_ENDPOINT_RX,
                                                                ),
                                                                endpoint_rx_audit_before,
                                                                "poisoned continuation must not emit EndpointRx policy audit",
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
            },
        );
    });
}
