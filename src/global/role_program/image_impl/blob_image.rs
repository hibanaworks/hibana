use super::super::{
    ColumnRange, PackedLaneRange, PackedLocalEventRow, PackedRollScopeRow,
    ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE,
    ROLE_IMAGE_LANE_RANGE_STRIDE, ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROLL_SCOPE_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE, ROLE_IMAGE_ROUTE_ARM_STRIDE,
    ROLE_IMAGE_ROUTE_SCOPE_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleImageBuild, RoleImageBytes,
    RoleImageColumns, RoleImagePlan, RoleImageRef, RouteArmLaneStepRow, RuntimeRoleFacts,
    ScopeKind,
};
use super::{projection, route_arm_row_index};
use crate::global::compiled::images::CompiledProgramRef;
use crate::global::const_dsl::EffList;
use crate::global::typestate::{PackedEventConflict, PackedLocalDependency};

mod lanes;
mod layout;
use layout::validate_role_image_layout;

impl RoleImagePlan {
    pub(crate) const fn build_if_fits<const N: usize, const E: usize>(
        &self,
        eff_list: &EffList<E>,
        facts: RuntimeRoleFacts,
        role: u8,
    ) -> Option<RoleImageBuild<N>> {
        if self.blob_len() > N {
            return None;
        }
        Some(RoleImageBytes::<N>::emit(
            eff_list,
            facts,
            role,
            self.columns,
        ))
    }
}

impl<const N: usize> RoleImageBuild<N> {
    #[inline(always)]
    pub(crate) const fn image_ref(
        &'static self,
        program: &'static CompiledProgramRef,
        role: u8,
        facts: RuntimeRoleFacts,
    ) -> RoleImageRef {
        self.bytes.image_ref(
            program,
            role,
            facts,
            self.columns,
            self.active_lane_row,
            self.first_active_lane,
        )
    }
}

impl<const N: usize> RoleImageBytes<N> {
    #[inline(always)]
    const fn empty() -> Self {
        Self { bytes: [0; N] }
    }

    #[inline(always)]
    pub(crate) const fn image_ref(
        &'static self,
        program: &'static CompiledProgramRef,
        role: u8,
        facts: RuntimeRoleFacts,
        columns: RoleImageColumns,
        active_lane_row: PackedLaneRange,
        first_active_lane: u16,
    ) -> RoleImageRef {
        RoleImageRef::new(
            program,
            role,
            facts,
            columns,
            &self.bytes,
            active_lane_row,
            first_active_lane,
        )
    }

    #[inline(always)]
    const fn write_u8(&mut self, offset: usize, value: u8) {
        if offset >= self.bytes.len() {
            panic!("role image");
        }
        self.bytes[offset] = value;
    }

    #[inline(always)]
    const fn write_u16(&mut self, offset: usize, value: u16) {
        self.write_u8(offset, value as u8);
        self.write_u8(offset + 1, (value >> 8) as u8);
    }

    #[inline(always)]
    const fn write_u32(&mut self, offset: usize, value: u32) {
        self.write_u16(offset, value as u16);
        self.write_u16(offset + 2, (value >> 16) as u16);
    }

    #[inline(always)]
    const fn column_offset(column: ColumnRange, row: usize, stride: usize) -> usize {
        if row >= column.len as usize {
            panic!("role image");
        }
        column.offset as usize + row * stride
    }

    #[inline(always)]
    const fn w8(&mut self, column: ColumnRange, row: usize, stride: usize, value: u8) {
        self.write_u8(Self::column_offset(column, row, stride), value);
    }

    #[inline(always)]
    const fn w16(&mut self, column: ColumnRange, row: usize, stride: usize, value: u16) {
        self.write_u16(Self::column_offset(column, row, stride), value);
    }

    #[inline(always)]
    const fn w32(&mut self, column: ColumnRange, row: usize, stride: usize, value: u32) {
        self.write_u32(Self::column_offset(column, row, stride), value);
    }

