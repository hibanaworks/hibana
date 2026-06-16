use super::*;

#[derive(Clone, Copy)]
struct DeadlineRecvTransport {
    error: hibana::runtime::transport::TransportError,
}

struct DeadlineRx {
    session_id: SessionId,
    lane: u8,
    role: u8,
}

impl Transport for DeadlineRecvTransport {
    type Error = hibana::runtime::transport::TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = DeadlineRx
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            (),
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
        _outgoing: hibana::runtime::transport::Outgoing<'f>,
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
        context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        core::hint::black_box(context.waker());
        core::hint::black_box((rx.session_id.raw(), rx.lane, rx.role));
        Poll::Ready(Err(self.error))
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        core::hint::black_box(tx);
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        core::hint::black_box(rx);
        Ok(())
    }
}

type DeadlineKitStorage = SessionKitStorage<'static, DeadlineRecvTransport, 2>;

#[derive(Clone, Copy)]
struct MalformedRecvTransport {
    header: hibana::runtime::transport::FrameHeader,
}

struct MalformedRx {
    delivered: bool,
}

impl Transport for MalformedRecvTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = MalformedRx
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        core::hint::black_box(port);
        ((), MalformedRx { delivered: false })
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: hibana::runtime::transport::Outgoing<'f>,
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
        context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        core::hint::black_box(context.waker());
        if rx.delivered {
            return Poll::Pending;
        }
        rx.delivered = true;
        Poll::Ready(Ok(ReceivedFrame::framed(
            self.header,
            Payload::new(b"bad!"),
        )))
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        core::hint::black_box(tx);
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        core::hint::black_box(rx);
        Ok(())
    }
}

type MalformedKitStorage = SessionKitStorage<'static, MalformedRecvTransport, 2>;

std::thread_local! {
    static DEADLINE_SESSION_SLOT: UnsafeCell<DeadlineKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static MALFORMED_SESSION_SLOT: UnsafeCell<MalformedKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn malformed_header(
    session_id: SessionId,
    lane: u8,
    source_role: u8,
    target_role: u8,
    frame_label: u8,
) -> hibana::runtime::transport::FrameHeader {
    let session = session_id.raw().to_be_bytes();
    hibana::runtime::transport::FrameHeader::from_bytes([
        session[0],
        session[1],
        session[2],
        session[3],
        lane,
        source_role,
        target_role,
        frame_label,
    ])
}

fn assert_malformed_direct_recv_fails_closed(
    sid: SessionId,
    header: hibana::runtime::transport::FrameHeader,
    expected_reason: u8,
) {
    with_runtime_workspace(|slab| {
        let mismatch = with_resident_tls_ref(&MALFORMED_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources(slab),
                    MalformedRecvTransport { header },
                )
                .expect("register rendezvous");
            let mut tap = rv.tap();

            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let error = futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                .expect_err("descriptor mismatch must fail closed");
            let rendered = format!("{error:?}");
            assert!(
                rendered.contains("PhaseInvariant"),
                "descriptor mismatch must report terminal invariant evidence: {rendered}"
            );

            let mismatch = tap
                .find(|event| event.id() == hibana::runtime::tap::TRANSPORT_MISMATCH)
                .expect("descriptor mismatch must emit transport mismatch evidence");
            assert_eq!(mismatch.evidence().reason(), expected_reason);
            assert_eq!(mismatch.arg0(), sid.raw());
            assert!(
                !tap.any(|event| event.id() == hibana::runtime::tap::TRANSPORT_FRAME),
                "rejected mismatch must not also emit accepted frame evidence"
            );

            let poisoned =
                futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                    .expect_err("same generation must be poisoned after mismatch");
            let rendered = format!("{poisoned:?}");
            assert!(
                rendered.contains("SessionFault") && rendered.contains("ProgressInvariantViolated"),
                "mismatch must poison same generation: {rendered}"
            );
            mismatch
        });
        assert_eq!(mismatch.evidence().reason(), expected_reason);
    });
}

