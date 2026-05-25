//! Transport abstraction bridging Hibana frames onto concrete mediums.
//!
//! Implementations are expected to integrate with external async runtimes via
//! explicit `poll_*` methods. The transport owns whatever pending state and
//! waker bookkeeping it needs inside its `Tx` / `Rx` handles or shared state.
//!
//! Receive buffers must be exposed as borrowed views. The rendezvous layer
//! provides a slab (see [`crate::runtime::config::Config::slab`]) that transports can pin
//! behind their `Rx` handle so [`Transport::poll_recv`] yields payload views borrowed
//! from that storage. This keeps the runtime allocation-free while allowing
//! DMA/SHM backed zero-copy paths.
//!
//! Implementations also bridge device interrupts to the task waker stored by
//! their pending send/recv state. When a poll parks it must record the current
//! [`core::task::Waker`] so the interrupt handler can call `wake_by_ref`
//! instead of relying on polling loops.

use core::task::{Context, Poll};

use crate::{
    eff::EffIndex,
    transport::wire::{CodecError, Payload, WireEncode, WirePayload, require_exact_len},
};

mod labels;
mod snapshot;

pub use labels::FrameLabel;
pub(crate) use labels::{FrameLabelMask, LogicalLabel};
#[cfg(test)]
pub(crate) use snapshot::TransportAlgorithm;
pub(crate) use snapshot::TransportSnapshot;

/// Metrics facade returned by transports to feed routing SLO checks.
pub trait TransportMetrics {
    /// Convert the current readings into packed policy attributes.
    fn attrs(&self) -> context::PolicyAttrs;
}

impl TransportMetrics for () {
    fn attrs(&self) -> context::PolicyAttrs {
        context::PolicyAttrs::EMPTY
    }
}

/// Direction of a send operation from the local role's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalDirection {
    /// Sending to a peer over the transport.
    Send,
    /// Local-only self-send that must not hit the wire.
    Local,
}

/// Transport-owned metadata for an outgoing payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SendMeta {
    /// Effect index (stable identifier for the choreography step).
    pub eff_index: EffIndex,
    /// Application/choreography logical label.
    pub logical_label: LogicalLabel,
    /// Transport/binding demux discriminator.
    pub frame_label: FrameLabel,
    /// Target peer role.
    pub peer: u8,
    /// Logical lane for this message.
    pub lane: u8,
    /// Direction from the local role's perspective.
    pub direction: LocalDirection,
    /// Whether this is a control message.
    pub is_control: bool,
}

impl SendMeta {
    #[inline]
    pub const fn is_send(&self) -> bool {
        matches!(self.direction, LocalDirection::Send)
    }

    #[inline]
    pub const fn is_local(&self) -> bool {
        matches!(self.direction, LocalDirection::Local)
    }
}

/// Transport-owned outgoing frame.
#[derive(Clone, Copy, Debug)]
pub struct Outgoing<'f> {
    pub(crate) meta: SendMeta,
    pub(crate) payload: Payload<'f>,
}

impl<'f> Outgoing<'f> {
    #[inline]
    pub const fn frame_label(&self) -> FrameLabel {
        self.meta.frame_label
    }

    #[inline]
    pub const fn peer(&self) -> u8 {
        self.meta.peer
    }

    #[inline]
    pub const fn lane(&self) -> u8 {
        self.meta.lane
    }

    #[inline]
    pub const fn is_control(&self) -> bool {
        self.meta.is_control
    }

    #[inline]
    pub const fn is_send(&self) -> bool {
        self.meta.is_send()
    }

    #[inline]
    pub const fn is_local(&self) -> bool {
        self.meta.is_local()
    }

    #[inline]
    pub const fn payload(&self) -> Payload<'f> {
        self.payload
    }
}

/// Transport-level telemetry event taxonomy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportEventKind {
    Ack,
    Loss,
    KeepaliveTx,
    KeepaliveRx,
    CloseStart,
    CloseDraining,
    CloseRemote,
    Timeout,
}

impl WireEncode for TransportEventKind {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = match self {
            TransportEventKind::Ack => 0,
            TransportEventKind::Loss => 1,
            TransportEventKind::KeepaliveTx => 2,
            TransportEventKind::KeepaliveRx => 3,
            TransportEventKind::CloseStart => 4,
            TransportEventKind::CloseDraining => 5,
            TransportEventKind::CloseRemote => 6,
            TransportEventKind::Timeout => 7,
        };
        Ok(1)
    }
}

