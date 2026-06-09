use super::super::{
    PackedColumn, PackedLaneRange, PackedLocalEventRow, ROLE_IMAGE_CONFLICT_STRIDE,
    ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_LANE_RANGE_STRIDE,
    ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleFacts,
    RoleImageBlobStorage, RoleImageColumns, RoleLaneScratch,
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
    pub(crate) const fn projected_len(scratch: RoleLaneScratch, facts: RoleFacts) -> usize {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let dependency_len = scratch.dependency_row_len();
        let conflict_len = scratch.conflict_row_len();
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len.saturating_mul(2);
        let route_arm_lane_step_len = route_arm_len.saturating_mul(footprint.logical_lane_count);
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
            + (route_arm_lane_step_len * ROLE_IMAGE_U16_STRIDE * 2)
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
    const fn route_arm_lane_step_bounds(
        scratch: RoleLaneScratch,
        arm_row: usize,
        lane: usize,
        local_len: usize,
    ) -> (u16, u16) {
        if lane > u8::MAX as usize {
            panic!("role image");
        }
        let row = scratch.route_arm_rows[arm_row].event_row();
        if row.is_empty() {
            return (u16::MAX, u16::MAX);
        }
        let mut first = u16::MAX;
        let mut last = u16::MAX;
        let mut pos = row.start();
        let end = row.end();
        while pos < end && pos < local_len {
            if scratch.local_step_lanes[pos] as usize == lane {
                if pos > u16::MAX as usize {
                    panic!("role image");
                }
                let step = pos as u16;
                if first == u16::MAX {
                    first = step;
                }
                last = step;
            }
            pos += 1;
        }
        (first, last)
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
    pub(crate) const fn from_unselected_bucket_or_empty(
        scratch: RoleLaneScratch,
        facts: RoleFacts,
    ) -> Self {
        if Self::projected_len(scratch, facts) > N {
            return Self::empty();
        }
        Self::from_scratch(scratch, facts)
    }

    #[inline(always)]
    pub(crate) const fn from_scratch(scratch: RoleLaneScratch, facts: RoleFacts) -> Self {
        let footprint = facts.footprint();
        let local_len = footprint.local_step_count;
        let dependency_len = scratch.dependency_row_len();
        let conflict_len = scratch.conflict_row_len();
        let route_scope_len = footprint.route_scope_count;
        let route_arm_len = route_scope_len.saturating_mul(2);
        let route_arm_lane_step_len = route_arm_len.saturating_mul(footprint.logical_lane_count);
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
            out.write_u32(
                Self::row_offset(out.columns.route_arms, idx),
                scratch.route_arm_rows[idx].raw(),
            );
            idx += 1;
        }
        offset = out.columns.route_arms.end_offset();

        out.columns.passive_children = PackedColumn::new(offset, 0, ROLE_IMAGE_U16_STRIDE);

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

        out.columns.route_arm_lane_first_steps =
            PackedColumn::new(offset, route_arm_lane_step_len, ROLE_IMAGE_U16_STRIDE);
        out.columns.route_arm_lane_last_steps = PackedColumn::new(
            out.columns.route_arm_lane_first_steps.end_offset(),
            route_arm_lane_step_len,
            ROLE_IMAGE_U16_STRIDE,
        );
        idx = 0;
        while idx < route_arm_lane_step_len {
            let arm_row = idx / footprint.logical_lane_count;
            let lane = idx - arm_row.saturating_mul(footprint.logical_lane_count);
            let (first, last) = Self::route_arm_lane_step_bounds(scratch, arm_row, lane, local_len);
            out.write_u16(
                Self::row_offset(out.columns.route_arm_lane_first_steps, idx),
                first,
            );
            out.write_u16(
                Self::row_offset(out.columns.route_arm_lane_last_steps, idx),
                last,
            );
            idx += 1;
        }
        offset = out.columns.route_arm_lane_last_steps.end_offset();

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
