use super::*;

#[test]
fn cursor_send_and_recv_manual_wire_control_token() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<
                Role<0>,
                Role<1>,
                Msg<
                    { MANUAL_WIRE_CONTROL_LOGICAL },
                    GenericCapToken<ManualWireControl>,
                    ManualWireControl,
                >,
                0,
            >();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(9);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(None)
                .expect("target endpoint");

            let token = manual_wire_token(sid, hibana::integration::ids::Lane::new(0), 1);

            let () = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<
                        { MANUAL_WIRE_CONTROL_LOGICAL },
                        GenericCapToken<ManualWireControl>,
                        ManualWireControl,
                    >>()
                    .expect("wire control flow")
                    .send(&token),
            )
            .expect("explicit wire control token send succeeds");

            let received = futures::executor::block_on(target_endpoint.recv::<Msg<
                { MANUAL_WIRE_CONTROL_LOGICAL },
                GenericCapToken<ManualWireControl>,
                ManualWireControl,
            >>())
            .expect("recv succeeds");

            assert_eq!(
                *received.as_view().expect("decode handle").handle(),
                (sid.raw(), 0)
            );
            assert_eq!(received.into_bytes(), token.into_bytes());
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn deterministic_recv_rejects_control_data_kind_mismatch() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<
                Role<0>,
                Role<1>,
                Msg<
                    { MANUAL_WIRE_CONTROL_LOGICAL },
                    GenericCapToken<ManualWireControl>,
                    ManualWireControl,
                >,
                0,
            >();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(91);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(None)
                .expect("target endpoint");

            let token = manual_wire_token(sid, hibana::integration::ids::Lane::new(0), 1);
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<
                        { MANUAL_WIRE_CONTROL_LOGICAL },
                        GenericCapToken<ManualWireControl>,
                        ManualWireControl,
                    >>()
                    .expect("wire control flow")
                    .send(&token),
            )
            .expect("explicit wire control token send succeeds");

            type ManualWireDataMsg = Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>;
            let recv_line = line!() + 1;
            let recv_future = target_endpoint.recv::<ManualWireDataMsg>();
            let err = match futures::executor::block_on(recv_future) {
                Ok(_) => panic!("deterministic recv must reject control as data"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "recv");
            assert!(
                err.file()
                    .ends_with("tests/cursor_send_recv/manual_wire.rs")
            );
            assert_eq!(err.line(), recv_line);
            assert_progress_invariant_fault(&err);
        });
    });

    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<
                Role<0>,
                Role<1>,
                Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>,
                0,
            >();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(92);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(None)
                .expect("target endpoint");

            let token_bytes =
                manual_wire_token(sid, hibana::integration::ids::Lane::new(0), 1).into_bytes();
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>>()
                    .expect("data flow")
                    .send(&token_bytes),
            )
            .expect("data send succeeds");

            let err = match futures::executor::block_on(target_endpoint.recv::<Msg<
                { MANUAL_WIRE_CONTROL_LOGICAL },
                GenericCapToken<ManualWireControl>,
                ManualWireControl,
            >>()) {
                Ok(_) => panic!("deterministic recv must reject data as control"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "recv");
            assert_progress_invariant_fault(&err);
        });
    });
}

#[test]
fn manual_wire_control_send_dispatches_exactly_one_abort_ack() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<
                Role<0>,
                Role<1>,
                Msg<
                    { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                    GenericCapToken<ManualWireAbortAckControl>,
                    ManualWireAbortAckControl,
                >,
                0,
            >();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (unsafe { &mut *tap_ptr }, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(10);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(None)
                .expect("target endpoint");

            let token =
                manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(0), 1, 0, 0);

            let () = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<
                        { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireAbortAckControl>,
                        ManualWireAbortAckControl,
                    >>()
                    .expect("wire abort-ack flow")
                    .send(&token),
            )
            .expect("explicit wire abort-ack send succeeds");

            let received = futures::executor::block_on(target_endpoint.recv::<Msg<
                { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                GenericCapToken<ManualWireAbortAckControl>,
                ManualWireAbortAckControl,
            >>())
            .expect("recv succeeds");
            assert_eq!(received.into_bytes(), token.into_bytes());
            assert!(transport_queue_is_empty(&transport));
        });
        let abort_ack_events = unsafe { &*tap_ptr }
            .iter()
            .filter(|event| event.id == ABORT_ACK_ID && event.arg0 == 10)
            .count();
        assert_eq!(
            abort_ack_events, 1,
            "explicit wire control send must execute exactly one abort-ack operation",
        );
    });
}

#[test]
fn manual_wire_control_send_rejects_scope_mismatch_before_transport() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<
                Role<0>,
                Role<1>,
                Msg<
                    { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                    GenericCapToken<ManualWireAbortAckControl>,
                    ManualWireAbortAckControl,
                >,
                0,
            >();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (unsafe { &mut *tap_ptr }, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(11);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            let target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(None)
                .expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let mismatched =
                manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(0), 1, 1, 0);

            let err = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<
                        { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireAbortAckControl>,
                        ManualWireAbortAckControl,
                    >>()
                    .expect("wire abort-ack flow")
                    .send(&mismatched),
            )
            .expect_err("descriptor/header mismatch must fail before transport");

            assert_eq!(err.operation(), "send");
            assert_progress_invariant_fault(&err);
            assert!(transport_queue_is_empty(&transport));
        });
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID && event.arg0 == 11),
            "rejected explicit control send must not execute abort-ack",
        );
    });
}

#[test]
fn manual_wire_control_send_rejects_session_binding_before_transport() {
    let sid = SessionId::new(12);
    let token = manual_wire_abort_ack_token(
        SessionId::new(13),
        hibana::integration::ids::Lane::new(0),
        1,
        0,
        0,
    );
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_lane_binding_before_transport() {
    let sid = SessionId::new(14);
    let token = manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(1), 1, 0, 0);
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_role_binding_before_transport() {
    let sid = SessionId::new(15);
    let token = manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(0), 0, 0, 0);
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_handle_mismatch_before_transport() {
    let sid = SessionId::new(16);
    let token = manual_wire_abort_ack_token_with_handle(
        sid,
        hibana::integration::ids::Lane::new(0),
        1,
        0,
        0,
        sid.raw(),
        1,
    );
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn localside_send_recv_sizes_stay_compact() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport,
                )
                .expect("register rendezvous");

            let sid = SessionId::new(3);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(None)
                .expect("target endpoint");

            let send = origin_endpoint
                .flow::<Msg<1, u32>>()
                .expect("send flow")
                .send(&42);
            let recv = target_endpoint.recv::<Msg<1, u32>>();

            let endpoint_bytes = size_of::<hibana::Endpoint<'static, 0>>();
            let send_future_bytes = size_of_val(&send);
            let recv_future_bytes = size_of_val(&recv);

            assert!(
                endpoint_bytes <= ENDPOINT_BYTES_MAX,
                "endpoint handle regressed: {endpoint_bytes} > {ENDPOINT_BYTES_MAX}"
            );
            assert!(
                send_future_bytes <= SEND_FUTURE_BYTES_MAX,
                "send future regressed: {send_future_bytes} > {SEND_FUTURE_BYTES_MAX}"
            );
            assert!(
                recv_future_bytes <= RECV_FUTURE_BYTES_MAX,
                "recv future regressed: {recv_future_bytes} > {RECV_FUTURE_BYTES_MAX}"
            );

            drop(send);
            drop(recv);
        });
    });
}
