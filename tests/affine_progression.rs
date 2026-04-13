#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    mem::MaybeUninit,
    pin::{Pin, pin},
    task::{Context, Poll},
};

use common::{
    RecvFuture, SendFuture, TestRx, TestTransport, TestTransportError, TestTransportMetrics, TestTx,
};
use futures::task::noop_waker_ref;
use hibana::{
    g,
    g::advanced::steps::{SendStep, StepCons, StepNil},
    g::advanced::{RoleProgram, project},
    g::{Msg, Role},
    substrate::{
        SessionId, SessionKit, Transport,
        binding::NoBinding,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
        transport::{Outgoing, TransportEvent},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

const LABEL_SEND: u8 = 10;

type SendOnly<const LABEL: u8, S, D, P> = StepCons<SendStep<S, D, Msg<LABEL, P>, 0>, StepNil>;
type TestKit<T> = SessionKit<'static, T, DefaultLabelUniverse, CounterClock, 2>;

const SEND_PROTOCOL: g::Program<SendOnly<LABEL_SEND, Role<0>, Role<1>, u32>> =
    g::send::<Role<0>, Role<1>, Msg<LABEL_SEND, u32>, 0>();

static CONTROLLER_SEND_PROGRAM: RoleProgram<
    'static,
    0,
    SendOnly<LABEL_SEND, Role<0>, Role<1>, u32>,
> = project(&SEND_PROTOCOL);

static WORKER_SEND_PROGRAM: RoleProgram<'static, 1, SendOnly<LABEL_SEND, Role<0>, Role<1>, u32>> =
    project(&SEND_PROTOCOL);

struct PendingSendState {
    ready: Cell<bool>,
    polls: Cell<usize>,
}

impl PendingSendState {
    const fn new() -> Self {
        Self {
            ready: Cell::new(false),
            polls: Cell::new(0),
        }
    }

    fn reset(&self) {
        self.ready.set(false);
        self.polls.set(0);
    }

    fn set_ready(&self, ready: bool) {
        self.ready.set(ready);
    }
}

std::thread_local! {
    static PENDING_SEND_STATE: PendingSendState = const { PendingSendState::new() };
    static TEST_KIT_SLOT: UnsafeCell<MaybeUninit<TestKit<TestTransport>>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static PENDING_SEND_KIT_SLOT: UnsafeCell<MaybeUninit<TestKit<PendingSendTransport>>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
}

#[derive(Clone)]
struct PendingSendTransport {
    inner: TestTransport,
    state: &'static PendingSendState,
}

struct PendingSendFuture<'a> {
    state: &'a PendingSendState,
    inner: SendFuture<'a>,
}

impl Future for PendingSendFuture<'_> {
    type Output = Result<(), TestTransportError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.state.polls.set(this.state.polls.get().wrapping_add(1));
        if !this.state.ready.get() {
            return Poll::Pending;
        }
        Pin::new(&mut this.inner).poll(cx)
    }
}

impl Transport for PendingSendTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx<'a>
    where
        Self: 'a;
    type Send<'a>
        = PendingSendFuture<'a>
    where
        Self: 'a;
    type Recv<'a>
        = RecvFuture<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(local_role, session_id)
    }

    fn send<'a, 'f>(&'a self, tx: &'a mut Self::Tx<'a>, outgoing: Outgoing<'f>) -> Self::Send<'a>
    where
        'a: 'f,
    {
        PendingSendFuture {
            state: self.state,
            inner: self.inner.send(tx, outgoing),
        }
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        self.inner.recv(rx)
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        self.inner.requeue(rx)
    }

    fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent)) {
        self.inner.drain_events(emit)
    }

    fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8> {
        self.inner.recv_label_hint(rx)
    }

    fn metrics(&self) -> Self::Metrics {
        self.inner.metrics()
    }

    fn apply_pacing_update(&self, interval_us: u32, burst_bytes: u16) {
        self.inner.apply_pacing_update(interval_us, burst_bytes)
    }
}

#[test]
fn drop_flow_keeps_endpoint_on_same_send_step() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &TEST_KIT_SLOT,
            |ptr| unsafe {
                ptr.write(TestKit::<TestTransport>::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(tap_buf, slab),
                        TestTransport::default(),
                    )
                    .expect("register rendezvous");
                let sid = SessionId::new(401);

                let mut controller = cluster
                    .enter(rv_id, sid, &CONTROLLER_SEND_PROGRAM, NoBinding)
                    .expect("attach controller");
                let mut worker = cluster
                    .enter(rv_id, sid, &WORKER_SEND_PROGRAM, NoBinding)
                    .expect("attach worker");

                futures::executor::block_on(async {
                    let flow = controller
                        .flow::<Msg<LABEL_SEND, u32>>()
                        .expect("initial flow preview");
                    drop(flow);

                    let outcome = controller
                        .flow::<Msg<LABEL_SEND, u32>>()
                        .expect("flow must remain available after drop")
                        .send(&77)
                        .await
                        .expect("send after dropped flow");
                    assert!(outcome.is_none());

                    let payload = worker
                        .recv::<Msg<LABEL_SEND, u32>>()
                        .await
                        .expect("worker recv after dropped flow");
                    assert_eq!(payload, 77);
                });
            },
        );
    });
}

#[test]
fn dropping_pending_send_future_keeps_endpoint_on_same_send_step() {
    PENDING_SEND_STATE.with(|state| {
        state.reset();
        let state: &'static PendingSendState = unsafe { &*(state as *const PendingSendState) };

        with_fixture(|clock, tap_buf, slab| {
            with_tls_ref(
                &PENDING_SEND_KIT_SLOT,
                |ptr| unsafe {
                    ptr.write(TestKit::<PendingSendTransport>::new(clock));
                },
                |cluster| {
                    let transport = PendingSendTransport {
                        inner: TestTransport::default(),
                        state,
                    };
                    let rv_id = cluster
                        .add_rendezvous_from_config(Config::new(tap_buf, slab), transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(402);

                    let mut controller = cluster
                        .enter(rv_id, sid, &CONTROLLER_SEND_PROGRAM, NoBinding)
                        .expect("attach controller");
                    let mut worker = cluster
                        .enter(rv_id, sid, &WORKER_SEND_PROGRAM, NoBinding)
                        .expect("attach worker");

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);
                    {
                        let flow = controller
                            .flow::<Msg<LABEL_SEND, u32>>()
                            .expect("initial flow preview");
                        let mut send = pin!(flow.send(&88));
                        assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
                        drop(send);
                    }
                    assert_eq!(state.polls.get(), 1, "pending send future should poll once");

                    state.set_ready(true);
                    futures::executor::block_on(async {
                        let outcome = controller
                            .flow::<Msg<LABEL_SEND, u32>>()
                            .expect("flow must remain available after cancellation")
                            .send(&99)
                            .await
                            .expect("send after cancelled future");
                        assert!(outcome.is_none());

                        let payload = worker
                            .recv::<Msg<LABEL_SEND, u32>>()
                            .await
                            .expect("worker recv after cancelled send future");
                        assert_eq!(payload, 99);
                    });
                },
            );
        });
    });
}
