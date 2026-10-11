use core::{
    cell::{Cell, UnsafeCell},
    task::{Context, Poll},
};
use std::rc::Rc;

use crate::common::{TestTransport, TestTx};
use hibana::runtime::{
    SessionKitStorage,
    transport::{Outgoing, ReceivedFrame, Transport, TransportError},
};

#[derive(Clone)]
pub(crate) struct PendingCancelTransport {
    inner: TestTransport,
    cancel_count: Rc<Cell<usize>>,
}

impl PendingCancelTransport {
    pub(crate) fn new() -> Self {
        Self {
            inner: TestTransport::new(),
            cancel_count: Rc::new(Cell::new(0)),
        }
    }

    pub(crate) fn cancel_count(&self) -> Rc<Cell<usize>> {
        self.cancel_count.clone()
    }

    pub(crate) fn queue_is_empty(&self) -> bool {
        self.inner.queue_is_empty()
    }
}

impl Transport for PendingCancelTransport {
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = crate::common::TestRx<'a>
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::runtime::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
        self.inner.stage_send(
            tx,
            outgoing.target_role(),
            outgoing.lane(),
            outgoing.frame_label().raw(),
            outgoing.payload().as_bytes(),
        );
        Poll::Pending
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.cancel_count.set(self.cancel_count.get() + 1);
        self.inner.cancel_send_staged(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        context: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        self.inner.poll_recv(rx, context)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        self.inner.requeue(rx)
    }
}

type PendingCancelKitStorage = SessionKitStorage<'static, PendingCancelTransport>;

std::thread_local! {
    pub(crate) static PENDING_CANCEL_SESSION_SLOT: UnsafeCell<PendingCancelKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}
