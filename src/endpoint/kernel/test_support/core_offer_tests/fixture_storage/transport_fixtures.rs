use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
use crate::transport::ReceivedPayload;
#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct HintOnlyTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) worker_hint: u8,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) payload_frame_label: u8,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) observe_payload_frame:
        bool,
}

impl HintOnlyTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn new(
        worker_hint: u8,
    ) -> Self {
        Self {
            worker_hint,
            payload_frame_label: 0,
            observe_payload_frame: false,
        }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn with_payload_frame_label(
        worker_hint: u8,
        payload_frame_label: u8,
    ) -> Self {
        Self {
            worker_hint,
            payload_frame_label,
            observe_payload_frame: true,
        }
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct HintOnlyRx {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) hint: Cell<u8>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) payload_frame_label: u8,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) observe_payload_frame:
        bool,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) payload_staged: Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) session_id:
        crate::control::types::SessionId,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) lane:
        crate::control::types::Lane,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct HintPendingTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) state:
        &'static PendingTransportState,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) worker_hint: u8,
}
#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct FreshHintPendingTransport
{
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) state:
        &'static PendingTransportState,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) worker_hint: u8,
}

impl HintPendingTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn new(
        state: &'static PendingTransportState,
        worker_hint: u8,
    ) -> Self {
        Self { state, worker_hint }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_count(
        &self,
    ) -> usize {
        self.state.polls.get()
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn assert_no_hint_drain_while_recv_parked(
        &self,
    ) {
        assert_eq!(
            self.state.hint_drains_while_recv_parked.get(),
            0,
            "offer must not drain route hints from a lane whose recv future is parked"
        );
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct HintPendingRx {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) hint: Cell<u8>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) payload_staged: Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) session_id:
        crate::control::types::SessionId,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) lane:
        crate::control::types::Lane,
}

impl FreshHintPendingTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn new(
        state: &'static PendingTransportState,
        worker_hint: u8,
    ) -> Self {
        Self { state, worker_hint }
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn poll_count(
        &self,
    ) -> usize {
        self.state.polls.get()
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn hint_drain_count(
        &self,
    ) -> usize {
        self.state.hint_drains_while_recv_parked.get()
    }

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn requeue_count(
        &self,
    ) -> usize {
        self.state.requeues.get()
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct FreshHintPendingRx {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) hint: Cell<u8>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) payload_staged: Cell<bool>,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) session_id:
        crate::control::types::SessionId,
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) lane:
        crate::control::types::Lane,
}

fn fixture_header(
    session_id: crate::control::types::SessionId,
    lane: crate::control::types::Lane,
    peer_role: u8,
    frame_label: u8,
) -> crate::transport::FrameHeader {
    let source_role = if peer_role == 0 { 1 } else { 0 };
    fixture_header_with_source(session_id, lane, source_role, peer_role, frame_label)
}

fn fixture_header_with_source(
    session_id: crate::control::types::SessionId,
    lane: crate::control::types::Lane,
    source_role: u8,
    peer_role: u8,
    frame_label: u8,
) -> crate::transport::FrameHeader {
    crate::transport::FrameHeader::new(
        session_id,
        lane,
        source_role,
        peer_role,
        FrameLabel::new(frame_label),
    )
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

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id();
        let lane = port.lane();
        core::hint::black_box((session_id.raw(), lane.as_wire()));
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintOnlyRx {
                hint: Cell::new(hint),
                payload_frame_label: self.payload_frame_label,
                observe_payload_frame: self.observe_payload_frame,
                payload_staged: Cell::new(false),
                session_id,
                lane,
            },
        )
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
        rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedPayload<'a>, Self::Error>> {
        let hint = rx.hint.get();
        let frame_label = if hint == HINT_NONE {
            rx.payload_frame_label
        } else {
            hint
        };
        core::hint::black_box((rx.session_id.raw(), rx.lane.as_wire(), frame_label));
        rx.payload_staged.set(true);
        let payload = Payload::new(&[0u8; 1]);
        if hint != HINT_NONE || rx.observe_payload_frame {
            Poll::Ready(Ok(ReceivedPayload::frame(
                fixture_header(rx.session_id, rx.lane, 1, frame_label),
                payload,
            )))
        } else {
            Poll::Ready(Ok(ReceivedPayload::new(payload)))
        }
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract implementation: `poll_recv` is stateless and leaves the
    // fixture frame observable without moving it between queues.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        // Nothing to restore.
        Ok(())
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

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id();
        let lane = port.lane();
        core::hint::black_box((session_id.raw(), lane.as_wire()));
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintPendingRx {
                hint: Cell::new(hint),
                payload_staged: Cell::new(false),
                session_id,
                lane,
            },
        )
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
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedPayload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            let hint = rx.hint.get();
            let frame_label = if hint == HINT_NONE { 0 } else { hint };
            core::hint::black_box((rx.session_id.raw(), rx.lane.as_wire(), frame_label));
            rx.payload_staged.set(true);
            let payload = Payload::new(&[]);
            if hint == HINT_NONE {
                Poll::Ready(Ok(ReceivedPayload::new(payload)))
            } else {
                Poll::Ready(Ok(ReceivedPayload::frame(
                    fixture_header(rx.session_id, rx.lane, 1, hint),
                    payload,
                )))
            }
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        unreachable!("this fixture never exercises endpoint rollback")
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

    fn open<'a>(&'a self, port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role = port.local_role();
        let session_id = port.session_id();
        let lane = port.lane();
        core::hint::black_box((local_role, session_id.raw(), lane.as_wire()));
        (
            (),
            FreshHintPendingRx {
                hint: Cell::new(HINT_NONE),
                payload_staged: Cell::new(false),
                session_id,
                lane,
            },
        )
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
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedPayload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            rx.hint.set(self.worker_hint);
            rx.payload_staged.set(true);
            let source_role = self.state.source_role.get();
            core::hint::black_box((
                rx.session_id.raw(),
                rx.lane.as_wire(),
                source_role,
                self.worker_hint,
            ));
            Poll::Ready(Ok(ReceivedPayload::frame(
                fixture_header_with_source(
                    rx.session_id,
                    rx.lane,
                    source_role,
                    1,
                    self.worker_hint,
                ),
                Payload::new(&[0x5a]),
            )))
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
        Ok(())
    }
}
