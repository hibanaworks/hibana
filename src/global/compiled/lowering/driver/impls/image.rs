use super::super::*;

impl CompiledProgramImage {
    const fn compact_role_count(role_count: usize) -> u8 {
        if role_count > crate::g::ROLE_DOMAIN_SIZE as usize {
            panic!("lowering role count exceeds choreography role domain");
        }
        role_count as u8
    }

    const fn scan_into(summary: &mut Self, eff_list: &EffList) {
        let mut scope_count = 0u16;
        let mut role_count = 0usize;
        let mut route_scope_ordinals = [0u8; ROUTE_SCOPE_ORDINAL_BYTES];
        summary.program.lowering_facts.eff_count = eff_list.len() as u16;
        let mut idx = 0usize;
        while idx < eff_list.len() {
            let node = eff_list.node_at(idx);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                let from = checked_role_index(atom.from);
                let to = checked_role_index(atom.to);
                summary.roles.facts[from].local_step_count =
                    increment_compact_count(summary.roles.facts[from].local_step_count);
                if to != from {
                    summary.roles.facts[to].local_step_count =
                        increment_compact_count(summary.roles.facts[to].local_step_count);
                }
                if from + 1 > role_count {
                    role_count = from + 1;
                }
                if to + 1 > role_count {
                    role_count = to + 1;
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
            let marker = src_scope_markers[scope_idx];
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
                        let ordinal = marker.scope_id.local_ordinal() as usize;
                        let byte = ordinal >> 3;
                        let bit = ordinal & 7;
                        if byte >= route_scope_ordinals.len() {
                            panic!("route scope ordinal overflow");
                        }
                        let mask = 1u8 << bit;
                        if (route_scope_ordinals[byte] & mask) == 0 {
                            route_scope_ordinals[byte] |= mask;
                            summary.program.lowering_facts.route_scope_count =
                                increment_compact_count(
                                    summary.program.lowering_facts.route_scope_count,
                                );
                            summary.program.compiled_program_counts.route_resolvers =
                                summary.program.lowering_facts.route_scope_count as usize;
                            if let Some(RouteResolver::Dynamic { resolver_id, .. }) =
                                eff_list.resolver_for_scope(marker.scope_id)
                            {
                                let _ = resolver_id;
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

        let mut role_idx = 0usize;
        while role_idx < role_count {
            let exact_facts =
                crate::global::compiled::lowering::seal::exact_role_resident_row_facts(
                    eff_list,
                    src_scope_markers,
                    role_idx as u8,
                );
            summary.roles.facts[role_idx].resident_row_count = exact_facts.resident_row_count;
            summary.roles.facts[role_idx].resident_row_lane_entry_count =
                exact_facts.resident_row_lane_entry_count;
            summary.roles.facts[role_idx].resident_row_lane_word_count =
                exact_facts.resident_row_lane_word_count;
            summary.roles.facts[role_idx].active_lane_count = exact_facts.active_lane_count;
            summary.roles.facts[role_idx].endpoint_lane_slot_count =
                exact_facts.endpoint_lane_slot_count;
            summary.roles.facts[role_idx].logical_lane_count = exact_facts.logical_lane_count;
            role_idx += 1;
        }

        summary.program.lowering_facts.scope_count = scope_count;
        summary.program.lowering_facts.max_active_scope_depth = max_active_scope_depth;
        summary.program.lowering_facts.max_route_stack_depth = if max_route_depth == 0 {
            0
        } else {
            increment_compact_count(max_route_depth)
        };
        summary.roles.count = Self::compact_role_count(role_count);
    }

    const fn scan_impl(eff_list: &EffList) -> Self {
        let mut summary = Self {
            program: ProgramImageData {
                compiled_program_counts: CompiledProgramCounts {
                    dynamic_resolver_sites: 0,
                    route_resolvers: 0,
                },
                lowering_facts: ProgramLoweringFacts::EMPTY,
            },
            roles: ProgramRoleImageData {
                facts: [RoleCompiledFacts::EMPTY; MAX_TRACKED_ROLE_FACTS],
                count: 0,
            },
        };
        Self::scan_into(&mut summary, eff_list);
        summary
    }

    #[inline(always)]
    pub(crate) const fn scan_const(eff_list: &EffList) -> Self {
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
        self.roles.count as usize
    }

    #[inline(always)]
    pub(crate) const fn role_lowering_counts(&self, role: u8) -> RoleCompiledCounts {
        self.roles
            .lowering_counts(self.program.lowering_facts, role)
    }

    #[inline(always)]
    pub(crate) const fn validate_projection_program(&self) {
        self.program.validate_projection_program();
    }
}
