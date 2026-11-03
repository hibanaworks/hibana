mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use core::{
    cell::Cell,
    pin::pin,
    task::{Context, Poll},
};

use common::{TestRx, TestTransport, TestTx};
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

const SEND_LOGICAL: u8 = 10;

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

    fn set_ready(&self, ready: bool) {
        self.ready.set(ready);
    }
}

#[derive(Clone)]
struct PendingSendTransport<'a> {
    inner: TestTransport,
    state: &'a PendingSendState,
}

#[derive(Clone)]
struct DeferredCaptureTransport<'a> {
    inner: TestTransport,
    state: &'a PendingSendState,
}

impl Transport for DeferredCaptureTransport<'_> {
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

    fn poll_send<'f>(
        &self,
        tx: &mut TestTx,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if !self.state.ready.get() {
            return Poll::Pending;
        }
        self.inner.stage_send(
            tx,
            outgoing.target_role(),
            outgoing.lane(),
            outgoing.frame_label().raw(),
            outgoing.payload().as_bytes(),
        );
        self.inner.poll_send_staged(tx)
    }

    fn cancel_send(&self, tx: &mut TestTx) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut TestRx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv_current(rx, cx)
    }

    fn requeue<'a>(&self, rx: &mut TestRx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(rx)
    }
}

impl Transport for PendingSendTransport<'_> {
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

    fn poll_send<'f>(
        &self,
        tx: &mut TestTx,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
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

    fn cancel_send(&self, tx: &mut TestTx) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut TestRx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv_current(rx, cx)
    }

    fn requeue<'a>(&self, rx: &mut TestRx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(rx)
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn drop_unpolled_send_future_keeps_endpoint_on_same_send_step() {
    let protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
    let controller_program: RoleProgram<0> = project(&protocol);
    let worker_program: RoleProgram<1> = project(&protocol);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(401);
    let mut controller = rv.enter(sid, &controller_program).unwrap();
    let mut worker = rv.enter(sid, &worker_program).unwrap();

    let dropped_payload = 66u32;
    let send = controller.send::<Msg<SEND_LOGICAL, u32>>(&dropped_payload);
    drop(send);
    controller
        .send::<Msg<SEND_LOGICAL, u32>>(&77)
        .await
        .expect("send after dropped send future");
    let payload = worker
        .recv::<Msg<SEND_LOGICAL, u32>>()
        .await
        .expect("worker recv after dropped send future");
    assert_eq!(payload, 77);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn dropping_pending_send_future_keeps_endpoint_on_same_send_step() {
    let state = PendingSendState::new();
    let protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
    let controller_program: RoleProgram<0> = project(&protocol);
    let worker_program: RoleProgram<1> = project(&protocol);
    let transport = PendingSendTransport {
        inner: TestTransport::new(),
        state: &state,
    };
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, transport).unwrap();
    let sid = SessionId::new(402);
    let mut controller = rv.enter(sid, &controller_program).unwrap();
    let mut worker = rv.enter(sid, &worker_program).unwrap();

    let mut cx = Context::from_waker(core::task::Waker::noop());
    {
        let payload = 88u32;
        let mut send = pin!(controller.send::<Msg<SEND_LOGICAL, u32>>(&payload));
        assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
    }
    assert_eq!(state.polls.get(), 1, "pending send future should poll once");
    state.set_ready(true);
    controller
        .send::<Msg<SEND_LOGICAL, u32>>(&99)
        .await
        .expect("send after cancelled future");
    let payload = worker
        .recv::<Msg<SEND_LOGICAL, u32>>()
        .await
        .expect("worker recv after cancelled send future");
    assert_eq!(payload, 99);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn interleaved_pending_sends_reencode_from_their_own_payloads() {
    let state = PendingSendState::new();
    let protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
    let sender_program: RoleProgram<0> = project(&protocol);
    let receiver_program: RoleProgram<1> = project(&protocol);
    let transport = DeferredCaptureTransport {
        inner: TestTransport::new(),
        state: &state,
    };
    let mut slab = [0; 48 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, transport).unwrap();
    let mut sender_a = rv.enter(SessionId::new(0x410), &sender_program).unwrap();
    let mut receiver_a = rv.enter(SessionId::new(0x410), &receiver_program).unwrap();
    let mut sender_b = rv.enter(SessionId::new(0x411), &sender_program).unwrap();
    let mut receiver_b = rv.enter(SessionId::new(0x411), &receiver_program).unwrap();
    let payload_a = 0x1122_3344u32;
    let payload_b = 0xaabb_ccddu32;
    {
        let mut send_a = pin!(sender_a.send::<Msg<SEND_LOGICAL, u32>>(&payload_a));
        let mut send_b = pin!(sender_b.send::<Msg<SEND_LOGICAL, u32>>(&payload_b));
        let mut cx = Context::from_waker(core::task::Waker::noop());
        assert!(matches!(send_a.as_mut().poll(&mut cx), Poll::Pending));
        assert!(matches!(send_b.as_mut().poll(&mut cx), Poll::Pending));
        state.set_ready(true);
        assert!(matches!(send_a.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        assert!(matches!(send_b.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(
        receiver_a
            .recv::<Msg<SEND_LOGICAL, u32>>()
            .await
            .expect("receive A"),
        payload_a
    );
    assert_eq!(
        receiver_b
            .recv::<Msg<SEND_LOGICAL, u32>>()
            .await
            .expect("receive B"),
        payload_b
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn forgotten_started_send_future_leaves_send_fail_closed() {
    let state = PendingSendState::new();
    let protocol = g::send::<0, 1, Msg<SEND_LOGICAL, u32>>();
    let controller_program: RoleProgram<0> = project(&protocol);
    let worker_program: RoleProgram<1> = project(&protocol);
    let transport = PendingSendTransport {
        inner: TestTransport::new(),
        state: &state,
    };
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, transport).unwrap();
    let sid = SessionId::new(403);
    let mut controller = rv.enter(sid, &controller_program).unwrap();
    let worker = rv.enter(sid, &worker_program).unwrap();
    core::hint::black_box(&worker);

    let mut cx = Context::from_waker(core::task::Waker::noop());
    let payload = 88u32;
    let mut abandoned_send =
        core::mem::ManuallyDrop::new(controller.send::<Msg<SEND_LOGICAL, u32>>(&payload));
    {
        let mut pinned_send = core::pin::Pin::new(&mut *abandoned_send);
        assert!(matches!(pinned_send.as_mut().poll(&mut cx), Poll::Pending));
    }
    let err = controller
        .send::<Msg<SEND_LOGICAL, u32>>(&payload)
        .await
        .expect_err("forgotten started send future must reject the same send");
    assert!(format!("{err:?}").contains("operation: \"send\""));
    assert!(
        format!("{err:?}").contains("PhaseInvariant"),
        "busy endpoint must report phase invariant evidence: {err:?}"
    );
}
