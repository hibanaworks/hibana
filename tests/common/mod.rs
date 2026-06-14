use core::ptr;
use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, FrameLabel, ReceivedFrame, Transport, TransportError},
    wire::Payload,
};
use std::cell::UnsafeCell;
use std::{
    mem::MaybeUninit,
    task::{Context, Poll, Waker},
};

const TEST_ROLE_CAPACITY: usize = 5;
const TEST_QUEUE_CAPACITY: usize = 16;
const TEST_LANE_CAPACITY: usize = 256;
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

    fn pop_front_matching(&mut self, mut matches: impl FnMut(&T) -> bool) -> Option<T> {
        let mut offset = 0usize;
        while offset < self.len {
            let idx = (self.head + offset) % N;
            if self.items[idx].as_ref().is_some_and(&mut matches) {
                let item = self.items[idx].take();
                let mut shift = offset;
                while shift + 1 < self.len {
                    let from = (self.head + shift + 1) % N;
                    let to = (self.head + shift) % N;
                    self.items[to] = self.items[from].take();
                    shift += 1;
                }
                let tail = (self.head + self.len - 1) % N;
                self.items[tail] = None;
                self.len -= 1;
                return item;
            }
            offset += 1;
        }
        None
    }
}

pub(crate) struct FrameOwned {
    session_id: SessionId,
    lane: u8,
    source_role: u8,
    frame_label: u8,
    hint_drained: bool,
    len: usize,
    payload: [u8; TEST_FRAME_PAYLOAD_CAPACITY],
}

