//! Cluster error types.
//!
//! `ClusterError` is the internal cluster failure catalogue. Public attach
//! failures use `AttachError`, which records the attach boundary for Debug
//! evidence so protocol runtimes can propagate attach errors with `?` without
//! adding an extra context type at every call site.

use core::fmt;

mod debug;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachOp {
    Attach,
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
    kind: AttachErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttachErrorKind {
    Cluster(ClusterError),
    Rendezvous(crate::rendezvous::error::RendezvousError),
}

impl fmt::Debug for AttachError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("AttachError");
        debug.field("operation", &self.op_name());
        debug.field("kind", &self.kind).finish()
    }
}

impl AttachError {
    #[inline]
    pub(crate) fn cluster(error: ClusterError) -> Self {
        Self {
            op: AttachOp::Attach,
            kind: AttachErrorKind::Cluster(error),
        }
    }

    #[inline]
    pub(crate) fn rendezvous(error: crate::rendezvous::error::RendezvousError) -> Self {
        Self {
            op: AttachOp::Attach,
            kind: AttachErrorKind::Rendezvous(error),
        }
    }

    #[inline]
    pub(crate) const fn with_operation(mut self, op: AttachOp) -> Self {
        self.op = op;
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
    const fn op_name(&self) -> &'static str {
        match self.op {
            AttachOp::Attach => "attach",
            AttachOp::Rendezvous => "rendezvous",
            AttachOp::Enter => "enter",
        }
    }
}

impl From<ClusterError> for AttachError {
    #[inline]
    fn from(err: ClusterError) -> Self {
        Self::cluster(err)
    }
}

impl From<crate::rendezvous::error::RendezvousError> for AttachError {
    #[inline]
    fn from(err: crate::rendezvous::error::RendezvousError) -> Self {
        Self::rendezvous(err)
    }
}

/// Cluster attach and resolver failure catalogue.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClusterError {
    /// Rendezvous ID mismatch.
    RendezvousMismatch { expected: u16, actual: u16 },

    /// Requested rendezvous is not registered in this session cluster.
    RendezvousUnregistered { id: u16 },

    /// Rendezvous exists but is currently protected by an affine lease.
    RendezvousBusy { id: u16 },

    /// Roles attached under one session ID came from different program images.
    SessionProgramMismatch { sid: u32 },

    /// Resource exhaustion in a specific cluster storage area.
    ResourceExhausted { resource: ResourceScope },

    /// Registered resolver rejected the operation.
    ResolverReject { resolver_id: u16 },

    /// Registered resolver metadata does not match the resident dynamic resolver site.
    DynamicResolverInvariant { resolver_id: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceScope {
    RendezvousTable,
    LaneStorage,
    ResolverTable,
    RouteTable,
    EndpointLease,
}

impl ResourceScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RendezvousTable => "rv",
            Self::LaneStorage => "lane",
            Self::ResolverTable => "resolver",
            Self::RouteTable => "route",
            Self::EndpointLease => "ep-lease",
        }
    }
}

impl ClusterError {
    #[inline]
    pub(crate) const fn resource_exhausted(resource: ResourceScope) -> Self {
        Self::ResourceExhausted { resource }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

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