    #[inline(always)]
    const fn write_event(&mut self, column: ColumnRange, row: usize, event: PackedLocalEventRow) {
        let offset = Self::column_offset(column, row, ROLE_IMAGE_EVENT_STRIDE);
        self.write_u16(offset, event.eff_index);
        self.write_u16(offset + 2, event.dependency_row);
        self.write_u16(offset + 4, event.conflict_row);
        self.write_u16(offset + 6, event.scope().raw());
        self.write_u8(offset + 8, event.frame_label);
        self.write_u8(offset + 9, event.flags);
    }

    #[inline(always)]
    const fn write_route_arm_lane_step(
        &mut self,
        column: ColumnRange,
        row: usize,
        step: RouteArmLaneStepRow,
    ) {
        let offset = Self::column_offset(column, row, ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE);
        self.write_u8(offset, step.lane());
        self.write_u16(offset + 1, step.first_step());
        self.write_u16(offset + 3, step.last_step());
    }

    #[inline(always)]
    const fn write_dependency_row(
        &mut self,
        column: ColumnRange,
        row: usize,
        dependency: PackedLocalDependency,
    ) {
        let offset = Self::column_offset(column, row, ROLE_IMAGE_DEPENDENCY_STRIDE);
        self.write_u16(offset, dependency.start());
        self.write_u16(offset + 2, dependency.end());
        self.write_u16(offset + 4, dependency.dep_ordinal());
        self.write_u16(offset + 6, dependency.conflict_route());
    }

    #[inline(always)]
    const fn write_route_arm_row(
        &mut self,
        column: ColumnRange,
        row: usize,
        arm_row: super::super::PackedRouteArmRow,
    ) {
        let offset = Self::column_offset(column, row, ROLE_IMAGE_ROUTE_ARM_STRIDE);
        self.write_u32(offset, arm_row.event_row_raw());
        self.write_u32(offset + 4, arm_row.lane_step_len_and_child_slot_raw());
    }