impl WirePayload for TransportEventKind {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        let bytes = input.as_bytes();
        require_exact_len(bytes.len(), 1, "payload length")?;
        match bytes[0] {
            0..=7 => Ok(()),
            _ => Err(CodecError::Invalid("transport event kind")),
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        decode_validated_event_kind(input.as_bytes()[0])
    }
}

#[inline]
fn decode_validated_event_kind(byte: u8) -> TransportEventKind {
    match byte {
        0 => TransportEventKind::Ack,
        1 => TransportEventKind::Loss,
        2 => TransportEventKind::KeepaliveTx,
        3 => TransportEventKind::KeepaliveRx,
        4 => TransportEventKind::CloseStart,
        5 => TransportEventKind::CloseDraining,
        6 => TransportEventKind::CloseRemote,
        _ => TransportEventKind::Timeout,
    }
}

/// Metadata used by transport implementations to describe an observation event.
///
/// This keeps detailed transport telemetry constructible through a typed
/// one-argument path without exposing raw multi-argument event constructors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportEventMeta {
    kind: TransportEventKind,
    packet_number: u64,
    payload_len: u32,
    retransmissions: u32,
    pn_space: u8,
    cid_tag: u8,
}

#[inline]
const fn saturate_tap_pn_space(pn_space: u8) -> u8 {
    if pn_space > 0x7 { 0x7 } else { pn_space }
}

impl TransportEventMeta {
    #[inline]
    pub const fn new(kind: TransportEventKind) -> Self {
        Self {
            kind,
            packet_number: 0,
            payload_len: 0,
            retransmissions: 0,
            pn_space: 0,
            cid_tag: 0,
        }
    }

    #[inline]
    pub const fn packet_number(mut self, packet_number: u64) -> Self {
        self.packet_number = packet_number;
        self
    }

    #[inline]
    pub const fn payload_len(mut self, payload_len: u32) -> Self {
        self.payload_len = payload_len;
        self
    }

    #[inline]
    pub const fn retransmissions(mut self, retransmissions: u32) -> Self {
        self.retransmissions = retransmissions;
        self
    }

    #[inline]
    /// Set the transport-defined packet number space identifier.
    ///
    /// Tap encoding has a three-bit field for this value, so inputs above `7`
    /// are saturated to `7` at the metadata boundary.
    pub const fn packet_number_space(mut self, pn_space: u8) -> Self {
        self.pn_space = saturate_tap_pn_space(pn_space);
        self
    }

    #[inline]
    pub const fn connection_id_tag(mut self, cid_tag: u8) -> Self {
        self.cid_tag = cid_tag;
        self
    }
}

/// Telemetry describing an acknowledged or lost packet emitted by a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportEvent {
    kind: TransportEventKind,
    packet_number: u64,
    payload_len: u32,
    retransmissions: u32,
    /// Packet number space identifier (transport-defined).
    pn_space: u8,
    /// Truncated tag identifying the relevant connection identifier (transport-defined).
    cid_tag: u8,
}

impl WireEncode for TransportEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(19)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        const LEN: usize = 19;
        if out.len() < LEN {
            return Err(CodecError::Truncated);
        }
        out[0] = match self.kind {
            TransportEventKind::Ack => 0,
            TransportEventKind::Loss => 1,
            TransportEventKind::KeepaliveTx => 2,
            TransportEventKind::KeepaliveRx => 3,
            TransportEventKind::CloseStart => 4,
            TransportEventKind::CloseDraining => 5,
            TransportEventKind::CloseRemote => 6,
            TransportEventKind::Timeout => 7,
        };
        out[1] = self.pn_space;
        out[2] = self.cid_tag;
        out[3..11].copy_from_slice(&self.packet_number.to_be_bytes());
        out[11..15].copy_from_slice(&self.payload_len.to_be_bytes());
        out[15..19].copy_from_slice(&self.retransmissions.to_be_bytes());
        Ok(LEN)
    }
}

impl WirePayload for TransportEvent {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        const LEN: usize = 19;
        let bytes = input.as_bytes();
        require_exact_len(bytes.len(), LEN, "payload length")?;
        match bytes[0] {
            0..=7 => Ok(()),
            _ => Err(CodecError::Invalid("transport event kind")),
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes = input.as_bytes();
        let kind = decode_validated_event_kind(bytes[0]);
        let pn_space = bytes[1];
        let cid_tag = bytes[2];
        let mut pn_bytes = [0u8; 8];
        pn_bytes.copy_from_slice(&bytes[3..11]);
        let mut payload_bytes = [0u8; 4];
        payload_bytes.copy_from_slice(&bytes[11..15]);
        let mut retrans_bytes = [0u8; 4];
        retrans_bytes.copy_from_slice(&bytes[15..19]);
        TransportEvent {
            kind,
            packet_number: u64::from_be_bytes(pn_bytes),
            payload_len: u32::from_be_bytes(payload_bytes),
            retransmissions: u32::from_be_bytes(retrans_bytes),
            pn_space,
            cid_tag,
        }
    }
}

