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

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) dynamic_resolver_sites: usize,
    pub(crate) route_resolvers: usize,
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompiledProgramCounts, EventSemanticKind};

    #[test]
    fn compiled_program_counts_remain_plain_derived_counts() {
        assert_eq!(size_of::<CompiledProgramCounts>(), 2 * size_of::<usize>());
        let max = CompiledProgramCounts {
            dynamic_resolver_sites: crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY,
            route_resolvers: crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY,
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
