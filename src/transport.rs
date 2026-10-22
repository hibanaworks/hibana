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
pub(crate) use labels::LogicalLabel;

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
    /// Return the projected wire label, distinct from the message's logical label.
    #[inline]
    pub const fn frame_label(&self) -> FrameLabel {
        self.meta.frame_label
    }

    /// Return the destination's logical role, independent of its current holder.
    #[inline]
    pub const fn target_role(&self) -> u8 {
        self.meta.target_role
    }

    /// Return the projected logical lane for this send.
    #[inline]
    pub const fn lane(&self) -> u8 {
        self.meta.lane
    }

    /// Borrow the encoded bytes for this poll. The transport must not retain them.
    #[inline]
    pub const fn payload(&self) -> Payload<'f> {
        self.payload
    }
}

/// Errors surfaced by transport operations.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// Logical direction is unavailable (e.g. link down). This does not prove
    /// that no frame was accepted before the failure became observable.
    Offline,
    /// Operation exceeded the transport's deadline. This does not establish
    /// peer death, non-delivery, or permission to reclaim a transferred endpoint.
    Deadline,
    /// Transport queue, ring, slab, or routing capacity was exhausted.
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
/// Hibana checks the session, lane, roles and frame label against the local
/// protocol before advancing it. The transport is responsible for peer binding.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader([u8; 9]);

impl FrameHeader {
    /// Wrap the received nine-byte header for validation by the runtime.
    ///
    /// Layout: big-endian session ID (four bytes), lane, source role, target
    /// role, and a big-endian two-byte message position in the global protocol.
    /// The runtime checks these fields on receive.
    #[inline]
    pub const fn from_bytes(bytes: [u8; 9]) -> Self {
        Self(bytes)
    }

    /// Return the header bytes in their wire order.
    #[inline]
    pub const fn bytes(self) -> [u8; 9] {
        self.0
    }

    #[inline]
    pub(crate) const fn from_parts(
        session: SessionId,
        lane: u8,
        source_role: u8,
        target_role: u8,
        label: FrameLabel,
    ) -> Self {
        let session = session.raw().to_be_bytes();
        let event = label.raw().to_be_bytes();
        Self([
            session[0],
            session[1],
            session[2],
            session[3],
            lane,
            source_role,
            target_role,
            event[0],
            event[1],
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
        FrameLabel::new(u16::from_be_bytes([self.0[7], self.0[8]]))
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
        lane: u8,
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
            lane: header.lane(),
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
                lane,
                source,
                target,
                label,
            } => Some(FrameHeader::from_parts(
                session, lane, source, target, label,
            )),
        }
    }
}

/// Transport-owned receive frame.
///
/// Contains borrowed payload bytes and, for a framed receive, their header.
/// Hibana checks both against the local protocol before advancing it.
#[derive(Clone, Copy, Debug)]
pub struct ReceivedFrame<'f> {
    evidence: IngressEvidence,
    payload: Payload<'f>,
}

impl<'f> ReceivedFrame<'f> {
    /// Return a payload without a frame header.
    ///
    /// The bound receive handle and requested message must identify exactly one
    /// enabled receive, or an existing branch preview must identify it. An
    /// unresolved choice requires [`Self::framed`] instead.
    #[inline]
    pub const fn deterministic(payload: Payload<'f>) -> Self {
        Self {
            evidence: IngressEvidence::Deterministic,
            payload,
        }
    }

    /// Pair observed header bytes with the payload from the same received frame.
    ///
    /// The runtime validates the header against the local protocol. The
    /// transport must bind these bytes to the expected peer and direction.
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

    /// Return the borrowed encoded bytes without changing protocol progress.
    #[inline]
    pub const fn payload(self) -> Payload<'f> {
        self.payload
    }
}

/// Role, session and lane for a port opened by the runtime.
///
/// Bind the returned send and receive handles to these values.
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

    /// Return the logical role whose port is being opened.
    #[inline]
    pub const fn local_role(self) -> u8 {
        self.local_role
    }

    /// Return the session ID to which both handles must be bound.
    #[inline]
    pub const fn session_id(self) -> crate::session::types::SessionId {
        self.session_id
    }

    /// Return the projected lane to which both handles must be bound.
    #[inline]
    pub const fn lane(self) -> u8 {
        self.lane.as_wire()
    }
}

