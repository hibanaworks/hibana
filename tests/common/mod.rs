use core::ptr;
use hibana::substrate::{
    Transport,
    policy::PolicyAttrs,
    transport::{
        TransportError,
        advanced::{TransportEvent, TransportMetrics},
    },
    wire::Payload,
};
use std::cell::UnsafeCell;
use std::{
    mem::MaybeUninit,
    task::{Context, Poll, Waker},
};

const TEST_ROLE_CAPACITY: usize = 4;
const TEST_QUEUE_CAPACITY: usize = 16;
const TEST_WAITER_CAPACITY: usize = 16;
const TEST_FRAME_PAYLOAD_CAPACITY: usize = 128;
const TEST_TRANSPORT_POOL_CAPACITY: usize = 4;

unsafe fn init_option_array<T, const N: usize>(dst: *mut Option<T>) {
    let mut idx = 0usize;
    while idx < N {
        unsafe {
            dst.add(idx).write(None);
        }
        idx += 1;
    }
}

pub(crate) struct FixedQueue<T, const N: usize> {
    items: [Option<T>; N],
    head: usize,
    pub(crate) len: usize,
}

impl<T, const N: usize> FixedQueue<T, N> {
    unsafe fn init(dst: *mut Self) {
        unsafe {
            init_option_array::<T, N>(ptr::addr_of_mut!((*dst).items).cast::<Option<T>>());
            ptr::addr_of_mut!((*dst).head).write(0);
            ptr::addr_of_mut!((*dst).len).write(0);
        }
    }

    fn reset(&mut self) {
        let mut idx = 0usize;
        while idx < N {
            self.items[idx] = None;
            idx += 1;
        }
        self.head = 0;
        self.len = 0;
    }

    fn push_back(&mut self, item: T) {
        assert!(self.len < N, "test transport queue capacity exceeded");
        let idx = (self.head + self.len) % N;
        self.items[idx] = Some(item);
        self.len += 1;
    }

