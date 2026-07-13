mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;
use core::future::Future;
use core::task::{Context, Poll};
use std::panic::{AssertUnwindSafe, catch_unwind};

use common::TestTransport;
use futures::FutureExt;
use hibana::g::Message;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
    transport::{ReceivedFrame, Transport, TransportError},
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;
type DeterministicKitStorage = SessionKitStorage<'static, DeterministicRecvTransport>;
type LabelRewriteKitStorage = SessionKitStorage<'static, LabelRewriteTransport>;
const MATERIALIZED_MISMATCH_RESOLVER: u16 = 41;
static MATERIALIZED_MISMATCH_STATE: () = ();

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static DETERMINISTIC_SESSION_SLOT: UnsafeCell<DeterministicKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static LABEL_REWRITE_SESSION_SLOT: UnsafeCell<LabelRewriteKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

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

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: hibana::runtime::transport::Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        self.0.poll_send(tx, outgoing, cx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.0.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
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
    frame_label: u8,
}

struct LabelRewriteRx<'a> {
    inner: <TestTransport as Transport>::Rx<'a>,
    session_id: SessionId,
    lane: u8,
    local_role: u8,
}

impl LabelRewriteTransport {
    fn new(frame_label: u8) -> Self {
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

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: hibana::runtime::transport::Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        self.inner.poll_send(tx, outgoing, cx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
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

fn with_route_workspace(
    run: impl FnOnce(
        &mut hibana::Endpoint<'static, 0>,
        &mut hibana::Endpoint<'static, 1>,
        &TestTransport,
    ),
) {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let sid = SessionId::new(901);
            let controller_program = route_program::<0>();
            let worker_program = route_program::<1>();
            let mut controller = rv
                .enter(sid, &controller_program)
                .expect("attach controller");
            let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
            run(&mut controller, &mut worker, &transport);
        });
    });
}

fn with_deterministic_route_workspace(
    run: impl FnOnce(
        &mut hibana::Endpoint<'static, 0>,
        &mut hibana::Endpoint<'static, 1>,
        &DeterministicRecvTransport,
    ),
) {
    with_runtime_workspace(|slab| {
        let transport = DeterministicRecvTransport::new();
        with_resident_tls_ref(&DETERMINISTIC_SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let sid = SessionId::new(902);
            let controller_program = route_program::<0>();
            let worker_program = route_program::<1>();
            let mut controller = rv
                .enter(sid, &controller_program)
                .expect("attach controller");
            let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
            run(&mut controller, &mut worker, &transport);
        });
    });
}

async fn send_left(controller: &mut hibana::Endpoint<'static, 0>, value: u32) {
    controller
        .send::<Msg<71, u32>>(&value)
        .await
        .expect("send left data");
}

async fn send_tail(controller: &mut hibana::Endpoint<'static, 0>, value: u32) {
    controller
        .send::<Msg<73, u32>>(&value)
        .await
        .expect("send tail");
}

#[test]
fn drop_public_preview_branch_preserves_offer_progression() {
    with_route_workspace(|controller, worker, _transport| {
        futures::executor::block_on(async {
            send_left(controller, 4444).await;

            let branch = worker.offer().await.expect("offer left arm");
            assert_eq!(branch.label(), <Msg<71, u32> as Message>::LOGICAL_LABEL);
            drop(branch);

            let branch = worker.offer().await.expect("re-offer left arm");
            let value = branch
                .recv::<Msg<71, u32>>()
                .await
                .expect("recv left data after dropped preview");
            assert_eq!(value, 4444);

            send_tail(controller, 55).await;
            assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 55);
        });
    });
}

#[test]
fn live_endpoint_branch_recv_survives_endpoint_lease_table_growth() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
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

            futures::executor::block_on(async {
                send_left(&mut controller, 8888).await;
                let branch = worker
                    .offer()
                    .await
                    .expect("offer survives endpoint lease root relocation");
                let value = branch
                    .recv::<Msg<71, u32>>()
                    .await
                    .expect("branch recv survives endpoint lease root relocation");
                assert_eq!(value, 8888);

                send_tail(&mut controller, 99).await;
                assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 99);
                assert!(transport.queue_is_empty());
            });
            drop(late_role);
        });
    });
}