    #[inline(always)]
    const fn write_roll_scope_row(
        &mut self,
        column: ColumnRange,
        row: usize,
        roll_row: PackedRollScopeRow,
    ) {
        let offset = Self::column_offset(column, row, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
        self.write_u16(offset, roll_row.scope_raw());
        self.write_u32(offset + 2, roll_row.event_row_raw());
    }

    pub(crate) const fn emit<const E: usize>(
        eff_list: &EffList<E>,
        facts: RuntimeRoleFacts,
        role: u8,
        columns: RoleImageColumns,
    ) -> RoleImageBuild<N> {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len * 2;
        validate_role_image_layout::<N>(columns, facts);

        let mut out = Self::empty();
        let markers = eff_list.scope_markers();
        let has_route = projection::scope_markers_contain_kind(markers, ScopeKind::Route);
        let mut dependency_row = 0usize;
        let mut conflict_row = 0usize;
        let mut local_step = 0usize;
        let mut first_active_lane = u16::MAX;
        let mut dependencies = projection::DependencyCursor::new(eff_list, role);
        let mut eff_idx = 0usize;
        while eff_idx < eff_list.len() {
            let node = eff_list.node_at(eff_idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    if atom.lane as usize >= footprint.logical_lane_count {
                        panic!("local event lane outside role logical domain");
                    }
                    if (atom.lane as u16) < first_active_lane {
                        first_active_lane = atom.lane as u16;
                    }
                    let mut event = projection::local_event_row_for_eff(
                        eff_list,
                        eff_idx,
                        eff_list.frame_label_at(eff_idx),
                        role,
                    );
                    let dependency = dependencies.next(eff_idx, atom.lane, local_step);
                    if !dependency.is_none() {
                        out.write_dependency_row(columns.dependencies, dependency_row, dependency);
                        event = event.with_dependency_row(dependency_row);
                        dependency_row += 1;
                    }
                    let conflict = if has_route {
                        projection::route_conflict_for_eff(markers, eff_idx)
                    } else {
                        PackedEventConflict::none()
                    };
                    if !conflict.is_none() {
                        out.w16(
                            columns.conflicts,
                            conflict_row,
                            ROLE_IMAGE_CONFLICT_STRIDE,
                            conflict.raw(),
                        );
                        event = event.with_conflict_row(conflict_row);
                        conflict_row += 1;
                    }
                    out.write_event(columns.events, local_step, event);
                    out.w8(columns.lanes, local_step, ROLE_IMAGE_LANE_STRIDE, atom.lane);
                    local_step += 1;
                }
            }
            eff_idx += 1;
        }
        if local_step != local_len
            || dependency_row != columns.dependencies.len as usize
            || conflict_row != columns.conflicts.len as usize
        {
            panic!("role image plan mismatch");
        }

        let active_lane_row =
            out.write_lane_bit_row(columns.lane_bits, 0, eff_list, role, 0, eff_list.len());
        let mut lane_bit_row = active_lane_row.end();

        let resident_row_count = if columns.resident_boundaries.len == 0 {
            0
        } else {
            columns.resident_boundaries.len as usize - 1
        };
        let mut resident_rows = projection::ResidentRowCursor::new(eff_list, role);
        let mut resident_row = 0usize;
        let mut resident_end = None;
        while let Some(row) = resident_rows.next() {
            if resident_row >= resident_row_count {
                panic!("role image plan mismatch");
            }
            if resident_row == 0 {
                out.w16(
                    columns.resident_boundaries,
                    0,
                    ROLE_IMAGE_U16_STRIDE,
                    row.start() as u16,
                );
            } else {
                let Some(previous_end) = resident_end else {
                    panic!("role resident row boundary missing");
                };
                if previous_end != row.start() {
                    panic!("role resident rows must be contiguous");
                }
            }
            out.w16(
                columns.resident_boundaries,
                resident_row + 1,
                ROLE_IMAGE_U16_STRIDE,
                row.end() as u16,
            );
            resident_end = Some(row.end());
            resident_row += 1;
        }

        let mut route_slot = 0usize;
        let mut route_arm_lane_step_row = 0usize;
        let mut route_commit_row = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if projection::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            {
                let scope = marker.scope_id;
                out.w16(
                    columns.route_scopes,
                    route_slot,
                    ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
                    scope.raw(),
                );
                let scope_conflict = PackedEventConflict::from_conflict(
                    projection::dependency_conflict_for_scope(markers, eff_list.len(), scope),
                )
                .with_route_reentry(marker.reentry);
                out.w16(
                    columns.route_scope_conflicts,
                    route_slot,
                    ROLE_IMAGE_CONFLICT_STRIDE,
                    scope_conflict.raw(),
                );
                let Some(ranges) = projection::route_arm_ranges(markers, scope) else {
                    panic!("route scope missing binary arm ranges");
                };
                let mut local_rows = [PackedLaneRange::EMPTY; 2];
                let mut arm = 0usize;
                while arm < 2 {
                    let (start_eff, end_eff) = ranges[arm];
                    let local_row = projection::local_step_range_for_eff_range(
                        eff_list, start_eff, end_eff, role,
                    );
                    local_rows[arm] = local_row;
                    let arm_row_index = route_arm_row_index(route_slot, arm as u8);
                    let lane_row = out.write_lane_bit_row(
                        columns.lane_bits,
                        lane_bit_row,
                        eff_list,
                        role,
                        start_eff,
                        end_eff,
                    );
                    lane_bit_row += lane_row.len();
                    out.w32(
                        columns.route_arm_lane_rows,
                        arm_row_index,
                        ROLE_IMAGE_LANE_RANGE_STRIDE,
                        lane_row.raw(),
                    );
                    let written_steps = out.write_route_arm_lane_steps(
                        columns.route_arm_lane_step_rows,
                        route_arm_lane_step_row,
                        eff_list,
                        role,
                        (start_eff, end_eff),
                        local_row,
                    );
                    let lane_step_row =
                        PackedLaneRange::new(route_arm_lane_step_row, written_steps);
                    route_arm_lane_step_row += written_steps;
                    let child_slot = match projection::passive_arm_child_scope(
                        markers,
                        eff_list.len(),
                        scope,
                        arm as u8,
                        start_eff,
                        end_eff,
                    ) {
                        Some(child_scope) => {
                            let Some(child_slot) =
                                projection::route_scope_slot_for_scope(markers, child_scope)
                            else {
                                panic!("passive route child scope missing route row slot");
                            };
                            if child_slot <= route_slot {
                                panic!("passive route child scope must follow parent route slot");
                            }
                            Some(child_slot)
                        }
                        None => None,
                    };
                    out.write_route_arm_row(
                        columns.route_arms,
                        arm_row_index,
                        super::super::PackedRouteArmRow::new(local_row, child_slot, lane_step_row),
                    );
                    let commit_len = projection::route_commit_row_count(
                        markers,
                        eff_list.len(),
                        scope,
                        arm as u8,
                    );
                    let commit_range = PackedLaneRange::new(route_commit_row, commit_len);
                    out.w32(
                        columns.route_commit_ranges,
                        arm_row_index,
                        ROLE_IMAGE_LANE_RANGE_STRIDE,
                        commit_range.raw(),
                    );
                    let mut pos = 0usize;
                    while pos < commit_len {
                        let target = commit_len - pos - 1;
                        out.w16(
                            columns.route_commit_rows,
                            route_commit_row + pos,
                            ROLE_IMAGE_CONFLICT_STRIDE,
                            projection::route_commit_conflict_at(
                                markers,
                                eff_list.len(),
                                scope,
                                arm as u8,
                                target,
                            )
                            .raw(),
                        );
                        pos += 1;
                    }
                    route_commit_row += commit_len;
                    arm += 1;
                }
                let offer_row = out.write_lane_bit_union_row(
                    columns.lane_bits,
                    lane_bit_row,
                    eff_list,
                    role,
                    ranges[0],
                    ranges[1],
                );
                lane_bit_row += offer_row.len();
                out.w32(
                    columns.route_offer_lane_rows,
                    route_slot,
                    ROLE_IMAGE_LANE_RANGE_STRIDE,
                    offer_row.raw(),
                );
                route_slot += 1;
            }
            marker_idx += 1;
        }

