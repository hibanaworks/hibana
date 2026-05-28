#[cfg(test)]
use super::ControlMarker;
use super::{
    CompiledProgramView, ControlDesc, ControlOp, EffAtom, EffStruct, MAX_COMPILED_IMAGE_NODES,
    MAX_COMPILED_PROGRAM_CONTROLS, MAX_COMPILED_PROGRAM_RESOURCES, MAX_COMPILED_PROGRAM_SCOPES,
    MAX_COMPILED_PROGRAM_TAP_EVENTS, MAX_SEGMENT_EFFS, PolicyMode, ProgramImageData,
    ProgramImageValidationData, ProgramLoweringFacts, ProgramRoleImageData, ProgramSourceLookup,
    RoleCompiledCounts, ScopeEvent, ScopeId, ScopeMarker, reject_dynamic_policy_unsupported,
};
impl<'a> CompiledProgramView<'a> {
    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(&self) -> &'a [ScopeMarker] {
        self.scope_markers
    }

    #[inline(always)]
    const fn segment_slot(offset: usize) -> (usize, usize) {
        if offset >= MAX_COMPILED_IMAGE_NODES {
            panic!("lowering offset out of bounds");
        }
        (offset / MAX_SEGMENT_EFFS, offset % MAX_SEGMENT_EFFS)
    }

    #[inline(always)]
    const fn offset_is_atom(&self, offset: usize) -> bool {
        if offset >= self.len {
            return false;
        }
        let (segment, local) = Self::segment_slot(offset);
        let segment = self.segments[segment];
        (segment.atom_mask & (1u128 << local)) != 0
    }

    #[inline(always)]
    pub(crate) const fn node_at(&self, offset: usize) -> EffStruct {
        if offset >= self.len {
            panic!("lowering node out of bounds");
        }
        let Some(atom) = self.atom_at(offset) else {
            return EffStruct::pure();
        };
        EffStruct::atom(atom)
    }

    #[inline(always)]
    pub(crate) const fn atom_at(&self, offset: usize) -> Option<EffAtom> {
        if !self.offset_is_atom(offset) {
            return None;
        }
        let (segment, _) = Self::segment_slot(offset);
        let segment = self.segments[segment];
        let mut row_idx = segment.atom_row_start as usize;
        let end = row_idx + segment.atom_row_len as usize;
        while row_idx < end {
            let row = self.atom_rows[row_idx];
            if row.offset as usize == offset {
                return Some(row.atom);
            }
            row_idx += 1;
        }
        panic!("compiled atom mask has no resident atom row");
    }

    #[inline(always)]
    pub(crate) fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        if offset < self.len {
            let (segment, _) = Self::segment_slot(offset);
            let segment = self.segments[segment];
            let mut row_idx = segment.policy_row_start as usize;
            let end = row_idx + segment.policy_row_len as usize;
            while row_idx < end {
                let row = self.policy_rows[row_idx];
                if row.offset as usize == offset {
                    return Some(row.policy);
                }
                row_idx += 1;
            }
            if !self.policy_rows_complete {
                return self.source_lookup.policy_at(offset);
            }
        }
        None
    }

    #[inline(always)]
    pub(crate) fn control_desc_at(&self, offset: usize) -> Option<ControlDesc> {
        if offset < self.len {
            let (segment, _) = Self::segment_slot(offset);
            let segment = self.segments[segment];
            let mut row_idx = segment.control_desc_row_start as usize;
            let end = row_idx + segment.control_desc_row_len as usize;
            while row_idx < end {
                let row = self.control_desc_rows[row_idx];
                if row.offset as usize == offset {
                    return row.desc;
                }
                row_idx += 1;
            }
            if !self.control_desc_rows_complete {
                return self.source_lookup.control_desc_at(offset);
            }
        }
        None
    }

    pub(crate) fn first_route_head_dynamic_policy_in_range(
        &self,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8, ControlOp)> {
        if route_enter_marker_idx >= self.scope_markers.len() {
            return None;
        }
        let route_marker = self.scope_markers[route_enter_marker_idx];
        if !matches!(route_marker.event, ScopeEvent::Enter)
            || !matches!(
                route_marker.scope_kind,
                crate::global::const_dsl::ScopeKind::Route
            )
            || route_marker.scope_id.canonical().raw() != route_scope.canonical().raw()
        {
            return None;
        }
        let scope_start = route_marker.offset;
        if scope_start >= MAX_COMPILED_IMAGE_NODES || scope_start >= scope_end {
            return None;
        }

        let mut marker_idx = route_enter_marker_idx + 1;
        let mut active_scope_depth = 1usize;
        let mut idx = scope_start;
        while idx < scope_end && idx < self.len {
            let mut scan_marker_idx = marker_idx;
            let mut depth_after_exits = active_scope_depth;
            while scan_marker_idx < self.scope_markers.len() {
                let marker = self.scope_markers[scan_marker_idx];
                if marker.offset != idx {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Exit) {
                    depth_after_exits = depth_after_exits.saturating_sub(1);
                }
                scan_marker_idx += 1;
            }

            let mut enter_count = 0usize;
            let mut nested_non_policy_enter = false;
            let mut next_marker_idx = marker_idx;
            while next_marker_idx < self.scope_markers.len() {
                let marker = self.scope_markers[next_marker_idx];
                if marker.offset != idx {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Enter) {
                    if depth_after_exits == 1
                        && !matches!(
                            marker.scope_kind,
                            crate::global::const_dsl::ScopeKind::Generic
                        )
                    {
                        nested_non_policy_enter = true;
                    }
                    enter_count += 1;
                }
                next_marker_idx += 1;
            }

            if depth_after_exits == 1 && !nested_non_policy_enter {
                if let Some(policy) = self.policy_at(idx) {
                    if policy.dynamic_policy_id().is_some() {
                        let control = match self.control_desc_at(idx) {
                            Some(control) => control,
                            None => panic!("dynamic route policy requires controller control op"),
                        };
                        if !control.supports_dynamic_policy() {
                            reject_dynamic_policy_unsupported();
                        }
                        return Some((policy, idx, control.resource_tag(), control.op()));
                    }
                }
            }
            active_scope_depth = depth_after_exits.saturating_add(enter_count);
            marker_idx = next_marker_idx;
            idx += 1;
        }
        None
    }
}

