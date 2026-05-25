mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    mem::{MaybeUninit, size_of, size_of_val},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use common::{TestTransport, TestTransportError, TestTransportMetrics, TestTx};
use hibana::{
    g::{self, Msg, Role},
    integration::program::{RoleProgram, project},
    integration::{
        SessionKit, SessionKitStorage,
        binding::{
            BindingSlot, NoBinding,
            advanced::{Channel, IngressEvidence, TransportOpsError},
        },
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
    ) -> Result<Payload<'a>, TransportOpsError> {
        Err(TransportOpsError::ChannelNotFound)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn hibana::integration::policy::PolicySignalsProvider> {
        None
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
            has_fin: false,
            channel: Channel::new(11),
        })
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        scratch: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        self.last_recv_channel.set(Some(channel));
        scratch[..4].copy_from_slice(b"bind");
        Ok(Payload::new(&scratch[..4]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn hibana::integration::policy::PolicySignalsProvider> {
        None
    }
}

const MANUAL_WIRE_CONTROL_LOGICAL: u8 = 122;
const MANUAL_WIRE_ABORT_ACK_LOGICAL: u8 = 123;
const MANUAL_WIRE_ONE_SHOT_ABORT_ACK_LOGICAL: u8 = 124;
const ABORT_ACK_ID: u16 = 0x0201;
const MANUAL_TOKEN_NONCE_LEN: usize = 16;
const MANUAL_TOKEN_HEADER_LEN: usize = 40;
const MANUAL_TOKEN_LEN: usize = MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN;

fn encode_manual_cap_header(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    role: u8,
    tag: u8,
    op: ControlOp,
    path: ControlPath,
    shot: CapShot,
    scope_kind: ControlScopeKind,
    flags: u8,
    scope_id: u16,
    epoch: u16,
    handle: [u8; CAP_HANDLE_LEN],
) -> [u8; MANUAL_TOKEN_HEADER_LEN] {
    let mut header = [0u8; MANUAL_TOKEN_HEADER_LEN];
    header[0] = 1;
    header[1..5].copy_from_slice(&sid.raw().to_be_bytes());
    header[5] = lane.as_wire();
    header[6] = role;
    header[7] = tag;
    header[8] = op.as_u8();
    header[9] = path.as_u8();
    header[10] = shot.as_u8();
    header[11] = scope_kind as u8;
    header[12] = flags;
    header[13..15].copy_from_slice(&scope_id.to_be_bytes());
    header[15..17].copy_from_slice(&epoch.to_be_bytes());
    header[17..].copy_from_slice(&handle);
    header
}

#[test]
fn add_rendezvous_from_config_returns_attach_error_at_callsite() {
    let clock = CounterClock::new();
    let mut tap_buf = [TapEvent::zero(); 128];
    let mut slab = [0u8; 4096];
    let mut kit_storage =
        SessionKitStorage::<TestTransport, DefaultLabelUniverse, CounterClock, 0>::uninit();
    let kit = kit_storage.init();
    let config = Config::from_resources((&mut tap_buf, &mut slab), clock);

    let add_line = line!() + 2;
    let error = kit
        .add_rendezvous_from_config(config, TestTransport::default())
        .expect_err("zero-capacity kit must reject rendezvous registration");

    assert_eq!(error.operation(), "add_rendezvous");
    assert!(error.file().ends_with("tests/cursor_send_recv.rs"));
    assert_eq!(error.line(), add_line);
}

fn assert_progress_invariant_fault(error: &hibana::EndpointError) {
    let rendered = format!("{error:?}");
    if rendered.contains("SessionFault") {
        assert!(
            rendered.contains("ProgressInvariantViolated"),
            "progress invariant poison must preserve terminal cause: {rendered}"
        );
    } else {
        assert!(
            rendered.contains("PhaseInvariant"),
            "first progress invariant fault must preserve root evidence: {rendered}"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualWireControl;

impl ResourceKind for ManualWireControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x72;
    const NAME: &'static str = "ManualWireControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_be_bytes());
        out[4..6].copy_from_slice(&handle.1.to_be_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_be_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = 0x0472;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        session: SessionId,
        lane: hibana::integration::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
        (session.raw(), lane.as_wire() as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualWireAbortAckControl;

impl ResourceKind for ManualWireAbortAckControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x74;
    const NAME: &'static str = "ManualWireAbortAckControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_le_bytes());
        out[4..6].copy_from_slice(&handle.1.to_le_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_le_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireAbortAckControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = ABORT_ACK_ID;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::AbortAck;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        session: SessionId,
        lane: hibana::integration::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
        (session.raw(), lane.as_wire() as u16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualWireOneShotAbortAckControl;

impl ResourceKind for ManualWireOneShotAbortAckControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x75;
    const NAME: &'static str = "ManualWireOneShotAbortAckControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_le_bytes());
        out[4..6].copy_from_slice(&handle.1.to_le_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_le_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireOneShotAbortAckControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = ABORT_ACK_ID;
    const SHOT: CapShot = CapShot::One;
    const OP: ControlOp = ControlOp::AbortAck;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(
        session: SessionId,
        lane: hibana::integration::ids::Lane,
        _scope: ScopeId,
    ) -> Self::Handle {
        (session.raw(), lane.as_wire() as u16)
    }
}

fn manual_wire_token(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
) -> GenericCapToken<ManualWireControl> {
    let handle = ManualWireControl::encode_handle(&(sid.raw(), lane.as_wire() as u16));
    let header = encode_manual_cap_header(
        sid,
        lane,
        peer,
        ManualWireControl::TAG,
        ManualWireControl::OP,
        ManualWireControl::PATH,
        ManualWireControl::SHOT,
        ManualWireControl::SCOPE,
        0,
        ScopeId::generic(0).local_ordinal(),
        0,
        handle,
    );

    let mut bytes = [0u8; MANUAL_TOKEN_LEN];
    bytes[..MANUAL_TOKEN_NONCE_LEN].copy_from_slice(&[0xAB; MANUAL_TOKEN_NONCE_LEN]);
    bytes[MANUAL_TOKEN_NONCE_LEN..MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN]
        .copy_from_slice(&header);
    GenericCapToken::from_bytes(bytes)
}

fn manual_wire_abort_ack_token(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    manual_wire_abort_ack_token_for::<ManualWireAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        sid.raw(),
        lane.as_wire() as u16,
    )
}

fn manual_wire_abort_ack_token_with_handle(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    manual_wire_abort_ack_token_for::<ManualWireAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        handle_sid,
        handle_lane,
    )
}

fn manual_wire_one_shot_abort_ack_token(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
) -> GenericCapToken<ManualWireOneShotAbortAckControl> {
    manual_wire_abort_ack_token_for::<ManualWireOneShotAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        sid.raw(),
        lane.as_wire() as u16,
    )
}

fn manual_wire_abort_ack_token_for<K>(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<K>
where
    K: ControlResourceKind + ResourceKind<Handle = (u32, u16)>,
{
    let handle = K::encode_handle(&(handle_sid, handle_lane));
    let header = encode_manual_cap_header(
        sid,
        lane,
        peer,
        K::TAG,
        K::OP,
        K::PATH,
        K::SHOT,
        K::SCOPE,
        0,
        scope_id,
        epoch,
        handle,
    );

    let mut bytes = [0u8; MANUAL_TOKEN_LEN];
    bytes[..MANUAL_TOKEN_NONCE_LEN].copy_from_slice(&[0xCD; MANUAL_TOKEN_NONCE_LEN]);
    bytes[MANUAL_TOKEN_NONCE_LEN..MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN]
        .copy_from_slice(&header);
    GenericCapToken::from_bytes(bytes)
}

type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

#[derive(Clone)]
struct AuditOrderTransport {
    inner: TestTransport,
    watch_audit: Rc<Cell<bool>>,
    requeued: Rc<Cell<bool>>,
    audit_before_requeue: Rc<Cell<bool>>,
}

impl Default for AuditOrderTransport {
    fn default() -> Self {
        Self {
            inner: TestTransport::default(),
            watch_audit: Rc::new(Cell::new(false)),
            requeued: Rc::new(Cell::new(false)),
            audit_before_requeue: Rc::new(Cell::new(false)),
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

    fn start_audit_order_check(&self) {
        self.requeued.set(false);
        self.audit_before_requeue.set(false);
        self.watch_audit.set(true);
    }

    fn audit_observed_before_requeue(&self) -> bool {
        self.audit_before_requeue.get()
    }

    fn record_audit_observation(&self) {
        if self.watch_audit.get() && !self.requeued.get() {
            self.audit_before_requeue.set(true);
        }
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
    type Metrics = TestTransportMetrics;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        self.inner.poll_send(tx, outgoing, context)
    }

    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>) {
        self.inner.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.inner.poll_recv(rx, context)
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        self.requeued.set(true);
        self.inner.requeue(rx);
    }

    fn drain_events(&self, emit: &mut dyn FnMut(hibana::integration::transport::TransportEvent)) {
        self.record_audit_observation();
        self.inner.drain_events(emit);
    }

    fn recv_frame_hint<'a>(
        &'a self,
        rx: &'a Self::Rx<'a>,
    ) -> Option<hibana::integration::transport::FrameLabel> {
        self.inner.recv_frame_hint(rx)
    }

    fn metrics(&self) -> Self::Metrics {
        self.record_audit_observation();
        self.inner.metrics()
    }
}

struct DeadlineTestTransport(TestTransport);

impl Default for DeadlineTestTransport {
    fn default() -> Self {
        Self(TestTransport::default())
    }
}

impl Clone for DeadlineTestTransport {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Transport for DeadlineTestTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = common::TestRx<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.0.open(port)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        self.0.poll_send(tx, outgoing, context)
    }

    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>) {
        self.0.cancel_send(tx);
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.0.poll_recv(rx, context)
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        self.0.requeue(rx);
    }

    fn drain_events(&self, emit: &mut dyn FnMut(hibana::integration::transport::TransportEvent)) {
        self.0.drain_events(emit);
    }

    fn recv_frame_hint<'a>(
        &'a self,
        rx: &'a Self::Rx<'a>,
    ) -> Option<hibana::integration::transport::FrameLabel> {
        self.0.recv_frame_hint(rx)
    }

    fn metrics(&self) -> Self::Metrics {
        self.0.metrics()
    }

    fn operational_deadline_ticks(&self) -> Option<u32> {
        Some(1)
    }
}

#[derive(Clone)]
struct DeadlinePendingTransport {
    inner: TestTransport,
    cancel_count: Rc<Cell<usize>>,
    deadline_ticks: Option<u32>,
}

impl Default for DeadlinePendingTransport {
    fn default() -> Self {
        Self {
            inner: TestTransport::default(),
            cancel_count: Rc::new(Cell::new(0)),
            deadline_ticks: Some(1),
        }
    }
}

impl DeadlinePendingTransport {
    fn without_deadline() -> Self {
        Self {
            deadline_ticks: None,
            ..Self::default()
        }
    }

    fn cancel_count(&self) -> Rc<Cell<usize>> {
        self.cancel_count.clone()
    }

    fn queue_is_empty(&self) -> bool {
        self.inner.queue_is_empty()
    }
}

impl Transport for DeadlinePendingTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = common::TestRx<'a>
    where
        Self: 'a;
    type Metrics = TestTransportMetrics;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.inner.open(port)
    }

    fn poll_send<'a, 'f>(
        &'a self,
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

    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>) {
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

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        self.inner.requeue(rx);
    }

    fn drain_events(&self, emit: &mut dyn FnMut(hibana::integration::transport::TransportEvent)) {
        self.inner.drain_events(emit);
    }

    fn recv_frame_hint<'a>(
        &'a self,
        rx: &'a Self::Rx<'a>,
    ) -> Option<hibana::integration::transport::FrameLabel> {
        self.inner.recv_frame_hint(rx)
    }

    fn metrics(&self) -> Self::Metrics {
        self.inner.metrics()
    }

    fn operational_deadline_ticks(&self) -> Option<u32> {
        self.deadline_ticks
    }
}

