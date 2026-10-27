#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) dynamic_resolver_sites: usize,
    pub(crate) route_resolvers: usize,
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::CompiledProgramCounts;

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
}
