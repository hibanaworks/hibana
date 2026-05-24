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

#[test]
fn cursor_recv_can_return_borrowed_frame_views() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let borrowed_program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
                let borrowed_origin_program: RoleProgram<0> = project(&borrowed_program);
                let borrowed_target_program: RoleProgram<1> = project(&borrowed_program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(2);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&borrowed_origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&borrowed_target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<2, FramePayload>>()
                        .expect("send flow")
                        .send(&FramePayload(*b"hiba")),
                )
                .expect("send succeeds");
                let payload =
                    futures::executor::block_on(target_endpoint.recv::<Msg<2, FramePayload>>())
                        .expect("recv succeeds");
                assert_eq!(payload.as_bytes(), b"hiba");
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn direct_recv_requeues_transport_payload_when_binding_wins_after_poll_recv() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, FramePayload>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(20);
                let origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                core::hint::black_box(&origin_endpoint);
                let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(&mut *binding)
                    .expect("target endpoint");

                let mut tx = TestTx::default();
                transport.stage_send(&mut tx, 1, 0, 0, b"wire");
                assert!(matches!(
                    transport.poll_send_staged(&mut tx),
                    Poll::Ready(Ok(()))
                ));

                {
                    let payload =
                        futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                            .expect("binding-backed recv succeeds");
                    assert_eq!(
                        payload.as_bytes(),
                        b"bind",
                        "binding payload must be the committed recv source"
                    );
                }
                drop(target_endpoint);
                assert_eq!(
                    binding.poll_count(),
                    2,
                    "fixture must poll binding once before and once after transport recv"
                );
                assert_eq!(
                    binding.last_recv_channel(),
                    Some(Channel::new(11)),
                    "recv must read from the late binding evidence"
                );
                assert!(
                    !transport_queue_is_empty(&transport),
                    "transport payload polled before binding won must be requeued"
                );

                let mut rx = transport.open_rx_for_test(1, 0);
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                match transport.poll_recv_current(&mut rx, &mut context) {
                    Poll::Ready(Ok(payload)) => assert_eq!(payload.as_bytes(), b"wire"),
                    Poll::Ready(Err(err)) => panic!("requeued payload read failed: {err:?}"),
                    Poll::Pending => panic!("requeued payload was not available"),
                }
            },
        );
    });
}

#[test]
fn direct_recv_late_binding_requeues_before_endpoint_rx_audit_flush() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = AuditOrderTransport::default();
        with_resident_tls_ref(
            &AUDIT_ORDER_SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, FramePayload>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(22);
                let origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                core::hint::black_box(&origin_endpoint);
                let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(&mut *binding)
                    .expect("target endpoint");

                let mut tx = TestTx::default();
                transport.stage_send(&mut tx, 1, 0, 0, b"wire");
                assert!(matches!(
                    transport.poll_send_staged(&mut tx),
                    Poll::Ready(Ok(()))
                ));

                transport.start_audit_order_check();
                let payload =
                    futures::executor::block_on(target_endpoint.recv::<Msg<1, FramePayload>>())
                        .expect("binding-backed recv succeeds");
                assert_eq!(payload.as_bytes(), b"bind");
                assert!(
                    !transport.audit_observed_before_requeue(),
                    "EndpointRx audit must observe transport after late-binding requeue"
                );
            },
        );
    });
}

#[test]
fn direct_recv_does_not_requeue_transport_payload_when_late_binding_payload_fails_validation() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u64>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(21);
                let origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                core::hint::black_box(&origin_endpoint);
                let binding = Box::leak(Box::new(LateDirectRecvBinding::new()));
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(&mut *binding)
                    .expect("target endpoint");

                let mut tx = TestTx::default();
                transport.stage_send(&mut tx, 1, 0, 0, b"wire");
                assert!(matches!(
                    transport.poll_send_staged(&mut tx),
                    Poll::Ready(Ok(()))
                ));

                let err = futures::executor::block_on(target_endpoint.recv::<Msg<1, u64>>())
                    .expect_err("short binding payload must fail validation");
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("Codec"),
                    "first recv failure must preserve codec evidence: {rendered}"
                );
                assert!(
                    !rendered.contains("SessionFault"),
                    "first recv failure must not be replaced by session poison: {rendered}"
                );
                drop(target_endpoint);
                assert_eq!(
                    binding.poll_count(),
                    2,
                    "fixture must poll binding once before and once after transport recv"
                );
                assert_eq!(
                    binding.last_recv_channel(),
                    Some(Channel::new(11)),
                    "recv must read from the late binding evidence"
                );
                assert!(
                    transport_queue_is_empty(&transport),
                    "transport payload must not be requeued before a binding-backed recv commits"
                );
            },
        );
    });
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

