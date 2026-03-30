//! Typestate owner and validation facade.

use super::{
    facts::{LocalNode, MAX_STATES},
    registry::ScopeRegistry,
};

pub(crate) use super::registry::{ARM_SHARED, MAX_FIRST_RECV_DISPATCH, ScopeRegion};

/// Role-specific typestate graph synthesized from a global effect list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleTypestate<const ROLE: u8> {
    pub(super) nodes: [LocalNode; MAX_STATES],
    pub(super) len: usize,
    pub(super) scope_registry: ScopeRegistry,
}

pub(crate) type RoleTypestateValue = RoleTypestate<0>;
