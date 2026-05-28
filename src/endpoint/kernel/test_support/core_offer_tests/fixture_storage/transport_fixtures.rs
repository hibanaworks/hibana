use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct HintOnlyTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) worker_hint: u8,
}

impl HintOnlyTransport {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn new(
        worker_hint: u8,
    ) -> Self {
        Self { worker_hint }
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct HintOnlyRx {
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) hint: Cell<u8>,
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
        Poll::Ready(Ok(Payload::new(&[0u8; 1])))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract implementation: `poll_recv` is stateless and leaves the
    // fixture frame observable without moving it between queues.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        // Nothing to restore.
    }

    fn recv_frame_hint<'a>(&self, rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        let hint = rx.hint.replace(HINT_NONE);
        if hint == HINT_NONE {
            None
        } else {
            Some(FrameLabel::new(hint))
        }
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
            Poll::Ready(Ok(Payload::new(&[])))
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
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        unreachable!("this fixture never exercises endpoint rollback")
    }

    fn recv_frame_hint<'a>(&self, rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
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

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn recv_frame_hint<'a>(&self, rx: &mut Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
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
}
