use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
struct CapacitySendTransport;

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

    fn poll_send<'f>(
        &self,
        _tx: &mut Self::Tx<'_>,
        _outgoing: hibana::runtime::transport::Outgoing<'f>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        Poll::Ready(Err(TransportError::Capacity))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'_>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        Poll::Pending
    }

    fn cancel_send(&self, _tx: &mut Self::Tx<'_>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        Ok(())
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn cursor_send_and_recv_roundtrip() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(1);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let () = (origin_endpoint.send::<Msg<1, u32>>(&42))
                .await
                .expect("send succeeds");
            let payload = (target_endpoint.recv::<Msg<1, u32>>())
                .await
                .expect("recv succeeds");
            assert_eq!(payload, 42u32);
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn session_template_instances_interleave_and_fault_independently() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::seq(
                g::send::<0, 1, Msg<90, u32>>(),
                g::send::<0, 1, Msg<91, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let left_sid = SessionId::new(0x5100);
            let right_sid = SessionId::new(0x5101);
            let mut left_origin = rv
                .enter(left_sid, &origin_program)
                .expect("left origin endpoint");
            let mut left_target = rv
                .enter(left_sid, &target_program)
                .expect("left target endpoint");
            let mut right_origin = rv
                .enter(right_sid, &origin_program)
                .expect("right origin endpoint");
            let mut right_target = rv
                .enter(right_sid, &target_program)
                .expect("right target endpoint");

            (async {
                left_origin
                    .send::<Msg<90, u32>>(&11)
                    .await
                    .expect("left first send");
                right_origin
                    .send::<Msg<90, u32>>(&22)
                    .await
                    .expect("right first send");
                assert_eq!(
                    right_target
                        .recv::<Msg<90, u32>>()
                        .await
                        .expect("right first receive"),
                    22
                );
                assert_eq!(
                    left_target
                        .recv::<Msg<90, u32>>()
                        .await
                        .expect("left first receive"),
                    11
                );

                drop(left_target);

                right_origin
                    .send::<Msg<91, u32>>(&33)
                    .await
                    .expect("other session survives peer drop");
                assert_eq!(
                    right_target
                        .recv::<Msg<91, u32>>()
                        .await
                        .expect("other session completes"),
                    33
                );

                let fault = left_origin
                    .send::<Msg<91, u32>>(&44)
                    .await
                    .expect_err("dropped peer terminates only its session");
                let rendered = format!("{fault:?}");
                assert!(
                    rendered.contains("EndpointDropped"),
                    "first session retains exact fault: {rendered}"
                );
            })
            .await;
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn same_lane_session_waiters_are_isolated() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<92, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let left_sid = SessionId::new(0x5200);
            let right_sid = SessionId::new(0x5201);
            let mut left_origin = rv
                .enter(left_sid, &origin_program)
                .expect("left origin endpoint");
            let mut left_target = rv
                .enter(left_sid, &target_program)
                .expect("left target endpoint");
            let mut right_origin = rv
                .enter(right_sid, &origin_program)
                .expect("right origin endpoint");
            let mut right_target = rv
                .enter(right_sid, &target_program)
                .expect("right target endpoint");

            let left_counter = Arc::new(WakeCount::default());
            let right_counter = Arc::new(WakeCount::default());
            let left_waker = Waker::from(left_counter.clone());
            let right_waker = Waker::from(right_counter.clone());
            let mut left_context = Context::from_waker(&left_waker);
            let mut right_context = Context::from_waker(&right_waker);
            let mut left_recv = Box::pin(left_target.recv::<Msg<92, u32>>());
            let mut right_recv = Box::pin(right_target.recv::<Msg<92, u32>>());

            assert!(matches!(
                Future::poll(left_recv.as_mut(), &mut left_context),
                Poll::Pending
            ));
            assert!(matches!(
                Future::poll(right_recv.as_mut(), &mut right_context),
                Poll::Pending
            ));

            (left_origin.send::<Msg<92, u32>>(&17))
                .await
                .expect("left send succeeds");
            assert!(
                left_counter.0.load(Ordering::SeqCst) > 0,
                "left send must wake left waiter"
            );
            assert_eq!(
                right_counter.0.load(Ordering::SeqCst),
                0,
                "left send must not wake or replace right-session authority"
            );
            assert!(matches!(
                Future::poll(left_recv.as_mut(), &mut left_context),
                Poll::Ready(Ok(17))
            ));

            (right_origin.send::<Msg<92, u32>>(&19))
                .await
                .expect("right send succeeds");
            assert!(
                right_counter.0.load(Ordering::SeqCst) > 0,
                "right waiter must survive the left-session delivery"
            );
            assert!(matches!(
                Future::poll(right_recv.as_mut(), &mut right_context),
                Poll::Ready(Ok(19))
            ));
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn direct_recv_same_label_parallel_frames_commit_by_observed_evidence() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::par(
                g::send::<1, 0, Msg<55, u32>>(),
                g::send::<2, 0, Msg<55, u32>>(),
            );
            let target_program: RoleProgram<0> = project(&program);
            let left_source_program: RoleProgram<1> = project(&program);
            let right_source_program: RoleProgram<2> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(55);
            let mut target = rv.enter(sid, &target_program).expect("target endpoint");
            let mut left_source = rv
                .enter(sid, &left_source_program)
                .expect("left source endpoint");
            let mut right_source = rv
                .enter(sid, &right_source_program)
                .expect("right source endpoint");

            (async {
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
            })
            .await;
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn direct_recv_preserves_a_different_selected_arm_while_receiving_an_independent_message() {
    type Shared = Msg<68, u32>;
    type Right = Msg<69, u32>;
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let global = g::par(
                g::route(g::send::<0, 2, Shared>(), g::send::<0, 2, Right>()),
                g::send::<1, 2, Shared>(),
            );
            let choice_program: RoleProgram<0> = project(&global);
            let independent_program: RoleProgram<1> = project(&global);
            let receiver_program: RoleProgram<2> = project(&global);
            let rv = cluster.rendezvous(&mut slab, transport.clone()).unwrap();
            let session = SessionId::new(68);
            let mut choice = rv.enter(session, &choice_program).unwrap();
            let mut independent = rv.enter(session, &independent_program).unwrap();
            let mut receiver = rv.enter(session, &receiver_program).unwrap();
            (async {
                choice.send::<Right>(&21).await.unwrap();
                independent.send::<Shared>(&42).await.unwrap();
                assert_eq!(receiver.recv::<Shared>().await.unwrap(), 42);
                assert!(
                    !transport.queue_is_empty(),
                    "the chosen arm must remain unread"
                );
                let branch = receiver.offer().await.unwrap();
                assert_eq!(branch.label(), 69);
                assert_eq!(branch.recv::<Right>().await.unwrap(), 21);
            })
            .await;
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn direct_recv_keeps_an_unrelated_head_while_another_lane_waits() {
    type Shared = Msg<68, u32>;
    type Right = Msg<69, u8>;
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let global = g::par(
                g::send::<1, 2, Shared>(),
                g::route(g::send::<0, 2, Shared>(), g::send::<0, 2, Right>()),
            );
            let choice_program: RoleProgram<0> = project(&global);
            let independent_program: RoleProgram<1> = project(&global);
            let receiver_program: RoleProgram<2> = project(&global);
            let rv = cluster.rendezvous(&mut slab, transport.clone()).unwrap();
            let session = SessionId::new(69);
            let mut choice = rv.enter(session, &choice_program).unwrap();
            let mut independent = rv.enter(session, &independent_program).unwrap();
            let mut receiver = rv.enter(session, &receiver_program).unwrap();
            (choice.send::<Right>(&21)).await.unwrap();
            let wakes = Arc::new(WakeCount::default());
            let waker = Waker::from(wakes.clone());
            let mut context = Context::from_waker(&waker);
            {
                let mut receive = core::pin::pin!(receiver.recv::<Shared>());
                assert!(matches!(receive.as_mut().poll(&mut context), Poll::Pending));
                assert!(!transport.queue_is_empty());
                wakes.0.store(0, Ordering::SeqCst);
                (independent.send::<Shared>(&42)).await.unwrap();
                assert!(
                    wakes.0.load(Ordering::SeqCst) > 0,
                    "the requested lane must retain its waiter"
                );
                assert!(matches!(
                    receive.as_mut().poll(&mut context),
                    Poll::Ready(Ok(42))
                ));
            }
            assert_eq!((receiver.recv::<Right>()).await.unwrap(), 21);
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn direct_recv_rejects_when_all_lane_heads_have_other_contracts() {
    type Shared = Msg<68, u32>;
    type RightA = Msg<69, u32>;
    type RightB = Msg<70, u32>;
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let global = g::par(
                g::route(g::send::<0, 2, Shared>(), g::send::<0, 2, RightA>()),
                g::route(g::send::<1, 2, Shared>(), g::send::<1, 2, RightB>()),
            );
            let a_program: RoleProgram<0> = project(&global);
            let b_program: RoleProgram<1> = project(&global);
            let receiver_program: RoleProgram<2> = project(&global);
            let rv = cluster.rendezvous(&mut slab, transport).unwrap();
            let session = SessionId::new(71);
            let mut a = rv.enter(session, &a_program).unwrap();
            let mut b = rv.enter(session, &b_program).unwrap();
            let mut receiver = rv.enter(session, &receiver_program).unwrap();
            (a.send::<RightA>(&21)).await.unwrap();
            (b.send::<RightB>(&42)).await.unwrap();
            let mut receive = core::pin::pin!(receiver.recv::<Shared>());
            let mut context = Context::from_waker(Waker::noop());
            let Poll::Ready(Err(error)) = receive.as_mut().poll(&mut context) else {
                panic!("a wrong contract must be rejected when every lane head excludes it");
            };
            assert!(format!("{error:?}").contains("PhaseInvariant"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn direct_recv_rejects_an_unknown_head_instead_of_skipping_to_a_valid_lane() {
    type Shared = Msg<68, u32>;
    type Right = Msg<69, u32>;
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let global = g::par(
                g::route(g::send::<0, 2, Shared>(), g::send::<0, 2, Right>()),
                g::send::<1, 2, Shared>(),
            );
            let independent_program: RoleProgram<1> = project(&global);
            let receiver_program: RoleProgram<2> = project(&global);
            let rv = cluster.rendezvous(&mut slab, transport.clone()).unwrap();
            let session = SessionId::new(70);
            let mut independent = rv.enter(session, &independent_program).unwrap();
            let mut receiver = rv.enter(session, &receiver_program).unwrap();
            let mut tx = crate::common::TestTx {
                session_id: session,
                local_role: 5,
                pending_role: None,
                pending_frame: None,
            };
            transport.stage_send_with_session(&mut tx, session, 2, 0, 1, &21_u32.to_be_bytes());
            assert!(matches!(
                transport.poll_send_staged(&mut tx),
                Poll::Ready(Ok(()))
            ));
            (independent.send::<Shared>(&42)).await.unwrap();
            let error = (receiver.recv::<Shared>()).await.unwrap_err();
            assert!(format!("{error:?}").contains("PhaseInvariant"));
            assert!(
                !transport.queue_is_empty(),
                "the valid later lane must stay unread"
            );
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_same_label_recv_requires_causal_exit_handoff() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let body = g::send::<1, 0, Msg<56, u32>>().roll();
            let exit = g::seq(
                g::send::<0, 1, Msg<76, ()>>(),
                g::seq(
                    g::send::<1, 2, Msg<77, ()>>(),
                    g::send::<2, 0, Msg<56, u32>>(),
                ),
            );
            let program = g::seq(body, exit);
            let target_program: RoleProgram<0> = project(&program);
            let body_source_program: RoleProgram<1> = project(&program);
            let exit_source_program: RoleProgram<2> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(56);
            let mut target = rv.enter(sid, &target_program).expect("target endpoint");
            let mut body_source = rv
                .enter(sid, &body_source_program)
                .expect("body source endpoint");
            let mut exit_source = rv
                .enter(sid, &exit_source_program)
                .expect("exit source endpoint");

            (async {
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

                target
                    .send::<Msg<76, ()>>(&())
                    .await
                    .expect("target selects the causal exit path");
                body_source
                    .recv::<Msg<76, ()>>()
                    .await
                    .expect("body source observes roll exit");
                body_source
                    .send::<Msg<77, ()>>(&())
                    .await
                    .expect("body source hands exit authority to final sender");
                exit_source
                    .recv::<Msg<77, ()>>()
                    .await
                    .expect("exit source receives causal authority");
                exit_source
                    .send::<Msg<56, u32>>(&303)
                    .await
                    .expect("exit sends same logical label after handoff");
                assert_eq!(
                    target
                        .recv::<Msg<56, u32>>()
                        .await
                        .expect("recv commits exit frame"),
                    303
                );
            })
            .await;
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn intrinsic_route_passive_same_label_recv_commits_by_frame_evidence() {
    fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
        let left = g::seq(
            g::send::<0, 1, Msg<61, u32>>(),
            g::send::<1, 3, Msg<63, u32>>(),
        );
        let right = g::seq(
            g::send::<0, 1, Msg<62, u32>>(),
            g::send::<1, 3, Msg<63, u32>>(),
        );
        project(&g::route(left, right))
    }

    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(63);
            let mut controller = rv.enter(sid, &program::<0>()).expect("controller endpoint");
            let mut shared_sender = rv
                .enter(sid, &program::<1>())
                .expect("shared sender endpoint");
            let mut passive = rv.enter(sid, &program::<3>()).expect("passive endpoint");

            (async {
                controller
                    .send::<Msg<62, u32>>(&808)
                    .await
                    .expect("controller selects right arm");
                assert_eq!(
                    shared_sender
                        .recv::<Msg<62, u32>>()
                        .await
                        .expect("shared sender commits controller frame evidence"),
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
            })
            .await;
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn live_endpoint_send_recv_survives_endpoint_lease_table_growth() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::seq(
                g::send::<0, 1, Msg<8, FramePayload>>(),
                g::send::<2, 0, Msg<9, FramePayload>>(),
            );
            let role0: RoleProgram<0> = project(&program);
            let role1: RoleProgram<1> = project(&program);
            let role2: RoleProgram<2> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
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

            (endpoint0.send::<Msg<8, FramePayload>>(&FramePayload(*b"r0-1")))
                .await
                .expect("role0 send survives endpoint lease root relocation");
            let payload = (endpoint1.recv::<Msg<8, FramePayload>>())
                .await
                .expect("role1 recv succeeds");
            assert_eq!(payload.as_bytes(), b"r0-1");

            (endpoint2.send::<Msg<9, FramePayload>>(&FramePayload(*b"r2-0")))
                .await
                .expect("role2 send succeeds");
            let payload = (endpoint0.recv::<Msg<9, FramePayload>>())
                .await
                .expect("role0 recv survives endpoint lease root relocation");
            assert_eq!(payload.as_bytes(), b"r2-0");
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn live_endpoint_send_survives_failed_later_attach_abort() {
    let mut exercised = false;
    for slab_bytes in (1024usize..=8192).step_by(128) {
        {
            let mut slab = [0; 64 * 1024];
            let transport = TestTransport::new();
            'attach_attempt: {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let program = g::send::<0, 1, Msg<10, FramePayload>>();
                let role0: RoleProgram<0> = project(&program);
                let role1: RoleProgram<1> = project(&program);
                let rv = cluster
                    .rendezvous(&mut slab[..slab_bytes], transport.clone())
                    .expect("register constrained rendezvous");
                let sid = SessionId::new(10);
                let Ok(mut endpoint0) = rv.enter(sid, &role0) else {
                    break 'attach_attempt;
                };

                if rv.enter(sid, &role1).is_ok() {
                    break 'attach_attempt;
                }

                (endpoint0.send::<Msg<10, FramePayload>>(&FramePayload(*b"r0ok")))
                    .await
                    .expect("existing endpoint send survives failed later attach abort");
                exercised = true;
            };
        };
        if exercised {
            break;
        }
    }
    assert!(
        exercised,
        "test must find a constrained slab where role0 attaches and role1 attach rolls back"
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn direct_send_capacity_emits_transport_fault_tap() {
    {
        let mut slab = [0; 64 * 1024];
        let fault = {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<3, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, CapacitySendTransport)
                .expect("register rendezvous");

            let sid = SessionId::new(74);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let error = (origin_endpoint.send::<Msg<3, FramePayload>>(&FramePayload(*b"full")))
                .await
                .expect_err("capacity send must surface transport error");
            let rendered = format!("{error:?}");
            assert!(
                rendered.contains("Capacity") || rendered.contains("Transport(C)"),
                "send error must preserve capacity transport cause: {rendered}"
            );
            let mut tap = rv.tap();
            tap.find(|event| event.id() == hibana::runtime::tap::TRANSPORT_FAULT)
                .expect("new tap port after failure must read retained transport fault evidence")
        };
        assert_eq!(
            fault.evidence().reason(),
            hibana::runtime::tap::TRANSPORT_FAULT_CAPACITY
        );
        assert_eq!(fault.arg0(), 74);
        assert_eq!(fault.arg1(), 0);
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_recv_future_repoll_is_fail_fast_and_does_not_advance_again() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::seq(
                g::send::<0, 1, Msg<41, u32>>(),
                g::send::<0, 1, Msg<41, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(41);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            (origin_endpoint.send::<Msg<41, u32>>(&11))
                .await
                .expect("first send succeeds");
            (origin_endpoint.send::<Msg<41, u32>>(&22))
                .await
                .expect("second send succeeds");

            let mut recv_future = Box::pin(target_endpoint.recv::<Msg<41, u32>>());
            let waker = core::task::Waker::noop();
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

            let second = (target_endpoint.recv::<Msg<41, u32>>())
                .await
                .expect("second recv remains available");
            assert_eq!(
                second, 22,
                "completed recv future repoll must not consume the next descriptor"
            );
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_send_future_repoll_is_fail_fast_and_does_not_advance_again() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::seq(
                g::send::<0, 1, Msg<42, u32>>(),
                g::send::<0, 1, Msg<42, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(42);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let first = 11u32;
            let mut send_future = Box::pin(origin_endpoint.send::<Msg<42, u32>>(&first));
            let waker = core::task::Waker::noop();
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
            (origin_endpoint.send::<Msg<42, u32>>(&second))
                .await
                .expect("second send succeeds");

            let first_recv = (target_endpoint.recv::<Msg<42, u32>>())
                .await
                .expect("first recv remains available");
            let second_recv = (target_endpoint.recv::<Msg<42, u32>>())
                .await
                .expect("second recv remains available");
            assert_eq!(first_recv, 11);
            assert_eq!(
                second_recv, 22,
                "completed send future repoll must not consume the next descriptor"
            );
            assert!(transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn send_preview_error_reports_debug_boundary_and_poison_session() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(11);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let err = (origin_endpoint.send::<Msg<2, u32>>(&22))
                .await
                .expect_err("send with wrong logical label must fail");
            assert!(format!("{err:?}").contains("operation: \"send\""));
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("LabelMismatch"),
                "failed send preview must report the preview mismatch: {rendered}"
            );

            let poisoned = (origin_endpoint.send::<Msg<1, u32>>(&22))
                .await
                .expect_err("preview mismatch must poison the affine session");
            let rendered = format!("{poisoned:?}");
            assert!(
                rendered.contains("SessionFault") && rendered.contains("ProtocolViolation"),
                "the first mismatch must be the preserved session fault: {rendered}"
            );
            assert!(transport.queue_is_empty());
        };
    };
}