#[test]
fn sequential_noncontiguous_lane_steps_progress_in_order() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::seq(
                    g::send::<Role<0>, Role<1>, Msg<31, u32>, 0>(),
                    g::seq(
                        g::send::<Role<0>, Role<1>, Msg<32, u32>, 1>(),
                        g::send::<Role<0>, Role<1>, Msg<33, u32>, 0>(),
                    ),
                );
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(31);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<31, u32>>()
                        .expect("lane 0 first flow")
                        .send(&31),
                )
                .expect("lane 0 first send");
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<32, u32>>()
                        .expect("lane 1 middle flow")
                        .send(&32),
                )
                .expect("lane 1 middle send");
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<33, u32>>()
                        .expect("lane 0 final flow")
                        .send(&33),
                )
                .expect("lane 0 final send");

                assert_eq!(
                    futures::executor::block_on(target_endpoint.recv::<Msg<31, u32>>())
                        .expect("lane 0 first recv"),
                    31
                );
                assert_eq!(
                    futures::executor::block_on(target_endpoint.recv::<Msg<32, u32>>())
                        .expect("lane 1 middle recv"),
                    32
                );
                assert_eq!(
                    futures::executor::block_on(target_endpoint.recv::<Msg<33, u32>>())
                        .expect("lane 0 final recv"),
                    33
                );
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

unsafe fn clone_count_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &COUNT_WAKER_VTABLE)
}

unsafe fn wake_count_waker(data: *const ()) {
    let count = unsafe { &*data.cast::<Cell<usize>>() };
    count.set(count.get() + 1);
}

unsafe fn drop_count_waker(data: *const ()) {
    core::hint::black_box(data);
}

static COUNT_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_count_waker,
    wake_count_waker,
    wake_count_waker,
    drop_count_waker,
);

fn counting_waker(count: &Cell<usize>) -> Waker {
    let data = core::ptr::from_ref(count).cast::<()>();
    unsafe { Waker::from_raw(RawWaker::new(data, &COUNT_WAKER_VTABLE)) }
}

#[test]
fn operational_deadline_poison_blocks_same_generation_progress() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = DeadlineTestTransport::default();
        with_resident_tls_ref(
            &DEADLINE_SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(201);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let mut recv_future =
                    std::pin::pin!(target_endpoint.recv::<Msg<2, FramePayload>>());
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                let mut recv_error = None;
                for attempt in 0..8 {
                    match recv_future.as_mut().poll(&mut context) {
                        Poll::Pending => {}
                        Poll::Ready(Ok(payload)) => {
                            core::hint::black_box(&payload);
                            panic!("recv unexpectedly progressed");
                        }
                        Poll::Ready(Err(error)) => {
                            assert_eq!(error.operation(), "recv");
                            assert!(
                                format!("{error:?}").contains("DeadlineExceeded"),
                                "recv error must keep deadline fault evidence: {error:?}"
                            );
                            recv_error = Some(error);
                            break;
                        }
                    }
                    assert!(attempt < 7, "deadline fuse did not trip");
                }
                drop(recv_future);
                assert!(recv_error.is_some(), "deadline fault must be observed");

                let flow_error = match origin_endpoint.flow::<Msg<2, FramePayload>>() {
                    Ok(_) => panic!("poisoned generation must not mint a same-generation flow"),
                    Err(error) => error,
                };
                assert_eq!(flow_error.operation(), "flow");
                assert!(
                    format!("{flow_error:?}").contains("DeadlineExceeded"),
                    "later same-generation operation must preserve original fault: {flow_error:?}"
                );
            },
        );
    });
}

