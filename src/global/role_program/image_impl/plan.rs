use super::super::{
    ColumnRange, LANE_DOMAIN_SIZE, ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_DEPENDENCY_STRIDE,
    ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_LANE_RANGE_STRIDE, ROLE_IMAGE_LANE_STRIDE,
    ROLE_IMAGE_ROLL_SCOPE_STRIDE, ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_ROUTE_SCOPE_STRIDE, ROLE_IMAGE_U16_STRIDE,
    RoleImageColumns, RoleImagePlan, RuntimeRoleFacts, lane_byte_count,
};
use super::projection;
use crate::global::const_dsl::{EffList, ScopeKind};

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

    pub(crate) const fn from_program<const E: usize>(
        eff_list: &EffList<E>,
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
    pub(super) const fn from_program<const E: usize>(
        eff_list: &EffList<E>,
        logical_lane_count: usize,
        role: u8,
    ) -> Self {
        if logical_lane_count == 0 || logical_lane_count > LANE_DOMAIN_SIZE {
            panic!("role logical lane domain invalid");
        }
        let markers = eff_list.scope_markers();
        let has_route = projection::scope_markers_contain_kind(markers, ScopeKind::Route);
        let mut local_step_count = 0usize;
        let mut dependency_rows = 0usize;
        let mut conflict_rows = 0usize;
        let mut max_lane_plus_one = 0usize;
        let mut dependencies = projection::DependencyCursor::new(eff_list, role);
        let mut idx = 0usize;
        while idx < eff_list.len() {
            let atom = eff_list.atom_at(idx);
            if atom.from == role || atom.to == role {
                let lane_plus_one = atom.lane as usize + 1;
                if lane_plus_one > logical_lane_count {
                    panic!("local event lane outside role logical domain");
                }
                if lane_plus_one > max_lane_plus_one {
                    max_lane_plus_one = lane_plus_one;
                }
                if !dependencies
                    .next(idx, atom.lane, local_step_count)
                    .is_none()
                {
                    dependency_rows += 1;
                }
                if has_route && !projection::route_conflict_for_eff(markers, idx).is_none() {
                    conflict_rows += 1;
                }
                local_step_count += 1;
            }
            idx += 1;
        }
        let resident_rows = Self::resident_row_count(eff_list, role);
        let route_facts = Self::route_facts(eff_list, role);
        let roll_scopes = Self::roll_scope_count(eff_list, role);
        let active_lane_bits = lane_byte_count(max_lane_plus_one);
        let resident_boundaries = if resident_rows == 0 {
            0
        } else {
            resident_rows + 1
        };
        Self {
            dependency_rows,
            conflict_rows,
            resident_boundaries,
            lane_bits: active_lane_bits + route_facts.lane_bits,
            route_arm_lane_steps: route_facts.route_arm_lane_steps,
            route_commit_rows: route_facts.route_commit_rows,
            roll_scopes,
        }
    }

    const fn resident_row_count<const E: usize>(eff_list: &EffList<E>, role: u8) -> usize {
        let mut rows = projection::ResidentRowCursor::new(eff_list, role);
        let mut row_count = 0usize;
        while rows.next().is_some() {
            row_count += 1;
        }
        row_count
    }

    const fn route_facts<const E: usize>(
        eff_list: &EffList<E>,
        role: u8,
    ) -> RouteImagePlanRouteFacts {
        let markers = eff_list.scope_markers();
        if !projection::scope_markers_contain_kind(markers, ScopeKind::Route) {
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
            let marker = markers.at(marker_idx);
            if markers.is_first_enter(marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            {
                let scope = marker.scope_id;
                let Some(ranges) = projection::route_arm_ranges(markers, scope) else {
                    panic!("route scope missing binary arm ranges");
                };
                let mut arm = 0usize;
                let mut arm_lane_bits = [0usize; 2];
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    let lanes =
                        projection::LocalLaneFacts::for_eff_range(eff_list, role, start, end);
                    arm_lane_bits[arm] = lanes.lane_bit_len();
                    facts.lane_bits += lanes.lane_bit_len();
                    facts.route_arm_lane_steps += lanes.relation_count();
                    facts.route_commit_rows += projection::route_commit_row_count(
                        markers,
                        eff_list.len(),
                        scope,
                        arm as u8,
                    );
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

    const fn roll_scope_count<const E: usize>(eff_list: &EffList<E>, role: u8) -> usize {
        let markers = eff_list.scope_markers();
        if !projection::scope_markers_contain_kind(markers, ScopeKind::Roll) {
            return 0;
        }
        let mut count = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if markers.is_first_enter(marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            {
                let end_eff =
                    projection::scope_segment_end(markers, marker_idx, Some(eff_list.len()));
                let row = projection::local_step_range_for_eff_range(
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
