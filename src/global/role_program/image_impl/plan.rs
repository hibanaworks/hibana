use super::super::{
    ColumnRange, LANE_DOMAIN_SIZE, MAX_LOCAL_STEP_LANES, MAX_ROUTE_SCOPE_LANE_ROWS,
    PackedLaneRange, ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_DEPENDENCY_STRIDE,
    ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_LANE_RANGE_STRIDE, ROLE_IMAGE_LANE_STRIDE,
    ROLE_IMAGE_ROLL_SCOPE_STRIDE, ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_ROUTE_SCOPE_STRIDE, ROLE_IMAGE_U16_STRIDE,
    RoleImageColumns, RoleImagePlan, RoleLaneScratch, RuntimeRoleFacts, lane_byte_count,
};
use crate::global::const_dsl::{EffList, ScopeEvent, ScopeId, ScopeKind};
use crate::global::typestate::{
    LocalConflict, LocalDependency, PackedEventConflict, PackedLocalDependency,
};

pub(super) struct RoleImageColumnCounts {
    pub(super) dependency_rows: usize,
    pub(super) conflict_rows: usize,
    pub(super) resident_boundaries: usize,
    pub(super) lane_bits: usize,
    pub(super) route_arm_lane_steps: usize,
    pub(super) route_commit_rows: usize,
    pub(super) roll_scopes: usize,
}

struct RouteImagePlanRouteFacts {
    lane_bits: usize,
    route_arm_lane_steps: usize,
    route_commit_rows: usize,
}

impl RoleImagePlan {
    #[inline(always)]
    pub(crate) const fn blob_len(&self) -> usize {
        self.columns.blob_len()
    }

    pub(crate) const fn from_program(
        eff_list: &EffList,
        facts: RuntimeRoleFacts,
        role: u8,
    ) -> Self {
        let counts = RoleImageColumnCounts::from_program(
            eff_list,
            facts.footprint().logical_lane_count,
            role,
        );
        Self {
            columns: counts.columns(facts),
        }
    }
}

impl RoleImageColumnCounts {
    const fn column_at(offset: usize, len: usize, stride: usize) -> (ColumnRange, usize) {
        let column = ColumnRange::new(offset, len, stride);
        (column, column.end_offset(stride))
    }

    pub(super) const fn from_scratch(scratch: &RoleLaneScratch) -> Self {
        Self {
            dependency_rows: scratch.dependency_row_len(),
            conflict_rows: scratch.conflict_row_len(),
            resident_boundaries: scratch.resident_boundary_count(),
            lane_bits: scratch.lane_bit_row_len(),
            route_arm_lane_steps: scratch.route_arm_lane_step_row_len as usize,
            route_commit_rows: scratch.route_commit_row_len(),
            roll_scopes: scratch.roll_scope_row_len(),
        }
    }

