use crate::global::const_dsl::ScopeId;

/// Precomputed dynamic resolver site discovered during program lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteResolverSite {
    scope: ScopeId,
    resolver_id: u16,
}

impl RouteResolverSite {
    #[inline(always)]
    pub(crate) const fn new(scope: ScopeId, resolver_id: u16) -> Self {
        Self { resolver_id, scope }
    }

    #[inline(always)]
    pub(crate) const fn resolver_id(&self) -> u16 {
        self.resolver_id
    }

    #[inline(always)]
    pub(crate) const fn scope(&self) -> ScopeId {
        self.scope
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum EventSemanticKind {
    ProtocolEvent = 0,
    DecisionArm = 1,
}

impl EventSemanticKind {
    #[inline(always)]
    pub(crate) const fn packed_bits(self) -> u8 {
        match self {
            Self::ProtocolEvent => 0,
            Self::DecisionArm => 1,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_bits(bits: u8) -> Self {
        match bits {
            0 => Self::ProtocolEvent,
            1 => Self::DecisionArm,
            _ => crate::invariant(),
        }
    }
}

pub(crate) const MAX_COMPILED_PROGRAM_SCOPES: usize = crate::eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) dynamic_resolver_sites: usize,
    pub(crate) route_resolvers: usize,
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompiledProgramCounts, EventSemanticKind, RouteResolverSite};
    use crate::global::const_dsl::ScopeId;

    #[test]
    fn route_resolver_site_is_scope_id_and_resolver_id_only() {
        assert_eq!(size_of::<ScopeId>(), 2);
        assert_eq!(size_of::<RouteResolverSite>(), 4);
    }

    #[test]
    fn compiled_program_counts_remain_plain_derived_counts() {
        assert_eq!(size_of::<CompiledProgramCounts>(), 2 * size_of::<usize>());
        let max = CompiledProgramCounts {
            dynamic_resolver_sites: crate::eff::meta::MAX_EFF_NODES,
            route_resolvers: crate::eff::meta::MAX_EFF_NODES,
        };
        assert!(max.dynamic_resolver_sites > 0);
        assert!(max.route_resolvers > 0);
    }

    #[test]
    fn compiled_program_marks_route_decision_semantics() {
        assert_eq!(
            EventSemanticKind::from_packed_bits(0),
            EventSemanticKind::ProtocolEvent
        );
        assert_eq!(
            EventSemanticKind::from_packed_bits(1),
            EventSemanticKind::DecisionArm
        );
    }
}