        let mut roll_scope = 0usize;
        marker_idx = 0;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if projection::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            {
                let end_eff = projection::scope_segment_end(markers, marker_idx, eff_list.len());
                let row = projection::local_step_range_for_eff_range(
                    eff_list,
                    marker.offset(),
                    end_eff,
                    role,
                );
                if !row.is_absent_or_zero_len() {
                    out.write_roll_scope_row(
                        columns.roll_scopes,
                        roll_scope,
                        PackedRollScopeRow::new(marker.scope_id, row),
                    );
                    roll_scope += 1;
                }
            }
            marker_idx += 1;
        }

        if route_slot != route_scope_len
            || route_slot * 2 != route_arm_len
            || resident_row != resident_row_count
            || lane_bit_row != columns.lane_bits.len as usize
            || route_arm_lane_step_row != columns.route_arm_lane_step_rows.len as usize
            || route_commit_row != columns.route_commit_rows.len as usize
            || roll_scope != columns.roll_scopes.len as usize
        {
            panic!("role image plan mismatch");
        }

        let offset = columns.blob_len();
        if offset > u16::MAX as usize {
            panic!("role image");
        }
        if offset > out.bytes.len() {
            panic!("role image");
        }
        RoleImageBuild {
            bytes: out,
            columns,
            active_lane_row,
            first_active_lane,
        }
    }
}
