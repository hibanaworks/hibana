//! Transport abstraction bridging Hibana frames onto concrete mediums.
//!
//! Implementations are expected to integrate with external async runtimes via
//! explicit `poll_*` methods. The transport owns whatever pending state and
//! waker bookkeeping it needs inside its `Tx` / `Rx` handles or shared state.
//!
//! Receive buffers must be exposed as borrowed views. Transport implementations
//! keep receive storage inside their `Rx` handle or medium-owned state so
//! [`Transport::poll_recv`] yields payload views without allocating. This keeps
//! the runtime allocation-free while allowing DMA/SHM backed zero-copy paths.
//!
//! Implementations also bridge device interrupts to the task waker stored by
//! their pending send/recv state. When a poll parks it must record the current
//! [`core::task::Waker`] so the interrupt handler can call `wake_by_ref`
//! instead of relying on polling loops.

use core::task::{Context, Poll};

use crate::{eff::EffIndex, session::types::SessionId, transport::wire::Payload};

mod labels;

pub use labels::FrameLabel;
pub(crate) use labels::{FrameLabelMask, LogicalLabel};

/// Transport-owned metadata for an outgoing payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SendMeta {
    /// Effect index (stable identifier for the choreography step).
    pub(crate) eff_index: EffIndex,
    /// Application/choreography logical label.
    pub(crate) logical_label: LogicalLabel,
    /// Transport/ingress demux discriminator.
    pub(crate) frame_label: FrameLabel,
    /// Target role for this outgoing message.
    pub(crate) target_role: u8,
    /// Logical lane for this message.
    pub(crate) lane: u8,
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
    pub const fn target_role(&self) -> u8 {
        self.meta.target_role
    }

    #[inline]
    pub const fn lane(&self) -> u8 {
        self.meta.lane
    }

    #[inline]
    pub const fn payload(&self) -> Payload<'f> {
        self.payload
    }
}

/// Errors surfaced by transport operations.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// Backing medium rejected the frame (e.g. link down).
    Offline,
    /// Operation exceeded a carrier-local deadline.
    Deadline,
    /// Carrier-local queue, ring, slab, or demux capacity was exhausted.
    Capacity,
    /// Transport encountered a fatal error (driver reset, etc.).
    Failed,
}

impl core::fmt::Debug for TransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            Self::Offline => "O",
            Self::Deadline => "D",
            Self::Capacity => "C",
            Self::Failed => "F",
        };
        f.write_str(name)
    }
}

/// Transport-owned header for one receive frame.
///
/// This is carrier observation, not session or descriptor authority. Hibana
/// runtime compares it against the endpoint's expected context before any
/// progress is committed.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader([u8; 8]);

impl FrameHeader {
    #[inline]
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub const fn bytes(self) -> [u8; 8] {
        self.0
    }

    #[inline]
    pub(crate) const fn from_parts(
        session: SessionId,
        carrier: u8,
        source_role: u8,
        target_role: u8,
        label: FrameLabel,
    ) -> Self {
        let session = session.raw().to_be_bytes();
        Self([
            session[0],
            session[1],
            session[2],
            session[3],
            carrier,
            source_role,
            target_role,
            label.raw(),
        ])
    }

    #[inline]
    pub(crate) const fn session(self) -> SessionId {
        SessionId::new(u32::from_be_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3],
        ]))
    }

    #[inline]
    pub(crate) const fn lane(self) -> u8 {
        self.0[4]
    }

    #[inline]
    pub(crate) const fn source_role(self) -> u8 {
        self.0[5]
    }

    #[inline]
    pub(crate) const fn target_role(self) -> u8 {
        self.0[6]
    }

    #[inline]
    pub(crate) const fn label(self) -> FrameLabel {
        FrameLabel::new(self.0[7])
    }
}

