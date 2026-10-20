use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, ReceivedFrame, Transport, TransportError},
    wire::Payload,
};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll, Waker},
    vec::Vec,
};

const TEST_QUEUE_CAPACITY: usize = 16;
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
    witness: Option<u64>,
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
            witness: None,
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

    fn evidence(&self, target_role: u8) -> FrameEvidence {
        FrameEvidence {
            witness: self.witness.expect("accepted packet witness"),
            header: frame_header_from_parts(
                self.session_id,
                self.lane,
                self.source_role,
                target_role,
                self.frame_label,
            ),
            len: self.len,
            payload: self.payload,
        }
    }
}

/// A copied observation, without the queue ownership of a real packet. The
/// witness survives requeue and is never put on the wire or read by Hibana.
#[derive(Clone)]
#[allow(
    dead_code,
    reason = "shared carrier; actor qualification reads the copied packet evidence"
)]
pub(crate) struct FrameEvidence {
    pub(crate) witness: u64,
    pub(crate) header: FrameHeader,
    len: usize,
    payload: [u8; TEST_FRAME_PAYLOAD_CAPACITY],
}

impl FrameEvidence {
    #[allow(
        dead_code,
        reason = "shared carrier; actor qualification reads the copied packet payload"
    )]
    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload[..self.len]
    }
}

struct SessionWaiter {
    session_id: SessionId,
    lane: u8,
    waker: Waker,
}

pub(crate) struct RoleState {
    pub(crate) queue: FixedQueue<FrameOwned, TEST_QUEUE_CAPACITY>,
    waiters: Vec<SessionWaiter>,
    received: Vec<FrameEvidence>,
}

impl RoleState {
    fn new() -> Self {
        Self {
            queue: FixedQueue::new(),
            waiters: Vec::new(),
            received: Vec::new(),
        }
    }

    fn add_waiter(&mut self, session_id: SessionId, lane: u8, waker: Waker) {
        if let Some(waiter) = self
            .waiters
            .iter_mut()
            .find(|waiter| waiter.session_id == session_id && waiter.lane == lane)
        {
            waiter.waker = waker;
        } else {
            self.waiters.push(SessionWaiter {
                session_id,
                lane,
                waker,
            });
        }
    }

    fn take_waiter(&mut self, session_id: SessionId, lane: u8) -> Option<Waker> {
        let index = self
            .waiters
            .iter()
            .position(|waiter| waiter.session_id == session_id && waiter.lane == lane)?;
        Some(self.waiters.swap_remove(index).waker)
    }
}

struct RoleSlot {
    role: u8,
    state: RoleState,
}

pub(crate) struct TestState {
    roles: Vec<RoleSlot>,
    next_witness: u64,
    closed_ports: Vec<(SessionId, u8, u8)>,
}

impl TestState {
    fn new() -> Self {
        Self {
            roles: Vec::new(),
            next_witness: 0,
            closed_ports: Vec::new(),
        }
    }

    fn role_mut(&mut self, role: u8) -> &mut RoleState {
        if let Some(index) = self.roles.iter().position(|slot| slot.role == role) {
            return &mut self.roles[index].state;
        }
        self.roles.push(RoleSlot {
            role,
            state: RoleState::new(),
        });
        &mut self.roles.last_mut().expect("inserted role slot").state
    }

    fn enqueue(&mut self, role: u8, mut frame: FrameOwned) -> Option<Waker> {
        assert!(frame.witness.is_none(), "packet accepted twice");
        frame.witness = Some(self.next_witness);
        self.next_witness = self
            .next_witness
            .checked_add(1)
            .expect("packet witness exhausted");
        let role_state = self.role_mut(role);
        let session_id = frame.session_id;
        let lane = frame.lane;
        role_state.queue.push_back(frame);
        role_state.take_waiter(session_id, lane)
    }

