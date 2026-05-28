mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    mem::{size_of, size_of_val},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use common::{TestTransport, TestTransportError, TestTx};
use hibana::{
    g::{self, Msg, Role},
    integration::program::{RoleProgram, project},
    integration::{
        SessionKitStorage,
        binding::{BindingError, BindingSlot, Channel, IngressEvidence, NoBinding},
        cap::{
            CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
            control::{
                CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
            },
        },
        ids::SessionId,
        runtime::{Config, CounterClock, DefaultLabelUniverse, LabelUniverse, TapEvent},
        transport::{Outgoing, Transport},
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

struct DemuxOnlyBinding;

impl BindingSlot for DemuxOnlyBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        None
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _scratch: &'a mut [u8],
    ) -> Result<Payload<'a>, BindingError> {
        Err(BindingError::ChannelUnavailable)
    }

    fn route_policy_signals(&self) -> hibana::integration::policy::signals::PolicySignals<'_> {
        hibana::integration::policy::signals::PolicySignals::ZERO
    }
}

struct LateDirectRecvBinding {
    polls: Cell<usize>,
    last_recv_channel: Cell<Option<Channel>>,
}

impl LateDirectRecvBinding {
    const fn new() -> Self {
        Self {
            polls: Cell::new(0),
            last_recv_channel: Cell::new(None),
        }
    }

    fn poll_count(&self) -> usize {
        self.polls.get()
    }

    fn last_recv_channel(&self) -> Option<Channel> {
        self.last_recv_channel.get()
    }
}

impl BindingSlot for LateDirectRecvBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        let polls = self.polls.get();
        self.polls.set(polls.saturating_add(1));
        if polls == 0 {
            return None;
        }
        Some(IngressEvidence {
            frame_label: hibana::integration::transport::FrameLabel::new(0),
            instance: 0,
            channel: Channel::new(11),
        })
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        scratch: &'a mut [u8],
    ) -> Result<Payload<'a>, BindingError> {
        self.last_recv_channel.set(Some(channel));
        scratch[..4].copy_from_slice(b"bind");
        Ok(Payload::new(&scratch[..4]))
    }

    fn route_policy_signals(&self) -> hibana::integration::policy::signals::PolicySignals<'_> {
        hibana::integration::policy::signals::PolicySignals::ZERO
    }
}

#[path = "cursor_send_recv/manual_wire_support.rs"]
mod manual_wire_support;
use manual_wire_support::*;

type TestKitStorage = SessionKitStorage<
    'static,
    TestTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

#[derive(Clone)]
struct AuditOrderTransport {
    inner: TestTransport,
    requeued: Rc<Cell<bool>>,
}

impl Default for AuditOrderTransport {
    fn default() -> Self {
        Self {
            inner: TestTransport::default(),
            requeued: Rc::new(Cell::new(false)),
        }
    }
}

impl AuditOrderTransport {
    fn stage_send(&self, tx: &mut TestTx, role: u8, lane: u8, frame_label: u8, payload: &[u8]) {
        self.inner.stage_send(tx, role, lane, frame_label, payload);
    }

    fn poll_send_staged(&self, tx: &mut TestTx) -> Poll<Result<(), TestTransportError>> {
        self.inner.poll_send_staged(tx)
    }

    fn reset_requeue_check(&self) {
        self.requeued.set(false);
    }

    fn requeued(&self) -> bool {
        self.requeued.get()
    }
}

impl Transport for AuditOrderTransport {
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
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        self.inner.poll_send(tx, outgoing, context)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.inner.poll_recv(rx, context)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) {
        self.requeued.set(true);
        self.inner.requeue(rx);
    }

    fn recv_frame_hint<'a>(
        &self,
        rx: &mut Self::Rx<'a>,
    ) -> Option<hibana::integration::transport::FrameLabel> {
        self.inner.recv_frame_hint(rx)
    }
}

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
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.inner.poll_recv(rx, context)
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) {
        self.inner.requeue(rx);
    }

    fn recv_frame_hint<'a>(
        &self,
        rx: &mut Self::Rx<'a>,
    ) -> Option<hibana::integration::transport::FrameLabel> {
        self.inner.recv_frame_hint(rx)
    }
}

type AuditOrderKitStorage = SessionKitStorage<
    'static,
    AuditOrderTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

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

// `Endpoint<'r, ROLE>` is already role-only opaque. Keep the measured bound
// tighter than the public v3 contract (`<= 40`) so regressions trip early even
// before the remaining future/branch compression lands.
const ENDPOINT_BYTES_MAX: usize = 24;
const SEND_FUTURE_BYTES_MAX: usize = 48;
const RECV_FUTURE_BYTES_MAX: usize = 48;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static LOW_LABEL_SESSION_SLOT: UnsafeCell<LowLabelKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static AUDIT_ORDER_SESSION_SLOT: UnsafeCell<AuditOrderKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static PENDING_CANCEL_SESSION_SLOT: UnsafeCell<PendingCancelKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn assert_manual_wire_abort_ack_send_rejected(
    token: GenericCapToken<ManualWireAbortAckControl>,
    sid: SessionId,
) {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut _;
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<
                Role<0>,
                Role<1>,
                Msg<
                    { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                    GenericCapToken<ManualWireAbortAckControl>,
                    ManualWireAbortAckControl,
                >,
                0,
            >();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (unsafe { &mut *tap_ptr }, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(NoBinding)
                .expect("origin endpoint");
            let target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(NoBinding)
                .expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let flow = origin_endpoint
                .flow::<Msg<
                    { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                    GenericCapToken<ManualWireAbortAckControl>,
                    ManualWireAbortAckControl,
                >>()
                .expect("wire abort-ack flow");
            let send_line = line!() + 1;
            let err = futures::executor::block_on(flow.send(&token))
                .expect_err("bound mismatch must fail before transport");

            assert_eq!(err.operation(), "send");
            assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
            assert_eq!(err.line(), send_line);
            assert_progress_invariant_fault(&err);
            assert!(transport_queue_is_empty(&transport));

            let valid =
                manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(0), 1, 0, 0);
            match origin_endpoint.flow::<Msg<
                { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                GenericCapToken<ManualWireAbortAckControl>,
                ManualWireAbortAckControl,
            >>() {
                Ok(flow) => {
                    let err = futures::executor::block_on(flow.send(&valid))
                        .expect_err("same generation must not recover after send fault");
                    assert_progress_invariant_fault(&err);
                }
                Err(err) => {
                    assert_progress_invariant_fault(&err);
                }
            }
            assert!(transport_queue_is_empty(&transport));
        });
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID),
            "rejected explicit control send must not execute abort-ack",
        );
    });
}

#[path = "cursor_send_recv/codec_demux.rs"]
mod codec_demux;
#[path = "cursor_send_recv/direct_recv.rs"]
mod direct_recv;
#[path = "cursor_send_recv/manual_wire.rs"]
mod manual_wire;
#[path = "cursor_send_recv/send_recv.rs"]
mod send_recv;
#[path = "cursor_send_recv/session_lifecycle.rs"]
mod session_lifecycle;