impl FrameOwned {
    fn from_bytes(
        session_id: SessionId,
        lane: u8,
        source_role: u8,
        frame_label: u8,
        bytes: &[u8],
    ) -> Self {
        assert!(
            bytes.len() <= TEST_FRAME_PAYLOAD_CAPACITY,
            "test transport payload exceeds fixed capacity"
        );
        let mut payload = [0u8; TEST_FRAME_PAYLOAD_CAPACITY];
        payload[..bytes.len()].copy_from_slice(bytes);
        Self {
            session_id,
            lane,
            source_role,
            frame_label,
            hint_drained: false,
            len: bytes.len(),
            payload,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.payload[..self.len]
    }
}

struct WaiterBatch {
    waiters: [Option<Waker>; TEST_LANE_CAPACITY],
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
    waiters: [Option<Waker>; TEST_LANE_CAPACITY],
}

impl RoleState {
    unsafe fn init(dst: *mut Self) {
        unsafe {
            FixedQueue::init(ptr::addr_of_mut!((*dst).queue));
            init_option_array::<Waker, TEST_LANE_CAPACITY>(
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

    fn add_waiter(&mut self, lane: u8, waker: Waker) {
        self.waiters[lane as usize] = Some(waker);
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
        match self.roles.get_mut(role as usize) {
            Some(role_state) => role_state,
            None => panic!("test transport role out of range: {role}"),
        }
    }

    fn role(&self, role: u8) -> &RoleState {
        match self.roles.get(role as usize) {
            Some(role_state) => role_state,
            None => panic!("test transport role out of range: {role}"),
        }
    }

    fn enqueue(&mut self, role: u8, frame: FrameOwned) -> WaiterBatch {
        let role_state = self.role_mut(role);
        role_state.queue.push_back(frame);
        role_state.take_waiters()
    }

    fn dequeue(&mut self, role: u8, lane: u8) -> Option<FrameOwned> {
        self.role_mut(role)
            .queue
            .pop_front_matching(|frame| frame.lane == lane)
    }

    fn requeue(&mut self, role: u8, frame: FrameOwned) {
        self.role_mut(role).queue.push_front(frame);
    }

    fn add_waiter(&mut self, role: u8, lane: u8, waker: Waker) {
        self.role_mut(role).add_waiter(lane, waker);
    }
    fn ensure_role(&self, role: u8) {
        let _ = self.role(role);
    }
}

struct TransportPool {
    initialized: UnsafeCell<[bool; TEST_TRANSPORT_POOL_CAPACITY]>,
    refs: UnsafeCell<[usize; TEST_TRANSPORT_POOL_CAPACITY]>,
    states: UnsafeCell<[MaybeUninit<TestState>; TEST_TRANSPORT_POOL_CAPACITY]>,
}

impl TransportPool {
    const fn new() -> Self {
        Self {
            initialized: UnsafeCell::new([false; TEST_TRANSPORT_POOL_CAPACITY]),
            refs: UnsafeCell::new([0; TEST_TRANSPORT_POOL_CAPACITY]),
            states: UnsafeCell::new(
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
            let states = &mut *self.states.get();
            let mut idx = 0usize;
            while idx < TEST_TRANSPORT_POOL_CAPACITY {
                if refs[idx] == 0 {
                    self.ensure_slot_initialized(idx);
                    refs[idx] = 1;
                    (&mut *states[idx].as_mut_ptr()).reset();
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

pub(crate) struct TestTx {
    pub(crate) session_id: SessionId,
    pub(crate) local_role: u8,
    pub(crate) pending_role: Option<u8>,
    pub(crate) pending_frame: Option<FrameOwned>,
}

pub(crate) struct TestRx<'a> {
    pool: &'a TransportPool,
    slot: usize,
    role: u8,
    lane: u8,
    current: Option<FrameOwned>,
    current_hint_drained: std::cell::Cell<bool>,
}

pub(crate) struct TestTransport {
    pool: &'static TransportPool,
    slot: usize,
}

impl TestTransport {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn queue_is_empty(&self) -> bool {
        self.pool.state_with(self.slot, |state| {
            state.roles.iter().all(|role| role.queue.len == 0)
        })
    }

    pub(crate) fn stage_send(
        &self,
        tx: &mut TestTx,
        role: u8,
        lane: u8,
        frame_label: u8,
        payload: &[u8],
    ) {
        if tx.pending_frame.is_none() {
            tx.pending_role = Some(role);
            tx.pending_frame = Some(FrameOwned::from_bytes(
                tx.session_id,
                lane,
                tx.local_role,
                frame_label,
                payload,
            ));
        }
    }

    pub(crate) fn stage_send_with_session(
        &self,
        tx: &mut TestTx,
        session_id: SessionId,
        role: u8,
        lane: u8,
        frame_label: u8,
        payload: &[u8],
    ) {
        if tx.pending_frame.is_none() {
            tx.pending_role = Some(role);
            tx.pending_frame = Some(FrameOwned::from_bytes(
                session_id,
                lane,
                tx.local_role,
                frame_label,
                payload,
            ));
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
        &self,
        rx: &'a mut TestRx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TestTransportError>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        if rx.current.is_none() {
            let dequeued = rx.pool.state_with_mut(rx.slot, |state| {
                if let Some(frame) = state.dequeue(rx.role, rx.lane) {
                    Some(frame)
                } else {
                    state.add_waiter(rx.role, rx.lane, cx.waker().clone());
                    None
                }
            });
            if let Some(frame) = dequeued {
                rx.current_hint_drained.set(frame.hint_drained);
                rx.current = Some(frame);
            } else {
                return Poll::Pending;
            }
        }
        let frame = rx.current.as_ref().expect("current frame");
        let header = FrameHeader::new(
            frame.session_id,
            frame.lane,
            frame.source_role,
            rx.role,
            FrameLabel::new(frame.frame_label),
        );
        let bytes: &'a [u8] = unsafe { &*(frame.as_slice() as *const [u8]) };
        Poll::Ready(Ok(ReceivedFrame::framed(header, Payload::new(bytes))))
    }

    pub(crate) fn open_rx(&self, role: u8, lane: u8) -> TestRx<'_> {
        self.pool
            .state_with(self.slot, |state| state.ensure_role(role));
        TestRx {
            pool: self.pool,
            slot: self.slot,
            role,
            lane,
            current: None,
            current_hint_drained: std::cell::Cell::new(false),
        }
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

const _: fn(&TestTransport) -> bool = TestTransport::queue_is_empty;
const _: for<'a> fn(&'a TestTransport, u8, u8) -> TestRx<'a> = TestTransport::open_rx;
const _: fn(&TestTransport, &mut TestTx, SessionId, u8, u8, u8, &[u8]) =
    TestTransport::stage_send_with_session;

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

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id();
        let lane = port.lane();
        self.pool
            .state_with(self.slot, |state| state.ensure_role(local_role));
        (
            TestTx {
                session_id,
                local_role,
                pending_role: None,
                pending_frame: None,
            },
            TestRx {
                pool: self.pool,
                slot: self.slot,
                role: local_role,
                lane,
                current: None,
                current_hint_drained: std::cell::Cell::new(false),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: hibana::runtime::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        self.stage_send(
            tx,
            outgoing.target_role(),
            outgoing.lane(),
            outgoing.frame_label().raw(),
            outgoing.payload().as_bytes(),
        );
        self.poll_send_staged(tx)
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        self.poll_recv_current(rx, cx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.cancel_send_staged(tx);
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        if let Some(mut frame) = rx.current.take() {
            frame.hint_drained = false;
            rx.current_hint_drained.set(false);
            rx.pool
                .state_with_mut(rx.slot, |state| state.requeue(rx.role, frame));
        }
        Ok(())
    }
}