    fn push_front(&mut self, item: T) {
        assert!(self.len < N, "test transport queue capacity exceeded");
        self.head = if self.head == 0 { N - 1 } else { self.head - 1 };
        self.items[self.head] = Some(item);
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

pub(crate) struct FrameOwned {
    len: usize,
    payload: [u8; TEST_FRAME_PAYLOAD_CAPACITY],
}

impl FrameOwned {
    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() <= TEST_FRAME_PAYLOAD_CAPACITY,
            "test transport payload exceeds fixed capacity"
        );
        let mut payload = [0u8; TEST_FRAME_PAYLOAD_CAPACITY];
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

struct WaiterBatch {
    waiters: [Option<Waker>; TEST_WAITER_CAPACITY],
}

impl WaiterBatch {
    fn new() -> Self {
        Self {
            waiters: core::array::from_fn(|_| None),
        }
    }

    fn push(&mut self, waker: Waker) {
        for slot in &mut self.waiters {
            if slot.is_none() {
                *slot = Some(waker);
                return;
            }
        }
        panic!("test transport waiter capacity exceeded");
    }

    fn wake_all(self) {
        for waker in self.waiters.into_iter().flatten() {
            waker.wake();
        }
    }
}

pub(crate) struct RoleState {
    pub(crate) queue: FixedQueue<FrameOwned, TEST_QUEUE_CAPACITY>,
    waiters: [Option<Waker>; TEST_WAITER_CAPACITY],
}

impl RoleState {
    unsafe fn init(dst: *mut Self) {
        unsafe {
            FixedQueue::init(ptr::addr_of_mut!((*dst).queue));
            init_option_array::<Waker, TEST_WAITER_CAPACITY>(
                ptr::addr_of_mut!((*dst).waiters).cast::<Option<Waker>>(),
            );
        }
    }

    fn reset(&mut self) {
        self.queue.reset();
        for waiter in &mut self.waiters {
            *waiter = None;
        }
    }

    fn add_waiter(&mut self, waker: Waker) {
        for slot in &mut self.waiters {
            if slot.is_none() {
                *slot = Some(waker);
                return;
            }
        }
        panic!("test transport waiter capacity exceeded");
    }

    fn take_waiters(&mut self) -> WaiterBatch {
        let mut batch = WaiterBatch::new();
        for slot in &mut self.waiters {
            if let Some(waker) = slot.take() {
                batch.push(waker);
            }
        }
        batch
    }
}

pub(crate) struct TestState {
    pub(crate) roles: [RoleState; TEST_ROLE_CAPACITY],
}

impl TestState {
    unsafe fn init(dst: *mut Self) {
        let roles = unsafe { ptr::addr_of_mut!((*dst).roles).cast::<RoleState>() };
        let mut idx = 0usize;
        while idx < TEST_ROLE_CAPACITY {
            unsafe {
                RoleState::init(roles.add(idx));
            }
            idx += 1;
        }
    }

    fn reset(&mut self) {
        for role in &mut self.roles {
            role.reset();
        }
    }

    fn role_mut(&mut self, role: u8) -> &mut RoleState {
        self.roles
            .get_mut(role as usize)
            .unwrap_or_else(|| panic!("test transport role out of range: {role}"))
    }

    fn role(&self, role: u8) -> &RoleState {
        self.roles
            .get(role as usize)
            .unwrap_or_else(|| panic!("test transport role out of range: {role}"))
    }

    fn enqueue(&mut self, role: u8, frame: FrameOwned) -> WaiterBatch {
        let role_state = self.role_mut(role);
        role_state.queue.push_back(frame);
        role_state.take_waiters()
    }

    fn dequeue(&mut self, role: u8) -> Option<FrameOwned> {
        self.role_mut(role).queue.pop_front()
    }

    fn requeue(&mut self, role: u8, frame: FrameOwned) {
        self.role_mut(role).queue.push_front(frame);
    }

    fn add_waiter(&mut self, role: u8, waker: Waker) {
        self.role_mut(role).add_waiter(waker);
    }
    fn ensure_role(&self, role: u8) {
        let _ = self.role(role);
    }
}

struct TransportPool {
    initialized: UnsafeCell<[bool; TEST_TRANSPORT_POOL_CAPACITY]>,
    refs: UnsafeCell<[usize; TEST_TRANSPORT_POOL_CAPACITY]>,
    states: UnsafeCell<[MaybeUninit<TestState>; TEST_TRANSPORT_POOL_CAPACITY]>,
    metrics: UnsafeCell<[MaybeUninit<PolicyAttrs>; TEST_TRANSPORT_POOL_CAPACITY]>,
}

impl TransportPool {
    const fn new() -> Self {
        Self {
            initialized: UnsafeCell::new([false; TEST_TRANSPORT_POOL_CAPACITY]),
            refs: UnsafeCell::new([0; TEST_TRANSPORT_POOL_CAPACITY]),
            states: UnsafeCell::new(
                [const { MaybeUninit::uninit() }; TEST_TRANSPORT_POOL_CAPACITY],
            ),
            metrics: UnsafeCell::new(
                [const { MaybeUninit::uninit() }; TEST_TRANSPORT_POOL_CAPACITY],
            ),
        }
    }

    fn ensure_slot_initialized(&self, idx: usize) {
        assert!(
            idx < TEST_TRANSPORT_POOL_CAPACITY,
            "transport slot out of range"
        );
        unsafe {
            let initialized = &mut *self.initialized.get();
            if !initialized[idx] {
                let states = &mut *self.states.get();
                TestState::init(states[idx].as_mut_ptr());
                initialized[idx] = true;
            }
        }
    }

    fn allocate(&self) -> Option<usize> {
        unsafe {
            let refs = &mut *self.refs.get();
            let metrics = &mut *self.metrics.get();
            let states = &mut *self.states.get();
            let mut idx = 0usize;
            while idx < TEST_TRANSPORT_POOL_CAPACITY {
                if refs[idx] == 0 {
                    self.ensure_slot_initialized(idx);
                    refs[idx] = 1;
                    (&mut *states[idx].as_mut_ptr()).reset();
                    metrics[idx].write(PolicyAttrs::EMPTY);
                    return Some(idx);
                }
                idx += 1;
            }
            None
        }
    }

    fn ref_inc(&self, idx: usize) {
        unsafe {
            let refs = &mut *self.refs.get();
            refs[idx] += 1;
        }
    }

    fn ref_dec(&self, idx: usize) {
        unsafe {
            let refs = &mut *self.refs.get();
            assert!(refs[idx] > 0, "test transport slot refcount underflow");
            refs[idx] -= 1;
        }
    }

    fn state_with<R>(&self, idx: usize, f: impl FnOnce(&TestState) -> R) -> R {
        self.ensure_slot_initialized(idx);
        unsafe { f(&*(*self.states.get())[idx].as_ptr()) }
    }

    fn state_with_mut<R>(&self, idx: usize, f: impl FnOnce(&mut TestState) -> R) -> R {
        self.ensure_slot_initialized(idx);
        unsafe { f(&mut *(*self.states.get())[idx].as_mut_ptr()) }
    }

    fn metrics_get(&self, idx: usize) -> PolicyAttrs {
        unsafe { (*self.metrics.get())[idx].assume_init_read() }
    }
}

impl Drop for TransportPool {
    fn drop(&mut self) {
        unsafe {
            let initialized = &mut *self.initialized.get();
            let states = &mut *self.states.get();
            let mut idx = 0usize;
            while idx < TEST_TRANSPORT_POOL_CAPACITY {
                if initialized[idx] {
                    core::ptr::drop_in_place(states[idx].as_mut_ptr());
                    initialized[idx] = false;
                }
                idx += 1;
            }
        }
    }
}

std::thread_local! {
    static TRANSPORT_POOL: TransportPool = const { TransportPool::new() };
}

#[derive(Default)]
pub(crate) struct TestTx {
    pending_role: Option<u8>,
    pending_frame: Option<FrameOwned>,
}

pub(crate) struct TestRx<'a> {
    pool: &'a TransportPool,
    slot: usize,
    role: u8,
    current: Option<FrameOwned>,
}

pub(crate) struct TestTransport {
    pool: &'static TransportPool,
    slot: usize,
}

impl Default for TestTransport {
    fn default() -> Self {
        TRANSPORT_POOL.with(|pool| {
            if let Some(slot) = pool.allocate() {
                let pool_ref = unsafe { &*(pool as *const TransportPool) };
                return Self {
                    pool: pool_ref,
                    slot,
                };
            }
            panic!("test transport slot pool exhausted");
        })
    }
}

impl Clone for TestTransport {
    fn clone(&self) -> Self {
        self.pool.ref_inc(self.slot);
        Self {
            pool: self.pool,
            slot: self.slot,
        }
    }
}

impl TestTransport {
    pub(crate) fn queue_is_empty(&self) -> bool {
        self.pool.state_with(self.slot, |state| {
            state.roles.iter().all(|role| role.queue.len == 0)
        })
    }

