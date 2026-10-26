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
    pub(super) roll_entry_views: usize,
    pub(super) owned_receipts: usize,
}

struct RouteImagePlanRouteFacts {
    lane_bits: usize,
    route_arm_lane_steps: usize,
    route_commit_rows: usize,
}

impl RoleImagePlan {
    #[cfg(hibana_profile)]
    pub(crate) const fn dependency_workspace_bytes<const E: usize>() -> usize {
        core::mem::size_of::<projection::DependencyCursor<'static, E>>()
    }

    #[inline(always)]
    pub(crate) const fn blob_len(&self) -> usize {
        self.columns.blob_len()
    }

    pub(crate) const fn from_program<const E: usize>(
        source: (&EffList<E>, &projection::ScopeFacts),
        facts: RuntimeRoleFacts,
        rows: projection::LocalRows<'_, E>,
    ) -> Self {
        let counts =
            RoleImageColumnCounts::from_program(source, facts.footprint().logical_lane_count, rows);
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
        if self.roll_entry_views > u16::MAX as usize || self.owned_receipts > u16::MAX as usize {
            panic!("role resource index count overflow");
        }
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
            roll_entry_view_count: self.roll_entry_views as u16,
            owned_receipt_count: self.owned_receipts as u16,
        }
    }
}

impl RoleImageColumnCounts {
    pub(super) const fn from_program<const E: usize>(
        source: (&EffList<E>, &projection::ScopeFacts),
        logical_lane_count: usize,
        rows: projection::LocalRows<'_, E>,
    ) -> Self {
        let (eff_list, scopes) = source;
        if logical_lane_count == 0 || logical_lane_count > LANE_DOMAIN_SIZE {
            panic!("role logical lane domain invalid");
        }
        let markers = eff_list.scope_markers();
        let has_route = projection::scope_markers_contain_kind(markers, ScopeKind::Route);
        let mut local_step_count = 0usize;
        let mut dependency_rows = 0usize;
        let mut conflict_rows = 0usize;
        let mut max_lane_plus_one = 0usize;
        let mut dependencies = projection::DependencyCursor::new(eff_list, scopes, rows);
        let mut idx = 0usize;
        while idx < eff_list.len() {
            let atom = eff_list.atom_at(idx);
            let mut side = 0;
            while side < rows.count(eff_list, idx) {
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
                side += 1;
            }
            idx += 1;
        }
        let resident_rows = Self::resident_row_count(eff_list, rows);
        let route_facts = Self::route_facts(source, rows);
        let roll_scopes = Self::roll_scope_count(eff_list, rows);
        let roll_entry_views = Self::roll_entry_view_count(eff_list, rows);
        let mut owned_receipts = 0;
        if roll_entry_views != 0 {
            let mut event = 0;
            while event < eff_list.len() {
                if matches!(
                    eff_list.atom_at(event).payload,
                    crate::eff::EventPayload::Endpoint(_)
                ) {
                    let mut side = 0;
                    while side < rows.count(eff_list, event) {
                        if matches!(
                            rows.direction(eff_list, event, side),
                            super::event_rows::RoleLocalDirection::Recv
                        ) {
                            owned_receipts += 1;
                        }
                        side += 1;
                    }
                }
                event += 1;
            }
        }
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
            lane_bits: active_lane_bits
                + route_facts.lane_bits
                + super::parallel_masks::MaskRows::byte_len(eff_list, rows),
            route_arm_lane_steps: route_facts.route_arm_lane_steps,
            route_commit_rows: route_facts.route_commit_rows,
            roll_scopes,
            roll_entry_views,
            owned_receipts,
        }
    }

    const fn roll_entry_view_count<const E: usize>(
        eff_list: &EffList<E>,
        rows: projection::LocalRows<'_, E>,
    ) -> usize {
        let markers = eff_list.scope_markers();
        let mut count = 0;
        let mut marker_idx = 0;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if markers.is_first_enter(marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            {
                let end = projection::scope_segment_end(markers, marker_idx, Some(eff_list.len()));
                let row = projection::local_step_range_for_eff_range(
                    eff_list,
                    marker.offset(),
                    end,
                    rows,
                );
                if !row.is_absent_or_zero_len() {
                    let mut role = 0usize;
                    while role < 256 {
                        if rows
                            .roll_entry_actor(eff_list, marker.offset(), end, role as u8)
                            .is_some()
                        {
                            count += 1;
                        }
                        role += 1;
                    }
                }
            }
            marker_idx += 1;
        }
        count
    }

    const fn resident_row_count<const E: usize>(
        eff_list: &EffList<E>,
        selection: projection::LocalRows<'_, E>,
    ) -> usize {
        let mut rows = projection::ResidentRowCursor::new(eff_list, selection);
        let mut row_count = 0usize;
        while rows.next().is_some() {
            row_count += 1;
        }
        row_count
    }

    const fn route_facts<const E: usize>(
        source: (&EffList<E>, &projection::ScopeFacts),
        rows: projection::LocalRows<'_, E>,
    ) -> RouteImagePlanRouteFacts {
        let (eff_list, scopes) = source;
        let markers = eff_list.scope_markers();
        let retained = projection::retained_route_scopes(source, rows);
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
                && retained[marker.scope_id.local_ordinal() as usize]
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
                        projection::LocalLaneFacts::for_eff_range(eff_list, rows, start, end);
                    arm_lane_bits[arm] = lanes.lane_bit_len();
                    facts.lane_bits += lanes.lane_bit_len();
                    facts.route_arm_lane_steps += lanes.relation_count();
                    arm += 1;
                }
                facts.route_commit_rows += 2 * projection::route_commit_row_count(scopes, scope);
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

    const fn roll_scope_count<const E: usize>(
        eff_list: &EffList<E>,
        rows: projection::LocalRows<'_, E>,
    ) -> usize {
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
                    rows,
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

#[cfg(all(test, hibana_repo_tests))]
mod reference_plan;
