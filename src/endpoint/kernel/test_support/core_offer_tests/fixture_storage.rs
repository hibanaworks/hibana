type PendingControllerEndpoint = CursorEndpoint<
    'static,
    0,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type PendingControllerBindingEndpoint = CursorEndpoint<
    'static,
    0,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    TestBinding,
>;
type HintPendingControllerEndpoint = CursorEndpoint<
    'static,
    0,
    HintPendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type HintPendingWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    HintPendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type FreshHintPendingWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    FreshHintPendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
const OFFER_CLUSTER_SLOT_BYTES: usize = max_usize(&[
    size_of::<OfferHintCluster>(),
    size_of::<PendingOfferCluster>(),
    size_of::<HintPendingOfferCluster>(),
    size_of::<
        SessionCluster<'static, DeferredIngressTransport, DefaultLabelUniverse, CounterClock, 4>,
    >(),
]);
const OFFER_VALUE_SLOT_BYTES: usize = max_usize(&[
    offer_endpoint_slot_bytes::<0, HintOnlyTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, HintOnlyTransport, NoBinding>(4),
    offer_endpoint_slot_bytes::<0, HintOnlyTransport, TestBinding>(4),
    offer_endpoint_slot_bytes::<1, HintOnlyTransport, TestBinding>(4),
    offer_endpoint_slot_bytes::<1, HintOnlyTransport, LaneAwareTestBinding>(3),
    offer_endpoint_slot_bytes::<0, PendingTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, PendingTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<0, PendingTransport, TestBinding>(1),
    offer_endpoint_slot_bytes::<1, PendingTransport, TestBinding>(1),
    offer_endpoint_slot_bytes::<0, HintPendingTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, HintPendingTransport, NoBinding>(1),
    size_of::<PendingTransportState>(),
    size_of::<DeferredIngressState>(),
    offer_endpoint_slot_bytes::<0, DeferredIngressTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, DeferredIngressTransport, DeferredIngressBinding>(1),
]);
type PendingWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type PendingWorkerBindingEndpoint = CursorEndpoint<
    'static,
    1,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    TestBinding,
>;

struct OfferTestFixtureGuard<const N: usize> {
    tap: *mut [TapEvent; RING_EVENTS],
    slab: *mut [u8; OFFER_FIXTURE_SLAB_CAPACITY],
    clock: *const CounterClock,
}

thread_local! {
    static OFFER_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static OFFER_TEST_SLAB: UnsafeCell<[u8; OFFER_FIXTURE_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; OFFER_FIXTURE_SLAB_CAPACITY]) };
    static OFFER_TEST_CLOCK: CounterClock = const { CounterClock::new() };
}

fn acquire_offer_fixture<const N: usize>() -> OfferTestFixtureGuard<N> {
    assert!(
        N <= OFFER_FIXTURE_SLAB_CAPACITY,
        "offer fixture slab too small"
    );
    OFFER_TEST_TAP.with(|tap| {
        OFFER_TEST_SLAB.with(|slab| unsafe {
            OFFER_TEST_CLOCK.with(|clock| {
                let tap_ptr = tap.get();
                (*tap_ptr).fill(TapEvent::zero());
                let slab_ptr = slab.get();
                (*slab_ptr).fill(0);
                OfferTestFixtureGuard {
                    tap: tap_ptr,
                    slab: slab_ptr,
                    clock: clock as *const CounterClock,
                }
            })
        })
    })
}

impl<const N: usize> OfferTestFixtureGuard<N> {
    fn config(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        let tap = unsafe { &mut *self.tap };
        let slab = unsafe { &mut *self.slab };
        Config::from_resources((tap, slab), CounterClock::new())
    }

    fn clock(&self) -> &'static CounterClock {
        unsafe { &*self.clock }
    }
}

#[repr(C, align(16))]
struct OfferClusterStorage {
    bytes: [u8; OFFER_CLUSTER_SLOT_BYTES],
}

#[repr(C, align(16))]
struct OfferValueStorage {
    bytes: [u8; OFFER_VALUE_SLOT_BYTES],
}

