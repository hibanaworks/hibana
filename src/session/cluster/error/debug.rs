use core::fmt;

use super::{ClusterError, ResourceScope};

impl fmt::Display for ClusterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RendezvousMismatch { expected, actual } => {
                write!(f, "rv-mismatch expected {} got {}", expected, actual)
            }
            Self::RendezvousUnregistered { id } => write!(f, "rv-unregistered {}", id),
            Self::RendezvousBusy { id } => write!(f, "rv-busy {}", id),
            Self::SessionProgramMismatch { sid } => {
                write!(f, "session-program-mismatch {}", sid)
            }
            Self::SessionMembershipSealed { sid } => {
                write!(f, "session-membership-sealed {}", sid)
            }
            Self::ResourceExhausted { resource } => write!(f, "exhausted {}", resource.as_str()),
            Self::ResolverReject { resolver_id } => {
                write!(f, "resolver-reject {}", resolver_id)
            }
            Self::DynamicResolverInvariant { resolver_id } => {
                write!(f, "dynamic-resolver-invariant {}", resolver_id)
            }
        }
    }
}

impl fmt::Debug for ClusterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for ResourceScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