    pub(super) const fn columns(self, facts: RuntimeRoleFacts) -> RoleImageColumns {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len * 2;

        let (events, offset) = Self::column_at(0, local_len, ROLE_IMAGE_EVENT_STRIDE);
        let (lanes, offset) = Self::column_at(offset, local_len, ROLE_IMAGE_LANE_STRIDE);
        let (dependencies, offset) =
            Self::column_at(offset, self.dependency_rows, ROLE_IMAGE_DEPENDENCY_STRIDE);
        let (conflicts, offset) =
            Self::column_at(offset, self.conflict_rows, ROLE_IMAGE_CONFLICT_STRIDE);
        let (route_scopes, offset) =
            Self::column_at(offset, route_scope_len, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
        let (route_scope_conflicts, offset) =
            Self::column_at(offset, route_scope_len, ROLE_IMAGE_CONFLICT_STRIDE);
        let (route_arms, offset) =
            Self::column_at(offset, route_arm_len, ROLE_IMAGE_ROUTE_ARM_STRIDE);
        let (resident_boundaries, offset) =
            Self::column_at(offset, self.resident_boundaries, ROLE_IMAGE_U16_STRIDE);
        let (lane_bits, offset) = Self::column_at(offset, self.lane_bits, ROLE_IMAGE_LANE_STRIDE);
        let (route_arm_lane_rows, offset) =
            Self::column_at(offset, route_arm_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        let (route_offer_lane_rows, offset) =
            Self::column_at(offset, route_scope_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        let (route_arm_lane_step_rows, offset) = Self::column_at(
            offset,
            self.route_arm_lane_steps,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        );
        let (route_commit_ranges, offset) =
            Self::column_at(offset, route_arm_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        let (route_commit_rows, _) =
            Self::column_at(offset, self.route_commit_rows, ROLE_IMAGE_CONFLICT_STRIDE);
        let (roll_scopes, _) = Self::column_at(
            route_commit_rows.end_offset(ROLE_IMAGE_CONFLICT_STRIDE),
            self.roll_scopes,
            ROLE_IMAGE_ROLL_SCOPE_STRIDE,
        );
        RoleImageColumns {
            events,
            lanes,
            dependencies,
            conflicts,
            route_scopes,
            route_scope_conflicts,
            route_arms,
            resident_boundaries,
            lane_bits,
            route_arm_lane_rows,
            route_offer_lane_rows,
            route_arm_lane_step_rows,
            route_commit_ranges,
            route_commit_rows,
            roll_scopes,
        }
    }
}

impl RoleImageColumnCounts {
    const fn from_program(eff_list: &EffList, logical_lane_count: usize, role: u8) -> Self {
        if logical_lane_count == 0 || logical_lane_count > LANE_DOMAIN_SIZE {
            panic!("role logical lane domain invalid");
        }
        let mut eff_indices = [0u16; MAX_LOCAL_STEP_LANES];
        let mut lanes = [0u8; MAX_LOCAL_STEP_LANES];
        let markers = eff_list.scope_markers();
        let has_route = RoleLaneScratch::scope_markers_contain_kind(markers, ScopeKind::Route);
        let mut conflict_rows = 0usize;
        let local_step_count = Self::collect_local_steps(
            eff_list,
            role,
            logical_lane_count,
            has_route,
            &mut eff_indices,
            &mut lanes,
            &mut conflict_rows,
        );
        let dependency_rows =
            Self::dependency_row_count(eff_list, role, local_step_count, &eff_indices, &lanes);
        let (resident_rows, resident_lane_bits) =
            Self::resident_row_facts(eff_list, role, local_step_count, &lanes);
        let route_facts = Self::route_facts(eff_list, role, &lanes);
        let roll_scopes = Self::roll_scope_count(eff_list, role);
        let active_lane_bits =
            Self::lane_byte_len_for_local_row(&lanes, PackedLaneRange::new(0, local_step_count));
        let resident_boundaries = if resident_rows == 0 {
            0
        } else {
            resident_rows + 1
        };
        Self {
            dependency_rows,
            conflict_rows,
            resident_boundaries,
            lane_bits: active_lane_bits + resident_lane_bits + route_facts.lane_bits,
            route_arm_lane_steps: route_facts.route_arm_lane_steps,
            route_commit_rows: route_facts.route_commit_rows,
            roll_scopes,
        }
    }

    const fn collect_local_steps(
        eff_list: &EffList,
        role: u8,
        logical_lane_count: usize,
        has_route: bool,
        eff_indices: &mut [u16; MAX_LOCAL_STEP_LANES],
        lanes: &mut [u8; MAX_LOCAL_STEP_LANES],
        conflict_rows: &mut usize,
    ) -> usize {
        let markers = eff_list.scope_markers();
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < eff_list.len() {
            let node = eff_list.node_at(idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    if atom.lane as usize >= logical_lane_count {
                        panic!("local event lane outside role logical domain");
                    }
                    if step >= MAX_LOCAL_STEP_LANES || idx > u16::MAX as usize {
                        panic!("role image plan local step overflow");
                    }
                    eff_indices[step] = idx as u16;
                    lanes[step] = atom.lane;
                    if has_route && !RoleLaneScratch::route_conflict_for_eff(markers, idx).is_none()
                    {
                        *conflict_rows += 1;
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        step
    }

    const fn local_row_contains_lane(
        lanes: &[u8; MAX_LOCAL_STEP_LANES],
        row: PackedLaneRange,
        lane: u8,
    ) -> bool {
        let mut pos = row.start();
        let end = row.end();
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if lanes[pos] == lane {
                return true;
            }
            pos += 1;
        }
        false
    }

    const fn dependency_row_count(
        eff_list: &EffList,
        role: u8,
        local_step_count: usize,
        eff_indices: &[u16; MAX_LOCAL_STEP_LANES],
        lanes: &[u8; MAX_LOCAL_STEP_LANES],
    ) -> usize {
        let markers = eff_list.scope_markers();
        let has_route = RoleLaneScratch::scope_markers_contain_kind(markers, ScopeKind::Route);
        let mut dependencies = [PackedLocalDependency::none(); MAX_LOCAL_STEP_LANES];
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            {
                let exit_eff = RoleLaneScratch::parallel_exit_for_enter(markers, marker_idx);
                let row = RoleLaneScratch::local_step_range_for_eff_range(
                    eff_list,
                    marker.offset(),
                    exit_eff,
                    role,
                );
                let start = row.start();
                let end = row.end();
                if start < end {
                    let parent_parallel_end =
                        RoleLaneScratch::nearest_parent_parallel_end(markers, marker_idx, exit_eff);
                    let scope = marker.scope_id;
                    let conflict = if has_route {
                        RoleLaneScratch::dependency_conflict_for_scope(
                            markers,
                            eff_list.len(),
                            scope,
                        )
                    } else {
                        LocalConflict::Unconditional
                    };
                    let dependency = PackedLocalDependency::from_dependency(
                        LocalDependency::with_conflict_range(scope, conflict, start, end),
                    );
                    let mut step = end;
                    while step < local_step_count && step < MAX_LOCAL_STEP_LANES {
                        let current_eff = eff_indices[step] as usize;
                        let current_lane = lanes[step];
                        let dependency_applies =
                            Self::local_row_contains_lane(lanes, row, current_lane)
                                || current_eff >= parent_parallel_end;
                        let current_dependency = dependencies[step];
                        let replaces_current = current_dependency.is_none()
                            || end >= current_dependency.end() as usize;
                        if dependency_applies && replaces_current {
                            dependencies[step] = dependency;
                        }
                        step += 1;
                    }
                }
            }
            marker_idx += 1;
        }
        let mut count = 0usize;
        let mut idx = 0usize;
        while idx < local_step_count && idx < MAX_LOCAL_STEP_LANES {
            if !dependencies[idx].is_none() {
                count += 1;
            }
            idx += 1;
        }
        count
    }

    const fn lane_byte_len_for_local_row(
        lanes: &[u8; MAX_LOCAL_STEP_LANES],
        row: PackedLaneRange,
    ) -> usize {
        if row.is_absent_or_zero_len() {
            return 0;
        }
        let mut max_lane_plus_one = 0usize;
        let mut pos = row.start();
        let end = row.end();
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            let lane_plus_one = lanes[pos] as usize + 1;
            if lane_plus_one > max_lane_plus_one {
                max_lane_plus_one = lane_plus_one;
            }
            pos += 1;
        }
        lane_byte_count(max_lane_plus_one)
    }

    const fn resident_row_facts(
        eff_list: &EffList,
        role: u8,
        local_step_count: usize,
        lanes: &[u8; MAX_LOCAL_STEP_LANES],
    ) -> (usize, usize) {
        let markers = eff_list.scope_markers();
        let mut row_count = 0usize;
        let mut lane_bits = 0usize;
        let mut current_eff = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            {
                let exit_eff = RoleLaneScratch::parallel_exit_for_enter(markers, marker_idx);
                let row = RoleLaneScratch::local_step_range_for_eff_range(
                    eff_list,
                    current_eff,
                    marker.offset(),
                    role,
                );
                if !row.is_absent_or_zero_len() {
                    row_count += 1;
                    lane_bits += Self::lane_byte_len_for_local_row(lanes, row);
                }
                let parallel_start = if marker.offset() > current_eff {
                    marker.offset()
                } else {
                    current_eff
                };
                let row = RoleLaneScratch::local_step_range_for_eff_range(
                    eff_list,
                    parallel_start,
                    exit_eff,
                    role,
                );
                if !row.is_absent_or_zero_len() {
                    row_count += 1;
                    lane_bits += Self::lane_byte_len_for_local_row(lanes, row);
                }
                current_eff = if exit_eff > current_eff {
                    exit_eff
                } else {
                    current_eff
                };
            }
            marker_idx += 1;
        }
        let row = RoleLaneScratch::local_step_range_for_eff_range(
            eff_list,
            current_eff,
            eff_list.len(),
            role,
        );
        if !row.is_absent_or_zero_len() {
            row_count += 1;
            lane_bits += Self::lane_byte_len_for_local_row(lanes, row);
        }
        if row_count == 0 && local_step_count > 0 {
            let row =
                RoleLaneScratch::local_step_range_for_eff_range(eff_list, 0, eff_list.len(), role);
            if !row.is_absent_or_zero_len() {
                row_count += 1;
                lane_bits += Self::lane_byte_len_for_local_row(lanes, row);
            }
        }
        (row_count, lane_bits)
    }

    const fn route_scope_conflict_for_commit(
        markers: &[crate::global::const_dsl::ScopeMarker],
        view_len: usize,
        scope: ScopeId,
    ) -> PackedEventConflict {
        let conflict = PackedEventConflict::from_conflict(
            RoleLaneScratch::dependency_conflict_for_scope(markers, view_len, scope),
        );
        match conflict.to_conflict() {
            Some(LocalConflict::RouteArm { scope: parent, .. }) if parent.same(scope) => {
                PackedEventConflict::none()
            }
            Some(_) | None => conflict,
        }
    }

    const fn route_commit_row_count(
        markers: &[crate::global::const_dsl::ScopeMarker],
        view_len: usize,
        scope: ScopeId,
        arm: u8,
    ) -> usize {
        let mut len = 0usize;
        let mut conflict = PackedEventConflict::route_arm(scope, arm);
        while len < MAX_ROUTE_SCOPE_LANE_ROWS + 1 {
            let Some(LocalConflict::RouteArm { scope, .. }) = conflict.to_conflict() else {
                return len;
            };
            if scope.is_none() {
                panic!("route commit scope missing");
            }
            len += 1;
            conflict = Self::route_scope_conflict_for_commit(markers, view_len, scope);
        }
        if len == MAX_ROUTE_SCOPE_LANE_ROWS + 1
            && matches!(conflict.to_conflict(), Some(LocalConflict::RouteArm { .. }))
        {
            panic!("route commit rows overflow");
        }
        len
    }

    const fn route_arm_lane_step_count(
        lanes: &[u8; MAX_LOCAL_STEP_LANES],
        local_row: PackedLaneRange,
    ) -> usize {
        let mut count = 0usize;
        let mut pos = local_row.start();
        let end = local_row.end();
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            let lane = lanes[pos];
            let mut seen = false;
            let mut scan = local_row.start();
            while scan < pos && scan < MAX_LOCAL_STEP_LANES {
                if lanes[scan] == lane {
                    seen = true;
                    break;
                }
                scan += 1;
            }
            if !seen {
                count += 1;
            }
            pos += 1;
        }
        count
    }

    const fn route_facts(
        eff_list: &EffList,
        role: u8,
        lanes: &[u8; MAX_LOCAL_STEP_LANES],
    ) -> RouteImagePlanRouteFacts {
        let markers = eff_list.scope_markers();
        if !RoleLaneScratch::scope_markers_contain_kind(markers, ScopeKind::Route) {
            return RouteImagePlanRouteFacts {
                lane_bits: 0,
                route_arm_lane_steps: 0,
                route_commit_rows: 0,
            };
        }
        let mut facts = RouteImagePlanRouteFacts {
            lane_bits: 0,
            route_arm_lane_steps: 0,
            route_commit_rows: 0,
        };
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if RoleLaneScratch::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            {
                let scope = marker.scope_id;
                let Some(ranges) = RoleLaneScratch::route_arm_ranges(markers, scope) else {
                    panic!("route scope missing binary arm ranges");
                };
                let mut arm = 0usize;
                let mut arm_lane_bits = [0usize; 2];
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    let row =
                        RoleLaneScratch::local_step_range_for_eff_range(eff_list, start, end, role);
                    let lane_bits = Self::lane_byte_len_for_local_row(lanes, row);
                    arm_lane_bits[arm] = lane_bits;
                    facts.lane_bits += lane_bits;
                    facts.route_arm_lane_steps += Self::route_arm_lane_step_count(lanes, row);
                    facts.route_commit_rows +=
                        Self::route_commit_row_count(markers, eff_list.len(), scope, arm as u8);
                    arm += 1;
                }
                let offer_lane_bits = if arm_lane_bits[0] > arm_lane_bits[1] {
                    arm_lane_bits[0]
                } else {
                    arm_lane_bits[1]
                };
                facts.lane_bits += offer_lane_bits;
            }
            marker_idx += 1;
        }
        facts
    }

    const fn roll_scope_count(eff_list: &EffList, role: u8) -> usize {
        let markers = eff_list.scope_markers();
        if !RoleLaneScratch::scope_markers_contain_kind(markers, ScopeKind::Roll) {
            return 0;
        }
        let mut count = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if RoleLaneScratch::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            {
                let end_eff =
                    RoleLaneScratch::scope_segment_end(markers, marker_idx, eff_list.len());
                let row = RoleLaneScratch::local_step_range_for_eff_range(
                    eff_list,
                    marker.offset(),
                    end_eff,
                    role,
                );
                if !row.is_absent_or_zero_len() {
                    count += 1;
                }
            }
            marker_idx += 1;
        }
        count
    }
}