trait OfferClusterInit {
    unsafe fn init_empty(dst: *mut Self, _clock: &'static CounterClock);
}

impl<T, U, const MAX_RV: usize> OfferClusterInit
    for SessionCluster<'static, T, U, CounterClock, MAX_RV>
where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
{
    unsafe fn init_empty(dst: *mut Self, _clock: &'static CounterClock) {
        unsafe { SessionCluster::init_empty(dst) };
    }
}

thread_local! {
    static OFFER_CLUSTER_STORAGE: UnsafeCell<MaybeUninit<OfferClusterStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_CONTROLLER_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_CONTROLLER_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_WORKER_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_WORKER_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_CLIENT_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_CLIENT_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_SERVER_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_SERVER_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_PENDING_STATE_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_PENDING_STATE_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_DEFERRED_STATE_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_DEFERRED_STATE_OCCUPIED: Cell<bool> = const { Cell::new(false) };
}

fn with_offer_cluster_slot<T, R>(
    _clock: &'static CounterClock,
    f: impl FnOnce(&'static T) -> R,
) -> R
where
    T: OfferClusterInit + 'static,
{
    assert!(
        size_of::<T>() <= OFFER_CLUSTER_SLOT_BYTES,
        "offer cluster slot too small"
    );
    assert!(
        align_of::<T>() <= align_of::<OfferClusterStorage>(),
        "offer cluster slot alignment too small"
    );
    OFFER_CLUSTER_STORAGE.with(|storage| unsafe {
        let ptr = (*storage.get()).as_mut_ptr().cast::<T>();
        T::init_empty(ptr, _clock);
        let result = f(&*ptr);
        core::ptr::drop_in_place(ptr);
        result
    })
}

struct OfferValueSlotGuard<'a, T> {
    value: *mut T,
    occupied: *const Cell<bool>,
    _marker: PhantomData<&'a mut T>,
}

fn with_offer_value_storage<'a, T: 'a, R>(
    storage: &UnsafeCell<MaybeUninit<OfferValueStorage>>,
    occupied: &Cell<bool>,
    f: impl FnOnce(&mut OfferValueSlotGuard<'a, T>) -> R,
) -> R {
    assert!(
        size_of::<T>() <= OFFER_VALUE_SLOT_BYTES,
        "offer value slot too small"
    );
    assert!(
        align_of::<T>() <= align_of::<OfferValueStorage>(),
        "offer value slot alignment too small"
    );
    occupied.set(false);
    let mut slot = OfferValueSlotGuard {
        value: unsafe { (*storage.get()).as_mut_ptr().cast::<T>() },
        occupied: occupied as *const Cell<bool>,
        _marker: PhantomData,
    };
    f(&mut slot)
}

fn with_offer_value_slot_storage<R>(
    slot_name: &str,
    f: impl FnOnce(&UnsafeCell<MaybeUninit<OfferValueStorage>>, &Cell<bool>) -> R,
) -> R {
    match slot_name {
        "controller_slot" => OFFER_CONTROLLER_STORAGE
            .with(|storage| OFFER_CONTROLLER_OCCUPIED.with(|occupied| f(storage, occupied))),
        "worker_slot" => OFFER_WORKER_STORAGE
            .with(|storage| OFFER_WORKER_OCCUPIED.with(|occupied| f(storage, occupied))),
        "client_slot" => OFFER_CLIENT_STORAGE
            .with(|storage| OFFER_CLIENT_OCCUPIED.with(|occupied| f(storage, occupied))),
        "server_slot" => OFFER_SERVER_STORAGE
            .with(|storage| OFFER_SERVER_OCCUPIED.with(|occupied| f(storage, occupied))),
        "pending_state_slot" => OFFER_PENDING_STATE_STORAGE
            .with(|storage| OFFER_PENDING_STATE_OCCUPIED.with(|occupied| f(storage, occupied))),
        "deferred_state_slot" => OFFER_DEFERRED_STATE_STORAGE
            .with(|storage| OFFER_DEFERRED_STATE_OCCUPIED.with(|occupied| f(storage, occupied))),
        _ => panic!("unknown offer value slot"),
    }
}

impl<T> OfferValueSlotGuard<'_, T> {
    fn occupied(&self) -> &Cell<bool> {
        unsafe { &*self.occupied }
    }

    fn ptr(&self) -> *mut T {
        self.occupied().set(true);
        self.value
    }

    fn store(&self, value: T) {
        unsafe {
            self.value.write(value);
        }
        self.occupied().set(true);
    }

    fn borrow_mut(&mut self) -> &mut T {
        assert!(self.occupied().get(), "offer value slot is empty");
        unsafe { &mut *self.value }
    }
}

impl<T> Drop for OfferValueSlotGuard<'_, T> {
    fn drop(&mut self) {
        if self.occupied().replace(false) {
            unsafe {
                core::ptr::drop_in_place(self.value);
            }
        }
    }
}