    pub(crate) fn stage_send(&self, tx: &mut TestTx, role: u8, payload: &[u8]) {
        if tx.pending_frame.is_none() {
            tx.pending_role = Some(role);
            tx.pending_frame = Some(FrameOwned::from_bytes(payload));
        }
    }

    pub(crate) fn poll_send_staged(&self, tx: &mut TestTx) -> Poll<Result<(), TestTransportError>> {
        let role = tx.pending_role.take().expect("queued role");
        let frame = tx.pending_frame.take().expect("queued frame");
        let waiters = self
            .pool
            .state_with_mut(self.slot, |state| state.enqueue(role, frame));
        waiters.wake_all();
        Poll::Ready(Ok(()))
    }

    pub(crate) fn cancel_send_staged(&self, tx: &mut TestTx) {
        tx.pending_role = None;
        tx.pending_frame = None;
    }

    pub(crate) fn poll_recv_current<'a>(
        &'a self,
        rx: &'a mut TestRx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, TestTransportError>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        if rx.current.is_none() {
            let dequeued = rx.pool.state_with_mut(rx.slot, |state| {
                if let Some(frame) = state.dequeue(rx.role) {
                    Some(frame)
                } else {
                    state.add_waiter(rx.role, cx.waker().clone());
                    None
                }
            });
            if let Some(frame) = dequeued {
                rx.current = Some(frame);
            } else {
                return Poll::Pending;
            }
        }
        let frame = rx.current.as_ref().expect("current frame");
        let bytes: &'a [u8] = unsafe { &*(frame.as_slice() as *const [u8]) };
        Poll::Ready(Ok(Payload::new(bytes)))
    }
}

const _: fn(&TestTransport) -> bool = TestTransport::queue_is_empty;

impl Drop for TestTransport {
    fn drop(&mut self) {
        self.pool.ref_dec(self.slot);
    }
}

#[derive(Debug)]
pub enum TestTransportError {
    Empty,
}

impl From<TestTransportError> for TransportError {
    fn from(err: TestTransportError) -> Self {
        match err {
            TestTransportError::Empty => TransportError::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TestTransportMetrics {
    attrs: PolicyAttrs,
}

impl TransportMetrics for TestTransportMetrics {
    fn attrs(&self) -> PolicyAttrs {
        self.attrs
    }
}

impl Transport for TestTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.pool
            .state_with(self.slot, |state| state.ensure_role(local_role));
        (
            TestTx::default(),
            TestRx {
                pool: self.pool,
                slot: self.slot,
                role: local_role,
                current: None,
            },
        )
    }

    fn poll_send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: hibana::substrate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        self.stage_send(tx, outgoing.peer(), outgoing.payload().as_bytes());
        self.poll_send_staged(tx)
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.poll_recv_current(rx, cx)
    }

    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>) {
        self.cancel_send_staged(tx);
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        if let Some(frame) = rx.current.take() {
            rx.pool
                .state_with_mut(rx.slot, |state| state.requeue(rx.role, frame));
        }
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(TransportEvent)) {}

    fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        let attrs = self.pool.metrics_get(self.slot);
        TestTransportMetrics { attrs }
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}
