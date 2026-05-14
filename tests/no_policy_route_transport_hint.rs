#![cfg(feature = "std")]

use core::{
    cell::Cell,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use hibana::{
    g::{self, Msg, Role},
    substrate::{
        SessionKit, Transport,
        binding::NoBinding,
        cap::{
            GenericCapToken,
            advanced::{LoopBreakKind, LoopContinueKind},
        },
        ids::SessionId,
        program::{RoleProgram, project},
        runtime::{Config, CounterClock, DefaultLabelUniverse},
        transport::{FrameLabel, Outgoing, TransportError},
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

type LoopContinue =
    Msg<{ LOOP_CONTINUE_LABEL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>;
type LoopBreak = Msg<{ LOOP_BREAK_LABEL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>;
type First = Msg<FIRST_LABEL, u32>;
type FirstRet = Msg<FIRST_RET_LABEL, u32>;
type Second = Msg<SECOND_LABEL, u32>;
type SecondRet = Msg<SECOND_RET_LABEL, u32>;

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

#[derive(Clone)]
struct HintTransport {
    queues: Arc<Mutex<Queues>>,
}

impl HintTransport {
    fn new() -> Self {
        Self {
            queues: Arc::new(Mutex::new(Queues::new())),
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
    type Metrics = ();

    fn open<'a>(&'a self, local_role: u8, _: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
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
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        assert_ne!(tx.local_role, outgoing.peer());
        let peer = outgoing.peer() as usize;
        self.queues.lock().expect("test queue lock").by_role[peer]
            .push_back(Frame::new(outgoing.frame_label(), outgoing.payload()));
        cx.waker().wake_by_ref();
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        let role = rx.local_role as usize;
        let Some(frame) = self.queues.lock().expect("test queue lock").by_role[role].pop_front()
        else {
            return Poll::Pending;
        };
        rx.current_label = Some(frame.label);
        rx.hint.set(Some(frame.label));
        rx.len = frame.len;
        rx.bytes[..frame.len].copy_from_slice(frame.payload());
        cx.waker().wake_by_ref();
        Poll::Ready(Ok(Payload::new(&rx.bytes[..rx.len])))
    }

    fn cancel_send<'a>(&'a self, _: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        if let Some(label) = rx.current_label.take() {
            let role = rx.local_role as usize;
            let frame = Frame::new(label, Payload::new(&rx.bytes[..rx.len]));
            self.queues.lock().expect("test queue lock").by_role[role].push_front(frame);
        }
        rx.hint.set(None);
    }

    fn drain_events(
        &self,
        _: &mut dyn FnMut(hibana::substrate::transport::advanced::TransportEvent),
    ) {
    }

    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<FrameLabel> {
        rx.hint.take()
    }

    fn metrics(&self) -> Self::Metrics {}

    fn apply_pacing_update(&self, _: u32, _: u16) {}
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

fn poll_bounded<F>(future: F, rounds: u8) -> Option<F::Output>
where
    F: core::future::Future,
{
    let waker = noop_waker();
    let mut task_context = Context::from_waker(&waker);
    let mut future = core::pin::pin!(future);
    let mut poll_round = 0;
    while poll_round < rounds {
        if let Poll::Ready(output) = future.as_mut().poll(&mut task_context) {
            return Some(output);
        }
        poll_round += 1;
    }
    None
}

#[test]
fn no_policy_static_route_uses_descriptor_checked_transport_hint() {
    let program = g::route(
        g::seq(
            g::send::<Role<1>, Role<1>, LoopContinue, 1>(),
            g::seq(
                g::send::<Role<1>, Role<0>, First, 1>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, FirstRet, 1>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, Second, 1>(),
                        g::send::<Role<0>, Role<1>, SecondRet, 1>(),
                    ),
                ),
            ),
        ),
        g::send::<Role<1>, Role<1>, LoopBreak, 1>(),
    );
    let driver_program: RoleProgram<0> = project(&program);
    let engine_program: RoleProgram<1> = project(&program);
    let mut tap0 = [hibana::substrate::tap::TapEvent::zero(); 128];
    let mut tap1 = [hibana::substrate::tap::TapEvent::zero(); 128];
    let mut slab0 = [0u8; 256 * 1024];
    let mut slab1 = [0u8; 256 * 1024];
    let clock0 = CounterClock::new();
    let clock1 = CounterClock::new();
    let transport = HintTransport::new();
    let driver_kit =
        SessionKit::<HintTransport, DefaultLabelUniverse, CounterClock, 1>::new(&clock0);
    let engine_kit =
        SessionKit::<HintTransport, DefaultLabelUniverse, CounterClock, 1>::new(&clock1);
    let driver_rv = driver_kit
        .add_rendezvous_from_config(
            Config::new(&mut tap0, &mut slab0, 0..8, 1, CounterClock::new()),
            transport.clone(),
        )
        .expect("driver rendezvous");
    let engine_rv = engine_kit
        .add_rendezvous_from_config(
            Config::new(&mut tap1, &mut slab1, 0..8, 1, CounterClock::new()),
            transport,
        )
        .expect("engine rendezvous");
    let session = SessionId::new(0x5400);
    let mut driver = driver_kit
        .enter(driver_rv, session, &driver_program, NoBinding)
        .expect("driver endpoint");
    let mut engine = engine_kit
        .enter(engine_rv, session, &engine_program, NoBinding)
        .expect("engine endpoint");

    futures::executor::block_on(
        engine
            .flow::<LoopContinue>()
            .expect("continue flow")
            .send(()),
    )
    .expect("send continue");
    futures::executor::block_on(engine.flow::<First>().expect("first flow").send(&10))
        .expect("send first request");

    let branch = poll_bounded(driver.offer(), 16)
        .expect("offer should complete from transport-observed frame hint")
        .expect("offer should succeed");
    assert_eq!(branch.label(), FIRST_LABEL);
    let first = futures::executor::block_on(branch.decode::<First>()).expect("decode first");
    assert_eq!(first, 10);

    futures::executor::block_on(driver.flow::<FirstRet>().expect("first ret").send(&11))
        .expect("send first reply");
    let first_reply = futures::executor::block_on(engine.recv::<FirstRet>()).expect("first reply");
    assert_eq!(first_reply, 11);

    futures::executor::block_on(engine.flow::<Second>().expect("second flow").send(&20))
        .expect("send second request");
    let second = futures::executor::block_on(driver.recv::<Second>()).expect("recv second");
    assert_eq!(second, 20);

    futures::executor::block_on(driver.flow::<SecondRet>().expect("second ret").send(&21))
        .expect("send second reply");
    let second_reply =
        futures::executor::block_on(engine.recv::<SecondRet>()).expect("second reply");
    assert_eq!(second_reply, 21);
}