macro_rules! offer_fixture {
    ($size:expr, $clock:ident, $config:ident) => {
        let mut __offer_fixture = acquire_offer_fixture::<$size>();
        let $clock = __offer_fixture.clock();
        let $config = __offer_fixture.config();
    };
}

macro_rules! with_offer_cluster {
    ($clock:expr, $cluster_ty:ty, $cluster_ref:ident, $body:block) => {{ with_offer_cluster_slot::<$cluster_ty, _>($clock, |$cluster_ref| $body) }};
}

macro_rules! with_offer_value_slot {
    ($value_ty:ty, $slot:ident, $body:block) => {{
        with_offer_value_slot_storage(stringify!($slot), |storage, occupied| {
            with_offer_value_storage::<$value_ty, _>(storage, occupied, |$slot| $body)
        })
    }};
}

fn poll_ready_ok<F, T, E>(cx: &mut Context<'_>, mut fut: core::pin::Pin<&mut F>, context: &str) -> T
where
    F: Future<Output = Result<T, E>>,
    E: core::fmt::Debug,
{
    let mut spins = 0usize;
    loop {
        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok(value)) => return value,
            Poll::Ready(Err(err)) => panic!("{context} failed: {err:?}"),
            Poll::Pending => {
                spins += 1;
                if spins > 8 {
                    panic!("{context} unexpectedly pending");
                }
                cx.waker().wake_by_ref();
            }
        }
    }
}

