mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use core::future::Future;
use core::task::{Context, Poll};
use std::panic::{AssertUnwindSafe, catch_unwind};

use common::TestTransport;
use hibana::g::Message;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
    transport::{ReceivedFrame, Transport, TransportError},
};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_choice_after_a_parallel_join_survives_repeated_preview()
-> core::result::Result<(), hibana::EndpointError> {
    type First = Msg<220, ()>;
    type FirstReply = Msg<221, ()>;
    type Second = Msg<222, ()>;
    type SecondReply = Msg<223, ()>;
    type Left = Msg<224, ()>;
    type Right = Msg<225, ()>;
    let global = g::seq(
        g::par(
            g::seq(g::send::<0, 1, First>(), g::send::<1, 0, FirstReply>()),
            g::seq(g::send::<0, 1, Second>(), g::send::<1, 0, SecondReply>()),
        ),
        g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()).resolve::<0>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = TestTransport::new();
    let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let sid = SessionId::new(9483);
    let mut sender = rv0.enter(sid, &p0).unwrap();
    let mut receiver = rv1.enter(sid, &p1).unwrap();
    {
        (async {
            {
                let [mut first, mut second] = sender.par().unwrap();
                let [mut first_peer, mut second_peer] = receiver.par().unwrap();
                async {
                    async {
                        {
                            first.send::<First>(&()).await.unwrap();
                            first.recv::<FirstReply>().await.unwrap();
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        {
                            second.send::<Second>(&()).await.unwrap();
                            second.recv::<SecondReply>().await.unwrap();
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        {
                            first_peer.recv::<First>().await.unwrap();
                            first_peer.send::<FirstReply>(&()).await.unwrap();
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        {
                            second_peer.recv::<Second>().await.unwrap();
                            second_peer.send::<SecondReply>(&()).await.unwrap();
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await
                .expect("all child results are data");
            }
            sender
                .offer()
                .await
                .unwrap()
                .send::<Left>(&())
                .await
                .unwrap();
            let preview = receiver.offer().await.unwrap();
            assert_eq!(preview.label(), 224);
            drop(preview);
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), 224);
            branch.recv::<Left>().await.unwrap();
            assert!(network.queue_is_empty());
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_completed_parallel_sibling_does_not_authorize_the_join_receive()
-> core::result::Result<(), hibana::EndpointError> {
    type Left = Msg<230, ()>;
    type Right = Msg<231, ()>;
    type Independent = Msg<232, ()>;
    type Tail = Msg<233, ()>;
    let global = g::seq(
        g::par(
            g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
            g::send::<0, 1, Independent>(),
        ),
        g::send::<0, 1, Tail>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = TestTransport::new();
    let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let sid = SessionId::new(9482);
    let mut sender = rv0.enter(sid, &p0).unwrap();
    let mut receiver = rv1.enter(sid, &p1).unwrap();
    {
        sender.send::<Independent>(&()).await.unwrap();
        receiver.recv::<Independent>().await.unwrap();
        sender.send::<Left>(&()).await.unwrap();
        let tail = crate::in_process::timeout(
            core::time::Duration::from_millis(1),
            receiver.recv::<Tail>(),
        )
        .await;
        tail.expect("an out-of-order receive must return without waiting")
            .expect_err("the branch frame was consumed as its sequential continuation");
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_completed_parallel_sibling_keeps_the_other_choice_observable()
-> core::result::Result<(), hibana::EndpointError> {
    type Left = Msg<230, ()>;
    type Right = Msg<231, ()>;
    type Independent = Msg<232, ()>;
    type Tail = Msg<233, ()>;
    let global = g::seq(
        g::par(
            g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
            g::send::<0, 1, Independent>(),
        ),
        g::send::<0, 1, Tail>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = TestTransport::new();
    let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let sid = SessionId::new(9481);
    let mut sender = rv0.enter(sid, &p0).unwrap();
    let mut receiver = rv1.enter(sid, &p1).unwrap();
    {
        sender.send::<Independent>(&()).await.unwrap();
        receiver.recv::<Independent>().await.unwrap();
        sender.send::<Left>(&()).await.unwrap();
        let branch = receiver.offer().await.unwrap();
        assert_eq!(branch.label(), 230);
        drop(branch);
        let branch = receiver.offer().await.unwrap();
        assert_eq!(branch.label(), 230);
        branch.recv::<Left>().await.unwrap();
        sender.send::<Tail>(&()).await.unwrap();
        receiver.recv::<Tail>().await.unwrap();
        assert!(network.queue_is_empty());
    };

    Ok(())
}

const MATERIALIZED_MISMATCH_RESOLVER: u16 = 41;
static MATERIALIZED_MISMATCH_STATE: () = ();

#[derive(Clone)]
struct DeterministicRecvTransport(TestTransport);

impl DeterministicRecvTransport {
    fn new() -> Self {
        Self(TestTransport::new())
    }

    fn queue_is_empty(&self) -> bool {
        self.0.queue_is_empty()
    }
}

impl Transport for DeterministicRecvTransport {
    type Tx<'a>
        = <TestTransport as Transport>::Tx<'a>
    where
        Self: 'a;
    type Rx<'a>
        = <TestTransport as Transport>::Rx<'a>
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.0.open(port)
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: hibana::runtime::transport::Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        self.0.poll_send(tx, outgoing, cx)
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        self.0.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        match self.0.poll_recv(rx, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => {
                Poll::Ready(Ok(ReceivedFrame::deterministic(frame.payload())))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        self.0.requeue(rx)
    }
}

#[derive(Clone)]
struct LabelRewriteTransport {
    inner: TestTransport,
    frame_label: u16,
}

struct LabelRewriteRx<'a> {
    inner: <TestTransport as Transport>::Rx<'a>,
    session_id: SessionId,
    lane: u8,
    local_role: u8,
}

impl LabelRewriteTransport {
    fn new(frame_label: u16) -> Self {
        Self {
            inner: TestTransport::new(),
            frame_label,
        }
    }

    fn queue_is_empty(&self) -> bool {
        self.inner.queue_is_empty()
    }
}

impl Transport for LabelRewriteTransport {
    type Tx<'a>
        = <TestTransport as Transport>::Tx<'a>
    where
        Self: 'a;
    type Rx<'a>
        = LabelRewriteRx<'a>
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let session_id = port.session_id();
        let lane = port.lane();
        let local_role = port.local_role();
        let (tx, inner) = self.inner.open(port);
        (
            tx,
            LabelRewriteRx {
                inner,
                session_id,
                lane,
                local_role,
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: hibana::runtime::transport::Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        self.inner.poll_send(tx, outgoing, cx)
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        match self.inner.poll_recv(&mut rx.inner, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => {
                let header = common::frame_header_from_parts(
                    rx.session_id,
                    rx.lane,
                    0,
                    rx.local_role,
                    self.frame_label,
                );
                Poll::Ready(Ok(ReceivedFrame::framed(header, frame.payload())))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(&mut rx.inner)
    }
}

fn route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm);
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    project(&program)
}

fn resolved_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm).resolve::<MATERIALIZED_MISMATCH_RESOLVER>();
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    project(&program)
}

fn route_with_late_role2<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm);
    let program = g::seq(
        g::seq(route, g::send::<0, 1, Msg<73, u32>>()),
        g::send::<2, 0, Msg<74, u32>>(),
    );
    project(&program)
}

fn choose_left(
    _: &(),
) -> Result<hibana::runtime::resolver::DecisionArm, hibana::runtime::resolver::ResolverError> {
    Ok(DecisionArm::Left)
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn drop_public_preview_branch_preserves_offer_progression() {
    let mut slab = [0; 64 * 1024];
    let _transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, _transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    {
        controller
            .send::<Msg<71, u32>>(&4444)
            .await
            .expect("send data");

        let branch = worker.offer().await.expect("offer left arm");
        assert_eq!(branch.label(), <Msg<71, u32> as Message>::LOGICAL_LABEL);
        drop(branch);

        let branch = worker.offer().await.expect("re-offer left arm");
        let value = branch
            .recv::<Msg<71, u32>>()
            .await
            .expect("recv left data after dropped preview");
        assert_eq!(value, 4444);

        controller
            .send::<Msg<73, u32>>(&55)
            .await
            .expect("send data");
        assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 55);
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_parked_frame_rejects_the_other_arm_without_polling_its_port_again() {
    let mut slab = [0; 64 * 1024];
    let _transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, _transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    (async {
        controller.send::<Msg<71, u32>>(&4444).await.unwrap();
        let branch = worker.offer().await.unwrap();
        assert_eq!(branch.label(), 71);
        drop(branch);

        let result = core::pin::pin!(worker.recv::<Msg<72, u32>>())
            .as_mut()
            .poll(&mut Context::from_waker(core::task::Waker::noop()));
        assert!(matches!(result, Poll::Ready(Err(_))), "{result:?}");
    })
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_parked_frame_keeps_an_independent_receive_live() {
    type Left = Msg<234, u32>;
    type Right = Msg<235, u32>;
    type Independent = Msg<236, u32>;
    let global = g::par(
        g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
        g::send::<0, 1, Independent>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    for wait in [false, true] {
        let network = TestTransport::new();
        let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let sid = SessionId::new(9484);
        let mut sender = rv0.enter(sid, &p0).unwrap();
        let mut receiver = rv1.enter(sid, &p1).unwrap();
        {
            sender.send::<Left>(&41).await.unwrap();
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), 234);
            drop(branch);

            if wait {
                assert!(
                    core::pin::pin!(receiver.recv::<Independent>())
                        .as_mut()
                        .poll(&mut Context::from_waker(core::task::Waker::noop()))
                        .is_pending()
                );
            }
            sender.send::<Independent>(&73).await.unwrap();
            assert_eq!(receiver.recv::<Independent>().await.unwrap(), 73);
            assert_eq!(receiver.recv::<Left>().await.unwrap(), 41);
            assert!(network.queue_is_empty());
        }
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn live_endpoint_branch_recv_survives_endpoint_lease_table_growth() {
    let mut slab = [0; 64 * 1024];
    let transport = TestTransport::new();
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let rv = cluster
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(913);
        let role0 = route_with_late_role2::<0>();
        let role1 = route_with_late_role2::<1>();
        let role2 = route_with_late_role2::<2>();
        let mut controller = rv.enter(sid, &role0).expect("attach route controller");
        let mut worker = rv.enter(sid, &role1).expect("attach route worker");
        let late_role = rv
            .enter(sid, &role2)
            .expect("attach late role and grow lease table");
        core::hint::black_box(&late_role);

        {
            controller
                .send::<Msg<71, u32>>(&8888)
                .await
                .expect("send data");
            let branch = worker
                .offer()
                .await
                .expect("offer survives endpoint lease root relocation");
            let value = branch
                .recv::<Msg<71, u32>>()
                .await
                .expect("branch recv survives endpoint lease root relocation");
            assert_eq!(value, 8888);

            controller
                .send::<Msg<73, u32>>(&99)
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 99);
            assert!(transport.queue_is_empty());
        }
        drop(late_role);
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn forgotten_route_branch_leaves_endpoint_fail_closed() {
    let mut slab = [0; 64 * 1024];
    let _transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, _transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    (async {
        controller
            .send::<Msg<71, u32>>(&4444)
            .await
            .expect("send data");

        let branch = worker.offer().await.expect("offer left arm");
        assert_eq!(branch.label(), <Msg<71, u32> as Message>::LOGICAL_LABEL);
        core::mem::forget(branch);

        let err = match worker.offer().await {
            Ok(_) => panic!("forgotten route branch must leave endpoint fail-closed"),
            Err(err) => err,
        };
        assert!(format!("{err:?}").contains("operation: \"offer\""));
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("PhaseInvariant")
                || rendered.contains("ProgressInvariantViolated")
                || rendered.contains("SessionFault"),
            "busy endpoint must preserve terminal progress evidence: {rendered}"
        );
    })
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn branch_recv_transport_consumes_frame_once() {
    let mut slab = [0; 64 * 1024];
    let transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    {
        controller
            .send::<Msg<71, u32>>(&1234)
            .await
            .expect("send data");
        let branch = worker.offer().await.expect("offer left arm");
        assert_eq!(branch.label(), 71);
        assert_eq!(
            branch.recv::<Msg<71, u32>>().await.expect("recv left data"),
            1234
        );
        assert!(
            transport.queue_is_empty(),
            "decoded transport frame must not remain queued"
        );

        controller
            .send::<Msg<73, u32>>(&77)
            .await
            .expect("send data");
        assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 77);
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_offer_future_repoll_is_fail_fast_and_does_not_advance_again() {
    let mut slab = [0; 64 * 1024];
    let transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    controller
        .send::<Msg<71, u32>>(&1234)
        .await
        .expect("send data");

    let mut offer = Box::pin(worker.offer());
    let waker = core::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    let branch = match Future::poll(offer.as_mut(), &mut context) {
        Poll::Ready(Ok(branch)) => branch,
        Poll::Ready(Err(error)) => panic!("offer failed: {error:?}"),
        Poll::Pending => panic!("offer must be ready"),
    };
    assert_eq!(branch.label(), 71);

    let repoll = catch_unwind(AssertUnwindSafe(|| {
        let _ = Future::poll(offer.as_mut(), &mut context);
    }));
    assert!(
        repoll.is_err(),
        "completed offer future must fail fast on post-Ready poll"
    );
    drop(offer);

    let value = (branch.recv::<Msg<71, u32>>())
        .await
        .expect("recv returned branch");
    assert_eq!(value, 1234);
    assert!(transport.queue_is_empty());
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_route_recv_future_repoll_is_fail_fast_and_does_not_advance_again() {
    let mut slab = [0; 64 * 1024];
    let transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    controller
        .send::<Msg<71, u32>>(&1234)
        .await
        .expect("send data");

    let branch = (worker.offer()).await.expect("offer left arm");
    let mut recv = Box::pin(branch.recv::<Msg<71, u32>>());
    let waker = core::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    match Future::poll(recv.as_mut(), &mut context) {
        Poll::Ready(Ok(value)) => assert_eq!(value, 1234),
        Poll::Ready(Err(error)) => panic!("route branch recv failed: {error:?}"),
        Poll::Pending => panic!("route branch recv must be ready"),
    }

    let repoll = catch_unwind(AssertUnwindSafe(|| {
        let _ = Future::poll(recv.as_mut(), &mut context);
    }));
    assert!(
        repoll.is_err(),
        "completed route branch recv future must fail fast on post-Ready poll"
    );
    drop(recv);
    assert!(transport.queue_is_empty());
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn offer_requires_framed_receive_evidence_for_branch_demux() {
    let mut slab = [0; 64 * 1024];
    let transport = DeterministicRecvTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(902);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    controller
        .send::<Msg<71, u32>>(&1234)
        .await
        .expect("send data");
    let error = match match core::pin::pin!(worker.offer())
        .as_mut()
        .poll(&mut Context::from_waker(core::task::Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("headerless demux frame must fail closed"),
    } {
        Ok(_) => panic!("headerless demux frame must not materialize a route branch"),
        Err(error) => error,
    };
    assert!(format!("{error:?}").contains("operation: \"offer\""));
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("PhaseInvariant"),
        "headerless demux frame must report terminal invariant evidence: {rendered}"
    );
    assert!(
        transport.queue_is_empty(),
        "headerless demux frame must not be buffered as route authority"
    );

    let poisoned = match (worker.offer()).await {
        Ok(_) => panic!("headerless demux frame must poison the same generation"),
        Err(error) => error,
    };
    let rendered = format!("{poisoned:?}");
    assert!(
        rendered.contains("SessionFault") && rendered.contains("ProgressInvariantViolated"),
        "headerless demux frame must poison same generation: {rendered}"
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn offer_materialized_label_mismatch_fails_closed() {
    let mut slab = [0; 64 * 1024];
    // A valid alternate-arm label would be an undetectable violation of the
    // carrier's exact-frame contract, not an endpoint-level label mismatch.
    let transport = LabelRewriteTransport::new(u16::MAX);
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let rv = cluster
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous");
        let mut tap = rv.tap();
        let sid = SessionId::new(903);
        let controller_program = resolved_route_program::<0>();
        let worker_program = resolved_route_program::<1>();
        rv.set_resolver(
            &controller_program,
            ResolverRef::<MATERIALIZED_MISMATCH_RESOLVER>::decision_state(
                &MATERIALIZED_MISMATCH_STATE,
                choose_left,
            ),
        )
        .expect("install controller resolver");
        rv.set_resolver(
            &worker_program,
            ResolverRef::<MATERIALIZED_MISMATCH_RESOLVER>::decision_state(
                &MATERIALIZED_MISMATCH_STATE,
                choose_left,
            ),
        )
        .expect("install worker resolver");
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("attach controller");
        let mut worker = rv.enter(sid, &worker_program).expect("attach worker");

        controller
            .send::<Msg<71, u32>>(&1234)
            .await
            .expect("send data");
        let error = match match core::pin::pin!(worker.offer())
            .as_mut()
            .poll(&mut Context::from_waker(core::task::Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("materialized label mismatch must fail closed"),
        } {
            Ok(_) => panic!("materialized label mismatch must fail closed"),
            Err(error) => error,
        };
        assert!(format!("{error:?}").contains("operation: \"offer\""));
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("PhaseInvariant"),
            "label mismatch must report terminal invariant evidence: {rendered}"
        );

        let mismatch = tap
            .find(|event| event.id() == hibana::runtime::tap::TRANSPORT_MISMATCH)
            .expect("materialized label mismatch must emit transport mismatch evidence");
        assert_eq!(
            mismatch.evidence().reason(),
            hibana::runtime::tap::TRANSPORT_MISMATCH_LABEL
        );
        assert!(transport.queue_is_empty());

        let poisoned = match (worker.offer()).await {
            Ok(_) => panic!("materialized label mismatch must poison same generation"),
            Err(error) => error,
        };
        let rendered = format!("{poisoned:?}");
        assert!(
            rendered.contains("SessionFault") && rendered.contains("ProgressInvariantViolated"),
            "label mismatch must poison same generation: {rendered}"
        );
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn schema_mismatch_in_public_route_recv_poisons_same_generation() {
    let mut slab = [0; 64 * 1024];
    let _transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, _transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    (async {
        controller
            .send::<Msg<71, u32>>(&1234)
            .await
            .expect("send data");
        let branch = worker.offer().await.expect("offer left arm");
        let recv_result = branch.recv::<Msg<71, i32>>().await;
        let err = match recv_result {
            Ok(_) => panic!("wrong payload schema must fail recv"),
            Err(err) => err,
        };
        assert!(format!("{err:?}").contains("operation: \"recv\""));
        assert!(
            format!("{err:?}").contains("PayloadMismatch"),
            "same-width wrong payload must preserve schema evidence: {err:?}"
        );

        let err = match worker.offer().await {
            Ok(_) => panic!("poisoned generation must not re-offer the route arm"),
            Err(err) => err,
        };
        assert!(format!("{err:?}").contains("operation: \"offer\""));
    })
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn forgotten_route_recv_future_leaves_endpoint_fail_closed() {
    let mut slab = [0; 64 * 1024];
    let _transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, _transport.clone())
        .expect("register rendezvous");
    let sid = SessionId::new(901);
    let controller_program = route_program::<0>();
    let worker_program = route_program::<1>();
    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    (async {
        controller
            .send::<Msg<71, u32>>(&1234)
            .await
            .expect("send data");
        let branch = worker.offer().await.expect("offer left arm");
        let recv = branch.recv::<Msg<71, u32>>();
        core::mem::forget(recv);

        let err = match worker.offer().await {
            Ok(_) => {
                panic!("forgotten route branch recv future must leave endpoint fail-closed")
            }
            Err(err) => err,
        };
        assert!(format!("{err:?}").contains("operation: \"offer\""));
    })
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_same_lane_receive_choices_keep_their_exact_branch_admission()
-> core::result::Result<(), hibana::EndpointError> {
    {
        for choice in 0..3u8 {
            for receiver_first in [false, true] {
                type A = Msg<210, ()>;
                type B = Msg<211, ()>;
                type C = Msg<212, ()>;
                let global = g::route(
                    g::send::<0, 1, A>(),
                    g::route(g::send::<0, 1, B>(), g::send::<0, 1, C>()),
                );
                let p0 = project::<0>(&global);
                let p1 = project::<1>(&global);
                let network = TestTransport::new();
                let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
                let mut storage0 = SessionKitStorage::uninit();
                let mut storage1 = SessionKitStorage::uninit();
                let rv0 = storage0
                    .init()
                    .rendezvous(&mut slab0, network.clone())
                    .unwrap();
                let rv1 = storage1
                    .init()
                    .rendezvous(&mut slab1, network.clone())
                    .unwrap();
                let sid = SessionId::new(9490);
                let mut sender = rv0.enter(sid, &p0).unwrap();
                let mut receiver = rv1.enter(sid, &p1).unwrap();
                crate::in_process::timeout(core::time::Duration::from_secs(1), async {
                    let send = async {
                        match choice {
                            0 => sender.send::<A>(&()).await.unwrap(),
                            1 => sender.send::<B>(&()).await.unwrap(),
                            2 => sender.send::<C>(&()).await.unwrap(),
                            _ => unreachable!(),
                        }
                        let header = network.queued_frame(1).0.bytes();
                        assert_eq!(&header[4..7], &[0, 0, 1]);
                        assert_eq!(
                            u16::from_be_bytes([header[7], header[8]]),
                            u16::from(choice)
                        );
                    };
                    let receive = async {
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(
                            branch.label(),
                            210 + choice,
                            "choice={choice} receiver_first={receiver_first}"
                        );
                        drop(branch);
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(
                            branch.label(),
                            210 + choice,
                            "repeated preview choice={choice} receiver_first={receiver_first}"
                        );
                        match choice {
                            0 => branch.recv::<A>().await.unwrap(),
                            1 => branch.recv::<B>().await.unwrap(),
                            2 => branch.recv::<C>().await.unwrap(),
                            _ => unreachable!(),
                        }
                    };
                    if receiver_first {
                        async {
                            async {
                                let _: () = (receive).await;
                                Ok::<_, hibana::EndpointError>(())
                            };
                            async {
                                let _: () = (send).await;
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    } else {
                        async {
                            async {
                                let _: () = (send).await;
                                Ok::<_, hibana::EndpointError>(())
                            };
                            async {
                                let _: () = (receive).await;
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    }
                })
                .await
                .unwrap_or_else(|error| {
                    panic!("choice={choice} receiver_first={receiver_first}: {error:?}")
                });
                assert!(network.queue_is_empty());
            }
        }
    };

    Ok(())
}
