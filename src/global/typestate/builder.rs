//! Typestate owner and validation facade.

#[cfg(test)]
use super::facts::MAX_STATES;
use super::{facts::LocalNode, registry::ScopeRegistry};

pub(crate) use super::registry::{ARM_SHARED, MAX_FIRST_RECV_DISPATCH, ScopeRegion};

#[inline(always)]
pub(super) const fn encode_typestate_len(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("typestate length overflow");
    }
    value as u16
}

/// Role-specific typestate graph synthesized from a global effect list.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleTypestate<const ROLE: u8> {
    pub(super) nodes: [LocalNode; MAX_STATES],
    pub(super) len: u16,
    pub(super) scope_registry: ScopeRegistry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoleTypestateValue {
    pub(super) nodes: *const LocalNode,
    pub(super) len: u16,
    pub(super) scope_registry: ScopeRegistry,
}
