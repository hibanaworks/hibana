use super::{
    MAX_COMPILED_IMAGE_NODES, MAX_COMPILED_PROGRAM_SCOPES, ProgramImageData, ProgramLoweringFacts,
    ProgramRoleImageData, RoleCompiledCounts,
};

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

impl ProgramRoleImageData {
    #[inline(always)]
    const fn lowering_counts(&self, program: ProgramLoweringFacts, role: u8) -> RoleCompiledCounts {
        let role = self.facts[role as usize];
        RoleCompiledCounts {
            max_route_stack_depth: program.max_route_stack_depth as usize,
            local_step_count: role.local_step_count as usize,
            route_scope_count: program.route_scope_count as usize,
            active_lane_count: role.active_lane_count as usize,
            endpoint_lane_slot_count: role.endpoint_lane_slot_count as usize,
            logical_lane_count: role.logical_lane_count as usize,
        }
    }
}

mod image;