/// Transport-owned receive evidence.
///
/// Evidence is descriptor input, not route authority. `Deterministic` is valid
/// only for direct receives, or after a route branch has already materialized a
/// unique receive descriptor. Offer and unresolved route demux paths must
/// receive framed evidence and fail closed when the carrier cannot provide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IngressEvidence {
    /// Headerless receive evidence for a deterministic single-resident recv.
    Deterministic,
    /// Carrier-observed frame metadata.
    Framed {
        session: SessionId,
        carrier: u8,
        source: u8,
        target: u8,
        label: FrameLabel,
    },
}

impl IngressEvidence {
    #[inline]
    pub const fn from_header(header: FrameHeader) -> Self {
        Self::Framed {
            session: header.session(),
            carrier: header.lane(),
            source: header.source_role(),
            target: header.target_role(),
            label: header.label(),
        }
    }

    #[inline]
    pub(crate) const fn frame_header(self) -> Option<FrameHeader> {
        match self {
            Self::Deterministic => None,
            Self::Framed {
                session,
                carrier,
                source,
                target,
                label,
            } => Some(FrameHeader::from_parts(
                session, carrier, source, target, label,
            )),
        }
    }
}

/// Transport-owned receive frame.
///
/// The payload and carrier evidence cross the transport boundary as one value.
/// Hibana compares evidence against descriptor/session authority before endpoint
/// progress can consume the payload.
#[derive(Clone, Copy, Debug)]
pub struct ReceivedFrame<'f> {
    evidence: IngressEvidence,
    payload: Payload<'f>,
}

impl<'f> ReceivedFrame<'f> {
    #[inline]
    pub const fn deterministic(payload: Payload<'f>) -> Self {
        Self {
            evidence: IngressEvidence::Deterministic,
            payload,
        }
    }

    #[inline]
    pub const fn framed(header: FrameHeader, payload: Payload<'f>) -> Self {
        Self {
            evidence: IngressEvidence::from_header(header),
            payload,
        }
    }

    #[inline]
    pub(crate) const fn evidence(&self) -> IngressEvidence {
        self.evidence
    }

    #[inline]
    pub const fn payload(self) -> Payload<'f> {
        self.payload
    }
}

/// Descriptor-derived fact for opening one transport port.
///
/// This is produced by endpoint materialization, not by app code. Transport
/// implementations receive one opaque value instead of recombining raw role,
/// session, and lane scalars themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortOpen {
    local_role: u8,
    session_id: crate::session::types::SessionId,
    lane: crate::session::types::Lane,
}

