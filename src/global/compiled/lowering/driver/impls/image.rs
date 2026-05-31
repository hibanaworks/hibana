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
        } else if offset == current_len && offset % MAX_SEGMENT_EFFS == 0 {
            (offset / MAX_SEGMENT_EFFS) - 1
        } else {
            offset / MAX_SEGMENT_EFFS
        }
    }

    const fn segment_for_effect_indexed_marker_offset(offset: usize) -> usize {
        if offset >= MAX_COMPILED_IMAGE_NODES {
            panic!("lowering effect marker offset out of bounds");
        }
        offset / MAX_SEGMENT_EFFS
    }

    const fn scan_into(summary: &mut Self, eff_list: &EffList) {
        let mut lane0 = ProgramStamp::mix_u64(ProgramStamp::SEED0, eff_list.len() as u64);
        let mut lane1 = ProgramStamp::mix_u64(ProgramStamp::SEED1, eff_list.scope_budget() as u64);
        let mut scope_count = 0u16;
        let mut policy_markers_len = 0u16;
        let mut role_count = 0usize;
        let mut route_scope_ordinals = [0u64; ROUTE_SCOPE_ORDINAL_WORDS];
        let mut lease_budget = LeaseGraphBudget::new();
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
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_eff_struct(lane1, node);
                let policy = if let Some((policy, _scope)) = eff_list.policy_with_scope(idx) {
                    let row_idx = summary.validation.policy_row_len;
                    if row_idx < MAX_COMPILED_POLICY_ROWS {
                        summary.validation.policy_rows[row_idx] =
                            ProgramPolicyRow::new(idx, policy);
                        summary.validation.policy_row_len += 1;
                        if summary.validation.segments[segment].policy_row_len == 0 {
                            summary.validation.segments[segment].policy_row_start =
                                ProgramImageSegmentData::compact_count(row_idx);
                        }
                        summary.validation.segments[segment].policy_row_len =
                            ProgramImageSegmentData::compact_count(
                                summary.validation.segments[segment]
                                    .policy_row_len
                                    .saturating_add(1) as usize,
                            );
                    } else {
                        summary.validation.policy_rows_complete = false;
                    }
                    policy_markers_len = policy_markers_len.saturating_add(1);
                    lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                    lane1 = ProgramStamp::mix_policy(lane1, policy);
                    policy
                } else {
                    PolicyMode::Static
                };
                let mut current_control_desc = None;
                if let Some(spec) = eff_list.control_spec_at(idx) {
                    let desc = ControlDesc::from_static(spec).with_sites(
                        crate::eff::EffIndex::from_dense_ordinal(idx),
                        ControlDesc::STATIC_POLICY_SITE,
                    );
                    current_control_desc = Some(desc);
                    let row_idx = summary.validation.control_desc_row_len;
                    if row_idx < MAX_COMPILED_CONTROL_DESC_ROWS {
                        summary.validation.control_desc_rows[row_idx] =
                            ProgramControlDescRow::new(idx, desc);
                        summary.validation.control_desc_row_len += 1;
                        if summary.validation.segments[segment].control_desc_row_len == 0 {
                            summary.validation.segments[segment].control_desc_row_start =
                                ProgramImageSegmentData::compact_count(row_idx);
                        }
                        summary.validation.segments[segment].control_desc_row_len =
                            ProgramImageSegmentData::compact_count(
                                summary.validation.segments[segment]
                                    .control_desc_row_len
                                    .saturating_add(1) as usize,
                            );
                    } else {
                        summary.validation.control_desc_rows_complete = false;
                    }
                    lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                    lane1 = ProgramStamp::mix_control_desc(lane1, desc);
                }
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
                        ProgramImageSegmentData::compact_count(
                            summary.validation.segments[segment]
                                .atom_row_len
                                .saturating_add(1) as usize,
                        );
                    let from = checked_role_index(atom.from);
                    let to = checked_role_index(atom.to);
                    summary.roles.facts[from].local_step_count =
                        summary.roles.facts[from].local_step_count.saturating_add(1);
                    if to != from {
                        summary.roles.facts[to].local_step_count =
                            summary.roles.facts[to].local_step_count.saturating_add(1);
                    }
                    if from + 1 > role_count {
                        role_count = from + 1;
                    }
                    if to + 1 > role_count {
                        role_count = to + 1;
                    }
                    lease_budget = lease_budget.include_atom(current_control_desc, policy);
                    if atom.is_control {
                        if policy.is_dynamic()
                            && let Some(control_spec) = current_control_desc
                            && !control_spec.supports_dynamic_policy()
                        {
                            reject_dynamic_policy_unsupported();
                        }
                        if atom.resource.is_some() {
                            summary.program.compiled_program_counts.resources += 1;
                        }
                    }
                    if policy.is_dynamic() {
                        summary.program.compiled_program_counts.dynamic_policy_sites += 1;
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
            summary.validation.segments[marker_segment].scope_marker_len =
                ProgramImageSegmentData::compact_count(
                    summary.validation.segments[marker_segment]
                        .scope_marker_len
                        .saturating_add(1) as usize,
                );
            if matches!(marker.event, ScopeEvent::Enter) {
                scope_count = scope_count.saturating_add(1);
                active_scope_depth = active_scope_depth.saturating_add(1);
                if active_scope_depth > max_active_scope_depth {
                    max_active_scope_depth = active_scope_depth;
                }
                if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Parallel
                ) {
                    summary.program.lowering_facts.parallel_enter_count = summary
                        .program
                        .lowering_facts
                        .parallel_enter_count
                        .saturating_add(1);
                } else if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Route
                ) {
                    active_route_depth = active_route_depth.saturating_add(1);
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
                        summary.program.lowering_facts.route_scope_count = summary
                            .program
                            .lowering_facts
                            .route_scope_count
                            .saturating_add(1);
                        summary.program.compiled_program_counts.route_controls =
                            summary.program.lowering_facts.route_scope_count as usize;
                        if marker.linger
                            && let Some(controller_role) = marker.controller_role
                        {
                            let mut role_idx = 0usize;
                            while role_idx < summary.roles.facts.len() {
                                if role_idx != controller_role as usize {
                                    summary.roles.facts[role_idx]
                                        .passive_linger_route_scope_count = summary.roles.facts
                                        [role_idx]
                                        .passive_linger_route_scope_count
                                        .saturating_add(1);
                                }
                                role_idx += 1;
                            }
                        }
                    }
                }
            } else {
                if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Route
                ) {
                    active_route_depth = active_route_depth.saturating_sub(1);
                }
                active_scope_depth = active_scope_depth.saturating_sub(1);
            }
            lane0 = ProgramStamp::mix_u64(lane0, scope_idx as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.offset as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.scope_id.raw());
            lane0 = ProgramStamp::mix_u64(lane0, marker.scope_kind as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.event as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.linger as u64);
            lane1 = ProgramStamp::mix_u64(
                lane1,
                match marker.controller_role {
                    Some(role) => role as u64,
                    None => u8::MAX as u64,
                },
            );
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
                let view = summary.validation.view(ProgramSourceLookup::empty());
                crate::global::compiled::lowering::seal::exact_role_phase_facts(
                    eff_list,
                    view.scope_markers(),
                    role_idx as u8,
                )
            };
            summary.roles.facts[role_idx].phase_count = exact_facts.phase_count;
            summary.roles.facts[role_idx].phase_lane_entry_count =
                exact_facts.phase_lane_entry_count;
            summary.roles.facts[role_idx].phase_lane_word_count = exact_facts.phase_lane_word_count;
            summary.roles.facts[role_idx].active_lane_count = exact_facts.active_lane_count;
            summary.roles.facts[role_idx].endpoint_lane_slot_count =
                exact_facts.endpoint_lane_slot_count;
            summary.roles.facts[role_idx].logical_lane_count = exact_facts.logical_lane_count;
            role_idx += 1;
        }

        let src_control_markers = eff_list.control_markers();
        summary.program.compiled_program_counts.controls = src_control_markers.len();
        let mut control_idx = 0usize;
        while control_idx < src_control_markers.len() {
            let marker = src_control_markers[control_idx];
            if control_idx < MAX_COMPILED_CONTROL_MARKERS {
                summary.program.control_markers[control_idx] = marker;
                summary.program.control_marker_len += 1;
                let marker_segment =
                    Self::segment_for_effect_indexed_marker_offset(marker.offset as usize);
                if summary.validation.segments[marker_segment].control_marker_len == 0 {
                    summary.validation.segments[marker_segment].control_marker_start =
                        ProgramImageSegmentData::compact_count(control_idx);
                }
                summary.validation.segments[marker_segment].control_marker_len =
                    ProgramImageSegmentData::compact_count(
                        summary.validation.segments[marker_segment]
                            .control_marker_len
                            .saturating_add(1) as usize,
                    );
            } else {
                summary.program.control_markers_complete = false;
            }
            summary.program.control_scope_mask |= control_scope_mask_bit(marker.scope_kind);
            lane0 = ProgramStamp::mix_u64(lane0, control_idx as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.offset as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.scope_kind as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.tap_id as u64);
            if marker.tap_id != 0 {
                summary.program.compiled_program_counts.tap_events += 1;
            }
            control_idx += 1;
        }
        lease_budget.validate();

        summary.program.lowering_facts.scope_count = scope_count;
        summary.program.lowering_facts.max_active_scope_depth = max_active_scope_depth;
        summary.program.lowering_facts.max_route_stack_depth = if max_route_depth == 0 {
            0
        } else {
            max_route_depth.saturating_add(1)
        };
        summary.program.lease_budget = lease_budget;
        summary.roles.count = if role_count > u8::MAX as usize {
            u8::MAX
        } else {
            role_count as u8
        };
        summary.program.stamp = ProgramStamp { lane0, lane1 };
    }

    const fn scan_impl(eff_list: &EffList, source_lookup: ProgramSourceLookup) -> Self {
        let src_scope_markers = eff_list.scope_markers();
        let mut summary = Self {
            validation: ProgramImageValidationData {
                segments: [ProgramImageSegmentData::EMPTY; MAX_SEGMENTS],
                len: eff_list.len(),
                atom_rows: [ProgramAtomRow::EMPTY; MAX_COMPILED_ATOM_ROWS],
                atom_row_len: 0,
                scope_markers: [ScopeMarker::empty(); MAX_COMPILED_SCOPE_MARKERS],
                scope_marker_len: src_scope_markers.len(),
                policy_rows: [ProgramPolicyRow::EMPTY; MAX_COMPILED_POLICY_ROWS],
                policy_row_len: 0,
                policy_rows_complete: true,
                control_desc_rows: [ProgramControlDescRow::EMPTY; MAX_COMPILED_CONTROL_DESC_ROWS],
                control_desc_row_len: 0,
                control_desc_rows_complete: true,
            },
            program: ProgramImageData {
                control_markers: [ControlMarker::empty(); MAX_COMPILED_CONTROL_MARKERS],
                control_marker_len: 0,
                control_markers_complete: true,
                lease_budget: LeaseGraphBudget::new(),
                compiled_program_counts: CompiledProgramCounts {
                    tap_events: 0,
                    resources: 0,
                    controls: 0,
                    dynamic_policy_sites: 0,
                    route_controls: 0,
                },
                lowering_facts: ProgramLoweringFacts::EMPTY,
                control_scope_mask: 0,
                stamp: ProgramStamp {
                    lane0: ProgramStamp::SEED0,
                    lane1: ProgramStamp::SEED1,
                },
            },
            roles: ProgramRoleImageData {
                facts: [RoleCompiledFacts::EMPTY; MAX_TRACKED_ROLE_FACTS],
                count: 0,
            },
            source_lookup,
        };
        Self::scan_into(&mut summary, eff_list);
        if summary.validation.policy_rows_complete && summary.validation.control_desc_rows_complete
        {
            summary.source_lookup = ProgramSourceLookup::empty();
        }
        summary
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn scan_const(eff_list: &EffList) -> Self {
        Self::scan_const_with_lookup(eff_list, ProgramSourceLookup::empty())
    }

    #[inline(always)]
    pub(crate) const fn scan_const_with_lookup(
        eff_list: &EffList,
        source_lookup: ProgramSourceLookup,
    ) -> Self {
        Self::scan_impl(eff_list, source_lookup)
    }

    #[inline(always)]
    pub(crate) const fn view(&self) -> CompiledProgramView<'_> {
        self.validation.view(self.source_lookup)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn segment_summary(&self, segment: usize) -> SegmentSummary {
        if segment >= crate::eff::meta::MAX_SEGMENTS {
            panic!("lowering segment summary out of bounds");
        }
        self.validation.segments[segment].summary
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.program.stamp
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
    pub(crate) const fn compiled_program_control_scope_mask(&self) -> u8 {
        self.program.control_scope_mask
    }

    #[inline(always)]
    pub(crate) const fn validate_projection_program(&self) {
        self.program
            .validate_projection_program(self.validation.scope_marker_len);
    }
}