#[test]
fn cursor_recv_can_return_borrowed_frame_views() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let borrowed_program = g::send::<0, 1, Msg<2, FramePayload>>();
            let borrowed_origin_program: RoleProgram<0> = project(&borrowed_program);
            let borrowed_target_program: RoleProgram<1> = project(&borrowed_program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport.clone())
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
    with_runtime_workspace(|slab| {
        let fault = with_resident_tls_ref(&DEADLINE_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources(slab),
                    DeadlineRecvTransport {
                        error: hibana::runtime::transport::TransportError::Deadline,
                    },
                )
                .expect("register rendezvous");
            let mut tap = rv.tap();

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
            tap.find(|event| event.id() == hibana::runtime::tap::TRANSPORT_FAULT)
                .expect("deadline must emit transport fault evidence")
        });
        assert_eq!(
            fault.evidence().reason(),
            hibana::runtime::tap::TRANSPORT_FAULT_DEADLINE
        );
        assert_eq!(fault.arg0(), 73);
        assert_eq!(fault.arg1(), 0);
    });
}

#[test]
fn transport_poll_recv_returns_framed_payload() {
    let transport = TestTransport::new();
    let sid = SessionId::new(94);
    let mut tx = TestTx {
        session_id: sid,
        local_role: 0,
        pending_role: None,
        pending_frame: None,
    };
    transport.stage_send_with_session(&mut tx, sid, 1, 0, 6, b"peek");
    assert!(matches!(
        transport.poll_send_staged(&mut tx),
        Poll::Ready(Ok(()))
    ));

    let mut rx = transport.open_rx(1, 0);
    let waker = futures::task::noop_waker_ref();
    let mut context = Context::from_waker(waker);
    let received = match Transport::poll_recv(&transport, &mut rx, &mut context) {
        Poll::Ready(Ok(received)) => received,
        Poll::Ready(Err(err)) => panic!("staged frame must poll successfully: {err:?}"),
        Poll::Pending => panic!("staged frame must be ready"),
    };
    assert_eq!(received.payload().as_bytes(), b"peek");
}

#[test]
fn direct_recv_session_mismatch_fails_closed_and_poisons_generation() {
    let sid = SessionId::new(71);
    let bad_sid = SessionId::new(72);
    assert_malformed_direct_recv_fails_closed(
        sid,
        malformed_header(bad_sid, 0, 0, 1, 0),
        hibana::runtime::tap::TRANSPORT_MISMATCH_SESSION,
    );
}

#[test]
fn direct_recv_lane_source_target_label_mismatch_fails_closed() {
    let cases = [
        (
            SessionId::new(81),
            malformed_header(SessionId::new(81), 1, 0, 1, 0),
            hibana::runtime::tap::TRANSPORT_MISMATCH_LANE,
        ),
        (
            SessionId::new(82),
            malformed_header(SessionId::new(82), 0, 2, 1, 0),
            hibana::runtime::tap::TRANSPORT_MISMATCH_SOURCE_ROLE,
        ),
        (
            SessionId::new(83),
            malformed_header(SessionId::new(83), 0, 0, 2, 0),
            hibana::runtime::tap::TRANSPORT_MISMATCH_PEER_ROLE,
        ),
        (
            SessionId::new(84),
            malformed_header(SessionId::new(84), 0, 0, 1, 9),
            hibana::runtime::tap::TRANSPORT_MISMATCH_LABEL,
        ),
    ];
    for (sid, header, reason) in cases {
        assert_malformed_direct_recv_fails_closed(sid, header, reason);
    }
}

#[test]
fn mismatch_tap_is_emitted_before_terminal_error() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&MALFORMED_SESSION_SLOT, |cluster| {
            let sid = SessionId::new(85);
            let program = g::send::<0, 1, Msg<1, FramePayload>>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources(slab),
                    MalformedRecvTransport {
                        header: malformed_header(sid, 0, 0, 1, 9),
                    },
                )
                .expect("register rendezvous");
            let mut tap = rv.tap();
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let mut recv = core::pin::pin!(target_endpoint.recv::<Msg<1, FramePayload>>());
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match recv.as_mut().poll(&mut context) {
                Poll::Ready(Err(error)) => {
                    let rendered = format!("{error:?}");
                    assert!(
                        rendered.contains("PhaseInvariant"),
                        "mismatch must be terminal: {rendered}"
                    );
                }
                Poll::Ready(Ok(_)) => panic!("mismatch must not commit recv"),
                Poll::Pending => panic!("mismatch must not remain pending"),
            }

            tap.find(|event| event.id() == hibana::runtime::tap::TRANSPORT_MISMATCH)
                .expect("mismatch tap must be emitted by the failing poll");
        });
    });
}