#[test]
fn forgotten_route_branch_leaves_endpoint_fail_closed() {
    with_route_workspace(|controller, worker, _transport| {
        futures::executor::block_on(async {
            send_left(controller, 4444).await;

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
        });
    });
}

#[test]
fn branch_recv_transport_consumes_frame_once() {
    with_route_workspace(|controller, worker, transport| {
        futures::executor::block_on(async {
            send_left(controller, 1234).await;
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

            send_tail(controller, 77).await;
            assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 77);
        });
    });
}

#[test]
fn completed_offer_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_route_workspace(|controller, worker, transport| {
        futures::executor::block_on(send_left(controller, 1234));

        let mut offer = Box::pin(worker.offer());
        let waker = futures::task::noop_waker_ref();
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

        let value = futures::executor::block_on(branch.recv::<Msg<71, u32>>())
            .expect("recv returned branch");
        assert_eq!(value, 1234);
        assert!(transport.queue_is_empty());
    });
}

#[test]
fn completed_route_recv_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_route_workspace(|controller, worker, transport| {
        futures::executor::block_on(send_left(controller, 1234));

        let branch = futures::executor::block_on(worker.offer()).expect("offer left arm");
        let mut recv = Box::pin(branch.recv::<Msg<71, u32>>());
        let waker = futures::task::noop_waker_ref();
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
    });
}

fn assert_headerless_demux_frame_fails_closed_not_pending() {
    with_deterministic_route_workspace(|controller, worker, transport| {
        futures::executor::block_on(send_left(controller, 1234));
        let error = match worker
            .offer()
            .now_or_never()
            .expect("headerless demux frame must fail closed")
        {
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

        let poisoned = match futures::executor::block_on(worker.offer()) {
            Ok(_) => panic!("headerless demux frame must poison the same generation"),
            Err(error) => error,
        };
        let rendered = format!("{poisoned:?}");
        assert!(
            rendered.contains("SessionFault") && rendered.contains("ProgressInvariantViolated"),
            "headerless demux frame must poison same generation: {rendered}"
        );
    });
}

#[test]
fn offer_requires_framed_receive_evidence_for_branch_demux() {
    assert_headerless_demux_frame_fails_closed_not_pending();
}

#[test]
fn offer_headerless_demux_frame_fails_closed_not_pending() {
    assert_headerless_demux_frame_fails_closed_not_pending();
}

#[test]
fn offer_materialized_label_mismatch_fails_closed() {
    with_runtime_workspace(|slab| {
        // A valid alternate-arm label would be an undetectable violation of the
        // carrier's exact-frame contract, not an endpoint-level label mismatch.
        let transport = LabelRewriteTransport::new(u8::MAX);
        with_resident_tls_ref(&LABEL_REWRITE_SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
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

            futures::executor::block_on(send_left(&mut controller, 1234));
            let error = match worker
                .offer()
                .now_or_never()
                .expect("materialized label mismatch must fail closed")
            {
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

            let poisoned = match futures::executor::block_on(worker.offer()) {
                Ok(_) => panic!("materialized label mismatch must poison same generation"),
                Err(error) => error,
            };
            let rendered = format!("{poisoned:?}");
            assert!(
                rendered.contains("SessionFault") && rendered.contains("ProgressInvariantViolated"),
                "label mismatch must poison same generation: {rendered}"
            );
        });
    });
}

#[test]
fn schema_mismatch_in_public_route_recv_poisons_same_generation() {
    with_route_workspace(|controller, worker, _transport| {
        futures::executor::block_on(async {
            send_left(controller, 1234).await;
            let branch = worker.offer().await.expect("offer left arm");
            let recv_result = branch.recv::<Msg<71, i32>>().await;
            let err = match recv_result {
                Ok(_) => panic!("wrong payload schema must fail recv"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("operation: \"recv\""));
            assert!(
                format!("{err:?}").contains("SchemaMismatch"),
                "same-width wrong payload must preserve schema evidence: {err:?}"
            );

            let err = match worker.offer().await {
                Ok(_) => panic!("poisoned generation must not re-offer the route arm"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("operation: \"offer\""));
        });
    });
}

#[test]
fn forgotten_route_recv_future_leaves_endpoint_fail_closed() {
    with_route_workspace(|controller, worker, _transport| {
        futures::executor::block_on(async {
            send_left(controller, 1234).await;
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
        });
    });
}
