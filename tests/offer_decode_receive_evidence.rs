#![cfg(feature = "std")]
mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;
use core::future::Future;
use core::task::{Context, Poll};
use std::panic::{AssertUnwindSafe, catch_unwind};

use common::{TestTransport, TestTransportError};
use futures::FutureExt;
use hibana::g::Message;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{
    Config, SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, DecisionResolution, ResolverRef},
    transport::{ReceivedFrame, Transport},
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport, 2>;
type DeterministicKitStorage = SessionKitStorage<'static, DeterministicRecvTransport, 2>;
type LabelRewriteKitStorage = SessionKitStorage<'static, LabelRewriteTransport, 2>;
const MATERIALIZED_MISMATCH_RESOLVER: u16 = 41;

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
    type Error = TestTransportError;
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
    ) -> Poll<Result<(), Self::Error>>
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
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        match self.0.poll_recv(rx, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => {
                Poll::Ready(Ok(ReceivedFrame::deterministic(frame.payload())))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
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
    type Error = TestTransportError;
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
    ) -> Poll<Result<(), Self::Error>>
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
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        match self.inner.poll_recv(&mut rx.inner, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(frame)) => {
                let header = common::frame_header_from_parts(
                    rx.session_id,
                    rx.lane,
                    0,
                    rx.local_role,
                    hibana::runtime::transport::FrameLabel::new(self.frame_label),
                );
                Poll::Ready(Ok(ReceivedFrame::framed(header, frame.payload())))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        self.inner.requeue(&mut rx.inner)
    }
}

fn controller_program() -> RoleProgram<0> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm);
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm);
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    project(&program)
}

fn resolved_controller_program() -> RoleProgram<0> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm).resolve::<MATERIALIZED_MISMATCH_RESOLVER>();
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    project(&program)
}

fn resolved_worker_program() -> RoleProgram<1> {
    let left_arm = g::send::<0, 1, Msg<71, u32>>();
    let right_arm = g::send::<0, 1, Msg<72, u32>>();
    let route = g::route(left_arm, right_arm).resolve::<MATERIALIZED_MISMATCH_RESOLVER>();
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    project(&program)
}

fn choose_left() -> Result<DecisionResolution, hibana::runtime::resolver::ResolverError> {
    Ok(DecisionResolution::Arm(DecisionArm::Left))
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
            let config = Config::from_resources(slab);
            let rv = cluster
                .rendezvous(config, transport.clone())
                .expect("register rendezvous");
            let sid = SessionId::new(901);
            let controller_program = controller_program();
            let worker_program = worker_program();
            let mut controller = rv
                .session(sid)
                .role(&controller_program)
                .enter()
                .expect("attach controller");
            let mut worker = rv
                .session(sid)
                .role(&worker_program)
                .enter()
                .expect("attach worker");
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
            let config = Config::from_resources(slab);
            let rv = cluster
                .rendezvous(config, transport.clone())
                .expect("register rendezvous");
            let sid = SessionId::new(902);
            let controller_program = controller_program();
            let worker_program = worker_program();
            let mut controller = rv
                .session(sid)
                .role(&controller_program)
                .enter()
                .expect("attach controller");
            let mut worker = rv
                .session(sid)
                .role(&worker_program)
                .enter()
                .expect("attach worker");
            run(&mut controller, &mut worker, &transport);
        });
    });
}

async fn send_left(controller: &mut hibana::Endpoint<'static, 0>, value: u32) {
    controller
        .flow::<Msg<71, u32>>()
        .expect("left data flow")
        .send(&value)
        .await
        .expect("send left data");
}

