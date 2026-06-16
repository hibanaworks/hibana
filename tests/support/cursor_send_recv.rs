#![allow(dead_code, unused_imports)]

pub(crate) use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
pub(crate) use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

pub(crate) use crate::common::{TestTransport, TestTransportError, TestTx};
pub(crate) use crate::runtime_support::with_runtime_workspace;
pub(crate) use crate::tls_ref_support::with_resident_tls_ref;
pub(crate) use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
    runtime::{
        Config, SessionKitStorage,
        ids::SessionId,
        transport::{Outgoing, ReceivedFrame, Transport},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct FramePayload(pub(crate) [u8; 4]);

impl WireEncode for FramePayload {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < self.0.len() {
            return Err(CodecError::Truncated);
        }
        out[..self.0.len()].copy_from_slice(&self.0);
        Ok(self.0.len())
    }
}

impl WirePayload for FramePayload {
    type Decoded<'a> = Payload<'a>;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        match input.as_bytes().len() {
            4 => Ok(()),
            0..=3 => Err(CodecError::Truncated),
            _ => Err(CodecError::Malformed),
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input
    }
}

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

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
    type Error = TestTransportError;
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
    ) -> Poll<Result<(), Self::Error>>
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
    ) -> Poll<Result<ReceivedFrame<'a>, Self::Error>> {
        self.inner.poll_recv(rx, context)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        self.inner.requeue(rx)
    }
}

type PendingCancelKitStorage = SessionKitStorage<'static, PendingCancelTransport>;

std::thread_local! {
    pub(crate) static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    pub(crate) static PENDING_CANCEL_SESSION_SLOT: UnsafeCell<PendingCancelKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

pub(crate) fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}
