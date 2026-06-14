use super::super::*;

impl CompiledProgramImage {
    #[inline(always)]
    const fn segment_for_scope_marker_offset(
        offset: usize,
        current_len: usize,
        event: ScopeEvent,
    ) -> usize {
        if offset > current_len || current_len > MAX_COMPILED_IMAGE_NODES {
            panic!("lowering marker offset out of bounds");
        }
        if matches!(event, ScopeEvent::Enter) {
            if offset >= MAX_COMPILED_IMAGE_NODES {
                panic!("lowering marker offset out of bounds");
            }
            return offset / MAX_SEGMENT_EFFS;
        }
        if current_len == 0 {
            0
        } else if offset == current_len && offset.is_multiple_of(MAX_SEGMENT_EFFS) {
            (offset / MAX_SEGMENT_EFFS) - 1
        } else {
            offset / MAX_SEGMENT_EFFS
        }
    }

    const fn compact_role_count(role_count: usize) -> u8 {
        if role_count > crate::g::ROLE_DOMAIN_SIZE as usize {
            panic!("lowering role count exceeds choreography role domain");
        }
        role_count as u8
    }

    const fn scan_into(summary: &mut Self, eff_list: &EffList) {
        let mut scope_count = 0u16;
        let mut resolver_markers_len = 0u16;
        let mut role_count = 0usize;
        let mut route_scope_ordinals = [0u64; ROUTE_SCOPE_ORDINAL_WORDS];
        summary.program.lowering_facts.eff_count = eff_list.len() as u16;
        let mut segment = 0usize;
        while segment < eff_list.segment_count() {
            let segment_start = EffList::segment_start(segment);
            let segment_len = eff_list.segment_len(segment);
            summary.validation.segments[segment].summary = eff_list.segment_summary(segment);
            summary.validation.segments[segment].node_len =
                ProgramImageSegmentData::compact_count(segment_len);
            let mut local = 0usize;
            while local < segment_len {
                let idx = segment_start + local;
                let node = eff_list.node_at(idx);
                let resolver = if let Some((resolver, _scope)) = eff_list.resolver_with_scope(idx) {
                    let row_idx = summary.validation.resolver_row_len;
                    if row_idx < MAX_COMPILED_RESOLVER_ROWS {
                        summary.validation.resolver_rows[row_idx] =
                            ProgramResolverRow::new(idx, resolver);
                        summary.validation.resolver_row_len += 1;
                        if summary.validation.segments[segment].resolver_row_len == 0 {
                            summary.validation.segments[segment].resolver_row_start =
                                ProgramImageSegmentData::compact_count(row_idx);
                        }
                        summary.validation.segments[segment].resolver_row_len =
                            increment_compact_count(
                                summary.validation.segments[segment].resolver_row_len,
                            );
                    } else {
                        panic!("CompiledProgram: resolver side table exceeded");
                    }
                    resolver_markers_len = increment_compact_count(resolver_markers_len);
                    resolver
                } else {
                    RouteResolver::Intrinsic
                };
                if matches!(node.kind, EffKind::Atom) {
                    let atom = node.atom_data();
                    summary.validation.segments[segment].atom_mask |= 1u128 << local;
                    let row_idx = summary.validation.atom_row_len;
                    if row_idx >= MAX_COMPILED_ATOM_ROWS {
                        panic!("CompiledProgram: atom side table exceeded");
                    }
                    summary.validation.atom_rows[row_idx] = ProgramAtomRow::new(idx, atom);
                    summary.validation.atom_row_len += 1;
                    if summary.validation.segments[segment].atom_row_len == 0 {
                        summary.validation.segments[segment].atom_row_start =
                            ProgramImageSegmentData::compact_count(row_idx);
                    }
                    summary.validation.segments[segment].atom_row_len =
                        increment_compact_count(summary.validation.segments[segment].atom_row_len);
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
                    if resolver.is_dynamic() {
                        summary
                            .program
                            .compiled_program_counts
                            .dynamic_resolver_sites += 1;
                    }
                }
                local += 1;
            }
            segment += 1;
        }

        let src_scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        let mut active_scope_depth = 0u16;
        let mut max_active_scope_depth = 0u16;
        let mut active_route_depth = 0u16;
        let mut max_route_depth = 0u16;
        while scope_idx < src_scope_markers.len() {
            let marker = src_scope_markers[scope_idx];
            if scope_idx >= MAX_COMPILED_SCOPE_MARKERS {
                panic!("CompiledProgram: scope marker table exceeded");
            }
            summary.validation.scope_markers[scope_idx] = marker;
            let marker_segment =
                Self::segment_for_scope_marker_offset(marker.offset, eff_list.len(), marker.event);
            if summary.validation.segments[marker_segment].scope_marker_len == 0 {
                summary.validation.segments[marker_segment].scope_marker_start =
                    ProgramImageSegmentData::compact_count(scope_idx);
            }
            summary.validation.segments[marker_segment].scope_marker_len = increment_compact_count(
                summary.validation.segments[marker_segment].scope_marker_len,
            );
            if matches!(marker.event, ScopeEvent::Enter) {
                scope_count = increment_compact_count(scope_count);
                active_scope_depth = increment_compact_count(active_scope_depth);
                if active_scope_depth > max_active_scope_depth {
                    max_active_scope_depth = active_scope_depth;
                }
                if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Parallel
                ) {
                    summary.program.lowering_facts.parallel_enter_count = increment_compact_count(
                        summary.program.lowering_facts.parallel_enter_count,
                    );
                } else if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Route
                ) {
                    active_route_depth = increment_compact_count(active_route_depth);
                    if active_route_depth > max_route_depth {
                        max_route_depth = active_route_depth;
                    }
                    let ordinal = marker.scope_id.local_ordinal() as usize;
                    let word = ordinal / 64;
                    let bit = ordinal % 64;
                    if word >= route_scope_ordinals.len() {
                        panic!("route scope ordinal overflow");
                    }
                    let mask = 1u64 << bit;
                    if (route_scope_ordinals[word] & mask) == 0 {
                        route_scope_ordinals[word] |= mask;
                        summary.program.lowering_facts.route_scope_count = increment_compact_count(
                            summary.program.lowering_facts.route_scope_count,
                        );
                        summary.program.compiled_program_counts.route_resolvers =
                            summary.program.lowering_facts.route_scope_count as usize;
                    }
                }
            } else {
                if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Route
                ) {
                    active_route_depth = decrement_compact_count(active_route_depth);
                }
                active_scope_depth = decrement_compact_count(active_scope_depth);
            }
            if let Some(controller_role) = marker.controller_role {
                let controller_role = checked_role_index(controller_role);
                if controller_role + 1 > role_count {
                    role_count = controller_role + 1;
                }
            }
            scope_idx += 1;
        }

        let mut role_idx = 0usize;
        while role_idx < role_count {
            let exact_facts = {
                let view = summary.validation.view();
                crate::global::compiled::lowering::seal::exact_role_resident_row_facts(
                    eff_list,
                    view.scope_markers(),
                    role_idx as u8,
                )
            };
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
        let src_scope_markers = eff_list.scope_markers();
        let mut summary = Self {
            validation: ProgramImageValidationData {
                segments: [ProgramImageSegmentData::EMPTY; MAX_SEGMENTS],
                len: eff_list.len(),
                atom_rows: [ProgramAtomRow::EMPTY; MAX_COMPILED_ATOM_ROWS],
                atom_row_len: 0,
                scope_markers: [ScopeMarker::empty(); MAX_COMPILED_SCOPE_MARKERS],
                scope_marker_len: src_scope_markers.len(),
                resolver_rows: [ProgramResolverRow::EMPTY; MAX_COMPILED_RESOLVER_ROWS],
                resolver_row_len: 0,
            },
            program: ProgramImageData {
                compiled_program_counts: CompiledProgramCounts {
                    tap_events: 0,
                    resources: 0,
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
    pub(crate) const fn view(&self) -> CompiledProgramView<'_> {
        self.validation.view()
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
    pub(crate) const fn role_lowering_counts<const ROLE: u8>(&self) -> RoleCompiledCounts {
        self.roles
            .lowering_counts::<ROLE>(self.program.lowering_facts)
    }

    #[inline(always)]
    pub(crate) const fn validate_projection_program(&self) {
        self.program
            .validate_projection_program(self.validation.scope_marker_len);
    }
}
