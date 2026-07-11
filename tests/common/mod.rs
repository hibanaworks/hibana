use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, ReceivedFrame, Transport, TransportError},
    wire::Payload,
};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
};

const TEST_ROLE_CAPACITY: usize = 5;
const TEST_QUEUE_CAPACITY: usize = 16;
const TEST_LANE_CAPACITY: usize = 256;
const TEST_FRAME_PAYLOAD_CAPACITY: usize = 128;

pub(crate) const fn frame_header_from_parts(
    session: SessionId,
    lane: u8,
    source_role: u8,
    target_role: u8,
    label: u8,
) -> FrameHeader {
    let session = session.raw().to_be_bytes();
    FrameHeader::from_bytes([
        session[0],
        session[1],
        session[2],
        session[3],
        lane,
        source_role,
        target_role,
        label,
    ])
}

pub(crate) struct FixedQueue<T, const N: usize> {
    items: [Option<T>; N],
    head: usize,
    pub(crate) len: usize,
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

    fn front_mut(&mut self) -> Option<&mut T> {
        if self.len == 0 {
            None
        } else {
            self.items[self.head].as_mut()
        }
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
    fn new() -> Self {
        Self {
            queue: FixedQueue::new(),
            waiters: core::array::from_fn(|_| None),
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
    fn new() -> Self {
        Self {
            roles: core::array::from_fn(|_| RoleState::new()),
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

pub(crate) struct TestTx {
    pub(crate) session_id: SessionId,
    pub(crate) local_role: u8,
    pub(crate) pending_role: Option<u8>,
    pub(crate) pending_frame: Option<FrameOwned>,
}

pub(crate) struct TestRx<'a> {
    state: &'a RefCell<TestState>,
    role: u8,
    lane: u8,
    current: Option<FrameOwned>,
    current_hint_drained: std::cell::Cell<bool>,
}

pub(crate) struct TestTransport {
    state: Rc<RefCell<TestState>>,
}

impl TestTransport {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(TestState::new())),
        }
    }

    pub(crate) fn queue_is_empty(&self) -> bool {
        self.state
            .borrow()
            .roles
            .iter()
            .all(|role| role.queue.len == 0)
    }

    pub(crate) fn truncate_next_payload(&self, role: u8, len: usize) {
        let mut state = self.state.borrow_mut();
        let frame = state
            .role_mut(role)
            .queue
            .front_mut()
            .expect("test transport frame to truncate");
        assert!(len < frame.len, "test corruption must truncate the payload");
        frame.len = len;
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

    pub(crate) fn poll_send_staged(&self, tx: &mut TestTx) -> Poll<Result<(), TransportError>> {
        let role = tx.pending_role.take().expect("queued role");
        let frame = tx.pending_frame.take().expect("queued frame");
        let waiters = self.state.borrow_mut().enqueue(role, frame);
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
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        if rx.current.is_none() {
            let dequeued = {
                let mut state = rx.state.borrow_mut();
                if let Some(frame) = state.dequeue(rx.role, rx.lane) {
                    Some(frame)
                } else {
                    state.add_waiter(rx.role, rx.lane, cx.waker().clone());
                    None
                }
            };
            if let Some(frame) = dequeued {
                rx.current_hint_drained.set(frame.hint_drained);
                rx.current = Some(frame);
            } else {
                return Poll::Pending;
            }
        }
        let frame: &'a FrameOwned = rx.current.as_ref().expect("current frame");
        let header = frame_header_from_parts(
            frame.session_id,
            frame.lane,
            frame.source_role,
            rx.role,
            frame.frame_label,
        );
        let bytes: &'a [u8] = frame.as_slice();
        Poll::Ready(Ok(ReceivedFrame::framed(header, Payload::new(bytes))))
    }

    pub(crate) fn open_rx(&self, role: u8, lane: u8) -> TestRx<'_> {
        self.state.borrow().ensure_role(role);
        TestRx {
            state: self.state.as_ref(),
            role,
            lane,
            current: None,
            current_hint_drained: std::cell::Cell::new(false),
        }
    }
}

impl Clone for TestTransport {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
        }
    }
}

const _: fn(&TestTransport) -> bool = TestTransport::queue_is_empty;
const _: fn(&TestTransport, u8, usize) = TestTransport::truncate_next_payload;
const _: for<'a> fn(&'a TestTransport, u8, u8) -> TestRx<'a> = TestTransport::open_rx;
const _: fn(&TestTransport, &mut TestTx, SessionId, u8, u8, u8, &[u8]) =
    TestTransport::stage_send_with_session;

impl Transport for TestTransport {
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
        self.state.borrow().ensure_role(local_role);
        (
            TestTx {
                session_id,
                local_role,
                pending_role: None,
                pending_frame: None,
            },
            TestRx {
                state: self.state.as_ref(),
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
    ) -> Poll<Result<(), TransportError>>
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
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.poll_recv_current(rx, cx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.cancel_send_staged(tx);
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        if let Some(mut frame) = rx.current.take() {
            frame.hint_drained = false;
            rx.current_hint_drained.set(false);
            rx.state.borrow_mut().requeue(rx.role, frame);
        }
        Ok(())
    }
}