async fn send_tail(controller: &mut hibana::Endpoint<'static, 0>, value: u32) {
    controller
        .flow::<Msg<73, u32>>()
        .expect("tail flow")
        .send(&value)
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
                .decode::<Msg<71, u32>>()
                .await
                .expect("decode left data after dropped preview");
            assert_eq!(value, 4444);

            send_tail(controller, 55).await;
            assert_eq!(worker.recv::<Msg<73, u32>>().await.expect("recv tail"), 55);
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
            assert_eq!(err.operation(), "offer");
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
fn offer_decode_transport_consumes_frame_once() {
    with_route_workspace(|controller, worker, transport| {
        futures::executor::block_on(async {
            send_left(controller, 1234).await;
            let branch = worker.offer().await.expect("offer left arm");
            assert_eq!(branch.label(), 71);
            assert_eq!(
                branch
                    .decode::<Msg<71, u32>>()
                    .await
                    .expect("decode left data"),
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

        let value = futures::executor::block_on(branch.decode::<Msg<71, u32>>())
            .expect("decode returned branch");
        assert_eq!(value, 1234);
        assert!(transport.queue_is_empty());
    });
}

#[test]
fn completed_decode_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_route_workspace(|controller, worker, transport| {
        futures::executor::block_on(send_left(controller, 1234));

        let branch = futures::executor::block_on(worker.offer()).expect("offer left arm");
        let mut decode = Box::pin(branch.decode::<Msg<71, u32>>());
        let waker = futures::task::noop_waker_ref();
        let mut context = Context::from_waker(waker);
        match Future::poll(decode.as_mut(), &mut context) {
            Poll::Ready(Ok(value)) => assert_eq!(value, 1234),
            Poll::Ready(Err(error)) => panic!("decode failed: {error:?}"),
            Poll::Pending => panic!("decode must be ready"),
        }

        let repoll = catch_unwind(AssertUnwindSafe(|| {
            let _ = Future::poll(decode.as_mut(), &mut context);
        }));
        assert!(
            repoll.is_err(),
            "completed decode future must fail fast on post-Ready poll"
        );
        drop(decode);
        assert!(transport.queue_is_empty());
    });
}

#[test]
fn offer_headerless_demux_frame_fails_closed_not_pending() {
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
        assert_eq!(error.operation(), "offer");
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
fn offer_materialized_label_mismatch_fails_closed() {
    with_runtime_workspace(|slab| {
        let transport = LabelRewriteTransport::new(1);
        with_resident_tls_ref(&LABEL_REWRITE_SESSION_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let rv = cluster
                .rendezvous(config, transport.clone())
                .expect("register rendezvous");
            let mut tap = rv.tap();
            let sid = SessionId::new(903);
            let controller_program = resolved_controller_program();
            let worker_program = resolved_worker_program();
            rv.role(&controller_program)
                .set_resolver(ResolverRef::<MATERIALIZED_MISMATCH_RESOLVER>::decision_fn(
                    choose_left,
                ))
                .expect("install controller resolver");
            rv.role(&worker_program)
                .set_resolver(ResolverRef::<MATERIALIZED_MISMATCH_RESOLVER>::decision_fn(
                    choose_left,
                ))
                .expect("install worker resolver");
            let mut controller = rv
                .session(sid)
                .role(&controller_program)
                .enter()
                .expect("attach controller");
            let mut worker = rv
                .session(sid)
                .role(&worker_program)
                .enter()
                .expect("attach worker");

            futures::executor::block_on(send_left(&mut controller, 1234));
            let error = match worker
                .offer()
                .now_or_never()
                .expect("materialized label mismatch must fail closed")
            {
                Ok(_) => panic!("materialized label mismatch must fail closed"),
                Err(error) => error,
            };
            assert_eq!(error.operation(), "offer");
            let rendered = format!("{error:?}");
            assert!(
                rendered.contains("PhaseInvariant"),
                "label mismatch must report terminal invariant evidence: {rendered}"
            );

            let mismatch = tap
                .find(|event| event.id == hibana::runtime::tap::TRANSPORT_MISMATCH)
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
fn codec_error_in_public_decode_poisons_same_generation() {
    with_route_workspace(|controller, worker, _transport| {
        futures::executor::block_on(async {
            send_left(controller, 1234).await;
            let branch = worker.offer().await.expect("offer left arm");
            let decode_result = branch.decode::<Msg<71, u64>>().await;
            let err = match decode_result {
                Ok(_) => panic!("wrong payload shape must fail decode"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "decode");
            assert!(
                format!("{err:?}").contains("Codec"),
                "wrong payload shape must preserve codec evidence: {err:?}"
            );

            let err = match worker.offer().await {
                Ok(_) => panic!("poisoned generation must not re-offer the route arm"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "offer");
        });
    });
}

#[test]
fn forgotten_decode_future_leaves_endpoint_fail_closed() {
    with_route_workspace(|controller, worker, _transport| {
        futures::executor::block_on(async {
            send_left(controller, 1234).await;
            let branch = worker.offer().await.expect("offer left arm");
            let decode = branch.decode::<Msg<71, u32>>();
            core::mem::forget(decode);

            let err = match worker.offer().await {
                Ok(_) => panic!("forgotten decode future must leave endpoint fail-closed"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "offer");
        });
    });
}
