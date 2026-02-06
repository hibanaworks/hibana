//! Cursor-driven endpoint API built on the typestate DSL.
//!
//! Applications interact with cursor-driven endpoints that are materialised
//! from `RoleProgram` projections.

/// Affine endpoint helpers.
pub mod affine;
/// Control-plane helpers for endpoints.
pub mod control;
/// Cursor endpoint implementation.
pub mod cursor;
/// Delegation helpers.
pub mod delegate;
/// Flow-based send API.
pub mod flow;
/// Endpoint metadata helpers.
pub mod meta;
/// Resolver plumbing for endpoints.
pub mod resolver;

pub use control::ControlOutcome;
pub use cursor::{CursorEndpoint, LoopDecision, RouteBranch};
pub use flow::CapFlow;

use crate::{
    rendezvous::RendezvousError,
    transport::{TransportError, wire::CodecError},
};

/// Unified endpoint error type used by cursor endpoints.
#[derive(Debug)]
pub enum Cancel {
    /// Control-plane invariant violation (e.g., loop metadata mismatch).
    ControlPlaneInvariant,
    /// Rendezvous-level error surfaced during execution.
    Rendezvous(RendezvousError),
    /// Transport error bubbled from the underlying `Transport`.
    Transport(TransportError),
}

impl From<RendezvousError> for Cancel {
    fn from(err: RendezvousError) -> Self {
        Self::Rendezvous(err)
    }
}

impl From<TransportError> for Cancel {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

/// Endpoint-level result type.
pub type Result<T> = core::result::Result<T, Cancel>;

/// Send error placeholder (will specialise once send/recv API lands).
/// Errors surfaced when sending frames through a cursor endpoint.
#[derive(Debug)]
pub enum SendError {
    /// Payload encoding failed.
    Codec(CodecError),
    /// Transport returned an error while transmitting the frame.
    Transport(TransportError),
    /// Endpoint typestate did not permit a send at this point.
    PhaseInvariant,
    /// Attempted to send a message whose label does not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Policy VM aborted the send operation.
    PolicyAbort { reason: u16 },
    /// Binding layer hook returned an error.
    Binding,
}

/// Errors surfaced when receiving frames through a cursor endpoint.
#[derive(Debug)]
pub enum RecvError {
    /// Transport returned an error while awaiting the next frame.
    Transport(TransportError),
    /// Binding layer failed to read from channel.
    Binding(crate::binding::TransportOpsError),
    /// Payload decoding failed.
    Codec(CodecError),
    /// Endpoint typestate did not permit a receive at this point.
    PhaseInvariant,
    /// Incoming frame label did not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Incoming frame originated from an unexpected peer role.
    PeerMismatch { expected: u8, actual: u8 },
    /// Session or lane did not match the endpoint.
    SessionMismatch {
        expected_sid: u32,
        received_sid: u32,
        expected_lane: u8,
        received_lane: u8,
    },
    /// Policy VM aborted the receive operation.
    PolicyAbort { reason: u16 },
}

/// Guard that keeps the receive buffer borrowed for the duration of message
/// decoding.
///
/// Cursor endpoints borrow frame payloads directly from the rendezvous slab.
/// Dropping the guard releases that borrow so the next receive can reuse the
/// backing storage. The guard is intentionally zero-sized; it exists purely to
/// express the lifetime dependency at the type level.
pub struct RecvGuard<'a>(core::marker::PhantomData<&'a ()>);

/// Send result alias.
pub type SendResult<T> = core::result::Result<T, SendError>;

/// Receive result alias.
pub type RecvResult<T> = core::result::Result<T, RecvError>;

/// Errors surfaced when executing local actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalFailureReason {
    code: u16,
    tag: &'static str,
}

impl LocalFailureReason {
    /// Generic handler error reported by user code.
    pub const HANDLER: Self = Self::custom(0x0001, "handler");
    /// Payload or prerequisite state was missing.
    pub const PAYLOAD_UNAVAILABLE: Self = Self::custom(0x0002, "payload_unavailable");
    /// Internal invariant violation detected by the runtime.
    pub const INTERNAL: Self = Self::custom(0xFFFF, "internal");

    /// Create a custom failure reason (0x0000-0xFFFE reserved for user space).
    pub const fn custom(code: u16, tag: &'static str) -> Self {
        Self { code, tag }
    }

    /// Raw numeric representation used in tap events.
    pub const fn value(self) -> u16 {
        self.code
    }

    /// Human-readable identifier associated with the failure code.
    pub const fn tag(self) -> &'static str {
        self.tag
    }

    #[inline]
    pub const fn from_raw(raw: u16) -> Self {
        Self {
            code: raw,
            tag: "unknown",
        }
    }
}
