//! A reported I/O failure, independent of whether a peer is alive. This
//! adapter changes only test transport outcomes, never Endpoint admission.
use super::*;
use crate::{
    test_support::transport::{TestRx, TestTx},
    transport::{EndpointBinding, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
};
use core::task::{Context, Poll};
use std::{cell::RefCell, rc::Rc, vec::Vec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum At {
    SendAfterAcceptance,
    ReceiveBeforeDelivery,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Fault {
    pub(super) at: At,
    pub(super) error: TransportError,
    pub(super) session: SessionId,
    pub(super) view: u8,
    pub(super) lane: u8,
}

pub(super) struct Io {
    inner: TestTransport,
    actor: u8,
    failure: Option<(At, TransportError)>,
    pub(super) observed: Rc<RefCell<Vec<Fault>>>,
}

pub(super) struct Rx<'a> {
    inner: TestRx<'a>,
    port: PortOpen,
}

impl Io {
    pub(super) fn new(
        inner: TestTransport,
        actor: u8,
        failure: Option<(At, TransportError)>,
    ) -> Self {
        Self {
            inner,
            actor,
            failure,
            observed: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl Transport for Io {
    type Tx<'a> = TestTx;
    type Rx<'a> = Rx<'a>;

    fn initial_endpoint_binding(&self, role: u8) -> Option<&EndpointBinding> {
        if self.actor == 0 && role == 10 {
            self.inner.initial_endpoint_binding(role)
        } else {
            None
        }
    }

    fn open(&self, port: PortOpen) -> (Self::Tx<'_>, Self::Rx<'_>) {
        let (tx, rx) = self.inner.open(port);
        (tx, Rx { inner: rx, port })
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        let result = self.inner.poll_send(tx, outgoing, cx);
        if result == Poll::Ready(Ok(()))
            && let Some((At::SendAfterAcceptance, error)) = self.failure
        {
            self.observed.borrow_mut().push(Fault {
                at: At::SendAfterAcceptance,
                error,
                session: tx.session_id,
                view: tx.local_role,
                lane: outgoing.lane(),
            });
            return Poll::Ready(Err(error));
        }
        result
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if let Some((At::ReceiveBeforeDelivery, error)) = self.failure {
            self.observed.borrow_mut().push(Fault {
                at: At::ReceiveBeforeDelivery,
                error,
                session: rx.port.session_id(),
                view: rx.port.local_role(),
                lane: rx.port.lane(),
            });
            return Poll::Ready(Err(error));
        }
        self.inner.poll_recv_current(&mut rx.inner, cx)
    }

    fn cancel_send(&self, tx: &mut Self::Tx<'_>) {
        self.inner.cancel_send(tx);
    }

    fn requeue(&self, rx: &mut Self::Rx<'_>) -> Result<(), TransportError> {
        self.inner.requeue(&mut rx.inner)
    }
}
