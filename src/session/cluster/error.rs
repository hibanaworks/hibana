//! Cluster error types.
//!
//! `ClusterError` is the internal cluster failure catalogue. Public attach
//! failures use `AttachError`, which records the public attach operation
//! callsite so protocol runtimes can propagate attach errors with `?`
//! without adding an extra context type at every call site.

use core::{fmt, panic::Location};

mod debug;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
pub(crate) enum AttachOp {
    Internal,
    Rendezvous,
    Enter,
}

/// Errors raised while attaching cursor endpoints to the session cluster.
///
/// Attach failures are public evidence for rendezvous/endpoint setup. They are
/// intentionally separate from endpoint progress errors so `?` can preserve the
/// failing boundary without a wide crate-level error enum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AttachError {
    op: AttachOp,
    location: ErrorLocation,
    kind: AttachErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttachErrorKind {
    Cluster(ClusterError),
    Rendezvous(crate::rendezvous::error::RendezvousError),
}

impl fmt::Debug for AttachError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachError")
            .field("operation", &self.operation())
            .field("file", &self.file())
            .field("line", &self.line())
            .field("column", &self.column())
            .field("kind", &self.kind)
            .finish()
    }
}

impl AttachError {
    #[inline]
    #[track_caller]
    pub(crate) fn cluster(error: ClusterError) -> Self {
        Self {
            op: AttachOp::Internal,
            location: ErrorLocation::caller(),
            kind: AttachErrorKind::Cluster(error),
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn rendezvous(error: crate::rendezvous::error::RendezvousError) -> Self {
        Self {
            op: AttachOp::Internal,
            location: ErrorLocation::caller(),
            kind: AttachErrorKind::Rendezvous(error),
        }
    }

    #[inline]
    pub(crate) const fn with_operation(mut self, op: AttachOp, location: ErrorLocation) -> Self {
        self.op = op;
        self.location = location;
        self
    }

    #[inline]
    pub(crate) const fn cluster_cause(&self) -> Option<ClusterError> {
        match self.kind {
            AttachErrorKind::Cluster(error) => Some(error),
            AttachErrorKind::Rendezvous(_) => None,
        }
    }

    #[inline]
    pub const fn operation(&self) -> &'static str {
        match self.op {
            AttachOp::Internal => "attach",
            AttachOp::Rendezvous => "rendezvous",
            AttachOp::Enter => "enter",
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

impl From<ClusterError> for AttachError {
    #[inline]
    #[track_caller]
    fn from(err: ClusterError) -> Self {
        Self::cluster(err)
    }
}

impl From<crate::rendezvous::error::RendezvousError> for AttachError {
    #[inline]
    #[track_caller]
    fn from(err: crate::rendezvous::error::RendezvousError) -> Self {
        Self::rendezvous(err)
    }
}

/// Cluster attach and resolver error type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClusterError {
    /// Rendezvous ID mismatch.
    RendezvousMismatch { expected: u16, actual: u16 },

    /// Requested rendezvous was not registered in this session cluster.
    RendezvousMissing { id: u16 },

    /// Rendezvous exists but is currently protected by an affine lease.
    RendezvousBusy { id: u16 },

    /// Resource exhaustion in a specific cluster storage area.
    ResourceExhausted { resource: ResourceScope },

    /// Effect not supported by the target session descriptor.
    UnsupportedEffect(u8),

    /// Registered resolver rejected the operation.
    ResolverReject { resolver_id: u16 },

    /// Registered resolver metadata does not match the resident dynamic resolver site.
    DynamicResolverInvariant { resolver_id: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResourceScope {
    RendezvousTable,
    LaneStorage,
    ResolverTable,
    RouteTable,
    EndpointLease,
    EndpointBounds,
    EndpointMark,
    EndpointHeader,
}

impl ResourceScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RendezvousTable => "rv",
            Self::LaneStorage => "lane",
            Self::ResolverTable => "resolver",
            Self::RouteTable => "route",
            Self::EndpointLease => "ep-lease",
            Self::EndpointBounds => "ep-bounds",
            Self::EndpointMark => "ep-mark",
            Self::EndpointHeader => "ep-header",
        }
    }
}

impl ClusterError {
    #[inline]
    pub const fn resource_exhausted(resource: ResourceScope) -> Self {
        Self::ResourceExhausted { resource }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ClusterError {}

// Tests use `format!` which requires `alloc`/`std`. Gate them behind `std` so
// that rust-analyzer (no_std default) doesn't flag errors, while CI runs them
// under `--all-features` (which enables `std`).
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ClusterError::RendezvousMismatch {
            expected: 1,
            actual: 2,
        };
        let s = format!("{}", err);
        assert!(s.contains("expected 1"));
        assert!(s.contains("got 2"));
    }
}
