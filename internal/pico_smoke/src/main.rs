#![no_std]
#![no_main]

#[cfg(feature = "fanout-heavy")]
mod fanout_program;
#[cfg(all(not(feature = "linear-heavy"), not(feature = "fanout-heavy")))]
mod huge_program;
#[cfg(feature = "linear-heavy")]
mod linear_program;
mod localside;
#[cfg(not(feature = "linear-heavy"))]
mod route_control_kinds;

use core::{
    arch::asm,
    cell::UnsafeCell,
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use hibana::{
    Endpoint,
    g,
    g::advanced::project,
    substrate::{
        SessionId, SessionKit, Transport,
        binding::NoBinding,
        cap::advanced::MintConfig,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
        tap::TapEvent,
        transport::{Outgoing, TransportError, TransportEvent},
        wire::Payload,
    },
};

#[cfg(all(feature = "linear-heavy", feature = "fanout-heavy"))]
compile_error!("pico smoke accepts at most one alternate huge choreography shape feature");

#[cfg(feature = "linear-heavy")]
use linear_program as sample_program;
#[cfg(feature = "fanout-heavy")]
use fanout_program as sample_program;
#[cfg(all(not(feature = "linear-heavy"), not(feature = "fanout-heavy")))]
use huge_program as sample_program;

const RING_EVENTS: usize = 128;
const SLAB_BYTES: usize = 32_768;
const STACK_CANARY_WORD: u32 = 0xC0DE_CAFE;

const QUEUE_CAPACITY: usize = 16;
const PAYLOAD_CAPACITY: usize = 96;

type PicoKit = SessionKit<'static, PicoTransport, DefaultLabelUniverse, CounterClock, 1>;
type ControllerEndpoint = Endpoint<'static, 0, PicoKit>;
type WorkerEndpoint = Endpoint<'static, 1, PicoKit>;

const PROGRAM: g::Program<sample_program::ProgramSteps> = sample_program::PROGRAM;
static CONTROLLER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 0, MintConfig> =
    project(&PROGRAM);
static WORKER_PROGRAM: hibana::g::advanced::RoleProgram<'static, 1, MintConfig> =
    project(&PROGRAM);

#[derive(Clone, Copy)]
struct FrameOwned {
    len: usize,
    payload: [u8; PAYLOAD_CAPACITY],
}

impl FrameOwned {
    const fn empty() -> Self {
        Self {
            len: 0,
            payload: [0; PAYLOAD_CAPACITY],
        }
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() > PAYLOAD_CAPACITY {
            panic!("pico smoke payload exceeds fixed capacity");
        }
        let mut payload = [0u8; PAYLOAD_CAPACITY];
        payload[..bytes.len()].copy_from_slice(bytes);
        Self {
            len: bytes.len(),
            payload,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.payload[..self.len]
    }
}

#[derive(Clone, Copy)]
struct FixedQueue {
    items: [FrameOwned; QUEUE_CAPACITY],
    head: usize,
    len: usize,
}

impl FixedQueue {
    const fn new() -> Self {
        Self {
            items: [FrameOwned::empty(); QUEUE_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn push_back(&mut self, item: FrameOwned) {
        if self.len >= QUEUE_CAPACITY {
            panic!("pico smoke transport queue capacity exceeded");
        }
        let idx = (self.head + self.len) % QUEUE_CAPACITY;
        self.items[idx] = item;
        self.len += 1;
    }

    fn push_front(&mut self, item: FrameOwned) {
        if self.len >= QUEUE_CAPACITY {
            panic!("pico smoke transport queue capacity exceeded");
        }
        self.head = if self.head == 0 {
            QUEUE_CAPACITY - 1
        } else {
            self.head - 1
        };
        self.items[self.head] = item;
        self.len += 1;
    }

    fn pop_front(&mut self) -> Option<FrameOwned> {
        if self.len == 0 {
            return None;
        }
        let idx = self.head;
        self.head = (self.head + 1) % QUEUE_CAPACITY;
        self.len -= 1;
        Some(self.items[idx])
    }
}

#[derive(Clone, Copy)]
struct RoleState {
    queue: FixedQueue,
}

impl RoleState {
    const fn new() -> Self {
        Self {
            queue: FixedQueue::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct PicoTransportState {
    roles: [RoleState; 2],
}

impl PicoTransportState {
    const fn new() -> Self {
        Self {
            roles: [RoleState::new(), RoleState::new()],
        }
    }

    fn role_mut(&mut self, role: u8) -> &mut RoleState {
        match role {
            0 | 1 => &mut self.roles[role as usize],
            _ => panic!("pico smoke transport role out of range"),
        }
    }

    fn role(&self, role: u8) -> &RoleState {
        match role {
            0 | 1 => &self.roles[role as usize],
            _ => panic!("pico smoke transport role out of range"),
        }
    }
}

struct SmokeStorage {
    clock: CounterClock,
    tap_storage: [TapEvent; RING_EVENTS],
    slab_storage: [u8; SLAB_BYTES],
    session_storage: MaybeUninit<PicoKit>,
    controller_storage: MaybeUninit<ControllerEndpoint>,
    worker_storage: MaybeUninit<WorkerEndpoint>,
    transport_state: PicoTransportState,
    stack_peak_bytes: usize,
}

impl SmokeStorage {
    const fn new() -> Self {
        Self {
            clock: CounterClock::new(),
            tap_storage: [TapEvent::zero(); RING_EVENTS],
            slab_storage: [0u8; SLAB_BYTES],
            session_storage: MaybeUninit::uninit(),
            controller_storage: MaybeUninit::uninit(),
            worker_storage: MaybeUninit::uninit(),
            transport_state: PicoTransportState::new(),
            stack_peak_bytes: 0,
        }
    }
}

struct SmokeStorageCell(UnsafeCell<SmokeStorage>);

unsafe impl Sync for SmokeStorageCell {}

static STORAGE: SmokeStorageCell = SmokeStorageCell(UnsafeCell::new(SmokeStorage::new()));

#[inline(always)]
unsafe fn smoke_storage() -> *mut SmokeStorage {
    STORAGE.0.get()
}

#[inline(always)]
unsafe fn transport_state() -> *mut PicoTransportState {
    let storage = unsafe { smoke_storage() };
    unsafe { core::ptr::addr_of_mut!((*storage).transport_state) }
}

unsafe extern "C" {
    static __stack_top: u8;
    static __stack_limit: u8;
}

#[inline(always)]
fn stack_top_addr() -> usize {
    (&raw const __stack_top) as usize
}

#[inline(always)]
fn stack_limit_addr() -> usize {
    (&raw const __stack_limit) as usize
}

#[inline(always)]
fn stack_reserved_bytes() -> usize {
    stack_top_addr().saturating_sub(stack_limit_addr())
}

#[inline(always)]
unsafe fn current_stack_pointer() -> usize {
    let sp: usize;
    unsafe {
        asm!("mov {0}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    sp
}

unsafe fn initialize_stack_canary() {
    let limit = stack_limit_addr();
    let current_sp = unsafe { current_stack_pointer() } & !0x3usize;
    if current_sp <= limit {
        return;
    }
    let mut ptr = limit as *mut u32;
    let end = current_sp as *mut u32;
    while ptr < end {
        unsafe { ptr.write(STACK_CANARY_WORD) };
        ptr = unsafe { ptr.add(1) };
    }
}

unsafe fn measure_peak_stack_bytes() -> usize {
    let limit = stack_limit_addr();
    let top = stack_top_addr();
    let mut ptr = limit as *const u32;
    let end = top as *const u32;
    while ptr < end && unsafe { ptr.read() } == STACK_CANARY_WORD {
        ptr = unsafe { ptr.add(1) };
    }
    top.saturating_sub(ptr as usize)
}

#[repr(C)]
struct VectorTable {
    initial_stack_pointer: *const u32,
    reset: unsafe extern "C" fn() -> !,
}

unsafe impl Sync for VectorTable {}

#[unsafe(link_section = ".vector_table.reset_vector")]
#[used]
static VECTOR_TABLE: VectorTable = VectorTable {
    initial_stack_pointer: core::ptr::addr_of!(__stack_top) as *const u32,
    reset: Reset,
};

#[derive(Clone, Copy)]
struct PicoTransport;

struct PicoTx;

struct PicoRx {
    role: u8,
    current: Option<FrameOwned>,
}

struct PicoSendFuture {
    role: u8,
    frame: Option<FrameOwned>,
}

impl Future for PicoSendFuture {
    type Output = Result<(), TransportError>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(frame) = self.frame.take() {
            unsafe { &mut *transport_state() }
                .role_mut(self.role)
                .queue
                .push_back(frame);
        }
        Poll::Ready(Ok(()))
    }
}

struct PicoRecvFuture<'a> {
    rx: &'a mut PicoRx,
}

impl<'a> Future for PicoRecvFuture<'a> {
    type Output = Result<Payload<'a>, TransportError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.rx.current.is_none() {
            let dequeued = unsafe { &mut *transport_state() }
                .role_mut(this.rx.role)
                .queue
                .pop_front();
            match dequeued {
                Some(frame) => this.rx.current = Some(frame),
                None => return Poll::Pending,
            }
        }
        let frame = this.rx.current.as_ref().expect("queued transport frame");
        let bytes: &'a [u8] = unsafe { &*(frame.as_slice() as *const [u8]) };
        Poll::Ready(Ok(Payload::new(bytes)))
    }
}

impl Transport for PicoTransport {
    type Error = TransportError;
    type Tx<'a>
        = PicoTx
    where
        Self: 'a;
    type Rx<'a>
        = PicoRx
    where
        Self: 'a;
    type Send<'a>
        = PicoSendFuture
    where
        Self: 'a;
    type Recv<'a>
        = PicoRecvFuture<'a>
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let _ = unsafe { &*transport_state() }.role(local_role);
        (
            PicoTx,
            PicoRx {
                role: local_role,
                current: None,
            },
        )
    }

    fn send<'a, 'f>(&'a self, _tx: &'a mut Self::Tx<'a>, outgoing: Outgoing<'f>) -> Self::Send<'a>
    where
        'a: 'f,
    {
        PicoSendFuture {
            role: outgoing.meta.peer,
            frame: Some(FrameOwned::from_bytes(outgoing.payload.as_bytes())),
        }
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        rx.current = None;
        PicoRecvFuture { rx }
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        if let Some(frame) = rx.current.take() {
            unsafe { &mut *transport_state() }
                .role_mut(rx.role)
                .queue
                .push_front(frame);
        }
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(TransportEvent)) {}

    fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
        None
    }

    fn metrics(&self) -> Self::Metrics {}

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

fn must<T, E>(value: Result<T, E>) -> T {
    match value {
        Ok(value) => value,
        Err(_) => loop {
            core::hint::spin_loop();
        },
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

fn block_on<F: Future>(mut future: F) -> F::Output {
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => core::hint::spin_loop(),
        }
    }
}

fn drive<F: Future>(future: F) -> F::Output {
    block_on(future)
}

fn transport_queue_is_empty() -> bool {
    let state = unsafe { &*transport_state() };
    state.roles.iter().all(|role| role.queue.len == 0)
}

fn retain_fixture_symbols() {
    let _ = localside::worker_offer_decode_u8::<
        0,
        PicoTransport,
        DefaultLabelUniverse,
        CounterClock,
        1,
    > as fn(
        &mut localside::WorkerEndpoint<'_, PicoTransport, DefaultLabelUniverse, CounterClock, 1>,
    ) -> u8;
}

fn run_smoke() {
    retain_fixture_symbols();
    assert_eq!(sample_program::ROUTE_SCOPE_COUNT, sample_program::ACK_LABELS.len());
    assert_eq!(
        sample_program::ROUTE_SCOPE_COUNT,
        sample_program::EXPECTED_WORKER_BRANCH_LABELS.len()
    );
    unsafe {
        let storage = &mut *smoke_storage();
        storage.transport_state = PicoTransportState::new();
        storage.tap_storage.fill(TapEvent::zero());
        storage.slab_storage.fill(0);
        let session_ptr = storage.session_storage.as_mut_ptr();
        session_ptr.write(SessionKit::new(&storage.clock));
        let kit = &*session_ptr;
        let rv_id = must(kit.add_rendezvous_from_config(
            Config::new(&mut storage.tap_storage, &mut storage.slab_storage),
            PicoTransport,
        ));
        let sid = SessionId::new(1);
        let controller_ptr = storage.controller_storage.as_mut_ptr();
        controller_ptr.write(must(kit.enter(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)));
        let controller = &mut *controller_ptr;
        let worker_ptr = storage.worker_storage.as_mut_ptr();
        worker_ptr.write(must(kit.enter(rv_id, sid, &WORKER_PROGRAM, NoBinding)));
        let worker = &mut *worker_ptr;
        sample_program::run(controller, worker);
        assert!(transport_queue_is_empty());

        core::ptr::drop_in_place(worker_ptr);
        core::ptr::drop_in_place(controller_ptr);
        core::ptr::drop_in_place(session_ptr);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn Reset() -> ! {
    unsafe {
        initialize_stack_canary();
    }
    run_smoke();
    unsafe {
        let storage = &mut *smoke_storage();
        storage.stack_peak_bytes = measure_peak_stack_bytes();
        assert!(storage.stack_peak_bytes <= stack_reserved_bytes());
    }
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