/// Asynchronous transport interface with explicit Tx/Rx handles.
///
/// Handles may borrow buffers from the transport. Keep pending I/O state and
/// task wakers in those handles or in transport-owned storage.
///
/// # Frame checks
/// Each framed receive is checked against the local protocol: its session,
/// lane, sender, receiver, frame label and payload format must match a permitted
/// receive. A mismatch returns an error and stops the local session run.
/// [`ReceivedFrame::deterministic`] has no header and requires a uniquely
/// identified receive, as described on that constructor.
///
/// These checks do not authenticate senders or detect every replay. The
/// transport must bind frames to the expected peer and direction. Participants
/// must agree on the global protocol through deployment or application setup.
///
/// # Delivery and closure
/// Protocol fidelity and progress require the transport to:
///
/// - Preserve FIFO order within each logical direction, without duplication or
///   replay. Independent directions can progress independently.
/// - Eventually deliver an accepted frame or make closure of its direction
///   observable. A successful send means acceptance, not peer receipt.
/// - On closure, drain or isolate accepted frames, wake waiting receivers and
///   return [`TransportError::Offline`] or [`TransportError::Failed`]. Frames
///   isolated by closure must never reappear. Known closure cannot stay pending.
/// - Make retirement of all handles for a direction observable at its peer.
///   A transport can close a logical substream while keeping its connection open.
/// - Isolate session runs. Reusing a [`SessionId`] requires fresh receive state
///   that excludes all frames from the earlier run, including buffered frames.
///   Address migration within a run must preserve its delivery guarantees.
/// - Delay or reject replayable early data until it meets the no-replay contract.
///
/// An error stops the observing runtime's run and preserves its first fault.
/// Other runtimes may still receive and use accepted endpoints. An error alone
/// neither rolls back a transfer nor closes the remote direction. The transport
/// must separately deliver or isolate accepted frames and make closure visible.
/// A deadline does not prove peer death or non-delivery. Protocol-defined close
/// or timeout messages can instead be ordinary messages in the global protocol.
/// Remote participants learn of cancellation through closure or those messages.
///
/// # Endpoint delegation
/// Each runtime has its own storage. After delegation, the new holder opens the
/// same logical session, role and lane through its own transport. It must be able
/// to receive earlier accepted frames as well as new ones, preserving FIFO and
/// replay exclusion across holders. Transport buffers and [`Self::requeue`]
/// must keep unread frames available to that holder.
///
/// Across machines, this can use a stable network inbox or forwarding that keeps
/// unread frames in order when the destination changes. The transport implements
/// this routing. Passing an endpoint transfers its protocol continuation, rather
/// than a Rust address or a socket.
pub trait Transport {
    /// Send handle, which may borrow storage from this transport for `'a`.
    ///
    /// Each send poll and cancellation borrows the handle independently of `'a`.
    type Tx<'a>: 'a
    where
        Self: 'a;
    /// Receive handle, which may borrow storage from this transport for `'a`.
    ///
    /// A returned frame borrows the handle for that receive, not for all of `'a`.
    type Rx<'a>: 'a
    where
        Self: 'a;

    /// Return the binding used to create this resource's first endpoint.
    ///
    /// Called when attaching the initial endpoint of a role that the protocol
    /// can delegate. Ordinary message-only roles do not need a binding.
    /// Returning `None` prevents initial attachment of a delegated role, but
    /// still allows receiving that endpoint through the protocol.
    ///
    /// Use `session` and `role` to select the physical resource. All aliases of
    /// one resource must return the same [`EndpointBinding`], across runtimes
    /// and sessions. Distinct resources can have separate bindings even when
    /// their role numbers match. Starting a session does not renew a used binding.
    fn initial_endpoint_binding(&self, _session: SessionId, _role: u8) -> Option<&EndpointBinding> {
        None
    }

    /// Open handles for the role, session and lane in `port`.
    ///
    /// Handles may borrow storage for `'a`. Each poll or cancellation borrows
    /// the handle separately for that call. On a shared medium, route incoming
    /// frames to the matching port and return their headers with their payloads.
    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);

    /// Progress a send operation using the provided Tx handle.
    ///
    /// `Ready(Ok(()))` accepts one frame. Choose framing and protection in the
    /// transport. The runtime has already checked the requested protocol step.
    ///
    /// After returning [`Poll::Pending`], implementations may retain progress in
    /// `tx` but must not retain the payload pointer. A later poll provides the
    /// same encoded content from scratch storage whose address may differ.
    /// The handle borrow and payload borrow are independent of each other and
    /// of any storage borrowed inside `Tx`. Neither can be retained in `Tx`.
    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>;

    /// Cancel any transport-owned pending send state bound to `tx`.
    ///
    /// Called when a pending send future is dropped. Discard staged bytes so a
    /// later send cannot transmit them. If some bytes can no longer be withdrawn,
    /// disable the logical send direction for the rest of the run. Later sends
    /// must return a terminal [`TransportError`] rather than resume a partial or
    /// cancelled frame.
    /// The mutable handle borrow ends when this call returns.
    fn cancel_send(&self, tx: &mut Self::Tx<'_>);

    /// Progress a receive operation using the provided Rx handle.
    ///
    /// Return payload bytes and their header in one borrowed [`ReceivedFrame`].
    /// Its lifetime `'a` is tied to this call's borrow of `rx`, separately from
    /// `Rx`'s internal storage lifetime. The caller must release the frame before
    /// using that receive handle again. On `Pending`, store the current waker so
    /// an I/O event can wake the task.
    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>>;

    /// Requeue the most recent frame obtained from [`poll_recv`](Transport::poll_recv).
    ///
    /// The runtime calls this after taking a frame without advancing the local
    /// protocol. Make that same frame available to the next receive on this
    /// `Rx`, before later frames. Returning it again must not duplicate a message.
    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError>;
}

mod binding;
/// Observability helpers for logical frame inspection.
pub(crate) mod trace;
pub use binding::EndpointBinding;
/// Wire helpers: payload views and serialization traits.
pub(crate) mod wire;

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
