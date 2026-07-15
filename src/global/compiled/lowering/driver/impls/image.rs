use super::super::*;

impl CompiledProgramImage {
    const fn scan_into<const E: usize>(summary: &mut Self, eff_list: &EffList<E>) {
        let mut scope_count = 0u16;
        let mut max_role = 0u8;
        let mut has_atom = false;
        if eff_list.len() > u16::MAX as usize {
            panic!("choreography exceeds compact event domain");
        }
        summary.program.lowering_facts.eff_count = eff_list.len() as u16;
        let mut idx = 0usize;
        while idx < eff_list.len() {
            let node = eff_list.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                has_atom = true;
                if atom.from > max_role {
                    max_role = atom.from;
                }
                if atom.to > max_role {
                    max_role = atom.to;
                }
            }
            idx += 1;
        }

        let src_scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        let mut active_scope_depth = 0u16;
        let mut max_active_scope_depth = 0u16;
        let mut active_route_depth = 0u16;
        let mut max_route_depth = 0u16;
        while scope_idx < src_scope_markers.len() {
            let marker = src_scope_markers.at(scope_idx);
            match marker.event {
                ScopeEvent::Enter => {
                    scope_count = increment_compact_count(scope_count);
                    active_scope_depth = increment_compact_count(active_scope_depth);
                    if active_scope_depth > max_active_scope_depth {
                        max_active_scope_depth = active_scope_depth;
                    }
                    if matches!(
                        marker.scope_id.kind(),
                        Some(crate::global::const_dsl::ScopeKind::Parallel)
                    ) {
                        summary.program.lowering_facts.parallel_enter_count =
                            increment_compact_count(
                                summary.program.lowering_facts.parallel_enter_count,
                            );
                    } else if matches!(
                        marker.scope_id.kind(),
                        Some(crate::global::const_dsl::ScopeKind::Route)
                    ) {
                        active_route_depth = increment_compact_count(active_route_depth);
                        if active_route_depth > max_route_depth {
                            max_route_depth = active_route_depth;
                        }
                        if src_scope_markers.is_first_enter(scope_idx) {
                            summary.program.lowering_facts.route_scope_count =
                                increment_compact_count(
                                    summary.program.lowering_facts.route_scope_count,
                                );
                            summary.program.compiled_program_counts.route_resolvers =
                                summary.program.lowering_facts.route_scope_count as usize;
                            if eff_list.resolver_for_scope(marker.scope_id).is_some() {
                                summary
                                    .program
                                    .compiled_program_counts
                                    .dynamic_resolver_sites += 1;
                            }
                        }
                    }
                }
                ScopeEvent::Exit => {
                    if matches!(
                        marker.scope_id.kind(),
                        Some(crate::global::const_dsl::ScopeKind::Route)
                    ) {
                        if active_route_depth == 0 {
                            panic!("route scope depth underflow");
                        }
                        active_route_depth -= 1;
                    }
                    if active_scope_depth == 0 {
                        panic!("scope depth underflow");
                    }
                    active_scope_depth -= 1;
                }
                ScopeEvent::Split => {}
            }
            scope_idx += 1;
        }

        if !has_atom {
            panic!("compiled choreography requires at least one event");
        }

        summary.program.lowering_facts.scope_count = scope_count;
        summary.program.lowering_facts.max_active_scope_depth = max_active_scope_depth;
        summary.program.lowering_facts.max_route_stack_depth = if max_route_depth == 0 {
            0
        } else {
            increment_compact_count(max_route_depth)
        };
        summary.max_role = max_role;
    }

    const fn scan_impl<const E: usize>(eff_list: &EffList<E>) -> Self {
        let mut summary = Self {
            program: ProgramImageData {
                compiled_program_counts: CompiledProgramCounts {
                    dynamic_resolver_sites: 0,
                    route_resolvers: 0,
                },
                lowering_facts: ProgramLoweringFacts::EMPTY,
            },
            max_role: 0,
        };
        Self::scan_into(&mut summary, eff_list);
        summary
    }

    #[inline(always)]
    pub(crate) const fn scan_const<const E: usize>(eff_list: &EffList<E>) -> Self {
        Self::scan_impl(eff_list)
    }

    #[inline(always)]
    pub(in crate::global::compiled::lowering) const fn max_route_stack_depth_for_projection(
        &self,
    ) -> usize {
        self.program.lowering_facts.max_route_stack_depth as usize
    }

    #[inline(always)]
    pub(crate) const fn compiled_program_role_count(&self) -> usize {
        self.max_role as usize + 1
    }

    #[inline(always)]
    pub(crate) const fn contains_role(&self, role: u8) -> bool {
        role <= self.max_role
    }

    #[inline(always)]
    pub(crate) const fn max_role(&self) -> u8 {
        self.max_role
    }

    #[inline(always)]
    pub(crate) const fn role_lowering_counts<const E: usize>(
        &self,
        eff_list: &EffList<E>,
        role: u8,
    ) -> RoleCompiledCounts {
        if !self.contains_role(role) {
            panic!("projected role is outside the choreography role range");
        }
        let role = crate::global::compiled::lowering::seal::exact_role_facts(eff_list, role);
        RoleCompiledCounts {
            max_route_stack_depth: self.program.lowering_facts.max_route_stack_depth as usize,
            local_step_count: role.local_step_count as usize,
            route_scope_count: self.program.lowering_facts.route_scope_count as usize,
            active_lane_count: role.active_lane_count as usize,
            endpoint_lane_slot_count: role.endpoint_lane_slot_count as usize,
            logical_lane_count: role.logical_lane_count as usize,
        }
    }

    #[inline(always)]
    pub(crate) const fn validate_projection_program(&self) {
        self.program.validate_projection_program();
    }
}
