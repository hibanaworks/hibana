use super::super::{
    ColumnRange, LANE_DOMAIN_SIZE, PackedLaneRange, PackedLocalEventRow, PackedRollScopeRow,
    ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE,
    ROLE_IMAGE_LANE_RANGE_STRIDE, ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROLL_SCOPE_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE, ROLE_IMAGE_ROUTE_ARM_STRIDE,
    ROLE_IMAGE_ROUTE_SCOPE_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleImageBytes, RoleImageColumns,
    RoleImageRef, RoleLaneScratch, RouteArmLaneStepRow, RuntimeRoleFacts,
};
use crate::global::compiled::images::CompiledProgramRef;
use crate::global::const_dsl::ScopeId;
use crate::global::typestate::{PackedEventConflict, PackedLocalDependency};

impl<const N: usize> RoleImageBytes<N> {
    #[inline(always)]
    const fn empty() -> Self {
        Self { bytes: [0; N] }
    }

    #[inline(always)]
    pub(crate) const fn projected_len(scratch: RoleLaneScratch, facts: RuntimeRoleFacts) -> usize {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let dependency_len = scratch.dependency_row_len();
        let conflict_len = scratch.conflict_row_len();
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len * 2;
        let route_arm_lane_step_row_len = scratch.route_arm_lane_step_row_len as usize;
        let resident_boundary_len = scratch.resident_boundary_count();
        let lane_bit_len = scratch.lane_bit_row_len();
        let route_commit_len = scratch.route_commit_row_len();
        let roll_scope_len = scratch.roll_scope_row_len();

        (local_len * ROLE_IMAGE_EVENT_STRIDE)
            + (local_len * ROLE_IMAGE_LANE_STRIDE)
            + (dependency_len * ROLE_IMAGE_DEPENDENCY_STRIDE)
            + (conflict_len * ROLE_IMAGE_CONFLICT_STRIDE)
            + (route_scope_len * ROLE_IMAGE_ROUTE_SCOPE_STRIDE)
            + (route_scope_len * ROLE_IMAGE_CONFLICT_STRIDE)
            + (route_arm_len * ROLE_IMAGE_ROUTE_ARM_STRIDE)
            + (resident_boundary_len * ROLE_IMAGE_U16_STRIDE)
            + (lane_bit_len * ROLE_IMAGE_LANE_STRIDE)
            + (route_arm_len * ROLE_IMAGE_LANE_RANGE_STRIDE)
            + (route_scope_len * ROLE_IMAGE_LANE_RANGE_STRIDE)
            + (route_arm_lane_step_row_len * ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE)
            + (route_arm_len * ROLE_IMAGE_LANE_RANGE_STRIDE)
            + (route_commit_len * ROLE_IMAGE_CONFLICT_STRIDE)
            + (roll_scope_len * ROLE_IMAGE_ROLL_SCOPE_STRIDE)
    }

    #[inline(always)]
    const fn column_at(offset: usize, len: usize, stride: usize) -> (ColumnRange, usize) {
        let column = ColumnRange::new(offset, len, stride);
        (column, column.end_offset(stride))
    }

