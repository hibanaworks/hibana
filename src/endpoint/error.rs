use crate::{
    diag::Callsite,
    transport::{TransportError, wire::CodecError},
};
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointOp {
    Send,
    Recv,
    Offer,
}

/// Domain error for endpoint progress.
///
/// The API shape stays on `send/recv/offer/decode`; this error records
/// which operation failed, so callers can keep using plain `?` without extra
/// context types. The diagnostic kind is deliberately private: application code
/// should not match endpoint failures to continue the same generation on an
/// alternate route.
#[derive(Clone, Copy)]
pub struct EndpointError {
    op: EndpointOp,
    _location: Callsite,
    kind: EndpointErrorKind,
}

impl fmt::Debug for EndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("EndpointError");
        debug.field("operation", &self.op_name());
        #[cfg(feature = "std")]
        {
            debug
                .field("file", &self._location.file())
                .field("line", &self._location.line())
                .field("column", &self._location.column());
        }
        debug.field("kind", &self.kind).finish()
    }
}

impl EndpointError {
    #[inline]
    pub(super) fn new<E>(op: EndpointOp, location: Callsite, error: E) -> Self
    where
        EndpointErrorKind: From<E>,
    {
        Self {
            op,
            _location: location,
            kind: EndpointErrorKind::from(error),
        }
    }

    #[inline]
    const fn op_name(&self) -> &'static str {
        match self.op {
            EndpointOp::Send => "send",
            EndpointOp::Recv => "recv",
            EndpointOp::Offer => "offer",
        }
    }
}

/// Endpoint progress failure kind independent of the public call boundary.
#[derive(Clone, Copy)]
pub(super) enum EndpointErrorKind {
    Codec(CodecError),
    Transport(TransportError),
    PhaseInvariant,
    LabelMismatch { expected: u8, actual: u8 },
    ResolverReject { resolver_id: u16 },
    SessionFault(crate::rendezvous::SessionFaultKind),
}

impl fmt::Debug for EndpointErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => formatter.debug_tuple("Codec").field(error).finish(),
            Self::Transport(error) => formatter.debug_tuple("Transport").field(error).finish(),
            Self::PhaseInvariant => formatter.write_str("PhaseInvariant"),
            Self::LabelMismatch { expected, actual } => formatter
                .debug_struct("LabelMismatch")
                .field("expected", expected)
                .field("actual", actual)
                .finish(),
            Self::ResolverReject { resolver_id } => formatter
                .debug_struct("ResolverReject")
                .field("resolver_id", resolver_id)
                .finish(),
            Self::SessionFault(kind) => formatter.debug_tuple("SessionFault").field(kind).finish(),
        }
    }
}

impl From<SendError> for EndpointErrorKind {
    #[inline]
    fn from(error: SendError) -> Self {
        match error {
            SendError::Codec(error) => Self::Codec(error),
            SendError::Transport(error) => Self::Transport(error),
            SendError::PhaseInvariant => Self::PhaseInvariant,
            SendError::LabelMismatch { expected, actual } => {
                Self::LabelMismatch { expected, actual }
            }
            SendError::ResolverReject { resolver_id } => Self::ResolverReject { resolver_id },
            SendError::SessionFault(kind) => Self::SessionFault(kind),
        }
    }
}

impl From<RecvError> for EndpointErrorKind {
    #[inline]
    fn from(error: RecvError) -> Self {
        match error {
            RecvError::Transport(error) => Self::Transport(error),
            RecvError::Codec(error) => Self::Codec(error),
            RecvError::PhaseInvariant => Self::PhaseInvariant,
            RecvError::LabelMismatch { expected, actual } => {
                Self::LabelMismatch { expected, actual }
            }
            RecvError::ResolverReject { resolver_id } => Self::ResolverReject { resolver_id },
            RecvError::SessionFault(kind) => Self::SessionFault(kind),
        }
    }
}

/// Canonical endpoint result returned by public endpoint operations.
pub type EndpointResult<T> = core::result::Result<T, EndpointError>;

/// Errors surfaced inside the endpoint send kernel.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SendError {
    /// Payload encoding failed.
    Codec(CodecError),
    /// Transport returned an error while transmitting the frame.
    Transport(TransportError),
    /// Endpoint typestate or descriptor facts did not permit this send.
    PhaseInvariant,
    /// Attempted to send a message whose label does not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Resolver rejected the send operation.
    ResolverReject { resolver_id: u16 },
    /// Current session generation has terminal fault evidence.
    SessionFault(crate::rendezvous::SessionFaultKind),
}

/// Errors surfaced inside the endpoint receive/decode kernel.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RecvError {
    /// Transport returned an error while awaiting the next frame.
    Transport(TransportError),
    /// Payload decoding failed.
    Codec(CodecError),
    /// Endpoint typestate or descriptor facts did not permit this receive.
    PhaseInvariant,
    /// Choreography logical label did not match the projected typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Resolver rejected the receive operation.
    ResolverReject { resolver_id: u16 },
    /// Current session generation has terminal fault evidence.
    SessionFault(crate::rendezvous::SessionFaultKind),
}

pub(crate) type SendResult<T> = core::result::Result<T, SendError>;

pub(crate) type RecvResult<T> = core::result::Result<T, RecvError>;