impl TransportEvent {
    pub const fn new(kind: TransportEventKind) -> Self {
        Self::from_meta(TransportEventMeta::new(kind))
    }

    pub const fn from_meta(meta: TransportEventMeta) -> Self {
        Self {
            kind: meta.kind,
            packet_number: meta.packet_number,
            payload_len: meta.payload_len,
            retransmissions: meta.retransmissions,
            pn_space: saturate_tap_pn_space(meta.pn_space),
            cid_tag: meta.cid_tag,
        }
    }

    #[inline]
    pub const fn kind(&self) -> TransportEventKind {
        self.kind
    }

    #[inline]
    pub const fn packet_number(&self) -> u64 {
        self.packet_number
    }

    #[inline]
    pub const fn payload_len(&self) -> u32 {
        self.payload_len
    }

    #[inline]
    pub const fn retransmissions(&self) -> u32 {
        self.retransmissions
    }

    #[inline]
    pub const fn pn_space(&self) -> u8 {
        self.pn_space
    }

    #[inline]
    pub const fn cid_tag(&self) -> u8 {
        self.cid_tag
    }

    /// Encode the event into tap payload arguments.
    ///
    /// * `arg0` — lower 32 bits of the packet number
    /// * `arg1` — `[ kind | pn_space | cid_tag | payload_len | retransmissions ]`
    ///   * bits 29–31 store the event kind (0=Ack,1=Loss,2=KeepaliveTx,3=KeepaliveRx,4=CloseStart,5=CloseDraining,6=CloseRemote,7=Timeout)
    ///   * bits 26–28 store the packet number space identifier (saturated to 3 bits)
    ///   * bits 18–25 store the connection identifier tag (8 bits)
    ///   * bits 8–17 store the payload length (saturated to 10 bits)
    ///   * bits 0–7 store the retransmission counter (saturated to 8 bits)
    pub fn encode_tap_args(&self) -> (u32, u32) {
        let arg0 = (self.packet_number & 0xFFFF_FFFF) as u32;
        let kind_bits = match self.kind {
            TransportEventKind::Ack => 0u32,
            TransportEventKind::Loss => 1u32,
            TransportEventKind::KeepaliveTx => 2u32,
            TransportEventKind::KeepaliveRx => 3u32,
            TransportEventKind::CloseStart => 4u32,
            TransportEventKind::CloseDraining => 5u32,
            TransportEventKind::CloseRemote => 6u32,
            TransportEventKind::Timeout => 7u32,
        };
        let pn_space = saturate_tap_pn_space(self.pn_space) as u32;
        let cid_tag = (self.cid_tag as u32) & 0xFF;
        let payload = self.payload_len.min(0x3FF) as u32;
        let retrans = self.retransmissions.min(0xFF) as u32;
        let arg1 =
            (kind_bits << 29) | (pn_space << 26) | (cid_tag << 18) | (payload << 8) | retrans;
        (arg0, arg1)
    }
}

/// Errors surfaced by transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// Backing medium rejected the frame (e.g. link down).
    Offline,
    /// Transport encountered a fatal error (driver reset, etc.).
    Failed,
}

/// Descriptor-derived fact for opening one transport port.
///
/// This is produced by endpoint materialization, not by app code. Transport
/// implementations receive one opaque value instead of recombining raw role,
/// session, and lane scalars themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortOpen {
    local_role: u8,
    session_id: crate::control::types::SessionId,
    lane: crate::control::types::Lane,
}

impl PortOpen {
    #[inline]
    pub(crate) const fn from_descriptor(
        local_role: u8,
        session_id: crate::control::types::SessionId,
        lane: crate::control::types::Lane,
    ) -> Self {
        Self {
            local_role,
            session_id,
            lane,
        }
    }

    #[inline]
    pub const fn local_role(self) -> u8 {
        self.local_role
    }

    #[inline]
    pub const fn session_id(self) -> crate::control::types::SessionId {
        self.session_id
    }

    #[inline]
    pub const fn lane(self) -> crate::control::types::Lane {
        self.lane
    }
}