fn run_offer_regression_test<F>(name: &'static str, test: F)
where
    F: FnOnce() + Send + 'static,
{
    let _ = name;
    test();
}

const TEST_BINDING_QUEUE_CAPACITY: usize = 8;
const TEST_BINDING_PAYLOAD_CAPACITY: usize = 64;

struct FixedQueue<T, const N: usize> {
    items: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T, const N: usize> FixedQueue<T, N> {
    fn new() -> Self {
        Self {
            items: core::array::from_fn(|_| None),
            head: 0,
            len: 0,
        }
    }

    fn push_back(&mut self, item: T) {
        assert!(self.len < N, "fixed queue capacity exceeded");
        let idx = (self.head + self.len) % N;
        self.items[idx] = Some(item);
        self.len += 1;
    }

    fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let idx = self.head;
        self.head = (self.head + 1) % N;
        self.len -= 1;
        self.items[idx].take()
    }
}

struct FixedPayload {
    len: usize,
    bytes: [u8; TEST_BINDING_PAYLOAD_CAPACITY],
}

impl FixedPayload {
    fn from_bytes(payload: &[u8]) -> Self {
        assert!(
            payload.len() <= TEST_BINDING_PAYLOAD_CAPACITY,
            "test binding payload exceeds fixed capacity"
        );
        let mut bytes = [0u8; TEST_BINDING_PAYLOAD_CAPACITY];
        bytes[..payload.len()].copy_from_slice(payload);
        Self {
            len: payload.len(),
            bytes,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

struct TestBinding {
    incoming: FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>,
    recv_payloads: FixedQueue<FixedPayload, TEST_BINDING_QUEUE_CAPACITY>,
    polls: Cell<usize>,
    last_recv_channel: Cell<Option<Channel>>,
}

impl TestBinding {
    fn with_incoming(incoming: &[IngressEvidence]) -> Self {
        let mut binding = Self::default();
        for evidence in incoming.iter().copied() {
            binding.incoming.push_back(evidence);
        }
        binding
    }

    fn with_incoming_and_payloads(incoming: &[IngressEvidence], recv_payloads: &[&[u8]]) -> Self {
        let mut binding = Self::with_incoming(incoming);
        for payload in recv_payloads {
            binding
                .recv_payloads
                .push_back(FixedPayload::from_bytes(payload));
        }
        binding
    }

    fn poll_count(&self) -> usize {
        self.polls.get()
    }

    fn last_recv_channel(&self) -> Option<Channel> {
        self.last_recv_channel.get()
    }
}

impl Default for TestBinding {
    fn default() -> Self {
        Self {
            incoming: FixedQueue::new(),
            recv_payloads: FixedQueue::new(),
            polls: Cell::new(0),
            last_recv_channel: Cell::new(None),
        }
    }
}

struct LaneAwareTestBinding {
    incoming: std::vec::Vec<FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>>,
    polls: std::vec::Vec<usize>,
}

impl LaneAwareTestBinding {
    fn with_lane_incoming(incoming: &[(u8, IngressEvidence)]) -> Self {
        let lane_capacity = incoming
            .iter()
            .map(|(lane, _)| usize::from(*lane).saturating_add(1))
            .max()
            .unwrap_or(1);
        let mut binding = Self {
            incoming: std::iter::repeat_with(FixedQueue::new)
                .take(lane_capacity)
                .collect(),
            polls: std::vec![0; lane_capacity],
        };
        for (lane, evidence) in incoming.iter().copied() {
            let lane_idx = lane as usize;
            if lane_idx < binding.incoming.len() {
                binding.incoming[lane_idx].push_back(evidence);
            }
        }
        binding
    }

    fn poll_count_for_lane(&self, lane_idx: usize) -> usize {
        self.polls.get(lane_idx).copied().unwrap_or(0)
    }
}

impl BindingSlot for LaneAwareTestBinding {
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IngressEvidence> {
        let lane_idx = logical_lane as usize;
        if lane_idx >= self.incoming.len() {
            return None;
        }
        self.polls[lane_idx] = self.polls[lane_idx].saturating_add(1);
        self.incoming[lane_idx].pop_front()
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _buf: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        Ok(Payload::new(&[]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

impl BindingSlot for TestBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        self.polls.set(self.polls.get().saturating_add(1));
        self.incoming.pop_front()
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        self.last_recv_channel.set(Some(channel));
        let Some(payload) = self.recv_payloads.pop_front() else {
            return Ok(Payload::new(&[]));
        };
        let payload = payload.as_slice();
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(Payload::new(&buf[..len]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

const HINT_NONE: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct HintOnlyTransport {
    worker_hint: u8,
}

impl HintOnlyTransport {
    const fn new(worker_hint: u8) -> Self {
        Self { worker_hint }
    }
}

struct HintOnlyRx {
    hint: Cell<u8>,
}

#[derive(Clone, Copy)]
struct HintPendingTransport {
    state: &'static PendingTransportState,
    worker_hint: u8,
}

#[derive(Clone, Copy)]
struct FreshHintPendingTransport {
    state: &'static PendingTransportState,
    worker_hint: u8,
}

impl HintPendingTransport {
    const fn new(state: &'static PendingTransportState, worker_hint: u8) -> Self {
        Self { state, worker_hint }
    }

    fn poll_count(&self) -> usize {
        self.state.polls.get()
    }

    fn assert_no_hint_drain_while_recv_parked(&self) {
        assert_eq!(
            self.state.hint_drains_while_recv_parked.get(),
            0,
            "offer must not drain route hints from a lane whose recv future is parked"
        );
    }
}

struct HintPendingRx {
    hint: Cell<u8>,
}

impl FreshHintPendingTransport {
    const fn new(state: &'static PendingTransportState, worker_hint: u8) -> Self {
        Self { state, worker_hint }
    }

    fn poll_count(&self) -> usize {
        self.state.polls.get()
    }

    fn hint_drain_count(&self) -> usize {
        self.state.hint_drains_while_recv_parked.get()
    }

    fn requeue_count(&self) -> usize {
        self.state.requeues.get()
    }
}

struct FreshHintPendingRx {
    hint: Cell<u8>,
}

impl Transport for HintOnlyTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = HintOnlyRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((session_id, lane));
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintOnlyRx {
                hint: Cell::new(hint),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        Poll::Ready(Ok(Payload::new(&[0u8; 1])))
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract implementation: `poll_recv` is stateless and leaves the
    // fixture frame observable without moving it between queues.
    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        // Nothing to restore.
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        let hint = rx.hint.replace(HINT_NONE);
        if hint == HINT_NONE {
            None
        } else {
            Some(FrameLabel::new(hint))
        }
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }
}

impl Transport for HintPendingTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = HintPendingRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((session_id, lane));
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintPendingRx {
                hint: Cell::new(hint),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            Poll::Ready(Ok(Payload::new(&[])))
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        if self.state.recv_parked.get() {
            self.state.hint_drains_while_recv_parked.set(
                self.state
                    .hint_drains_while_recv_parked
                    .get()
                    .wrapping_add(1),
            );
            assert!(
                !self.state.panic_on_hint_drain_while_recv_parked.get(),
                "transport hint drain must not touch rx while recv future is parked"
            );
        }
        let hint = rx.hint.replace(HINT_NONE);
        if hint == HINT_NONE {
            None
        } else {
            Some(FrameLabel::new(hint))
        }
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }
}

impl Transport for FreshHintPendingTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = FreshHintPendingRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((local_role, session_id, lane));
        (
            (),
            FreshHintPendingRx {
                hint: Cell::new(HINT_NONE),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            rx.hint.set(self.worker_hint);
            Poll::Ready(Ok(Payload::new(&[0x5a])))
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        let hint = rx.hint.replace(HINT_NONE);
        if hint == HINT_NONE {
            None
        } else {
            self.state.hint_drains_while_recv_parked.set(
                self.state
                    .hint_drains_while_recv_parked
                    .get()
                    .wrapping_add(1),
            );
            Some(FrameLabel::new(hint))
        }
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }
}

#[derive(Clone, Copy)]
struct PendingTransport {
    state: &'static PendingTransportState,
}

impl PendingTransport {
    fn new(state: &'static PendingTransportState) -> Self {
        Self { state }
    }

    fn poll_count(&self) -> usize {
        self.state.polls.get()
    }

    fn requeue_count(&self) -> usize {
        self.state.requeues.get()
    }
}

#[derive(Default)]
struct PendingTransportState {
    polls: Cell<usize>,
    requeues: Cell<usize>,
    ready: Cell<bool>,
    recv_parked: Cell<bool>,
    hint_drains_while_recv_parked: Cell<usize>,
    panic_on_hint_drain_while_recv_parked: Cell<bool>,
    waker: UnsafeCell<Option<Waker>>,
}

struct DeferredIngressState {
    incoming: UnsafeCell<FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>>,
    recv_payloads: UnsafeCell<FixedQueue<FixedPayload, TEST_BINDING_QUEUE_CAPACITY>>,
    available: Cell<usize>,
    requeues: Cell<usize>,
}

impl DeferredIngressState {
    fn new() -> Self {
        Self {
            incoming: UnsafeCell::new(FixedQueue::new()),
            recv_payloads: UnsafeCell::new(FixedQueue::new()),
            available: Cell::new(0),
            requeues: Cell::new(0),
        }
    }

    fn push_incoming(&self, evidence: IngressEvidence) {
        unsafe {
            (&mut *self.incoming.get()).push_back(evidence);
        }
    }

    fn push_recv_payload(&self, payload: FixedPayload) {
        unsafe {
            (&mut *self.recv_payloads.get()).push_back(payload);
        }
    }

    fn pop_incoming(&self) -> Option<IngressEvidence> {
        unsafe { (&mut *self.incoming.get()).pop_front() }
    }

    fn pop_recv_payload(&self) -> Option<FixedPayload> {
        unsafe { (&mut *self.recv_payloads.get()).pop_front() }
    }

    fn requeue_count(&self) -> usize {
        self.requeues.get()
    }
}

struct DeferredIngressBinding {
    state: &'static DeferredIngressState,
    polls: Cell<usize>,
}

impl DeferredIngressBinding {
    fn with_incoming_and_payloads(
        state: &'static DeferredIngressState,
        incoming: &[IngressEvidence],
        recv_payloads: &[&[u8]],
    ) -> Self {
        for evidence in incoming.iter().copied() {
            state.push_incoming(evidence);
        }
        for payload in recv_payloads {
            state.push_recv_payload(FixedPayload::from_bytes(payload));
        }
        Self {
            state,
            polls: Cell::new(0),
        }
    }
}

impl BindingSlot for DeferredIngressBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        self.polls.set(self.polls.get().saturating_add(1));
        if self.state.available.get() == 0 {
            return None;
        }
        let evidence = self.state.pop_incoming()?;
        self.state
            .available
            .set(self.state.available.get().saturating_sub(1));
        Some(evidence)
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        let Some(payload) = self.state.pop_recv_payload() else {
            return Ok(Payload::new(&[]));
        };
        let payload = payload.as_slice();
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(Payload::new(&buf[..len]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

struct DeferredIngressTransport {
    state: &'static DeferredIngressState,
}

impl DeferredIngressTransport {
    fn new(state: &'static DeferredIngressState) -> Self {
        Self { state }
    }
}

struct DeferredIngressRx;

struct PendingRx;

impl Transport for PendingTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = PendingRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((local_role, session_id, lane));
        ((), PendingRx)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            Poll::Ready(Ok(Payload::new(&[0x5a])))
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(
        &'a self,
        _rx: &'a Self::Rx<'a>,
    ) -> Option<crate::transport::FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }
}

impl Transport for DeferredIngressTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = DeferredIngressRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((local_role, session_id, lane));
        ((), DeferredIngressRx)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state
            .available
            .set(self.state.available.get().wrapping_add(1));
        Poll::Ready(Ok(Payload::new(&[])))
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(
        &'a self,
        _rx: &'a Self::Rx<'a>,
    ) -> Option<crate::transport::FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }
}
