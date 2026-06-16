use super::*;

#[derive(Clone, Copy)]
struct CapacitySendTransport(());

impl Transport for CapacitySendTransport {
    type Error = hibana::runtime::transport::TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        _port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
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
        Poll::Ready(Err(hibana::runtime::transport::TransportError::Capacity))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        Poll::Pending
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        Ok(())
    }
}

type CapacitySendKitStorage = SessionKitStorage<'static, CapacitySendTransport>;

std::thread_local! {
    static CAPACITY_SEND_SESSION_SLOT: UnsafeCell<CapacitySendKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

#[test]
fn cursor_send_and_recv_roundtrip() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(1);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let () = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<1, u32>>()
                    .expect("send flow")
                    .send(&42),
            )
            .expect("send succeeds");
            let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                .expect("recv succeeds");
            assert_eq!(payload, 42u32);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn live_endpoint_send_recv_survives_endpoint_lease_table_growth() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<8, FramePayload>>(),
                g::send::<2, 0, Msg<9, FramePayload>>(),
            );
            let role0: RoleProgram<0> = project(&program);
            let role1: RoleProgram<1> = project(&program);
            let role2: RoleProgram<2> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(8);
            let mut endpoint0 = rv
                .session(sid)
                .role(&role0)
                .enter()
                .expect("attach role0 before lease table growth");
            let mut endpoint1 = rv
                .session(sid)
                .role(&role1)
                .enter()
                .expect("attach role1 and grow lease table");
            let mut endpoint2 = rv
                .session(sid)
                .role(&role2)
                .enter()
                .expect("attach role2 and grow lease table again");

            futures::executor::block_on(
                endpoint0
                    .flow::<Msg<8, FramePayload>>()
                    .expect("role0 send flow")
                    .send(&FramePayload(*b"r0-1")),
            )
            .expect("role0 send survives endpoint lease root relocation");
            let payload = futures::executor::block_on(endpoint1.recv::<Msg<8, FramePayload>>())
                .expect("role1 recv succeeds");
            assert_eq!(payload.as_bytes(), b"r0-1");

            futures::executor::block_on(
                endpoint2
                    .flow::<Msg<9, FramePayload>>()
                    .expect("role2 send flow")
                    .send(&FramePayload(*b"r2-0")),
            )
            .expect("role2 send succeeds");
            let payload = futures::executor::block_on(endpoint0.recv::<Msg<9, FramePayload>>())
                .expect("role0 recv survives endpoint lease root relocation");
            assert_eq!(payload.as_bytes(), b"r2-0");
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn live_endpoint_send_survives_failed_later_attach_rollback() {
    let mut exercised = false;
    for slab_bytes in [4608usize, 5120, 5632, 6144, 6656, 7168, 7680, 8192] {
        with_runtime_workspace(|slab| {
            let transport = TestTransport::new();
            with_resident_tls_ref(&SESSION_SLOT, |cluster| {
                let program = g::send::<0, 1, Msg<10, FramePayload>>();
                let role0: RoleProgram<0> = project(&program);
                let role1: RoleProgram<1> = project(&program);
                let rv = cluster
                    .rendezvous(
                        Config::from_resources(&mut slab[..slab_bytes]),
                        transport.clone(),
                    )
                    .expect("register constrained rendezvous");
                let sid = SessionId::new(10);
                let Ok(mut endpoint0) = rv.session(sid).role(&role0).enter() else {
                    return;
                };

                if rv.session(sid).role(&role1).enter().is_ok() {
                    return;
                }

                futures::executor::block_on(
                    endpoint0
                        .flow::<Msg<10, FramePayload>>()
                        .expect("role0 flow after failed attach")
                        .send(&FramePayload(*b"r0ok")),
                )
                .expect("existing endpoint send survives failed later attach rollback");
                exercised = true;
            });
        });
        if exercised {
            break;
        }
    }
    assert!(
        exercised,
        "test must find a constrained slab where role0 attaches and role1 attach rolls back"
    );
}

