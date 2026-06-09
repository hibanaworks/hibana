use super::super::{
    LANE_DOMAIN_SIZE, PackedColumn, PackedLaneRange, PackedLocalEventRow,
    ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE,
    ROLE_IMAGE_LANE_RANGE_STRIDE, ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleImageBlobStorage, RoleImageColumns,
    RoleLaneScratch, RouteArmLaneStepRow, RuntimeRoleFacts,
};

impl<const N: usize> RoleImageBlobStorage<N> {
    #[inline(always)]
    const fn empty() -> Self {
        Self {
            columns: RoleImageColumns::empty(),
            bytes: [0; N],
            len: 0,
            active_lane_row: PackedLaneRange::EMPTY,
            first_active_lane: RoleLaneScratch::NO_ACTIVE_LANE,
        }
    }

    #[inline(always)]
    pub(crate) const fn projected_len(scratch: RoleLaneScratch, facts: RuntimeRoleFacts) -> usize {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let dependency_len = scratch.dependency_row_len();
        let conflict_len = scratch.conflict_row_len();
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len.saturating_mul(2);
        let route_arm_lane_step_row_len = scratch.route_arm_lane_step_row_len as usize;
        let resident_boundary_len = scratch.resident_boundary_count();
        let lane_bit_len = scratch.lane_bit_row_len();
        let route_commit_len = scratch.route_commit_row_len();

        (local_len * ROLE_IMAGE_EVENT_STRIDE)
            + (local_len * ROLE_IMAGE_LANE_STRIDE)
            + (dependency_len * ROLE_IMAGE_DEPENDENCY_STRIDE)
            + (conflict_len * ROLE_IMAGE_CONFLICT_STRIDE)
            + (route_scope_len * ROLE_IMAGE_U16_STRIDE)
            + (route_scope_len * ROLE_IMAGE_CONFLICT_STRIDE)
            + (route_arm_len * ROLE_IMAGE_ROUTE_ARM_STRIDE)
            + (resident_boundary_len * ROLE_IMAGE_U16_STRIDE)
            + (lane_bit_len * ROLE_IMAGE_LANE_STRIDE)
            + (route_arm_len * ROLE_IMAGE_LANE_RANGE_STRIDE)
            + (route_scope_len * ROLE_IMAGE_LANE_RANGE_STRIDE)
            + (route_arm_lane_step_row_len * ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE)
            + (route_arm_len * ROLE_IMAGE_LANE_RANGE_STRIDE)
            + (route_commit_len * ROLE_IMAGE_CONFLICT_STRIDE)
    }