#[test]
fn send_deadline_cancels_pending_transport_state_once() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = DeadlinePendingTransport::default();
        let cancel_count = transport.cancel_count();
        with_resident_tls_ref(
            &DEADLINE_PENDING_SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                            (tap_buf, slab),
                            CounterClock::new(),
                        ),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(202);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");

                let payload = FramePayload(*b"hiba");
                let mut send_future = std::pin::pin!(
                    origin_endpoint
                        .flow::<Msg<2, FramePayload>>()
                        .expect("send flow")
                        .send(&payload)
                );
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                let mut send_error = None;
                for attempt in 0..8 {
                    match send_future.as_mut().poll(&mut context) {
                        Poll::Pending => {}
                        Poll::Ready(Ok(())) => panic!("send unexpectedly progressed"),
                        Poll::Ready(Err(error)) => {
                            assert_eq!(error.operation(), "send");
                            assert!(
                                format!("{error:?}").contains("DeadlineExceeded"),
                                "send error must keep deadline fault evidence: {error:?}"
                            );
                            send_error = Some(error);
                            break;
                        }
                    }
                    assert!(attempt < 7, "deadline fuse did not trip");
                }

                assert!(send_error.is_some(), "deadline fault must be observed");
                assert_eq!(
                    cancel_count.get(),
                    1,
                    "deadline send failure must cancel the pending transport send exactly once"
                );
                drop(send_future);
                assert_eq!(
                    cancel_count.get(),
                    1,
                    "completed send future drop must not cancel the same pending send twice"
                );
                assert!(
                    transport.queue_is_empty(),
                    "cancelled pending send must not leave a frame available for later flush"
                );
            },
        );
    });
}

#[test]
fn send_session_fault_cancels_pending_transport_state_once() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = DeadlinePendingTransport::without_deadline();
        let cancel_count = transport.cancel_count();
        with_resident_tls_ref(
            &DEADLINE_PENDING_SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                            (tap_buf, slab),
                            CounterClock::new(),
                        ),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(203);
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

                let payload = FramePayload(*b"hiba");
                let mut send_future = std::pin::pin!(
                    origin_endpoint
                        .flow::<Msg<2, FramePayload>>()
                        .expect("send flow")
                        .send(&payload)
                );
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                match send_future.as_mut().poll(&mut context) {
                    Poll::Pending => {}
                    Poll::Ready(Ok(())) => panic!("send unexpectedly progressed"),
                    Poll::Ready(Err(error)) => {
                        panic!("send failed before peer dropped: {error:?}");
                    }
                }
                assert_eq!(
                    cancel_count.get(),
                    0,
                    "initial pending send must not cancel before a terminal fault"
                );

                drop(target_endpoint);

                match send_future.as_mut().poll(&mut context) {
                    Poll::Ready(Err(error)) => {
                        assert_eq!(error.operation(), "send");
                        assert!(
                            format!("{error:?}").contains("EndpointDropped"),
                            "send error must keep session fault evidence: {error:?}"
                        );
                    }
                    Poll::Ready(Ok(())) => panic!("send unexpectedly progressed after peer drop"),
                    Poll::Pending => panic!("poisoned send remained pending"),
                }
                assert_eq!(
                    cancel_count.get(),
                    1,
                    "session fault send failure must cancel the pending transport send exactly once"
                );
                drop(send_future);
                assert_eq!(
                    cancel_count.get(),
                    1,
                    "completed send future drop must not cancel the same pending send twice"
                );
                assert!(
                    transport.queue_is_empty(),
                    "cancelled pending send must not leave a frame available for later flush"
                );
            },
        );
    });
}

#[test]
fn dropping_live_endpoint_poison_wakes_waiting_peer() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport,
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(202);
                let origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let mut recv_future =
                    std::pin::pin!(target_endpoint.recv::<Msg<2, FramePayload>>());
                let wake_count = Cell::new(0);
                let waker = counting_waker(&wake_count);
                let mut context = Context::from_waker(&waker);
                match recv_future.as_mut().poll(&mut context) {
                    Poll::Pending => {}
                    Poll::Ready(Ok(payload)) => {
                        core::hint::black_box(&payload);
                        panic!("recv unexpectedly progressed before sender drop");
                    }
                    Poll::Ready(Err(error)) => {
                        panic!("recv failed before sender drop: {error:?}");
                    }
                }
                assert_eq!(
                    wake_count.get(),
                    0,
                    "initial pending recv must only register its waiter"
                );

                drop(origin_endpoint);

                assert!(
                    wake_count.get() > 0,
                    "live endpoint drop must wake peers waiting in the same session"
                );
                match recv_future.as_mut().poll(&mut context) {
                    Poll::Ready(Err(error)) => {
                        assert_eq!(error.operation(), "recv");
                        assert!(
                            format!("{error:?}").contains("EndpointDropped"),
                            "waiting peer must observe EndpointDropped evidence: {error:?}"
                        );
                    }
                    Poll::Ready(Ok(payload)) => {
                        core::hint::black_box(&payload);
                        panic!("recv unexpectedly progressed after sender drop");
                    }
                    Poll::Pending => panic!("poisoned waiting peer remained pending"),
                }
            },
        );
    });
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

