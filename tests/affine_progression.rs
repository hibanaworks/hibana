mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::{Cell, UnsafeCell},
    pin::pin,
    task::{Context, Poll},
};

use common::{TestRx, TestTransport, TestTx};
use futures::task::noop_waker_ref;
use hibana::{
    g,
    g::Msg,
    runtime::program::{RoleProgram, project},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        transport::{Outgoing, ReceivedFrame, Transport, TransportError},
    },
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

const SEND_LOGICAL: u8 = 10;

type TestKitStorage<T> = SessionKitStorage<'static, T>;

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
    static TEST_KIT_SLOT: UnsafeCell<TestKitStorage<TestTransport>> =
        const { UnsafeCell::new(SessionKitStorage::uninit()) };
    static PENDING_SEND_KIT_SLOT: UnsafeCell<TestKitStorage<PendingSendTransport>> =
        const { UnsafeCell::new(SessionKitStorage::uninit()) };
}

#[derive(Clone)]
struct PendingSendTransport {
    inner: TestTransport,
    state: &'static PendingSendState,
}

impl Transport for PendingSendTransport {
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx<'a>
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        self.inner.stage_send(
            tx,
            outgoing.target_role(),
            outgoing.lane(),
            outgoing.frame_label().raw(),
            outgoing.payload().as_bytes(),
        );
        if !self.state.ready.get() {
            return Poll::Pending;
        }
        let _ = outgoing;
        self.inner.poll_send_staged(tx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv_current(rx, cx)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(rx)
    }
}

#[test]
fn drop_unpolled_send_future_keeps_endpoint_on_same_send_step() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&TEST_KIT_SLOT, |cluster| {
            let send_protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
            let controller_send_program: RoleProgram<0> = project(&send_protocol);
            let worker_send_program: RoleProgram<1> = project(&send_protocol);
            let rv = cluster
                .rendezvous(slab, TestTransport::new())
                .expect("register rendezvous");
            let sid = SessionId::new(401);

            let mut controller = rv
                .session(sid)
                .role(&controller_send_program)
                .enter()
                .expect("attach controller");
            let mut worker = rv
                .session(sid)
                .role(&worker_send_program)
                .enter()
                .expect("attach worker");

            futures::executor::block_on(async {
                let dropped_payload = 66u32;
                let send = controller.send::<Msg<SEND_LOGICAL, u32>>(&dropped_payload);
                drop(send);

                let () = controller
                    .send::<Msg<SEND_LOGICAL, u32>>(&77)
                    .await
                    .expect("send after dropped send future");

                let payload = worker
                    .recv::<Msg<SEND_LOGICAL, u32>>()
                    .await
                    .expect("worker recv after dropped send future");
                assert_eq!(payload, 77);
            });
        });
    });
}

#[test]
fn dropping_pending_send_future_keeps_endpoint_on_same_send_step() {
    PENDING_SEND_STATE.with(|state| {
        state.reset();
        let state: &'static PendingSendState = unsafe { &*(state as *const PendingSendState) };

        with_runtime_workspace(|slab| {
            with_resident_tls_ref(&PENDING_SEND_KIT_SLOT, |cluster| {
                let send_protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
                let controller_send_program: RoleProgram<0> = project(&send_protocol);
                let worker_send_program: RoleProgram<1> = project(&send_protocol);
                let transport = PendingSendTransport {
                    inner: TestTransport::new(),
                    state,
                };
                let rv = cluster
                    .rendezvous(slab, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(402);

                let mut controller = rv
                    .session(sid)
                    .role(&controller_send_program)
                    .enter()
                    .expect("attach controller");
                let mut worker = rv
                    .session(sid)
                    .role(&worker_send_program)
                    .enter()
                    .expect("attach worker");

                let waker = noop_waker_ref();
                let mut cx = Context::from_waker(waker);
                {
                    let payload = 88u32;
                    let mut send = pin!(controller.send::<Msg<SEND_LOGICAL, u32>>(&payload));
                    assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
                }
                assert_eq!(state.polls.get(), 1, "pending send future should poll once");

                state.set_ready(true);
                futures::executor::block_on(async {
                    let () = controller
                        .send::<Msg<SEND_LOGICAL, u32>>(&99)
                        .await
                        .expect("send after cancelled future");

                    let payload = worker
                        .recv::<Msg<SEND_LOGICAL, u32>>()
                        .await
                        .expect("worker recv after cancelled send future");
                    assert_eq!(payload, 99);
                });
            });
        });
    });
}

#[test]
fn forgotten_started_send_future_leaves_send_fail_closed() {
    PENDING_SEND_STATE.with(|state| {
        state.reset();
        let state: &'static PendingSendState = unsafe { &*(state as *const PendingSendState) };

        with_runtime_workspace(|slab| {
            with_resident_tls_ref(&PENDING_SEND_KIT_SLOT, |cluster| {
                let send_protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
                let controller_send_program: RoleProgram<0> = project(&send_protocol);
                let worker_send_program: RoleProgram<1> = project(&send_protocol);
                let transport = PendingSendTransport {
                    inner: TestTransport::new(),
                    state,
                };
                let rv = cluster
                    .rendezvous(slab, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(403);

                let mut controller = rv
                    .session(sid)
                    .role(&controller_send_program)
                    .enter()
                    .expect("attach controller");
                let worker = rv
                    .session(sid)
                    .role(&worker_send_program)
                    .enter()
                    .expect("attach worker");
                core::hint::black_box(&worker);

                let waker = noop_waker_ref();
                let mut cx = Context::from_waker(waker);
                let payload = 88u32;
                let mut send = Box::pin(controller.send::<Msg<SEND_LOGICAL, u32>>(&payload));
                assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
                core::mem::forget(send);

                let err = futures::executor::block_on(
                    controller.send::<Msg<SEND_LOGICAL, u32>>(&payload),
                )
                .expect_err("forgotten started send future must reject the same send");
                assert!(format!("{err:?}").contains("operation: \"send\""));
                assert!(
                    format!("{err:?}").contains("PhaseInvariant"),
                    "busy endpoint must report phase invariant evidence: {err:?}"
                );
            });
        });
    });
}
