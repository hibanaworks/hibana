use super::*;

#[test]
fn cursor_recv_can_return_borrowed_frame_views() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let borrowed_program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
            let borrowed_origin_program: RoleProgram<0> = project(&borrowed_program);
            let borrowed_target_program: RoleProgram<1> = project(&borrowed_program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(2);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&borrowed_origin_program)
                .enter(None)
                .expect("origin endpoint");
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&borrowed_target_program)
                .enter(None)
                .expect("target endpoint");

            let () = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<2, FramePayload>>()
                    .expect("send flow")
                    .send(&FramePayload(*b"hiba")),
            )
            .expect("send succeeds");
            let payload =
                futures::executor::block_on(target_endpoint.recv::<Msg<2, FramePayload>>())
                    .expect("recv succeeds");
            assert_eq!(payload.as_bytes(), b"hiba");
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn direct_recv_requeues_transport_payload_when_binding_wins_after_poll_recv() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<1, FramePayload>, 0>();
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

            let sid = SessionId::new(20);
            let origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(Some(&mut *binding))
                .expect("target endpoint");

            let mut tx = TestTx::default();
            transport.stage_send(&mut tx, 1, 0, 0, b"wire");
            assert!(matches!(
                transport.poll_send_staged(&mut tx),
                Poll::Ready(Ok(()))
            ));

            {
                let payload =
                    futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                        .expect("binding-backed recv succeeds");
                assert_eq!(
                    payload.as_bytes(),
                    b"bind",
                    "binding payload must be the committed recv source"
                );
            }
            drop(target_endpoint);
            assert_eq!(
                binding.poll_count(),
                2,
                "fixture must poll binding once before and once after transport recv"
            );
            assert_eq!(
                binding.last_recv_channel(),
                Some(Channel::new(11)),
                "recv must read from the late binding evidence"
            );
            assert!(
                !transport_queue_is_empty(&transport),
                "transport payload polled before binding won must be requeued"
            );

            let mut rx = transport.open_rx_for_test(1, 0);
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match transport.poll_recv_current(&mut rx, &mut context) {
                Poll::Ready(Ok(payload)) => assert_eq!(payload.as_bytes(), b"wire"),
                Poll::Ready(Err(err)) => panic!("requeued payload read failed: {err:?}"),
                Poll::Pending => panic!("requeued payload was not available"),
            }
        });
    });
}

#[test]
fn direct_recv_late_binding_requeues_staged_transport_payload() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = AuditOrderTransport::default();
        with_resident_tls_ref(&AUDIT_ORDER_SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<1, FramePayload>, 0>();
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

            let sid = SessionId::new(22);
            let origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(Some(&mut *binding))
                .expect("target endpoint");

            let mut tx = TestTx::default();
            transport.stage_send(&mut tx, 1, 0, 0, b"wire");
            assert!(matches!(
                transport.poll_send_staged(&mut tx),
                Poll::Ready(Ok(()))
            ));

            transport.reset_requeue_check();
            let payload =
                futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                    .expect("binding-backed recv succeeds");
            assert_eq!(payload.as_bytes(), b"bind");
            assert!(
                transport.requeued(),
                "late binding must requeue the staged transport payload before returning binding bytes"
            );
        });
    });
}

#[test]
fn direct_recv_does_not_requeue_transport_payload_when_late_binding_payload_fails_validation() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<1, u64>, 0>();
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

            let sid = SessionId::new(21);
            let origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(None)
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
            let mut target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(Some(&mut *binding))
                .expect("target endpoint");

            let mut tx = TestTx::default();
            transport.stage_send(&mut tx, 1, 0, 0, b"wire");
            assert!(matches!(
                transport.poll_send_staged(&mut tx),
                Poll::Ready(Ok(()))
            ));

            let err = futures::executor::block_on(target_endpoint.recv::<Msg<1, u64>>())
                .expect_err("short binding payload must fail validation");
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("Codec"),
                "first recv failure must preserve codec evidence: {rendered}"
            );
            assert!(
                !rendered.contains("SessionFault"),
                "first recv failure must not be replaced by session poison: {rendered}"
            );
            drop(target_endpoint);
            assert_eq!(
                binding.poll_count(),
                2,
                "fixture must poll binding once before and once after transport recv"
            );
            assert_eq!(
                binding.last_recv_channel(),
                Some(Channel::new(11)),
                "recv must read from the late binding evidence"
            );
            assert!(
                transport_queue_is_empty(&transport),
                "transport payload must not be requeued before a binding-backed recv commits"
            );
        });
    });
}
