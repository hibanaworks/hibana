use super::*;

#[derive(Clone, Copy)]
struct CapacitySendTransport(());

#[derive(Clone, Copy)]
enum CarrierSendFault {
    QueueFull,
}

impl CarrierSendFault {
    fn transport_error(self) -> TransportError {
        match self {
            Self::QueueFull => TransportError::Capacity,
        }
    }
}

impl Transport for CapacitySendTransport {
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
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        Poll::Ready(Err(CarrierSendFault::QueueFull.transport_error()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Pending
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
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
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(1);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let () = futures::executor::block_on(origin_endpoint.send::<Msg<1, u32>>(&42))
                .expect("send succeeds");
            let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                .expect("recv succeeds");
            assert_eq!(payload, 42u32);
            assert!(transport.queue_is_empty());
        });
    });
}

#[test]
fn direct_recv_same_label_parallel_frames_commit_by_observed_evidence() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::par(
                g::send::<1, 0, Msg<55, u32>>(),
                g::send::<2, 0, Msg<55, u32>>(),
            );
            let target_program: RoleProgram<0> = project(&program);
            let left_source_program: RoleProgram<1> = project(&program);
            let right_source_program: RoleProgram<2> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(55);
            let mut target = rv.enter(sid, &target_program).expect("target endpoint");
            let mut left_source = rv
                .enter(sid, &left_source_program)
                .expect("left source endpoint");
            let mut right_source = rv
                .enter(sid, &right_source_program)
                .expect("right source endpoint");

            futures::executor::block_on(async {
                right_source
                    .send::<Msg<55, u32>>(&202)
                    .await
                    .expect("right lane sends first");
                let first = target
                    .recv::<Msg<55, u32>>()
                    .await
                    .expect("recv commits observed right-lane frame first");
                assert_eq!(first, 202);

                left_source
                    .send::<Msg<55, u32>>(&101)
                    .await
                    .expect("left lane sends second");
                let second = target
                    .recv::<Msg<55, u32>>()
                    .await
                    .expect("recv commits remaining left-lane frame");
                assert_eq!(second, 101);
            });
            assert!(transport.queue_is_empty());
        });
    });
}

#[test]
fn rolled_same_label_recv_commits_reentry_or_exit_by_observed_evidence() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let body = g::send::<1, 0, Msg<56, u32>>().roll();
            let program = g::seq(body, g::send::<2, 0, Msg<56, u32>>());
            let target_program: RoleProgram<0> = project(&program);
            let body_source_program: RoleProgram<1> = project(&program);
            let exit_source_program: RoleProgram<2> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(56);
            let mut target = rv.enter(sid, &target_program).expect("target endpoint");
            let mut body_source = rv
                .enter(sid, &body_source_program)
                .expect("body source endpoint");
            let mut exit_source = rv
                .enter(sid, &exit_source_program)
                .expect("exit source endpoint");

            futures::executor::block_on(async {
                body_source
                    .send::<Msg<56, u32>>(&101)
                    .await
                    .expect("body sends first");
                assert_eq!(
                    target
                        .recv::<Msg<56, u32>>()
                        .await
                        .expect("recv commits body frame"),
                    101
                );

                body_source
                    .send::<Msg<56, u32>>(&202)
                    .await
                    .expect("body sends reentry");
                assert_eq!(
                    target
                        .recv::<Msg<56, u32>>()
                        .await
                        .expect("recv commits reentered body frame"),
                    202
                );

                exit_source
                    .send::<Msg<56, u32>>(&303)
                    .await
                    .expect("exit sends same logical label");
                assert_eq!(
                    target
                        .recv::<Msg<56, u32>>()
                        .await
                        .expect("recv commits exit frame"),
                    303
                );
            });
            assert!(transport.queue_is_empty());
        });
    });
}