type DeadlineTestKit = SessionKit<
    'static,
    DeadlineTestTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

type AuditOrderKit = SessionKit<
    'static,
    AuditOrderTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

type DeadlinePendingKit = SessionKit<
    'static,
    DeadlinePendingTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

#[derive(Clone, Copy, Debug, Default)]
struct LowLabelUniverse;

impl LabelUniverse for LowLabelUniverse {
    const MAX_LABEL: u8 = 127;
}

type LowLabelKit = SessionKit<'static, TestTransport, LowLabelUniverse, CounterClock, 2>;

// `Endpoint<'r, ROLE>` is already role-only opaque. Keep the measured bound
// tighter than the public v3 contract (`<= 40`) so regressions trip early even
// before the remaining future/branch compression lands.
const ENDPOINT_BYTES_MAX: usize = 24;
const SEND_FUTURE_BYTES_MAX: usize = 48;
const RECV_FUTURE_BYTES_MAX: usize = 48;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static LOW_LABEL_SESSION_SLOT: UnsafeCell<MaybeUninit<LowLabelKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static DEADLINE_SESSION_SLOT: UnsafeCell<MaybeUninit<DeadlineTestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static AUDIT_ORDER_SESSION_SLOT: UnsafeCell<MaybeUninit<AuditOrderKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static DEADLINE_PENDING_SESSION_SLOT: UnsafeCell<MaybeUninit<DeadlinePendingKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
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
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
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

                let valid = manual_wire_abort_ack_token(
                    sid,
                    hibana::integration::ids::Lane::new(0),
                    1,
                    0,
                    0,
                );
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
            },
        );
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
#[path = "cursor_send_recv/deadlines.rs"]
mod deadlines;
#[path = "cursor_send_recv/direct_recv.rs"]
mod direct_recv;
#[path = "cursor_send_recv/manual_wire.rs"]
mod manual_wire;
#[path = "cursor_send_recv/send_recv.rs"]
mod send_recv;