    #[inline(always)]
    pub(crate) const fn columns(
        scratch: RoleLaneScratch,
        facts: RuntimeRoleFacts,
    ) -> RoleImageColumns {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let dependency_len = scratch.dependency_row_len();
        let conflict_len = scratch.conflict_row_len();
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len * 2;
        let route_arm_lane_step_row_len = scratch.route_arm_lane_step_row_len as usize;
        let resident_boundary_len = scratch.resident_boundary_count();
        let lane_bit_len = scratch.lane_bit_row_len();
        let route_commit_len = scratch.route_commit_row_len();
        let roll_scope_len = scratch.roll_scope_row_len();

        let (events, offset) = Self::column_at(0, local_len, ROLE_IMAGE_EVENT_STRIDE);
        let (lanes, offset) = Self::column_at(offset, local_len, ROLE_IMAGE_LANE_STRIDE);
        let (dependencies, offset) =
            Self::column_at(offset, dependency_len, ROLE_IMAGE_DEPENDENCY_STRIDE);
        let (conflicts, offset) = Self::column_at(offset, conflict_len, ROLE_IMAGE_CONFLICT_STRIDE);
        let (route_scopes, offset) =
            Self::column_at(offset, route_scope_len, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
        let (route_scope_conflicts, offset) =
            Self::column_at(offset, route_scope_len, ROLE_IMAGE_CONFLICT_STRIDE);
        let (route_arms, offset) =
            Self::column_at(offset, route_arm_len, ROLE_IMAGE_ROUTE_ARM_STRIDE);
        let (resident_boundaries, offset) =
            Self::column_at(offset, resident_boundary_len, ROLE_IMAGE_U16_STRIDE);
        let (lane_bits, offset) = Self::column_at(offset, lane_bit_len, ROLE_IMAGE_LANE_STRIDE);
        let (route_arm_lane_rows, offset) =
            Self::column_at(offset, route_arm_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        let (route_offer_lane_rows, offset) =
            Self::column_at(offset, route_scope_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        let (route_arm_lane_step_rows, offset) = Self::column_at(
            offset,
            route_arm_lane_step_row_len,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        );
        let (route_commit_ranges, offset) =
            Self::column_at(offset, route_arm_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        let (route_commit_rows, _) =
            Self::column_at(offset, route_commit_len, ROLE_IMAGE_CONFLICT_STRIDE);
        let (roll_scopes, _) = Self::column_at(
            route_commit_rows.end_offset(ROLE_IMAGE_CONFLICT_STRIDE),
            roll_scope_len,
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

    #[inline(always)]
    pub(crate) const fn image_ref(
        &'static self,
        program: &'static CompiledProgramRef,
        role: u8,
        scratch: RoleLaneScratch,
        facts: RuntimeRoleFacts,
    ) -> RoleImageRef {
        let columns = Self::columns(scratch, facts);
        RoleImageRef::new(
            program,
            role,
            facts,
            columns,
            &self.bytes,
            scratch.active_lane_row,
            scratch.first_active_lane,
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
        self.write_u32(offset + 6, event.scope().raw());
        self.write_u8(offset + 10, event.frame_label);
        self.write_u8(offset + 11, event.flags);
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
        self.write_u32(offset, arm_row.event_and_child_raw());
        self.write_u32(offset + 4, arm_row.lane_step_raw());
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

    #[inline(always)]
    const fn route_arm_lane_step_index(
        rows: &[RouteArmLaneStepRow; LANE_DOMAIN_SIZE],
        len: usize,
        lane: u8,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while idx < len {
            if rows[idx].lane() == lane {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    const fn collect_route_arm_lane_steps(
        scratch: RoleLaneScratch,
        local_row: PackedLaneRange,
    ) -> ([RouteArmLaneStepRow; LANE_DOMAIN_SIZE], usize) {
        let mut rows = [RouteArmLaneStepRow::EMPTY; LANE_DOMAIN_SIZE];
        let mut len = 0usize;
        let mut pos = local_row.start();
        let end = local_row.end();
        while pos < end && pos < scratch.local_step_lanes.len() {
            let lane = scratch.local_step_lanes[pos];
            match Self::route_arm_lane_step_index(&rows, len, lane) {
                Some(row_idx) => {
                    rows[row_idx] = rows[row_idx].with_last_step(pos);
                }
                None => {
                    if len >= LANE_DOMAIN_SIZE {
                        panic!("route arm lane step row overflow");
                    }
                    rows[len] = RouteArmLaneStepRow::new(lane, pos, pos);
                    len += 1;
                }
            }
            pos += 1;
        }
        (rows, len)
    }

    #[inline(always)]
    const fn write_route_arm_lane_steps(
        &mut self,
        column: ColumnRange,
        row_start: usize,
        scratch: RoleLaneScratch,
        local_row: PackedLaneRange,
    ) -> usize {
        let (rows, len) = Self::collect_route_arm_lane_steps(scratch, local_row);
        let mut idx = 0usize;
        while idx < len {
            self.write_route_arm_lane_step(column, row_start + idx, rows[idx]);
            idx += 1;
        }
        len
    }

    #[inline(always)]
    const fn write_event_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        values: &[PackedLocalEventRow; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.write_event(column, idx, values[idx]);
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_u8_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        stride: usize,
        values: &[u8; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.w8(column, idx, stride, values[idx]);
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_u16_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        stride: usize,
        values: &[u16; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.w16(column, idx, stride, values[idx]);
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_scope_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        values: &[ScopeId; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.w32(
                column,
                idx,
                ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
                values[idx].raw(),
            );
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_dependency_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        values: &[PackedLocalDependency; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.write_dependency_row(column, idx, values[idx]);
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_conflict_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        values: &[PackedEventConflict; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.w16(column, idx, ROLE_IMAGE_CONFLICT_STRIDE, values[idx].raw());
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_route_arm_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        values: &[super::super::PackedRouteArmRow; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.write_route_arm_row(column, idx, values[idx]);
            idx += 1;
        }
    }

    #[inline(always)]
    const fn write_lane_range_rows<const M: usize>(
        &mut self,
        column: ColumnRange,
        len: usize,
        values: &[PackedLaneRange; M],
    ) {
        let mut idx = 0usize;
        while idx < len {
            self.w32(column, idx, ROLE_IMAGE_LANE_RANGE_STRIDE, values[idx].raw());
            idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) const fn from_capacity_bucket(
        scratch: RoleLaneScratch,
        facts: RuntimeRoleFacts,
    ) -> Self {
        if Self::projected_len(scratch, facts) > N {
            return Self::empty();
        }
        Self::from_scratch(scratch, facts)
    }

    #[inline(always)]
    pub(crate) const fn from_scratch(scratch: RoleLaneScratch, facts: RuntimeRoleFacts) -> Self {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let dependency_len = scratch.dependency_row_len();
        let conflict_len = scratch.conflict_row_len();
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len * 2;
        let route_arm_lane_step_row_len = scratch.route_arm_lane_step_row_len as usize;
        let resident_boundary_len = scratch.resident_boundary_count();
        let lane_bit_len = scratch.lane_bit_row_len();
        let roll_scope_len = scratch.roll_scope_row_len();
        let projected_len = Self::projected_len(scratch, facts);
        if projected_len > N {
            panic!("role image");
        }

        let mut out = Self::empty();
        let columns = Self::columns(scratch, facts);

        out.write_event_rows(columns.events, local_len, &scratch.local_step_events);
        out.write_u8_rows(
            columns.lanes,
            local_len,
            ROLE_IMAGE_LANE_STRIDE,
            &scratch.local_step_lanes,
        );
        out.write_dependency_rows(
            columns.dependencies,
            dependency_len,
            &scratch.local_step_dependencies,
        );
        out.write_conflict_rows(
            columns.conflicts,
            conflict_len,
            &scratch.local_step_conflicts,
        );
        out.write_scope_rows(
            columns.route_scopes,
            route_scope_len,
            &scratch.route_scope_rows,
        );
        out.write_conflict_rows(
            columns.route_scope_conflicts,
            route_scope_len,
            &scratch.route_scope_conflicts,
        );
        out.write_route_arm_rows(columns.route_arms, route_arm_len, &scratch.route_arm_rows);
        out.write_u16_rows(
            columns.resident_boundaries,
            resident_boundary_len,
            ROLE_IMAGE_U16_STRIDE,
            &scratch.resident_row_boundaries,
        );
        out.write_u8_rows(
            columns.lane_bits,
            lane_bit_len,
            ROLE_IMAGE_LANE_STRIDE,
            &scratch.lane_bit_rows,
        );
        out.write_lane_range_rows(
            columns.route_arm_lane_rows,
            route_arm_len,
            &scratch.route_arm_lane_rows,
        );
        out.write_lane_range_rows(
            columns.route_offer_lane_rows,
            route_scope_len,
            &scratch.route_offer_lane_rows,
        );
        let mut idx = 0usize;
        let mut written_route_arm_lane_step_rows = 0usize;
        while idx < route_arm_len {
            let arm_row = scratch.route_arm_rows[idx];
            let range = arm_row.lane_step_row();
            if range.start() != written_route_arm_lane_step_rows {
                panic!("role image");
            }
            let written = out.write_route_arm_lane_steps(
                columns.route_arm_lane_step_rows,
                range.start(),
                scratch,
                arm_row.event_row(),
            );
            if written != range.len() {
                panic!("role image");
            }
            written_route_arm_lane_step_rows += written;
            idx += 1;
        }
        if written_route_arm_lane_step_rows != route_arm_lane_step_row_len {
            panic!("role image");
        }

        out.write_lane_range_rows(
            columns.route_commit_ranges,
            route_arm_len,
            &scratch.route_commit_ranges,
        );

        idx = 0;
        while idx < route_arm_len {
            let range = scratch.route_commit_ranges[idx];
            if !range.is_empty() {
                let slot = idx / 2;
                let arm = (idx - slot * 2) as u8;
                let mut pos = 0usize;
                while pos < range.len() {
                    let target = range.len() - pos - 1;
                    out.w16(
                        columns.route_commit_rows,
                        range.start() + pos,
                        ROLE_IMAGE_CONFLICT_STRIDE,
                        scratch.route_commit_conflict_at(slot, arm, target).raw(),
                    );
                    pos += 1;
                }
            }
            idx += 1;
        }

        idx = 0;
        while idx < roll_scope_len {
            out.write_roll_scope_row(columns.roll_scopes, idx, scratch.roll_scope_rows[idx]);
            idx += 1;
        }

        let offset = columns.blob_len();
        if offset > u16::MAX as usize {
            panic!("role image");
        }
        if offset > out.bytes.len() {
            panic!("role image");
        }
        out
    }
}