impl ProgramImageValidationData {
    #[inline(always)]
    const fn view<'a>(&'a self, source_lookup: ProgramSourceLookup) -> CompiledProgramView<'a> {
        CompiledProgramView {
            segments: &self.segments,
            len: self.len,
            atom_rows: /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe {
                core::slice::from_raw_parts(self.atom_rows.as_ptr(), self.atom_row_len)
            },
            scope_markers: /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe {
                core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len)
            },
            policy_rows: /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe {
                core::slice::from_raw_parts(self.policy_rows.as_ptr(), self.policy_row_len)
            },
            policy_rows_complete: self.policy_rows_complete,
            control_desc_rows: /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe {
                core::slice::from_raw_parts(
                    self.control_desc_rows.as_ptr(),
                    self.control_desc_row_len,
                )
            },
            control_desc_rows_complete: self.control_desc_rows_complete,
            source_lookup,
        }
    }
}

impl ProgramImageData {
    #[cfg(test)]
    #[inline(always)]
    pub(super) const fn control_markers(&self) -> &[ControlMarker] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe {
            core::slice::from_raw_parts(self.control_markers.as_ptr(), self.control_marker_len)
        }
    }

    #[inline(always)]
    const fn validate_projection_program(&self, scope_marker_len: usize) {
        if self.compiled_program_counts.resources > MAX_COMPILED_PROGRAM_RESOURCES {
            panic!("CompiledProgram: MAX_RESOURCES exceeded");
        }
        if self.compiled_program_counts.tap_events > MAX_COMPILED_PROGRAM_TAP_EVENTS {
            panic!("CompiledProgram: MAX_TAP_EVENTS exceeded");
        }
        if self.compiled_program_counts.dynamic_policy_sites > MAX_COMPILED_IMAGE_NODES {
            panic!("CompiledProgram: MAX_DYNAMIC_POLICY_SITES exceeded");
        }
        if self.compiled_program_counts.route_controls > MAX_COMPILED_IMAGE_NODES {
            panic!("CompiledProgram: MAX_ROUTE_CONTROLS exceeded");
        }
        if self.compiled_program_counts.controls > MAX_COMPILED_PROGRAM_CONTROLS {
            panic!("CompiledProgram: MAX_CONTROLS exceeded");
        }
        if scope_marker_len > MAX_COMPILED_PROGRAM_SCOPES {
            panic!("CompiledProgram: MAX_SCOPES exceeded");
        }
        self.lease_budget.validate();
    }
}

impl ProgramRoleImageData {
    #[inline(always)]
    const fn lowering_counts<const ROLE: u8>(
        &self,
        program: ProgramLoweringFacts,
    ) -> RoleCompiledCounts {
        let role = self.facts[ROLE as usize];
        RoleCompiledCounts {
            scope_count: program.scope_count as usize,
            max_active_scope_depth: program.max_active_scope_depth as usize,
            max_route_stack_depth: program.max_route_stack_depth as usize,
            eff_count: program.eff_count as usize,
            local_step_count: role.local_step_count as usize,
            phase_count: role.phase_count as usize,
            phase_lane_entry_count: role.phase_lane_entry_count as usize,
            phase_lane_word_count: role.phase_lane_word_count as usize,
            parallel_enter_count: program.parallel_enter_count as usize,
            route_scope_count: program.route_scope_count as usize,
            passive_linger_route_scope_count: role.passive_linger_route_scope_count as usize,
            active_lane_count: role.active_lane_count as usize,
            endpoint_lane_slot_count: role.endpoint_lane_slot_count as usize,
            logical_lane_count: role.logical_lane_count as usize,
        }
    }
}

mod image;
