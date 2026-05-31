#![cfg(feature = "std")]

use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use hibana::{
    g::{self, Msg},
    integration::{
        SessionKitStorage,
        cap::control::{LoopBreakKind, LoopContinueKind},
        ids::SessionId,
        program::{RoleProgram, project},
        runtime::{Config, CounterClock, DefaultLabelUniverse},
        transport::{FrameLabel, Outgoing, Transport, TransportError},
        wire::Payload,
    },
};

const FRAME_BYTES: usize = 64;
const ROLE_COUNT: usize = 2;
const LOOP_CONTINUE_LABEL: u8 = 120;
const LOOP_BREAK_LABEL: u8 = 121;
const FIRST_LABEL: u8 = 85;
const FIRST_RET_LABEL: u8 = 86;
const SECOND_LABEL: u8 = 95;
const SECOND_RET_LABEL: u8 = 96;

#[derive(Clone, Copy)]
struct Frame {
    label: FrameLabel,
    len: usize,
    bytes: [u8; FRAME_BYTES],
}

impl Frame {
    fn new(label: FrameLabel, payload: Payload<'_>) -> Self {
        let bytes = payload.as_bytes();
        assert!(bytes.len() <= FRAME_BYTES);
        let mut out = [0; FRAME_BYTES];
        out[..bytes.len()].copy_from_slice(bytes);
        Self {
            label,
            len: bytes.len(),
            bytes: out,
        }
    }

    fn payload(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

struct Queues {
    by_role: [VecDeque<Frame>; ROLE_COUNT],
}

impl Queues {
    fn new() -> Self {
        Self {
            by_role: std::array::from_fn(|_| VecDeque::new()),
        }
    }
}

struct QueueStore {
    queues: UnsafeCell<Queues>,
}

impl QueueStore {
    fn new() -> Self {
        Self {
            queues: UnsafeCell::new(Queues::new()),
        }
    }

    fn edit<R>(&self, f: impl FnOnce(&mut Queues) -> R) -> R {
        unsafe { f(&mut *self.queues.get()) }
    }
}

#[derive(Clone)]
struct HintTransport {
    queues: Rc<QueueStore>,
}

impl HintTransport {
    fn new() -> Self {
        Self {
            queues: Rc::new(QueueStore::new()),
        }
    }
}

#[derive(Clone, Copy)]
struct HintTx {
    local_role: u8,
}

struct HintRx {
    local_role: u8,
    hint: Cell<Option<FrameLabel>>,
    current_label: Option<FrameLabel>,
    len: usize,
    bytes: [u8; FRAME_BYTES],
}

impl Transport for HintTransport {
    type Error = TransportError;
    type Tx<'a>
        = HintTx
    where
        Self: 'a;
    type Rx<'a>
        = HintRx
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        (
            HintTx { local_role },
            HintRx {
                local_role,
                hint: Cell::new(None),
                current_label: None,
                len: 0,
                bytes: [0; FRAME_BYTES],
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        assert_ne!(tx.local_role, outgoing.peer());
        let peer = outgoing.peer() as usize;
        self.queues.edit(|queues| {
            queues.by_role[peer].push_back(Frame::new(outgoing.frame_label(), outgoing.payload()))
        });
        cx.waker().wake_by_ref();
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        let role = rx.local_role as usize;
        let Some(frame) = self.queues.edit(|queues| queues.by_role[role].pop_front()) else {
            return Poll::Pending;
        };
        rx.current_label = Some(frame.label);
        rx.hint.set(Some(frame.label));
        rx.len = frame.len;
        rx.bytes[..frame.len].copy_from_slice(frame.payload());
        cx.waker().wake_by_ref();
        Poll::Ready(Ok(Payload::new(&rx.bytes[..rx.len])))
    }

    fn cancel_send<'a>(&self, _: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        if let Some(label) = rx.current_label.take() {
            let role = rx.local_role as usize;
            let frame = Frame::new(label, Payload::new(&rx.bytes[..rx.len]));
            self.queues
                .edit(|queues| queues.by_role[role].push_front(frame));
        }
        rx.hint.set(None);
        Ok(())
    }

    fn recv_frame_hint<'a>(&self, rx: &mut Self::Rx<'a>) -> Option<FrameLabel> {
        rx.hint.take()
    }
}

fn noop_waker() -> Waker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }

    unsafe fn wake(_: *const ()) {}

    unsafe fn wake_by_ref(_: *const ()) {}

    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}

