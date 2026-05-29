use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type PendingControllerEndpoint =
    CursorEndpoint<
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type PendingControllerBindingEndpoint =
    CursorEndpoint<
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintPendingControllerEndpoint =
    CursorEndpoint<
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type HintPendingWorkerEndpoint =
    CursorEndpoint<
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type FreshHintPendingWorkerEndpoint =
    CursorEndpoint<
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const OFFER_CLUSTER_SLOT_BYTES: usize = crate::endpoint::kernel::core::offer_regression_tests::cases::compact_and_helpers::max_usize(&[
    size_of::<OfferHintCluster>(),
    size_of::<PendingOfferCluster>(),
    size_of::<HintPendingOfferCluster>(),
    size_of::<
        SessionCluster<'static, DeferredIngressTransport, DefaultLabelUniverse, CounterClock, 4>,
    >(),
]);
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const OFFER_VALUE_SLOT_BYTES: usize = crate::endpoint::kernel::core::offer_regression_tests::cases::compact_and_helpers::max_usize(&[
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type PendingWorkerEndpoint =
    CursorEndpoint<
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type PendingWorkerBindingEndpoint =
    CursorEndpoint<
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct OfferTestFixtureGuard<
    const N: usize,
> {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) tap:
        *mut [TapEvent; RING_EVENTS],
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) slab:
        *mut [u8; OFFER_FIXTURE_SLAB_CAPACITY],
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) clock: *const CounterClock,
}

thread_local! {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) static OFFER_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) static OFFER_TEST_SLAB: UnsafeCell<[u8; OFFER_FIXTURE_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; OFFER_FIXTURE_SLAB_CAPACITY]) };
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) static OFFER_TEST_CLOCK: CounterClock = const { CounterClock::new() };
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn acquire_offer_fixture<
    const N: usize,
>() -> OfferTestFixtureGuard<N> {
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
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn config(
        &mut self,
    ) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        let tap = unsafe { &mut *self.tap };
        let slab = unsafe { &mut *self.slab };
        Config::from_resources((tap, slab), CounterClock::new())
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn clock(
        &self,
    ) -> &'static CounterClock {
        unsafe { &*self.clock }
    }
}

#[repr(C, align(16))]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct OfferClusterStorage {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) bytes:
        [u8; OFFER_CLUSTER_SLOT_BYTES],
}

#[repr(C, align(16))]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct OfferValueStorage {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) bytes:
        [u8; OFFER_VALUE_SLOT_BYTES],
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) trait OfferClusterInit {
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_offer_cluster_slot<
    T,
    R,
>(
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct OfferValueSlotGuard<
    'a,
    T,
> {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) value: *mut T,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) occupied:
        *const Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) _marker:
        PhantomData<&'a mut T>,
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_offer_value_storage<
    'a,
    T: 'a,
    R,
>(
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_offer_value_slot_storage<
    R,
>(
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
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn occupied(
        &self,
    ) -> &Cell<bool> {
        unsafe { &*self.occupied }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ptr(&self) -> *mut T {
        self.occupied().set(true);
        self.value
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn store(&self, value: T) {
        unsafe {
            self.value.write(value);
        }
        self.occupied().set(true);
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn borrow_mut(
        &mut self,
    ) -> &mut T {
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_ready_ok<F, T, E>(
    cx: &mut Context<'_>,
    mut fut: core::pin::Pin<&mut F>,
    context: &str,
) -> T
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn run_offer_regression_test<
    F,
>(
    name: &'static str,
    test: F,
) where
    F: FnOnce() + Send + 'static,
{
    let _ = name;
    test();
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_BINDING_QUEUE_CAPACITY: usize = 8;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_BINDING_PAYLOAD_CAPACITY: usize = 64;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct FixedQueue<
    T,
    const N: usize,
> {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) items: [Option<T>; N],
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) head: usize,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) len: usize,
}

impl<T, const N: usize> FixedQueue<T, N> {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn new() -> Self {
        Self {
            items: core::array::from_fn(|_| None),
            head: 0,
            len: 0,
        }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn push_back(
        &mut self,
        item: T,
    ) {
        assert!(self.len < N, "fixed queue capacity exceeded");
        let idx = (self.head + self.len) % N;
        self.items[idx] = Some(item);
        self.len += 1;
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn pop_front(
        &mut self,
    ) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let idx = self.head;
        self.head = (self.head + 1) % N;
        self.len -= 1;
        self.items[idx].take()
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct FixedPayload {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) len: usize,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) bytes:
        [u8; TEST_BINDING_PAYLOAD_CAPACITY],
}

impl FixedPayload {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn from_bytes(
        payload: &[u8],
    ) -> Self {
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

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn as_slice(
        &self,
    ) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct TestBinding {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) incoming:
        FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) recv_payloads:
        FixedQueue<FixedPayload, TEST_BINDING_QUEUE_CAPACITY>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) polls: Cell<usize>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) last_recv_channel:
        Cell<Option<Channel>>,
}

impl TestBinding {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_incoming(
        incoming: &[IngressEvidence],
    ) -> Self {
        let mut binding = Self::default();
        for evidence in incoming.iter().copied() {
            binding.incoming.push_back(evidence);
        }
        binding
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_incoming_and_payloads(
        incoming: &[IngressEvidence],
        recv_payloads: &[&[u8]],
    ) -> Self {
        let mut binding = Self::with_incoming(incoming);
        for payload in recv_payloads {
            binding
                .recv_payloads
                .push_back(FixedPayload::from_bytes(payload));
        }
        binding
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_count(
        &self,
    ) -> usize {
        self.polls.get()
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn last_recv_channel(
        &self,
    ) -> Option<Channel> {
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct LaneAwareTestBinding {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) incoming:
        std::vec::Vec<FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) polls:
        std::vec::Vec<usize>,
}

impl LaneAwareTestBinding {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_lane_incoming(
        incoming: &[(u8, IngressEvidence)],
    ) -> Self {
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

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_count_for_lane(
        &self,
        lane_idx: usize,
    ) -> usize {
        self.polls.get(lane_idx).copied().unwrap_or(0)
    }
}

impl EndpointSlot for LaneAwareTestBinding {
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
    ) -> Result<Payload<'a>, BindingError> {
        Ok(Payload::new(&[]))
    }
}

impl EndpointSlot for TestBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        self.polls.set(self.polls.get().saturating_add(1));
        self.incoming.pop_front()
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<Payload<'a>, BindingError> {
        self.last_recv_channel.set(Some(channel));
        let Some(payload) = self.recv_payloads.pop_front() else {
            return Ok(Payload::new(&[]));
        };
        let payload = payload.as_slice();
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(Payload::new(&buf[..len]))
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const HINT_NONE: u8 = u8::MAX;