#[test]
fn cursor_send_and_recv_roundtrip() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(1);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<1, u32>>()
                        .expect("send flow")
                        .send(&42),
                )
                .expect("send succeeds");
                let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                    .expect("recv succeeds");
                assert_eq!(payload, 42u32);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn completed_recv_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::seq(
                    g::send::<Role<0>, Role<1>, Msg<41, u32>, 0>(),
                    g::send::<Role<0>, Role<1>, Msg<41, u32>, 0>(),
                );
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                            (tap_buf, slab),
                            CounterClock::new(),
                        ),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(41);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<41, u32>>()
                        .expect("first send flow")
                        .send(&11),
                )
                .expect("first send succeeds");
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<41, u32>>()
                        .expect("second send flow")
                        .send(&22),
                )
                .expect("second send succeeds");

                let mut recv_future = Box::pin(target_endpoint.recv::<Msg<41, u32>>());
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                match Future::poll(recv_future.as_mut(), &mut context) {
                    Poll::Ready(Ok(value)) => assert_eq!(value, 11),
                    Poll::Ready(Err(error)) => panic!("first recv failed: {error:?}"),
                    Poll::Pending => panic!("first recv must be ready"),
                }

                let repoll = catch_unwind(AssertUnwindSafe(|| {
                    let _ = Future::poll(recv_future.as_mut(), &mut context);
                }));
                assert!(
                    repoll.is_err(),
                    "completed recv future must fail fast on post-Ready poll"
                );
                drop(recv_future);

                let second = futures::executor::block_on(target_endpoint.recv::<Msg<41, u32>>())
                    .expect("second recv remains available");
                assert_eq!(
                    second, 22,
                    "completed recv future repoll must not consume the next descriptor"
                );
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn completed_send_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::seq(
                    g::send::<Role<0>, Role<1>, Msg<42, u32>, 0>(),
                    g::send::<Role<0>, Role<1>, Msg<42, u32>, 0>(),
                );
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                            (tap_buf, slab),
                            CounterClock::new(),
                        ),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(42);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let first = 11u32;
                let mut send_future = Box::pin(
                    origin_endpoint
                        .flow::<Msg<42, u32>>()
                        .expect("first send flow")
                        .send(&first),
                );
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                match Future::poll(send_future.as_mut(), &mut context) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Ready(Err(error)) => panic!("first send failed: {error:?}"),
                    Poll::Pending => panic!("first send must be ready"),
                }

                let repoll = catch_unwind(AssertUnwindSafe(|| {
                    let _ = Future::poll(send_future.as_mut(), &mut context);
                }));
                assert!(
                    repoll.is_err(),
                    "completed send future must fail fast on post-Ready poll"
                );
                drop(send_future);

                let second = 22u32;
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<42, u32>>()
                        .expect("second send flow")
                        .send(&second),
                )
                .expect("second send succeeds");

                let first_recv =
                    futures::executor::block_on(target_endpoint.recv::<Msg<42, u32>>())
                        .expect("first recv remains available");
                let second_recv =
                    futures::executor::block_on(target_endpoint.recv::<Msg<42, u32>>())
                        .expect("second recv remains available");
                assert_eq!(first_recv, 11);
                assert_eq!(
                    second_recv, 22,
                    "completed send future repoll must not consume the next descriptor"
                );
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn flow_error_captures_public_callsite() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(11);
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

                let flow_line = line!() + 1;
                let err = match origin_endpoint.flow::<Msg<2, u32>>() {
                    Ok(_) => panic!("flow with wrong logical label must fail"),
                    Err(err) => err,
                };
                assert_eq!(err.operation(), "flow");
                assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
                assert_eq!(err.line(), flow_line);
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("LabelMismatch"),
                    "failed send preview must report the preview mismatch: {rendered}"
                );
                assert!(
                    !rendered.contains("SessionFault"),
                    "failed send preview must not poison before send consumes progress: {rendered}"
                );

                let offer_line = line!() + 1;
                let err = match futures::executor::block_on(origin_endpoint.offer()) {
                    Ok(_) => panic!("offer at deterministic send step must fail"),
                    Err(err) => err,
                };
                assert_eq!(err.operation(), "offer");
                assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
                assert_eq!(err.line(), offer_line);
                assert_progress_invariant_fault(&err);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn recv_codec_error_poisons_before_same_generation_continuation() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(12);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<1, u32>>()
                        .expect("send flow")
                        .send(&42),
                )
                .expect("send succeeds");

                let recv_line = line!() + 1;
                let err = match futures::executor::block_on(target_endpoint.recv::<Msg<1, u64>>()) {
                    Ok(_) => panic!("recv with wrong payload shape must fail"),
                    Err(err) => err,
                };
                assert_eq!(err.operation(), "recv");
                assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
                assert_eq!(err.line(), recv_line);
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("Codec"),
                    "first recv fault must preserve codec evidence: {rendered}"
                );
                assert!(
                    !rendered.contains("SessionFault"),
                    "first recv fault must not be replaced by session poison: {rendered}"
                );

                let continuation_line = line!() + 1;
                let err = match futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>()) {
                    Ok(_) => {
                        panic!("poisoned generation must not continue after recv decode fault")
                    }
                    Err(err) => err,
                };
                assert_eq!(err.operation(), "recv");
                assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
                assert_eq!(err.line(), continuation_line);
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("SessionFault") && rendered.contains("DecodeFailed"),
                    "continuation must report the poisoned session cause: {rendered}"
                );
            },
        );
    });
}

