use crate::transport::{TransportError, wire::CodecError};
use core::{fmt, panic::Location};

#[derive(Clone, Copy)]
pub(crate) struct ErrorLocation {
    location: &'static Location<'static>,
}

impl ErrorLocation {
    #[inline]
    #[track_caller]
    pub(crate) fn caller() -> Self {
        Self {
            location: Location::caller(),
        }
    }

    #[inline]
    const fn file(self) -> &'static str {
        self.location.file()
    }

    #[inline]
    const fn line(self) -> u32 {
        self.location.line()
    }

    #[inline]
    const fn column(self) -> u32 {
        self.location.column()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointOp {
    Flow,
    Send,
    Recv,
    Offer,
    Decode,
}

/// Domain error for endpoint progress.
///
/// The API shape stays on `flow/send/recv/offer/decode`; this error records
/// which operation failed and where the public operation was started, so callers
/// can keep using plain `?` without extra context types. The diagnostic kind is deliberately
/// private: application code should not match endpoint failures to continue the
/// same generation on an alternate route.
#[derive(Clone, Copy)]
pub struct EndpointError {
    op: EndpointOp,
    location: ErrorLocation,
    kind: EndpointErrorKind,
}

impl fmt::Debug for EndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EndpointError")
            .field("operation", &self.operation())
            .field("file", &self.file())
            .field("line", &self.line())
            .field("column", &self.column())
            .field("kind", &self.kind)
            .finish()
    }
}

impl EndpointError {
    #[inline]
    pub(super) fn new<E>(op: EndpointOp, location: ErrorLocation, error: E) -> Self
    where
        EndpointErrorKind: From<E>,
    {
        Self {
            op,
            location,
            kind: EndpointErrorKind::from(error),
        }
    }

    #[inline]
    pub const fn operation(&self) -> &'static str {
        match self.op {
            EndpointOp::Flow => "flow",
            EndpointOp::Send => "send",
            EndpointOp::Recv => "recv",
            EndpointOp::Offer => "offer",
            EndpointOp::Decode => "decode",
        }
    }

    #[inline]
    pub const fn file(&self) -> &'static str {
        self.location.file()
    }

    #[inline]
    pub const fn line(&self) -> u32 {
        self.location.line()
    }

    #[inline]
    pub const fn column(&self) -> u32 {
        self.location.column()
    }
}

/// Endpoint progress failure kind independent of the operation callsite.
#[derive(Clone, Copy)]
pub(super) enum EndpointErrorKind {
    Codec(CodecError),
    Transport(TransportError),
    Binding(crate::binding::BindingError),
    PhaseInvariant,
    LabelMismatch { expected: u8, actual: u8 },
    PeerMismatch { expected: u8, actual: u8 },
    PolicyAbort { reason: u16 },
    SessionFault(crate::rendezvous::SessionFaultKind),
}

impl fmt::Debug for EndpointErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => formatter.debug_tuple("Codec").field(error).finish(),
            Self::Transport(error) => formatter.debug_tuple("Transport").field(error).finish(),
            Self::Binding(error) => formatter.debug_tuple("Binding").field(error).finish(),
            Self::PhaseInvariant => formatter.write_str("PhaseInvariant"),
            Self::LabelMismatch { expected, actual } => formatter
                .debug_struct("LabelMismatch")
                .field("expected", expected)
                .field("actual", actual)
                .finish(),
            Self::PeerMismatch { expected, actual } => formatter
                .debug_struct("PeerMismatch")
                .field("expected", expected)
                .field("actual", actual)
                .finish(),
            Self::PolicyAbort { reason } => formatter
                .debug_struct("PolicyAbort")
                .field("reason", reason)
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
            SendError::PolicyAbort { reason } => Self::PolicyAbort { reason },
            SendError::SessionFault(kind) => Self::SessionFault(kind),
        }
    }
}

impl From<RecvError> for EndpointErrorKind {
    #[inline]
    fn from(error: RecvError) -> Self {
        match error {
            RecvError::Transport(error) => Self::Transport(error),
            RecvError::Binding(error) => Self::Binding(error),
            RecvError::Codec(error) => Self::Codec(error),
            RecvError::PhaseInvariant => Self::PhaseInvariant,
            RecvError::LabelMismatch { expected, actual } => {
                Self::LabelMismatch { expected, actual }
            }
            RecvError::PeerMismatch { expected, actual } => Self::PeerMismatch { expected, actual },
            RecvError::PolicyAbort { reason } => Self::PolicyAbort { reason },
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
    /// Policy VM aborted the send operation.
    PolicyAbort { reason: u16 },
    /// Current session generation has terminal fault evidence.
    SessionFault(crate::rendezvous::SessionFaultKind),
}

/// Errors surfaced inside the endpoint receive/decode kernel.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RecvError {
    /// Transport returned an error while awaiting the next frame.
    Transport(TransportError),
    /// Binding layer failed to read from channel.
    Binding(crate::binding::BindingError),
    /// Payload decoding failed.
    Codec(CodecError),
    /// Endpoint typestate or descriptor facts did not permit this receive.
    PhaseInvariant,
    /// Choreography logical label did not match the projected typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Received frame originated from an unexpected peer role.
    PeerMismatch { expected: u8, actual: u8 },
    /// Policy VM aborted the receive operation.
    PolicyAbort { reason: u16 },
    /// Current session generation has terminal fault evidence.
    SessionFault(crate::rendezvous::SessionFaultKind),
}

pub(crate) type SendResult<T> = core::result::Result<T, SendError>;

pub(crate) type RecvResult<T> = core::result::Result<T, RecvError>;