#[test]
fn intrinsic_route_passive_same_label_recv_commits_by_frame_evidence() {
    fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
        let left = g::seq(
            g::send::<0, 2, Msg<61, u32>>(),
            g::send::<1, 3, Msg<63, u32>>(),
        );
        let right = g::seq(
            g::send::<0, 2, Msg<62, u32>>(),
            g::send::<1, 3, Msg<63, u32>>(),
        );
        project(&g::route(left, right))
    }

    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(63);
            let mut controller = rv.enter(sid, &program::<0>()).expect("controller endpoint");
            let mut shared_sender = rv
                .enter(sid, &program::<1>())
                .expect("shared sender endpoint");
            let mut observer = rv.enter(sid, &program::<2>()).expect("observer endpoint");
            let mut passive = rv.enter(sid, &program::<3>()).expect("passive endpoint");

            futures::executor::block_on(async {
                controller
                    .send::<Msg<62, u32>>(&808)
                    .await
                    .expect("controller selects right arm");
                assert_eq!(
                    observer
                        .recv::<Msg<62, u32>>()
                        .await
                        .expect("observer commits controller frame"),
                    808
                );

                shared_sender
                    .send::<Msg<63, u32>>(&909)
                    .await
                    .expect("same-label shared sender sends selected arm");
                assert_eq!(
                    passive
                        .recv::<Msg<63, u32>>()
                        .await
                        .expect("passive recv commits by observed frame label"),
                    909
                );
            });
            assert!(transport.queue_is_empty());
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
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(8);
            let mut endpoint0 = rv
                .enter(sid, &role0)
                .expect("attach role0 before lease table growth");
            let mut endpoint1 = rv
                .enter(sid, &role1)
                .expect("attach role1 and grow lease table");
            let mut endpoint2 = rv
                .enter(sid, &role2)
                .expect("attach role2 and grow lease table again");

            futures::executor::block_on(
                endpoint0.send::<Msg<8, FramePayload>>(&FramePayload(*b"r0-1")),
            )
            .expect("role0 send survives endpoint lease root relocation");
            let payload = futures::executor::block_on(endpoint1.recv::<Msg<8, FramePayload>>())
                .expect("role1 recv succeeds");
            assert_eq!(payload.as_bytes(), b"r0-1");

            futures::executor::block_on(
                endpoint2.send::<Msg<9, FramePayload>>(&FramePayload(*b"r2-0")),
            )
            .expect("role2 send succeeds");
            let payload = futures::executor::block_on(endpoint0.recv::<Msg<9, FramePayload>>())
                .expect("role0 recv survives endpoint lease root relocation");
            assert_eq!(payload.as_bytes(), b"r2-0");
            assert!(transport.queue_is_empty());
        });
    });
}

#[test]
fn live_endpoint_send_survives_failed_later_attach_rollback() {
    let mut exercised = false;
    for slab_bytes in (1024usize..=8192).step_by(128) {
        with_runtime_workspace(|slab| {
            let transport = TestTransport::new();
            with_resident_tls_ref(&SESSION_SLOT, |cluster| {
                let program = g::send::<0, 1, Msg<10, FramePayload>>();
                let role0: RoleProgram<0> = project(&program);
                let role1: RoleProgram<1> = project(&program);
                let rv = cluster
                    .rendezvous(&mut slab[..slab_bytes], transport.clone())
                    .expect("register constrained rendezvous");
                let sid = SessionId::new(10);
                let Ok(mut endpoint0) = rv.enter(sid, &role0) else {
                    return;
                };

                if rv.enter(sid, &role1).is_ok() {
                    return;
                }

                futures::executor::block_on(
                    endpoint0.send::<Msg<10, FramePayload>>(&FramePayload(*b"r0ok")),
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
                .rendezvous(slab, CapacitySendTransport(()))
                .expect("register rendezvous");

            let sid = SessionId::new(74);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let error = futures::executor::block_on(
                origin_endpoint.send::<Msg<3, FramePayload>>(&FramePayload(*b"full")),
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
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(41);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            futures::executor::block_on(origin_endpoint.send::<Msg<41, u32>>(&11))
                .expect("first send succeeds");
            futures::executor::block_on(origin_endpoint.send::<Msg<41, u32>>(&22))
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
            assert!(transport.queue_is_empty());
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
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(42);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let first = 11u32;
            let mut send_future = Box::pin(origin_endpoint.send::<Msg<42, u32>>(&first));
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
            futures::executor::block_on(origin_endpoint.send::<Msg<42, u32>>(&second))
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
            assert!(transport.queue_is_empty());
        });
    });
}

#[test]
fn send_preview_error_reports_debug_boundary() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(11);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let err = futures::executor::block_on(origin_endpoint.send::<Msg<2, u32>>(&22))
                .expect_err("send with wrong logical label must fail");
            assert!(format!("{err:?}").contains("operation: \"send\""));
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
            assert!(format!("{err:?}").contains("operation: \"offer\""));
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("PhaseInvariant")
                    || rendered.contains("ProgressInvariantViolated")
                    || rendered.contains("SessionFault"),
                "offer at a deterministic send step must fail as a progress invariant: {rendered}"
            );
            assert!(transport.queue_is_empty());
        });
    });
}