#[test]
fn demux_binding_without_policy_signals_keeps_empty_transport_payload_nonsemantic() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(13);
                let origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                core::hint::black_box(&origin_endpoint);
                let binding = Box::leak(Box::new(DemuxOnlyBinding));
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(binding)
                    .expect("target endpoint");

                let mut tx = TestTx::default();
                transport.stage_send(&mut tx, 1, 0, 1, &[]);
                assert!(matches!(
                    transport.poll_send_staged(&mut tx),
                    Poll::Ready(Ok(()))
                ));

                let mut recv_future = std::pin::pin!(target_endpoint.recv::<Msg<1, u8>>());
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                match recv_future.as_mut().poll(&mut context) {
                    Poll::Pending => {}
                    Poll::Ready(Ok(value)) => {
                        panic!("empty transport payload was accepted as semantic data: {value}")
                    }
                    Poll::Ready(Err(error)) => {
                        panic!(
                            "binding without policy signals must wait for binding evidence, got {error:?}"
                        )
                    }
                }
                assert!(
                    transport_queue_is_empty(&transport),
                    "nonsemantic empty demux turns must not be requeued as payload"
                );
            },
        );
    });
}

#[test]
fn cursor_send_and_recv_high_logical_label_roundtrip() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<200, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(200);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<200, u32>>()
                        .expect("send flow")
                        .send(&0xC8C8_C8C8),
                )
                .expect("send succeeds");
                let payload = futures::executor::block_on(target_endpoint.recv::<Msg<200, u32>>())
                    .expect("recv succeeds");
                assert_eq!(payload, 0xC8C8_C8C8);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn custom_label_universe_rejects_high_logical_label_on_enter() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &LOW_LABEL_SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<200, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<LowLabelUniverse, _>::from_resources(
                            (tap_buf, slab),
                            CounterClock::new(),
                        ),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let bad_sid = SessionId::new(201);
                let enter_line = line!() + 5;
                let enter_result = cluster
                    .rendezvous(rv_id)
                    .session(bad_sid)
                    .role(&origin_program)
                    .enter(NoBinding);
                let err = match enter_result {
                    Ok(_) => panic!("custom label universe must reject high logical label"),
                    Err(err) => err,
                };

                let debug = format!("{err:?}");
                assert_eq!(err.operation(), "enter");
                assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
                assert_eq!(err.line(), enter_line);
                assert!(debug.contains("LabelOutOfUniverse"));
                assert!(debug.contains("max: 127"));
                assert!(debug.contains("actual: 200"));
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn cursor_send_and_recv_manual_wire_control_token() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_CONTROL_LOGICAL },
                        GenericCapToken<ManualWireControl>,
                        ManualWireControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(9);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let token = manual_wire_token(sid, hibana::integration::ids::Lane::new(0), 1);

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_CONTROL_LOGICAL },
                            GenericCapToken<ManualWireControl>,
                            ManualWireControl,
                        >>()
                        .expect("wire control flow")
                        .send(&token),
                )
                .expect("explicit wire control token send succeeds");

                let received = futures::executor::block_on(target_endpoint.recv::<Msg<
                    { MANUAL_WIRE_CONTROL_LOGICAL },
                    GenericCapToken<ManualWireControl>,
                    ManualWireControl,
                >>())
                .expect("recv succeeds");

                assert_eq!(
                    received.decode_handle().expect("decode handle"),
                    (sid.raw(), 0)
                );
                assert_eq!(received.into_bytes(), token.into_bytes());
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn deterministic_recv_rejects_control_data_kind_mismatch() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<
                        { MANUAL_WIRE_CONTROL_LOGICAL },
                        GenericCapToken<ManualWireControl>,
                        ManualWireControl,
                    >,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(91);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let token = manual_wire_token(sid, hibana::integration::ids::Lane::new(0), 1);
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_CONTROL_LOGICAL },
                            GenericCapToken<ManualWireControl>,
                            ManualWireControl,
                        >>()
                        .expect("wire control flow")
                        .send(&token),
                )
                .expect("explicit wire control token send succeeds");

                type ManualWireDataMsg =
                    Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>;
                let recv_line = line!() + 1;
                let recv_future = target_endpoint.recv::<ManualWireDataMsg>();
                let err = match futures::executor::block_on(recv_future) {
                    Ok(_) => panic!("deterministic recv must reject control as data"),
                    Err(err) => err,
                };
                assert_eq!(err.operation(), "recv");
                assert!(err.file().ends_with("tests/cursor_send_recv.rs"));
                assert_eq!(err.line(), recv_line);
                assert_progress_invariant_fault(&err);
            },
        );
    });

    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<
                    Role<0>,
                    Role<1>,
                    Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>,
                    0,
                >();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(92);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let token_bytes =
                    manual_wire_token(sid, hibana::integration::ids::Lane::new(0), 1).into_bytes();
                futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<{ MANUAL_WIRE_CONTROL_LOGICAL }, [u8; MANUAL_TOKEN_LEN]>>()
                        .expect("data flow")
                        .send(&token_bytes),
                )
                .expect("data send succeeds");

                let err = match futures::executor::block_on(target_endpoint.recv::<Msg<
                    { MANUAL_WIRE_CONTROL_LOGICAL },
                    GenericCapToken<ManualWireControl>,
                    ManualWireControl,
                >>()) {
                    Ok(_) => panic!("deterministic recv must reject data as control"),
                    Err(err) => err,
                };
                assert_eq!(err.operation(), "recv");
                assert_progress_invariant_fault(&err);
            },
        );
    });
}