/// Asynchronous transport interface with explicit Tx/Rx handles.
///
/// The trait uses GATs so that implementations can borrow buffers from the
/// surrounding environment without forcing allocations. Pending I/O state stays
/// in transport-owned handles instead of leaking transport future types into
/// higher layers.
pub trait Transport {
    type Error: Into<TransportError>;
    type Tx<'a>: 'a
    where
        Self: 'a;
    type Rx<'a>: 'a
    where
        Self: 'a;
    type Metrics: TransportMetrics;

    /// Open Tx/Rx handles bound to the lifetime of this transport reference.
    ///
    /// `port` carries the projected role/session/lane fact for the returned Tx/Rx
    /// handles. Carriers backed by a shared physical medium must preserve this
    /// lane in frame metadata and demultiplex received frames before returning
    /// payload bytes or route-observation hints to the endpoint.
    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);

    /// Progress a send operation using the provided Tx handle.
    ///
    /// Transport implementations select the appropriate packet class
    /// (for example, pre-auth, handshake, or application-data) based on
    /// internal cryptographic
    /// state, not application-layer metadata.
    fn poll_send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f;

    /// Cancel any transport-owned pending send state bound to `tx`.
    ///
    /// Public endpoint send futures are affine and may be dropped after
    /// `poll_send` parks. When a transport stages frame state inside `Tx` or
    /// transport-owned shared state before returning `Poll::Pending`, it must
    /// discard that staged state here so that a retry cannot flush the
    /// cancelled payload.
    fn cancel_send<'a>(&'a self, tx: &'a mut Self::Tx<'a>);

    /// Progress a receive operation using the provided Rx handle.
    ///
    /// The returned [`Payload`] view is borrowed from the transport-managed
    /// receive slab. Borrowing ties the lifetime `'a` to the mutable borrow of
    /// `rx`, allowing higher layers such as [`crate::Endpoint`] to enforce that
    /// the view is released before the next receive. Implementations should
    /// store the current waker whenever the poll parks so that hardware
    /// interrupts or other I/O notifications can wake the task directly instead
    /// of relying on polling loops.
    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>>;

    /// Requeue the most recent frame obtained from [`poll_recv`](Transport::poll_recv).
    ///
    /// Implementations must make that frame observable again by a later
    /// `poll_recv` on the same `Rx` handle. A no-op requeue violates the
    /// endpoint rollback contract: higher layers call this only after a
    /// descriptor-checked operation consumed transport state that it ultimately
    /// could not commit.
    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>);

    /// Drain transport-level telemetry events and forward them to the observer.
    ///
    /// Implementations invoke `emit` for each drained [`TransportEvent`].
    fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent));

    /// Drain one pending route-observation frame label for this Rx lane.
    ///
    /// When a transport receives a frame that maps to a specific hibana message
    /// frame label, it can return that discriminator here to help descriptor-
    /// checked passive route observation.
    ///
    /// This must be non-blocking and must not perform I/O; it should only
    /// inspect transport state already available via `rx`.
    ///
    /// This is not a receive operation: it must not consume payload bytes.
    /// It is, however, a hint-drain operation. Once a label has been yielded
    /// here, the transport must not yield the same observation again until a
    /// later `poll_recv` or `requeue` stages fresh receive state. The endpoint
    /// copies the drained label into its route table, and repeatedly returning
    /// the same label would re-inject already consumed evidence and prevent
    /// passive route observation from making progress.
    ///
    /// A frame label alone is not route authority. Shared carriers must scope
    /// this hint to the lane passed to [`Transport::open`]. The endpoint checks
    /// the hint against projected lane/descriptor metadata and never treats a
    /// hint as a route continuation by itself.
    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<FrameLabel>;

    /// Provide transport-level metrics for observation and policy input.
    ///
    /// Implementations supply latency estimates and queue depth information.
    /// Metrics are not route authority; resolvers may use them only as
    /// slot-scoped policy input.
    fn metrics(&self) -> Self::Metrics;

    /// Runtime-owned wait fuse for this transport instance.
    ///
    /// This is substrate evidence, not protocol authority. Expiry poisons the
    /// current session generation and never selects a choreography branch.
    /// Protocol-visible time must be modeled as a timer/clock role and an
    /// explicit route point.
    fn operational_deadline_ticks(&self) -> Option<u32> {
        None
    }
}

/// Transport context provider for resolver state access.
pub(crate) mod context;
/// Observability helpers for logical frame inspection.
pub(crate) mod trace;
/// Wire helpers: payload wrappers and serialization traits.
pub(crate) mod wire;

#[cfg(test)]
mod tests;
