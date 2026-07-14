use super::{MAX_COMPILED_IMAGE_NODES, MAX_COMPILED_PROGRAM_SCOPES, ProgramImageData};

impl ProgramImageData {
    #[inline(always)]
    const fn validate_projection_program(&self) {
        if self.compiled_program_counts.dynamic_resolver_sites > MAX_COMPILED_IMAGE_NODES {
            panic!("CompiledProgram: MAX_DYNAMIC_RESOLVER_SITES exceeded");
        }
        if self.compiled_program_counts.route_resolvers > MAX_COMPILED_IMAGE_NODES {
            panic!("CompiledProgram: MAX_ROUTE_RESOLVERS exceeded");
        }
        if self.lowering_facts.scope_count as usize > MAX_COMPILED_PROGRAM_SCOPES {
            panic!("CompiledProgram: MAX_SCOPES exceeded");
        }
    }
}

mod image;
