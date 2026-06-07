mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use common::{TestTransport, TestTransportError, TestTx};
use hibana::{
    g::{self, Msg},
    integration::program::{RoleProgram, project},
    integration::{
        SessionKitStorage,
        ids::SessionId,
        runtime::{Config, CounterClock, LabelUniverse},
        transport::{IngressEvidence, Outgoing, ReceivedFrame, Transport},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_resident_tls_ref;

#[derive(Clone, Copy)]
struct FramePayload([u8; 4]);

impl WireEncode for FramePayload {
    fn encoded_len(&self) -> Option<usize> {
        Some(self.0.len())
    }

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

    fn validate_payload(_input: Payload<'_>) -> Result<(), CodecError> {
        Ok(())
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input
    }
}

type TestKitStorage = SessionKitStorage<
    'static,
    TestTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

#[derive(Clone)]
struct PendingCancelTransport {
    inner: TestTransport,
    cancel_count: Rc<Cell<usize>>,
}

impl Default for PendingCancelTransport {
    fn default() -> Self {
        Self {
            inner: TestTransport::default(),
            cancel_count: Rc::new(Cell::new(0)),
        }
    }
}

impl PendingCancelTransport {
    fn cancel_count(&self) -> Rc<Cell<usize>> {
        self.cancel_count.clone()
    }

    fn queue_is_empty(&self) -> bool {
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
        = common::TestRx<'a>
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
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
            outgoing.peer(),
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

type PendingCancelKitStorage = SessionKitStorage<
    'static,
    PendingCancelTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

#[derive(Clone, Copy, Debug, Default)]
struct LowLabelUniverse;

impl LabelUniverse for LowLabelUniverse {
    const MAX_LABEL: u8 = 127;
}

type LowLabelKitStorage =
    SessionKitStorage<'static, TestTransport, LowLabelUniverse, CounterClock, 2>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static LOW_LABEL_SESSION_SLOT: UnsafeCell<LowLabelKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static PENDING_CANCEL_SESSION_SLOT: UnsafeCell<PendingCancelKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

#[path = "cursor_send_recv/codec_demux.rs"]
mod codec_demux;
#[path = "cursor_send_recv/direct_recv.rs"]
mod direct_recv;
#[path = "cursor_send_recv/send_recv.rs"]
mod send_recv;
#[path = "cursor_send_recv/session_lifecycle.rs"]
mod session_lifecycle;
