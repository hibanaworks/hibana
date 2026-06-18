use super::{
    CompiledProgramView, EffAtom, MAX_COMPILED_IMAGE_NODES, MAX_COMPILED_PROGRAM_SCOPES,
    MAX_SEGMENT_EFFS, ProgramImageData, ProgramImageValidationData, ProgramLoweringFacts,
    ProgramRoleImageData, RoleCompiledCounts, RouteResolver, ScopeMarker,
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
    pub(crate) const fn route_frontier_summary(
        &self,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<crate::global::const_dsl::RouteFrontierSummary> {
        if scope.is_none() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.route_frontiers.len() {
            let summary = self.route_frontiers[idx];
            if summary.scope().same(scope) {
                return Some(summary);
            }
            idx += 1;
        }
        None
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
    pub(crate) const fn resolver_for_scope(
        &self,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<RouteResolver> {
        if scope.is_none() {
            return None;
        }
        let mut row_idx = 0usize;
        while row_idx < self.route_resolver_sites.len() {
            let site = self.route_resolver_sites[row_idx];
            if site.scope().same(scope) {
                return Some(RouteResolver::Dynamic {
                    resolver_id: site.resolver_id(),
                    scope,
                });
            }
            row_idx += 1;
        }
        None
    }
}

impl ProgramImageValidationData {
    #[inline(always)]
    const fn view<'a>(&'a self) -> CompiledProgramView<'a> {
        /* SAFETY: `ProgramImageValidationData` owns these image validation arrays;
        lowering writes each initialized prefix before exposing the program view,
        every prefix len is bounded by its backing array capacity, and the view
        lifetime is tied to `&self` so no mutable alias exists while the slices are
        borrowed. */
        unsafe {
            CompiledProgramView {
                segments: &self.segments,
                len: self.len,
                atom_rows: core::slice::from_raw_parts(self.atom_rows.as_ptr(), self.atom_row_len),
                scope_markers: core::slice::from_raw_parts(
                    self.scope_markers.as_ptr(),
                    self.scope_marker_len,
                ),
                route_frontiers: core::slice::from_raw_parts(
                    self.route_frontiers.as_ptr(),
                    self.route_frontier_len,
                ),
                route_resolver_sites: core::slice::from_raw_parts(
                    self.route_resolver_sites.as_ptr(),
                    self.route_resolver_site_len,
                ),
            }
        }
    }
}

impl ProgramImageData {
    #[inline(always)]
    const fn validate_projection_program(&self, scope_marker_len: usize) {
        if self.compiled_program_counts.dynamic_resolver_sites > MAX_COMPILED_IMAGE_NODES {
            panic!("CompiledProgram: MAX_DYNAMIC_RESOLVER_SITES exceeded");
        }
        if self.compiled_program_counts.route_resolvers > MAX_COMPILED_IMAGE_NODES {
            panic!("CompiledProgram: MAX_ROUTE_RESOLVERS exceeded");
        }
        if scope_marker_len > MAX_COMPILED_PROGRAM_SCOPES {
            panic!("CompiledProgram: MAX_SCOPES exceeded");
        }
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
