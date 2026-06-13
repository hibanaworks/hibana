use core::fmt;

use super::{ClusterError, ResourceScope};

impl fmt::Display for ClusterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RendezvousMismatch { expected, actual } => {
                write!(f, "rv-mismatch expected {} got {}", expected, actual)
            }
            Self::RendezvousMissing { id } => write!(f, "rv-missing {}", id),
            Self::RendezvousBusy { id } => write!(f, "rv-busy {}", id),
            Self::ResourceExhausted { resource } => write!(f, "exhausted {}", resource.as_str()),
            Self::UnsupportedEffect(op) => write!(f, "effect {}", op),
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