#[test]
fn manual_wire_control_send_dispatches_exactly_one_abort_ack() {
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

                let sid = SessionId::new(10);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let token = manual_wire_abort_ack_token(
                    sid,
                    hibana::integration::ids::Lane::new(0),
                    1,
                    0,
                    0,
                );

                let () = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireAbortAckControl>,
                            ManualWireAbortAckControl,
                        >>()
                        .expect("wire abort-ack flow")
                        .send(&token),
                )
                .expect("explicit wire abort-ack send succeeds");

                let received = futures::executor::block_on(target_endpoint.recv::<Msg<
                    { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                    GenericCapToken<ManualWireAbortAckControl>,
                    ManualWireAbortAckControl,
                >>())
                .expect("recv succeeds");
                assert_eq!(received.into_bytes(), token.into_bytes());
                assert!(transport_queue_is_empty(&transport));
            },
        );
        let abort_ack_events = unsafe { &*tap_ptr }
            .iter()
            .filter(|event| event.id == ABORT_ACK_ID && event.arg0 == 10)
            .count();
        assert_eq!(
            abort_ack_events, 1,
            "explicit wire control send must execute exactly one abort-ack operation",
        );
    });
}

#[test]
fn manual_wire_one_shot_control_send_rejects_before_transport() {
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
                        { MANUAL_WIRE_ONE_SHOT_ABORT_ACK_LOGICAL },
                        GenericCapToken<ManualWireOneShotAbortAckControl>,
                        ManualWireOneShotAbortAckControl,
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

                let sid = SessionId::new(18);
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

                let token = manual_wire_one_shot_abort_ack_token(
                    sid,
                    hibana::integration::ids::Lane::new(0),
                    1,
                    0,
                    0,
                );

                let err = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ONE_SHOT_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireOneShotAbortAckControl>,
                            ManualWireOneShotAbortAckControl,
                        >>()
                        .expect("wire one-shot abort-ack flow")
                        .send(&token),
                )
                .expect_err("unclaimed one-shot manual wire token must fail before transport");

                assert_eq!(err.operation(), "send");
                assert_progress_invariant_fault(&err);
                assert!(transport_queue_is_empty(&transport));
            },
        );
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID && event.arg0 == 18),
            "unclaimed one-shot manual wire token must not execute abort-ack",
        );
    });
}

