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

use crate::{eff::EffIndex, transport::wire::Payload};

mod labels;

pub use labels::FrameLabel;
pub(crate) use labels::{FrameLabelMask, LogicalLabel};

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
    /// Whether this is a control message.
    pub is_control: bool,
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
    pub const fn payload(&self) -> Payload<'f> {
        self.payload
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
    /// Open Tx/Rx handles bound to the lifetime of this transport reference.
    ///
    /// `port` carries the projected role/session/lane fact for the returned Tx/Rx
    /// handles. Carriers backed by a shared physical medium must preserve this
    /// lane in frame metadata and demultiplex received frames before returning
    /// payload bytes or route-observation hints to the endpoint.
    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);

    /// Progress a send operation using the provided Tx handle.
    ///
    /// Transport implementations select any carrier-local framing or protection
    /// state internally. Hibana passes descriptor-checked bytes; it does not
    /// expose protocol-specific transport phases as core concepts.
    fn poll_send<'a, 'f>(
        &self,
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
    /// discard that staged state here so cancelled payload bytes cannot be
    /// flushed by a subsequent operation.
    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>);

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
    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error>;

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
    fn recv_frame_hint<'a>(&self, rx: &mut Self::Rx<'a>) -> Option<FrameLabel> {
        let _ = rx;
        None
    }
}

/// Transport context provider for resolver state access.
pub(crate) mod context;
/// Observability helpers for logical frame inspection.
pub(crate) mod trace;
/// Wire helpers: payload views and serialization traits.
pub(crate) mod wire;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
