use super::*;

#[derive(Clone, Copy, Default)]
struct DeadlineRecvTransport;

struct DeadlineTx;

struct DeadlineRx {
    session_id: SessionId,
    lane: hibana::integration::ids::Lane,
    role: u8,
}

impl Transport for DeadlineRecvTransport {
    type Error = hibana::integration::transport::TransportError;
    type Tx<'a>
        = DeadlineTx
    where
        Self: 'a;
    type Rx<'a>
        = DeadlineRx
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            DeadlineTx,
            DeadlineRx {
                session_id: port.session_id(),
                lane: port.lane(),
                role: port.local_role(),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: hibana::integration::transport::Outgoing<'f>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedPayload<'a>, Self::Error>> {
        core::hint::black_box((rx.session_id.raw(), rx.lane.as_wire(), rx.role));
        Poll::Ready(Err(
            hibana::integration::transport::TransportError::Deadline,
        ))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        Ok(())
    }
}

type DeadlineKitStorage = SessionKitStorage<
    'static,
    DeadlineRecvTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

std::thread_local! {
    static DEADLINE_SESSION_SLOT: UnsafeCell<DeadlineKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

#[test]
fn cursor_recv_can_return_borrowed_frame_views() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let borrowed_program = g::send::<0, 1, Msg<2, FramePayload>, 0>();
            let borrowed_origin_program: RoleProgram<0> = project(&borrowed_program);
            let borrowed_target_program: RoleProgram<1> = project(&borrowed_program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(2);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&borrowed_origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&borrowed_target_program)
                .enter()
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
fn direct_recv_deadline_emits_transport_fault_tap() {
    with_fixture(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&DEADLINE_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>, 0>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (unsafe { &mut *tap_ptr }, slab),
                        CounterClock::new(),
                    ),
                    DeadlineRecvTransport,
                )
                .expect("register rendezvous");

            let sid = SessionId::new(73);
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");
            let error = futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                .expect_err("deadline recv must surface transport error");
            let rendered = format!("{error:?}");
            assert!(
                rendered.contains("Deadline") || rendered.contains("Transport(D)"),
                "recv error must preserve deadline transport cause: {rendered}"
            );
        });

        let fault = unsafe { &*tap_ptr }
            .iter()
            .copied()
            .find(|event| event.id == hibana::integration::tap::TRANSPORT_FAULT)
            .expect("deadline must emit transport fault evidence");
        assert_eq!(
            fault.evidence().reason(),
            hibana::integration::tap::TRANSPORT_FAULT_DEADLINE
        );
        assert_eq!(fault.arg0, 73);
    });
}

#[test]
fn transport_poll_recv_returns_frame_header_with_payload() {
    let transport = TestTransport::default();
    let mut tx = TestTx::default();
    let sid = SessionId::new(94);
    transport.stage_send_with_session(&mut tx, sid, 1, 0, 6, b"peek");
    assert!(matches!(
        transport.poll_send_staged(&mut tx),
        Poll::Ready(Ok(()))
    ));

    let mut rx = transport.open_rx_for_test(1, 0);
    let waker = futures::task::noop_waker_ref();
    let mut context = Context::from_waker(waker);
    let received = match Transport::poll_recv(&transport, &mut rx, &mut context) {
        Poll::Ready(Ok(received)) => received,
        Poll::Ready(Err(err)) => panic!("staged frame must poll successfully: {err:?}"),
        Poll::Pending => panic!("staged frame must be ready"),
    };
    let header = received.header().expect("staged frame header");
    assert_eq!(header.session(), sid);
    assert_eq!(header.lane().as_wire(), 0);
    assert_eq!(header.source_role(), 0);
    assert_eq!(header.peer_role(), 1);
    assert_eq!(header.label().raw(), 6);

    assert_eq!(received.payload().as_bytes(), b"peek");
}

#[test]
fn direct_recv_session_mismatch_emits_tap_and_keeps_waiting() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>, 0>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (unsafe { &mut *tap_ptr }, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(71);
            let bad_sid = SessionId::new(72);
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let mut bad_tx = TestTx::default();
            transport.stage_send_with_session(&mut bad_tx, bad_sid, 1, 0, 0, b"bad!");
            assert!(matches!(
                transport.poll_send_staged(&mut bad_tx),
                Poll::Ready(Ok(()))
            ));

            let mut recv = core::pin::pin!(target_endpoint.recv::<Msg<1, FramePayload>>());
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match recv.as_mut().poll(&mut context) {
                Poll::Pending => {}
                Poll::Ready(Ok(_)) => panic!("session mismatch must not commit recv"),
                Poll::Ready(Err(error)) => {
                    panic!("session mismatch must stay pending until a transport fault: {error:?}")
                }
            }

            let mismatch = unsafe { &*tap_ptr }
                .iter()
                .copied()
                .find(|event| event.id == hibana::integration::tap::TRANSPORT_MISMATCH)
                .expect("session mismatch must emit transport mismatch evidence");
            assert_eq!(
                mismatch.evidence().reason(),
                hibana::integration::tap::TRANSPORT_MISMATCH_SESSION
            );
            assert_eq!(mismatch.arg0, sid.raw());
            assert_eq!(mismatch.arg1, bad_sid.raw());
            assert!(
                !unsafe { &*tap_ptr }.iter().any(|event| event.id == 0x0206),
                "mismatched frames must emit TransportMismatch only, not duplicate TransportFrame"
            );

            let mut good_tx = TestTx::default();
            transport.stage_send_with_session(&mut good_tx, sid, 1, 0, 0, b"good");
            assert!(matches!(
                transport.poll_send_staged(&mut good_tx),
                Poll::Ready(Ok(()))
            ));

            match recv.as_mut().poll(&mut context) {
                Poll::Ready(Ok(payload)) => assert_eq!(payload.as_bytes(), b"good"),
                Poll::Ready(Err(error)) => panic!("matching frame must complete recv: {error:?}"),
                Poll::Pending => {
                    panic!("matching frame should be available after mismatch discard")
                }
            }
        });
    });
}

#[test]
fn direct_recv_requeues_transport_payload_when_binding_wins_after_poll_recv() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(20);
            let origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .binding(&mut *binding)
                .enter()
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
                Poll::Ready(Ok(received)) => assert_eq!(received.payload().as_bytes(), b"wire"),
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
            let program = g::send::<0, 1, Msg<1, FramePayload>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(22);
            let origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .binding(&mut *binding)
                .enter()
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
            let program = g::send::<0, 1, Msg<1, u64>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(21);
            let origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .binding(&mut *binding)
                .enter()
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