#[test]
fn direct_send_capacity_emits_transport_fault_tap() {
    with_runtime_workspace(|slab| {
        let fault = with_resident_tls_ref(&CAPACITY_SEND_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<3, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), CapacitySendTransport(()))
                .expect("register rendezvous");

            let sid = SessionId::new(74);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let error = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<3, FramePayload>>()
                    .expect("send flow")
                    .send(&FramePayload(*b"full")),
            )
            .expect_err("capacity send must surface transport error");
            let rendered = format!("{error:?}");
            assert!(
                rendered.contains("Capacity") || rendered.contains("Transport(C)"),
                "send error must preserve capacity transport cause: {rendered}"
            );
            let mut tap = rv.tap();
            tap.find(|event| event.id() == hibana::runtime::tap::TRANSPORT_FAULT)
                .expect("new tap port after failure must read retained transport fault evidence")
        });
        assert_eq!(
            fault.evidence().reason(),
            hibana::runtime::tap::TRANSPORT_FAULT_CAPACITY
        );
        assert_eq!(fault.arg0(), 74);
        assert_eq!(fault.arg1(), 0);
    });
}

#[test]
fn completed_recv_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<41, u32>>(),
                g::send::<0, 1, Msg<41, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(41);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<41, u32>>()
                    .expect("first send flow")
                    .send(&11),
            )
            .expect("first send succeeds");
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<41, u32>>()
                    .expect("second send flow")
                    .send(&22),
            )
            .expect("second send succeeds");

            let mut recv_future = Box::pin(target_endpoint.recv::<Msg<41, u32>>());
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match Future::poll(recv_future.as_mut(), &mut context) {
                Poll::Ready(Ok(value)) => assert_eq!(value, 11),
                Poll::Ready(Err(error)) => panic!("first recv failed: {error:?}"),
                Poll::Pending => panic!("first recv must be ready"),
            }

            let repoll = catch_unwind(AssertUnwindSafe(|| {
                let _ = Future::poll(recv_future.as_mut(), &mut context);
            }));
            assert!(
                repoll.is_err(),
                "completed recv future must fail fast on post-Ready poll"
            );
            drop(recv_future);

            let second = futures::executor::block_on(target_endpoint.recv::<Msg<41, u32>>())
                .expect("second recv remains available");
            assert_eq!(
                second, 22,
                "completed recv future repoll must not consume the next descriptor"
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn completed_send_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<42, u32>>(),
                g::send::<0, 1, Msg<42, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(42);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let first = 11u32;
            let mut send_future = Box::pin(
                origin_endpoint
                    .flow::<Msg<42, u32>>()
                    .expect("first send flow")
                    .send(&first),
            );
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            let first_poll = Future::poll(send_future.as_mut(), &mut context);
            assert!(
                matches!(first_poll, Poll::Ready(Ok(()))),
                "first send must be ready: {first_poll:?}"
            );

            let repoll = catch_unwind(AssertUnwindSafe(|| {
                let _ = Future::poll(send_future.as_mut(), &mut context);
            }));
            assert!(
                repoll.is_err(),
                "completed send future must fail fast on post-Ready poll"
            );
            drop(send_future);

            let second = 22u32;
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<42, u32>>()
                    .expect("second send flow")
                    .send(&second),
            )
            .expect("second send succeeds");

            let first_recv = futures::executor::block_on(target_endpoint.recv::<Msg<42, u32>>())
                .expect("first recv remains available");
            let second_recv = futures::executor::block_on(target_endpoint.recv::<Msg<42, u32>>())
                .expect("second recv remains available");
            assert_eq!(first_recv, 11);
            assert_eq!(
                second_recv, 22,
                "completed send future repoll must not consume the next descriptor"
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn flow_error_reports_public_operation() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(11);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let err = match origin_endpoint.flow::<Msg<2, u32>>() {
                Ok(_) => panic!("flow with wrong logical label must fail"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "flow");
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("LabelMismatch"),
                "failed send preview must report the preview mismatch: {rendered}"
            );
            assert!(
                !rendered.contains("SessionFault"),
                "failed send preview must not poison before send consumes progress: {rendered}"
            );

            let err = match futures::executor::block_on(origin_endpoint.offer()) {
                Ok(_) => panic!("offer at deterministic send step must fail"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "offer");
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("PhaseInvariant")
                    || rendered.contains("ProgressInvariantViolated")
                    || rendered.contains("SessionFault"),
                "offer at a deterministic send step must fail as a progress invariant: {rendered}"
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}