#[test]
fn no_policy_static_route_uses_descriptor_checked_transport_hint() {
    let program = g::route(
        g::seq(
            g::send::<1, 1, Msg<{ LOOP_CONTINUE_LABEL }, (), LoopContinueKind>, 1>(),
            g::seq(
                g::send::<1, 0, Msg<FIRST_LABEL, u32>, 1>(),
                g::seq(
                    g::send::<0, 1, Msg<FIRST_RET_LABEL, u32>, 1>(),
                    g::seq(
                        g::send::<1, 0, Msg<SECOND_LABEL, u32>, 1>(),
                        g::send::<0, 1, Msg<SECOND_RET_LABEL, u32>, 1>(),
                    ),
                ),
            ),
        ),
        g::send::<1, 1, Msg<{ LOOP_BREAK_LABEL }, (), LoopBreakKind>, 1>(),
    );
    let driver_program: RoleProgram<0> = project(&program);
    let engine_program: RoleProgram<1> = project(&program);
    let mut tap0 = [hibana::integration::runtime::TapEvent::zero(); 128];
    let mut tap1 = [hibana::integration::runtime::TapEvent::zero(); 128];
    let mut slab0 = [0u8; 256 * 1024];
    let mut slab1 = [0u8; 256 * 1024];
    let clock0 = CounterClock::new();
    let clock1 = CounterClock::new();
    let transport = HintTransport::new();
    let mut driver_kit_storage =
        SessionKitStorage::<HintTransport, DefaultLabelUniverse, CounterClock, 1>::uninit();
    let mut engine_kit_storage =
        SessionKitStorage::<HintTransport, DefaultLabelUniverse, CounterClock, 1>::uninit();
    let driver_kit = driver_kit_storage.init();
    let engine_kit = engine_kit_storage.init();
    let driver_rv = driver_kit
        .add_rendezvous_from_config(
            Config::from_resources((&mut tap0, &mut slab0), clock0),
            transport.clone(),
        )
        .expect("driver rendezvous");
    let engine_rv = engine_kit
        .add_rendezvous_from_config(
            Config::from_resources((&mut tap1, &mut slab1), clock1),
            transport,
        )
        .expect("engine rendezvous");
    let session = SessionId::new(0x5400);
    let mut driver = driver_kit
        .rendezvous(driver_rv)
        .session(session)
        .role(&driver_program)
        .enter()
        .expect("driver endpoint");
    let mut engine = engine_kit
        .rendezvous(engine_rv)
        .session(session)
        .role(&engine_program)
        .enter()
        .expect("engine endpoint");

    futures::executor::block_on(
        engine
            .flow::<Msg<{ LOOP_CONTINUE_LABEL }, (), LoopContinueKind>>()
            .expect("continue flow")
            .send(&()),
    )
    .expect("send continue");
    futures::executor::block_on(
        engine
            .flow::<Msg<FIRST_LABEL, u32>>()
            .expect("first flow")
            .send(&10),
    )
    .expect("send first request");

    let mut offer = Box::pin(driver.offer());
    let waker = noop_waker();
    let mut task_context = Context::from_waker(&waker);
    let mut branch = None;
    for attempt in 0..16 {
        match Future::poll(offer.as_mut(), &mut task_context) {
            Poll::Pending => {}
            Poll::Ready(Ok(ready_branch)) => {
                branch = Some(ready_branch);
                break;
            }
            Poll::Ready(Err(error)) => panic!("offer failed: {error:?}"),
        }
        assert!(
            attempt < 15,
            "offer did not complete from transport-observed frame hint"
        );
    }
    let branch = branch.expect("offer should complete from transport-observed frame hint");
    let repoll = catch_unwind(AssertUnwindSafe(|| {
        let _ = Future::poll(offer.as_mut(), &mut task_context);
    }));
    assert!(
        repoll.is_err(),
        "completed offer future must fail fast on post-Ready poll"
    );
    drop(offer);
    assert_eq!(branch.label(), FIRST_LABEL);
    let mut decode = Box::pin(branch.decode::<Msg<FIRST_LABEL, u32>>());
    let first = match Future::poll(decode.as_mut(), &mut task_context) {
        Poll::Ready(Ok(value)) => value,
        Poll::Ready(Err(error)) => panic!("decode first failed: {error:?}"),
        Poll::Pending => panic!("decode first must be ready"),
    };
    let decode_repoll = catch_unwind(AssertUnwindSafe(|| {
        let _ = Future::poll(decode.as_mut(), &mut task_context);
    }));
    assert!(
        decode_repoll.is_err(),
        "completed decode future must fail fast on post-Ready poll"
    );
    drop(decode);
    assert_eq!(first, 10);

    futures::executor::block_on(
        driver
            .flow::<Msg<FIRST_RET_LABEL, u32>>()
            .expect("first ret")
            .send(&11),
    )
    .expect("send first reply");
    let first_reply = futures::executor::block_on(engine.recv::<Msg<FIRST_RET_LABEL, u32>>())
        .expect("first reply");
    assert_eq!(first_reply, 11);

    futures::executor::block_on(
        engine
            .flow::<Msg<SECOND_LABEL, u32>>()
            .expect("second flow")
            .send(&20),
    )
    .expect("send second request");
    let second =
        futures::executor::block_on(driver.recv::<Msg<SECOND_LABEL, u32>>()).expect("recv second");
    assert_eq!(second, 20);

    futures::executor::block_on(
        driver
            .flow::<Msg<SECOND_RET_LABEL, u32>>()
            .expect("second ret")
            .send(&21),
    )
    .expect("send second reply");
    let second_reply = futures::executor::block_on(engine.recv::<Msg<SECOND_RET_LABEL, u32>>())
        .expect("second reply");
    assert_eq!(second_reply, 21);
}