    fn dequeue(&mut self, role: u8, session_id: SessionId, lane: u8) -> Option<FrameOwned> {
        self.role_mut(role)
            .queue
            .pop_front_matching(|frame| frame.session_id == session_id && frame.lane == lane)
    }

    fn requeue(&mut self, role: u8, frame: FrameOwned) {
        self.role_mut(role).queue.push_front(frame);
    }

    fn observe_received(&mut self, role: u8, frame: FrameEvidence) {
        let received = &mut self.role_mut(role).received;
        if let Some(previous) = received
            .iter_mut()
            .find(|previous| previous.header.bytes()[..5] == frame.header.bytes()[..5])
        {
            *previous = frame;
        } else {
            received.push(frame);
        }
    }

    fn add_waiter(&mut self, role: u8, session_id: SessionId, lane: u8, waker: Waker) {
        self.role_mut(role).add_waiter(session_id, lane, waker);
    }
    fn ensure_role(&mut self, role: u8) {
        let _ = self.role_mut(role);
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
    session_id: SessionId,
    role: u8,
    lane: u8,
    current: Option<FrameOwned>,
    current_hint_drained: std::cell::Cell<bool>,
}

pub(crate) struct TestTransport {
    state: Rc<RefCell<TestState>>,
    pub(crate) bindings: Option<Rc<[hibana::runtime::transport::EndpointBinding; 256]>>,
    blocked_sends: Rc<RefCell<Vec<BlockedSend>>>,
}

struct BlockedSend {
    role: u8,
    waker: Option<Waker>,
}

impl TestTransport {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(TestState::new())),
            blocked_sends: Rc::new(RefCell::new(Vec::new())),
            bindings: Some(Rc::new(core::array::from_fn(|_| {
                // SAFETY: each slot owns one isolated test resource. All
                // transport clones share these same authorities, and the
                // fixture's resources have no operations outside its Endpoints.
                unsafe { hibana::runtime::transport::EndpointBinding::new() }
            }))),
        }
    }

    /// Test I/O readiness only: no bytes enter the peer queue until resumed.
    pub(crate) fn pause_send(&self, role: u8) {
        let mut blocked = self.blocked_sends.borrow_mut();
        assert!(!blocked.iter().any(|entry| entry.role == role));
        blocked.push(BlockedSend { role, waker: None });
    }

    pub(crate) fn resume_send(&self) {
        let blocked = core::mem::take(&mut *self.blocked_sends.borrow_mut());
        assert!(!blocked.is_empty(), "paused sender");
        for entry in blocked {
            if let Some(waker) = entry.waker {
                waker.wake();
            }
        }
    }

    /// Orderly EOF on one receive port, including all its incoming sources.
    /// Accepted frames drain before Offline. This is not a per-sender close.
    #[allow(
        dead_code,
        reason = "shared carrier; owned closure qualification injects EOF"
    )]
    pub(crate) fn close_port(&self, session: SessionId, receiver: u8, lane: u8) {
        let waiter = {
            let mut state = self.state.borrow_mut();
            let direction = (session, receiver, lane);
            if !state.closed_ports.contains(&direction) {
                state.closed_ports.push(direction);
            }
            state.role_mut(receiver).take_waiter(session, lane)
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    /// Abruptly isolate the accepted tail. No discarded frame becomes a
    /// receive observation. All incoming sources of this port stay closed.
    #[allow(
        dead_code,
        reason = "shared carrier; owned failure qualification injects abort"
    )]
    pub(crate) fn abort_port(&self, session: SessionId, receiver: u8, lane: u8) {
        let waiter = {
            let mut state = self.state.borrow_mut();
            while state.dequeue(receiver, session, lane).is_some() {}
            let direction = (session, receiver, lane);
            if !state.closed_ports.contains(&direction) {
                state.closed_ports.push(direction);
            }
            state.role_mut(receiver).take_waiter(session, lane)
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub(crate) fn queue_is_empty(&self) -> bool {
        self.state
            .borrow()
            .roles
            .iter()
            .all(|slot| slot.state.queue.len == 0)
    }

    /// Copy accepted wire evidence without consuming, requeueing or waking it.
    pub(crate) fn queued_frame(&self, role: u8) -> (FrameHeader, Vec<u8>) {
        self.queued_frame_at(role, 0)
    }

    /// Inspect a pipeline without changing the transport's receive position.
    pub(crate) fn queued_frame_at(&self, role: u8, offset: usize) -> (FrameHeader, Vec<u8>) {
        let state = self.state.borrow();
        let queue = &state
            .roles
            .iter()
            .find(|slot| slot.role == role)
            .expect("observed receiver")
            .state
            .queue;
        assert!(offset < queue.len, "accepted frame ordinal to observe");
        let frame = queue.items[(queue.head + offset) % TEST_QUEUE_CAPACITY]
            .as_ref()
            .expect("accepted frame to observe");
        (
            frame_header_from_parts(
                frame.session_id,
                frame.lane,
                frame.source_role,
                role,
                frame.frame_label,
            ),
            frame.as_slice().to_vec(),
        )
    }

    #[allow(
        dead_code,
        reason = "shared carrier; actor qualification correlates physical acceptances"
    )]
    pub(crate) fn accepted_frame(&self, role: u8, offset: usize) -> FrameEvidence {
        let state = self.state.borrow();
        let queue = &state
            .roles
            .iter()
            .find(|slot| slot.role == role)
            .expect("observed receiver")
            .state
            .queue;
        assert!(offset < queue.len, "accepted frame ordinal to observe");
        queue.items[(queue.head + offset) % TEST_QUEUE_CAPACITY]
            .as_ref()
            .expect("accepted frame to observe")
            .evidence(role)
    }

    #[allow(
        dead_code,
        reason = "shared carrier; phased I/O qualification observes actual acceptance"
    )]
    pub(crate) fn newest_accepted_frame(&self, role: u8) -> FrameEvidence {
        let state = self.state.borrow();
        let queue = &state
            .roles
            .iter()
            .find(|slot| slot.role == role)
            .expect("observed receiver")
            .state
            .queue;
        assert!(queue.len > 0, "just accepted frame");
        queue.items[(queue.head + queue.len - 1) % TEST_QUEUE_CAPACITY]
            .as_ref()
            .expect("just accepted frame")
            .evidence(role)
    }

    #[allow(
        dead_code,
        reason = "shared carrier; actor qualification correlates physical receives"
    )]
    pub(crate) fn received_frame(&self, role: u8, session: SessionId, lane: u8) -> FrameEvidence {
        self.state
            .borrow()
            .roles
            .iter()
            .find(|slot| slot.role == role)
            .expect("observed receiver")
            .state
            .received
            .iter()
            .find(|frame| {
                let bytes = frame.header.bytes();
                bytes[..4] == session.raw().to_be_bytes() && bytes[4] == lane
            })
            .expect("actual returned frame")
            .clone()
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

    /// Delay one accepted frame on the simulated network, retaining its bytes.
    /// The returned one-shot delivery restores FIFO position and readiness.
    #[allow(
        dead_code,
        reason = "shared carrier; only the nested-prefix suite delays delivery"
    )]
    pub(crate) fn hold_next_frame(
        &self,
        session_id: SessionId,
        role: u8,
        lane: u8,
    ) -> impl FnOnce() + 'static {
        let frame = self
            .state
            .borrow_mut()
            .dequeue(role, session_id, lane)
            .expect("accepted frame to delay");
        let state = self.state.clone();
        move || {
            let wake = {
                let mut state = state.borrow_mut();
                state.requeue(role, frame);
                state.role_mut(role).take_waiter(session_id, lane)
            };
            if let Some(wake) = wake {
                wake.wake();
            }
        }
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
        let waiter = {
            let mut state = self.state.borrow_mut();
            if state
                .closed_ports
                .contains(&(frame.session_id, role, frame.lane))
            {
                return Poll::Ready(Err(TransportError::Offline));
            }
            state.enqueue(role, frame)
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
        Poll::Ready(Ok(()))
    }

    pub(crate) fn cancel_send_staged(&self, tx: &mut TestTx) {
        tx.pending_role = None;
        tx.pending_frame = None;
    }

    pub(crate) fn poll_recv_current<'a>(
        &self,
        rx: &'a mut TestRx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        if rx.current.is_none() {
            let dequeued = {
                let mut state = rx.state.borrow_mut();
                if let Some(frame) = state.dequeue(rx.role, rx.session_id, rx.lane) {
                    Some(frame)
                } else if state
                    .closed_ports
                    .contains(&(rx.session_id, rx.role, rx.lane))
                {
                    return Poll::Ready(Err(TransportError::Offline));
                } else {
                    state.add_waiter(rx.role, rx.session_id, rx.lane, cx.waker().clone());
                    None
                }
            };
            if let Some(frame) = dequeued {
                rx.state
                    .borrow_mut()
                    .observe_received(rx.role, frame.evidence(rx.role));
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

    pub(crate) fn open_rx(&self, session_id: SessionId, role: u8, lane: u8) -> TestRx<'_> {
        self.state.borrow_mut().ensure_role(role);
        TestRx {
            state: self.state.as_ref(),
            session_id,
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
            bindings: self.bindings.clone(),
            blocked_sends: Rc::clone(&self.blocked_sends),
        }
    }
}

