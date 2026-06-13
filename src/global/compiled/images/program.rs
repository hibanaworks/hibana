use crate::{eff::EffIndex, global::const_dsl::ResolverMode};

/// Precomputed dynamic resolver site discovered during program lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicResolverSite {
    eff_index: EffIndex,
    logical_label: u8,
    resolver: ResolverMode,
}

impl DynamicResolverSite {
    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        logical_label: u8,
        resolver: ResolverMode,
    ) -> Self {
        Self {
            eff_index,
            logical_label,
            resolver,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(&self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    pub(crate) const fn logical_label(&self) -> u8 {
        self.logical_label
    }

    #[inline(always)]
    pub(crate) const fn resolver(&self) -> ResolverMode {
        self.resolver
    }

    #[inline(always)]
    pub(crate) const fn resolver_id(&self) -> u16 {
        match self.resolver {
            ResolverMode::Dynamic { resolver_id, .. } => resolver_id,
            ResolverMode::Static => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum EventSemanticKind {
    Other = 0,
    DecisionArm = 1,
}

impl EventSemanticKind {
    #[inline(always)]
    pub(crate) const fn packed_bits(self) -> u8 {
        match self {
            Self::Other => 0,
            Self::DecisionArm => 1,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Other,
            1 => Self::DecisionArm,
            _ => panic!("invalid packed event semantic bits"),
        }
    }
}

pub(crate) const MAX_COMPILED_PROGRAM_TAP_EVENTS: usize = 512;
pub(crate) const MAX_COMPILED_PROGRAM_RESOURCES: usize = 128;
pub(crate) const MAX_COMPILED_PROGRAM_SCOPES: usize = crate::eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) tap_events: usize,
    pub(crate) resources: usize,
    pub(crate) dynamic_resolver_sites: usize,
    pub(crate) route_resolvers: usize,
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompiledProgramCounts, EventSemanticKind};
    #[test]
    fn compiled_program_counts_remain_plain_derived_counts() {
        assert_eq!(size_of::<CompiledProgramCounts>(), 4 * size_of::<usize>());
        let max = CompiledProgramCounts {
            tap_events: super::MAX_COMPILED_PROGRAM_TAP_EVENTS,
            resources: super::MAX_COMPILED_PROGRAM_RESOURCES,
            dynamic_resolver_sites: crate::eff::meta::MAX_EFF_NODES,
            route_resolvers: crate::eff::meta::MAX_EFF_NODES,
        };
        assert!(max.tap_events > 0);
        assert!(max.resources > 0);
        assert!(max.dynamic_resolver_sites > 0);
        assert!(max.route_resolvers > 0);
    }

    #[test]
    fn compiled_program_marks_route_decision_semantics() {
        assert_eq!(
            EventSemanticKind::from_packed_bits(0),
            EventSemanticKind::Other
        );
        assert_eq!(
            EventSemanticKind::from_packed_bits(1),
            EventSemanticKind::DecisionArm
        );
    }
}