    #[inline(always)]
    pub(crate) const fn blob(&'static self) -> &'static [u8] {
        if self.len as usize > self.bytes.len() {
            panic!("role image");
        }
        // SAFETY: len is checked against this static backing array and the returned slice borrows it.
        unsafe { core::slice::from_raw_parts(self.bytes.as_ptr(), self.len as usize) }
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
    const fn write_u64(&mut self, offset: usize, value: u64) {
        self.write_u32(offset, value as u32);
        self.write_u32(offset + 4, (value >> 32) as u32);
    }

    #[inline(always)]
    const fn row_offset(column: PackedColumn, row: usize) -> usize {
        if row >= column.len as usize {
            panic!("role image");
        }
        column.offset as usize + row * column.stride as usize
    }

    #[inline(always)]
    const fn write_event(&mut self, column: PackedColumn, row: usize, event: PackedLocalEventRow) {
        let offset = Self::row_offset(column, row);
        self.write_u16(offset, event.eff_index);
        self.write_u16(offset + 2, event.dependency_row);
        self.write_u16(offset + 4, event.conflict_row);
        self.write_u16(offset + 6, event.packed_scope_slot());
        self.write_u8(offset + 8, event.frame_label);
        self.write_u8(offset + 9, event.flags);
    }

    #[inline(always)]
    const fn write_route_arm_lane_step(
        &mut self,
        column: PackedColumn,
        row: usize,
        step: RouteArmLaneStepRow,
    ) {
        let offset = Self::row_offset(column, row);
        self.write_u8(offset, step.lane());
        self.write_u16(offset + 1, step.first_step());
        self.write_u16(offset + 3, step.last_step());
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
        column: PackedColumn,
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
    pub(crate) const fn from_unselected_bucket_or_empty(
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
        let route_arm_len = route_scope_len.saturating_mul(2);
        let route_arm_lane_step_row_len = scratch.route_arm_lane_step_row_len as usize;
        let resident_boundary_len = scratch.resident_boundary_count();
        let lane_bit_len = scratch.lane_bit_row_len();
        let route_commit_len = scratch.route_commit_row_len();
        let projected_len = Self::projected_len(scratch, facts);
        if projected_len > N {
            panic!("role image");
        }

        let mut out = Self::empty();
        out.active_lane_row = scratch.active_lane_row;
        out.first_active_lane = scratch.first_active_lane;

        let mut offset = 0usize;
        out.columns.events = PackedColumn::new(offset, local_len, ROLE_IMAGE_EVENT_STRIDE);
        let mut idx = 0usize;
        while idx < local_len {
            out.write_event(out.columns.events, idx, scratch.local_step_events[idx]);
            idx += 1;
        }
        offset = out.columns.events.end_offset();

        out.columns.lanes = PackedColumn::new(offset, local_len, ROLE_IMAGE_LANE_STRIDE);
        idx = 0;
        while idx < local_len {
            out.write_u8(
                Self::row_offset(out.columns.lanes, idx),
                scratch.local_step_lanes[idx],
            );
            idx += 1;
        }
        offset = out.columns.lanes.end_offset();

        out.columns.dependencies =
            PackedColumn::new(offset, dependency_len, ROLE_IMAGE_DEPENDENCY_STRIDE);
        idx = 0;
        while idx < dependency_len {
            out.write_u64(
                Self::row_offset(out.columns.dependencies, idx),
                scratch.local_step_dependencies[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.dependencies.end_offset();

        out.columns.conflicts = PackedColumn::new(offset, conflict_len, ROLE_IMAGE_CONFLICT_STRIDE);
        idx = 0;
        while idx < conflict_len {
            out.write_u16(
                Self::row_offset(out.columns.conflicts, idx),
                scratch.local_step_conflicts[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.conflicts.end_offset();

        out.columns.route_scopes =
            PackedColumn::new(offset, route_scope_len, ROLE_IMAGE_U16_STRIDE);
        idx = 0;
        while idx < route_scope_len {
            out.write_u16(
                Self::row_offset(out.columns.route_scopes, idx),
                scratch.route_scope_rows[idx],
            );
            idx += 1;
        }
        offset = out.columns.route_scopes.end_offset();

        out.columns.route_scope_conflicts =
            PackedColumn::new(offset, route_scope_len, ROLE_IMAGE_CONFLICT_STRIDE);
        idx = 0;
        while idx < route_scope_len {
            out.write_u16(
                Self::row_offset(out.columns.route_scope_conflicts, idx),
                scratch.route_scope_conflicts[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.route_scope_conflicts.end_offset();

        out.columns.route_arms =
            PackedColumn::new(offset, route_arm_len, ROLE_IMAGE_ROUTE_ARM_STRIDE);
        idx = 0;
        while idx < route_arm_len {
            out.write_u64(
                Self::row_offset(out.columns.route_arms, idx),
                scratch.route_arm_rows[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.route_arms.end_offset();

        out.columns.resident_boundaries =
            PackedColumn::new(offset, resident_boundary_len, ROLE_IMAGE_U16_STRIDE);
        idx = 0;
        while idx < resident_boundary_len {
            out.write_u16(
                Self::row_offset(out.columns.resident_boundaries, idx),
                scratch.resident_row_boundaries[idx],
            );
            idx += 1;
        }
        offset = out.columns.resident_boundaries.end_offset();

        out.columns.lane_bits = PackedColumn::new(offset, lane_bit_len, ROLE_IMAGE_LANE_STRIDE);
        idx = 0;
        while idx < lane_bit_len {
            out.write_u8(
                Self::row_offset(out.columns.lane_bits, idx),
                scratch.lane_bit_rows[idx],
            );
            idx += 1;
        }
        offset = out.columns.lane_bits.end_offset();

        out.columns.route_arm_lane_rows =
            PackedColumn::new(offset, route_arm_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        idx = 0;
        while idx < route_arm_len {
            out.write_u32(
                Self::row_offset(out.columns.route_arm_lane_rows, idx),
                scratch.route_arm_lane_rows[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.route_arm_lane_rows.end_offset();

        out.columns.route_offer_lane_rows =
            PackedColumn::new(offset, route_scope_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        idx = 0;
        while idx < route_scope_len {
            out.write_u32(
                Self::row_offset(out.columns.route_offer_lane_rows, idx),
                scratch.route_offer_lane_rows[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.route_offer_lane_rows.end_offset();

        out.columns.route_arm_lane_step_rows = PackedColumn::new(
            offset,
            route_arm_lane_step_row_len,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        );
        idx = 0;
        let mut written_route_arm_lane_step_rows = 0usize;
        while idx < route_arm_len {
            let arm_row = scratch.route_arm_rows[idx];
            let range = arm_row.lane_step_row();
            if range.start() != written_route_arm_lane_step_rows {
                panic!("role image");
            }
            let written = out.write_route_arm_lane_steps(
                out.columns.route_arm_lane_step_rows,
                range.start(),
                scratch,
                arm_row.event_row(),
            );
            if written != range.len() {
                panic!("role image");
            }
            written_route_arm_lane_step_rows =
                written_route_arm_lane_step_rows.saturating_add(written);
            idx += 1;
        }
        if written_route_arm_lane_step_rows != route_arm_lane_step_row_len {
            panic!("role image");
        }
        offset = out.columns.route_arm_lane_step_rows.end_offset();

        out.columns.route_commit_ranges =
            PackedColumn::new(offset, route_arm_len, ROLE_IMAGE_LANE_RANGE_STRIDE);
        idx = 0;
        while idx < route_arm_len {
            out.write_u32(
                Self::row_offset(out.columns.route_commit_ranges, idx),
                scratch.route_commit_ranges[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.route_commit_ranges.end_offset();

        out.columns.route_commit_rows =
            PackedColumn::new(offset, route_commit_len, ROLE_IMAGE_CONFLICT_STRIDE);
        idx = 0;
        while idx < route_arm_len {
            let range = scratch.route_commit_ranges[idx];
            if !range.is_empty() {
                let slot = idx / 2;
                let arm = (idx - slot.saturating_mul(2)) as u8;
                let mut pos = 0usize;
                while pos < range.len() {
                    let target = range.len() - pos - 1;
                    out.write_u16(
                        Self::row_offset(out.columns.route_commit_rows, range.start() + pos),
                        scratch.route_commit_conflict_at(slot, arm, target).raw(),
                    );
                    pos += 1;
                }
            }
            idx += 1;
        }
        offset = out.columns.route_commit_rows.end_offset();

        if offset != out.columns.blob_len() {
            panic!("role image");
        }
        if offset > u16::MAX as usize {
            panic!("role image");
        }
        if offset > out.bytes.len() {
            panic!("role image");
        }
        if offset > u16::MAX as usize {
            panic!("role image");
        }
        out.len = offset as u16;
        out
    }
}