impl PortOpen {
    #[inline]
    pub(crate) const fn from_descriptor(
        local_role: u8,
        session_id: crate::session::types::SessionId,
        lane: crate::session::types::Lane,
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
    pub const fn session_id(self) -> crate::session::types::SessionId {
        self.session_id
    }

    #[inline]
    pub const fn lane(self) -> u8 {
        self.lane.as_wire()
    }
}

/// Asynchronous transport interface with explicit Tx/Rx handles.
///
/// The trait uses GATs so that implementations can borrow buffers from the
/// surrounding environment without forcing allocations. Pending I/O state stays
/// in transport-owned handles instead of leaking transport future types into
/// higher layers.
///
/// # Monitor boundary and delivery premise
/// Every successful [`poll_recv`](Transport::poll_recv) yields one carrier
/// observation. Hibana checks its session, lane, roles, frame label, and exact
/// descriptor occurrence before commit but cannot authenticate carrier identity. An
/// unexpected or repeated observation names an enabled event or fails closed; it is
/// never normalized into progress. A successful `poll_send` does not prove delivery.
/// Global fidelity and progress additionally assume affine delivery: accepted
/// observations are bound to the mapped peer/direction, FIFO without unsolicited
/// replay, and eventually arrive or end in terminal closure. Otherwise the
/// protocol must make loss, retry, and freshness explicit. Exact local safety remains,
/// but global theorems require that premise; `requeue` never creates a commit.
/// A terminal carrier fault may abandon sends that were accepted locally but
/// never exposed to the receiver. Those frames must not appear after closure or
/// in another transport generation. A reused `SessionId` therefore needs fresh
/// carrier-owned receive state. The lifetime of a fresh transport instance can
/// provide that generation. Address migration or identifier rotation within the
/// same instance preserves it; a fresh instance begins a new generation.
///
/// This trait neither receives nor negotiates compiled protocol images. Initial
/// all-role protocol agreement belongs to the protocol deployment or an
/// application-level bootstrap, while Hibana checks every received frame against
/// the local descriptor. A carrier must not add a transport handshake merely to
/// implement this trait. Replayable early data may be exposed only after the
/// carrier or application profile has made it safe for Hibana's no-replay
/// contract; otherwise it must be rejected or delayed.
///
/// The strong affine-delivery profile also requires observable peer closure:
/// after accepted FIFO frames drain, a parked or later
/// [`poll_recv`](Transport::poll_recv) must wake and return
/// [`TransportError::Offline`] or [`TransportError::Failed`]. This is the
/// distributed cancellation boundary when a faulted endpoint retires handles.
/// It is a logical Hibana direction, not necessarily the physical connection:
/// a multiplexed carrier can retire one substream and leave others alive.
/// Retiring every handle for that direction must signal the remote direction.
/// Returning `Pending` forever after known peer closure violates the strong
/// profile. A raw carrier may instead expose protocol-defined close or timeout
/// as ordinary observations; without either, no global cancellation-termination
/// conclusion applies.
pub trait Transport {
    type Tx<'a>: 'a
    where
        Self: 'a;
    type Rx<'a>: 'a
    where
        Self: 'a;
    /// Open Tx/Rx handles bound to the lifetime of this transport reference.
    ///
    /// `port` carries the projected role/session/lane fact for the returned Tx/Rx
    /// handles. Carriers backed by a shared physical medium must preserve this
    /// lane in frame metadata and demultiplex received frames before returning
    /// payload bytes to the endpoint; framed carriers attach observed metadata
    /// to the same [`ReceivedFrame`] value as the payload.
    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);

    /// Progress a send operation using the provided Tx handle.
    ///
    /// Transport implementations select any carrier-local framing or protection
    /// state internally. Hibana passes descriptor-checked bytes; it does not
    /// expose protocol-specific transport phases as core concepts.
    ///
    /// After returning [`Poll::Pending`], implementations may retain progress in
    /// `tx` but must not retain the payload pointer. A later poll provides the
    /// same encoded content from scratch storage whose address may differ.
    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f;

    /// Cancel any transport-owned pending send state bound to `tx`.
    ///
    /// Public endpoint send futures are affine and may be dropped after
    /// `poll_send` parks. When a transport stages frame state inside `Tx` or
    /// transport-owned shared state before returning `Poll::Pending`, it must
    /// discard that staged state here so cancelled payload bytes cannot be
    /// flushed by a subsequent operation. If carrier bytes have already become
    /// irrevocable, cancellation must retire that logical direction so a later
    /// send reports a terminal [`TransportError`]; it must not resume after a
    /// partial frame or publish the cancelled frame.
    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>);

    /// Progress a receive operation using the provided Rx handle.
    ///
    /// The returned [`ReceivedFrame`] view is borrowed from the
    /// transport-managed receive slab and carries any carrier-observed frame
    /// header together with the payload. Borrowing ties the lifetime `'a` to the
    /// mutable borrow of `rx`, allowing higher layers such as [`crate::Endpoint`]
    /// to enforce that the view is released before the next receive.
    /// Implementations should store the current waker whenever the poll parks so
    /// that hardware interrupts or other I/O notifications can wake the task
    /// directly instead of relying on polling loops.
    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>>;

    /// Requeue the most recent frame obtained from [`poll_recv`](Transport::poll_recv).
    ///
    /// Implementations must make that frame observable again by a later
    /// `poll_recv` on the same `Rx` handle. An empty requeue violates the
    /// endpoint restore contract: higher layers call this only after a
    /// descriptor-checked operation consumed transport state that it ultimately
    /// could not commit.
    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError>;
}

/// Observability helpers for logical frame inspection.
pub(crate) mod trace;
/// Wire helpers: payload views and serialization traits.
pub(crate) mod wire;

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
