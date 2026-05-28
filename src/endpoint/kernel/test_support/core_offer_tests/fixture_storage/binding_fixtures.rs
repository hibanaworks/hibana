use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct PendingTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) state:
        &'static PendingTransportState,
}

impl PendingTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn new(
        state: &'static PendingTransportState,
    ) -> Self {
        Self { state }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_count(
        &self,
    ) -> usize {
        self.state.polls.get()
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn requeue_count(
        &self,
    ) -> usize {
        self.state.requeues.get()
    }
}

#[derive(Default)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct PendingTransportState {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) polls: Cell<usize>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) requeues: Cell<usize>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) ready: Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) recv_parked: Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) hint_drains_while_recv_parked:
        Cell<usize>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) panic_on_hint_drain_while_recv_parked:
        Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) waker:
        UnsafeCell<Option<Waker>>,
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct DeferredIngressState {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) incoming:
        UnsafeCell<FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) recv_payloads:
        UnsafeCell<FixedQueue<FixedPayload, TEST_BINDING_QUEUE_CAPACITY>>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) available: Cell<usize>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) requeues: Cell<usize>,
}

impl DeferredIngressState {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn new() -> Self {
        Self {
            incoming: UnsafeCell::new(FixedQueue::new()),
            recv_payloads: UnsafeCell::new(FixedQueue::new()),
            available: Cell::new(0),
            requeues: Cell::new(0),
        }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn push_incoming(
        &self,
        evidence: IngressEvidence,
    ) {
        unsafe {
            (&mut *self.incoming.get()).push_back(evidence);
        }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn push_recv_payload(
        &self,
        payload: FixedPayload,
    ) {
        unsafe {
            (&mut *self.recv_payloads.get()).push_back(payload);
        }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn pop_incoming(
        &self,
    ) -> Option<IngressEvidence> {
        unsafe { (&mut *self.incoming.get()).pop_front() }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn pop_recv_payload(
        &self,
    ) -> Option<FixedPayload> {
        unsafe { (&mut *self.recv_payloads.get()).pop_front() }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn requeue_count(
        &self,
    ) -> usize {
        self.requeues.get()
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct DeferredIngressBinding {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) state:
        &'static DeferredIngressState,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) polls: Cell<usize>,
}

impl DeferredIngressBinding {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_incoming_and_payloads(
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
    ) -> Result<Payload<'a>, BindingError> {
        let Some(payload) = self.state.pop_recv_payload() else {
            return Ok(Payload::new(&[]));
        };
        let payload = payload.as_slice();
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(Payload::new(&buf[..len]))
    }

    fn policy_signals(&self) -> crate::transport::context::PolicySignals {
        crate::transport::context::PolicySignals::ZERO
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct DeferredIngressTransport
{
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) state:
        &'static DeferredIngressState,
}

impl DeferredIngressTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn new(
        state: &'static DeferredIngressState,
    ) -> Self {
        Self { state }
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct DeferredIngressRx;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct PendingRx;

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

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((local_role, session_id, lane));
        ((), PendingRx)
    }

    fn poll_send<'a, 'f>(
        &self,
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

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn recv_frame_hint<'a>(&self, _rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        None
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

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id().raw();
        let lane = port.lane().as_wire();
        core::hint::black_box((local_role, session_id, lane));
        ((), DeferredIngressRx)
    }

    fn poll_send<'a, 'f>(
        &self,
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

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn recv_frame_hint<'a>(&self, _rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        None
    }
}
