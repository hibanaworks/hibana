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

type DeadlineKitStorage = SessionKitStorage<'static, DeadlineRecvTransport, CounterClock, 2>;

std::thread_local! {
    static DEADLINE_SESSION_SLOT: UnsafeCell<DeadlineKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

#[test]
fn cursor_recv_can_return_borrowed_frame_views() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let borrowed_program = g::send::<0, 1, Msg<2, FramePayload>>();
            let borrowed_origin_program: RoleProgram<0> = project(&borrowed_program);
            let borrowed_target_program: RoleProgram<1> = project(&borrowed_program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
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
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&DEADLINE_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((unsafe { &mut *tap_ptr }, slab), CounterClock::zero()),
                    DeadlineRecvTransport {
                        error: hibana::runtime::transport::TransportError::Deadline,
                    },
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
            .find(|event| event.id == hibana::runtime::tap::TRANSPORT_FAULT)
            .expect("deadline must emit transport fault evidence");
        assert_eq!(
            fault.evidence().reason(),
            hibana::runtime::tap::TRANSPORT_FAULT_DEADLINE
        );
        assert_eq!(fault.arg0, 73);
        assert_eq!(fault.arg1, 0);
        assert_eq!(
            fault.arg2,
            u32::from(hibana::runtime::tap::TRANSPORT_FAULT_DEADLINE)
        );
    });
}

#[test]
fn transport_poll_recv_returns_frame_header_with_payload() {
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
    let IngressEvidence::Framed {
        session,
        carrier,
        source,
        target,
        label,
    } = received.evidence()
    else {
        panic!("staged transport frame must carry framed ingress evidence");
    };
    assert_eq!(session, sid);
    assert_eq!(carrier, 0);
    assert_eq!(source, 0);
    assert_eq!(target, 1);
    assert_eq!(label.raw(), 6);

    assert_eq!(received.payload().as_bytes(), b"peek");
}

#[test]
fn direct_recv_session_mismatch_emits_tap_and_keeps_waiting() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, FramePayload>>();
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((unsafe { &mut *tap_ptr }, slab), CounterClock::zero()),
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

            let mut bad_tx = TestTx {
                session_id: bad_sid,
                local_role: 0,
                pending_role: None,
                pending_frame: None,
            };
            transport.stage_send_with_session(&mut bad_tx, bad_sid, 1, 0, 0, b"bad!");
            assert!(matches!(
                transport.poll_send_staged(&mut bad_tx),
                Poll::Ready(Ok(()))
            ));

            let mut recv = core::pin::pin!(target_endpoint.recv::<Msg<1, FramePayload>>());
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            if let Poll::Ready(result) = recv.as_mut().poll(&mut context) {
                match result {
                    Ok(_) => panic!("session mismatch must not commit recv"),
                    Err(error) => {
                        panic!(
                            "session mismatch must stay pending until a transport fault: {error:?}"
                        )
                    }
                }
            }

            let mismatch = unsafe { &*tap_ptr }
                .iter()
                .copied()
                .find(|event| event.id == hibana::runtime::tap::TRANSPORT_MISMATCH)
                .expect("session mismatch must emit transport mismatch evidence");
            assert_eq!(
                mismatch.evidence().reason(),
                hibana::runtime::tap::TRANSPORT_MISMATCH_SESSION
            );
            assert_eq!(mismatch.arg0, sid.raw());
            assert_eq!(mismatch.arg1, bad_sid.raw());
            assert!(
                !unsafe { &*tap_ptr }.iter().any(|event| event.id == 0x0206),
                "mismatched frames must emit TransportMismatch only, not duplicate TransportFrame"
            );

            let mut good_tx = TestTx {
                session_id: sid,
                local_role: 0,
                pending_role: None,
                pending_frame: None,
            };
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
