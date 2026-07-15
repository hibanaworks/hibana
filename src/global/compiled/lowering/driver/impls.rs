use super::ProgramImageData;

impl ProgramImageData {
    #[inline(always)]
    const fn validate_projection_program(&self) {
        let scope_capacity = crate::global::const_dsl::ScopeId::LOCAL_CAPACITY as usize;
        if self.compiled_program_counts.dynamic_resolver_sites > scope_capacity {
            panic!("compiled program resolver-site scope domain exceeded");
        }
        if self.compiled_program_counts.route_resolvers > scope_capacity {
            panic!("compiled program route scope domain exceeded");
        }
        if self.lowering_facts.scope_count as usize > scope_capacity {
            panic!("compiled program scope domain exceeded");
        }
    }
}

mod image;