#[test]
fn manual_wire_control_send_rejects_scope_mismatch_before_transport() {
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

                let sid = SessionId::new(11);
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

                let mismatched = manual_wire_abort_ack_token(
                    sid,
                    hibana::integration::ids::Lane::new(0),
                    1,
                    1,
                    0,
                );

                let err = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<
                            { MANUAL_WIRE_ABORT_ACK_LOGICAL },
                            GenericCapToken<ManualWireAbortAckControl>,
                            ManualWireAbortAckControl,
                        >>()
                        .expect("wire abort-ack flow")
                        .send(&mismatched),
                )
                .expect_err("descriptor/header mismatch must fail before transport");

                assert_eq!(err.operation(), "send");
                assert_progress_invariant_fault(&err);
                assert!(transport_queue_is_empty(&transport));
            },
        );
        assert!(
            !unsafe { &*tap_ptr }
                .iter()
                .any(|event| event.id == ABORT_ACK_ID && event.arg0 == 11),
            "rejected explicit control send must not execute abort-ack",
        );
    });
}

#[test]
fn manual_wire_control_send_rejects_session_binding_before_transport() {
    let sid = SessionId::new(12);
    let token = manual_wire_abort_ack_token(
        SessionId::new(13),
        hibana::integration::ids::Lane::new(0),
        1,
        0,
        0,
    );
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_lane_binding_before_transport() {
    let sid = SessionId::new(14);
    let token = manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(1), 1, 0, 0);
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_role_binding_before_transport() {
    let sid = SessionId::new(15);
    let token = manual_wire_abort_ack_token(sid, hibana::integration::ids::Lane::new(0), 0, 0, 0);
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn manual_wire_control_send_rejects_handle_mismatch_before_transport() {
    let sid = SessionId::new(16);
    let token = manual_wire_abort_ack_token_with_handle(
        sid,
        hibana::integration::ids::Lane::new(0),
        1,
        0,
        0,
        sid.raw(),
        1,
    );
    assert_manual_wire_abort_ack_send_rejected(token, sid);
}

#[test]
fn localside_send_recv_sizes_stay_compact() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport,
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(3);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
                    .expect("target endpoint");

                let send = origin_endpoint
                    .flow::<Msg<1, u32>>()
                    .expect("send flow")
                    .send(&42);
                let recv = target_endpoint.recv::<Msg<1, u32>>();

                let endpoint_bytes = size_of::<hibana::Endpoint<'static, 0>>();
                let send_future_bytes = size_of_val(&send);
                let recv_future_bytes = size_of_val(&recv);

                assert!(
                    endpoint_bytes <= ENDPOINT_BYTES_MAX,
                    "endpoint handle regressed: {endpoint_bytes} > {ENDPOINT_BYTES_MAX}"
                );
                assert!(
                    send_future_bytes <= SEND_FUTURE_BYTES_MAX,
                    "send future regressed: {send_future_bytes} > {SEND_FUTURE_BYTES_MAX}"
                );
                assert!(
                    recv_future_bytes <= RECV_FUTURE_BYTES_MAX,
                    "recv future regressed: {recv_future_bytes} > {RECV_FUTURE_BYTES_MAX}"
                );

                drop(send);
                drop(recv);
            },
        );
    });
}