const _: fn(&TestTransport) -> bool = TestTransport::queue_is_empty;
const _: fn(&TestTransport, u8) = TestTransport::pause_send;
const _: fn(&TestTransport) = TestTransport::resume_send;
const _: fn(&TestTransport, u8) -> (FrameHeader, Vec<u8>) = TestTransport::queued_frame;
const _: fn(&TestTransport, u8, usize) -> (FrameHeader, Vec<u8>) = TestTransport::queued_frame_at;
const _: fn(&TestTransport, u8, usize) = TestTransport::truncate_next_payload;
const _: for<'a> fn(&'a TestTransport, SessionId, u8, u8) -> TestRx<'a> = TestTransport::open_rx;
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

    fn initial_endpoint_binding(
        &self,
        role: u8,
    ) -> Option<&hibana::runtime::transport::EndpointBinding> {
        self.bindings
            .as_ref()
            .map(|bindings| &bindings[usize::from(role)])
    }

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id();
        let lane = port.lane();
        self.state.borrow_mut().ensure_role(local_role);
        (
            TestTx {
                session_id,
                local_role,
                pending_role: None,
                pending_frame: None,
            },
            TestRx {
                state: self.state.as_ref(),
                session_id,
                role: local_role,
                lane,
                current: None,
                current_hint_drained: std::cell::Cell::new(false),
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: hibana::runtime::transport::Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        // Each repoll must encode the same frame, even after another view has
        // used the runtime's temporary scratch buffer in between.
        if let Some(frame) = &tx.pending_frame {
            assert_eq!(
                (
                    tx.pending_role,
                    frame.session_id,
                    frame.lane,
                    frame.source_role,
                    frame.frame_label
                ),
                (
                    Some(outgoing.target_role()),
                    tx.session_id,
                    outgoing.lane(),
                    tx.local_role,
                    outgoing.frame_label().raw()
                ),
                "pending frame header changed",
            );
            assert_eq!(
                frame.as_slice(),
                outgoing.payload().as_bytes(),
                "pending payload changed"
            );
        }
        self.stage_send(
            tx,
            outgoing.target_role(),
            outgoing.lane(),
            outgoing.frame_label().raw(),
            outgoing.payload().as_bytes(),
        );
        if let Some(blocked) = self
            .blocked_sends
            .borrow_mut()
            .iter_mut()
            .find(|entry| entry.role == tx.local_role)
        {
            blocked.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        self.poll_send_staged(tx)
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.poll_recv_current(rx, cx)
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
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
