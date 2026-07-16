use super::{
    super::{
        BlobPtr, ColumnRange, LaneSetView, PackedLaneRange, PackedLocalEventRow,
        PackedRollScopeRow, PackedRouteArmRow, ROLE_IMAGE_CONFLICT_STRIDE,
        ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_LANE_RANGE_STRIDE,
        ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROLL_SCOPE_STRIDE,
        ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE, ROLE_IMAGE_ROUTE_ARM_STRIDE,
        ROLE_IMAGE_ROUTE_SCOPE_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleLaneImage, RouteArmLaneStepRow,
    },
    route_arm_row_index,
};
use crate::global::const_dsl::ScopeId;
use crate::global::typestate::{
    LocalConflict, LocalDependency, PackedEventConflict, PackedLocalDependency,
};

mod decode;
pub(super) use decode::{
    decode_resident_local_step_lane, decode_resident_roll_scope,
    decode_resident_route_arm_lane_step, decode_resident_route_scope, route_commit_decisions_match,
};

#[cold]
#[inline(never)]
pub(super) const fn invalid_resident_descriptor() -> ! {
    crate::invariant()
}

impl<'a> RoleLaneImage<'a> {
    #[inline(always)]
    pub(crate) const fn new(columns: &'a super::super::RoleImageColumns, blob: BlobPtr) -> Self {
        Self { columns, blob }
    }

    #[inline(always)]
    const fn column_offset(&self, column: ColumnRange, row: usize, stride: usize) -> Option<usize> {
        if row >= column.len as usize {
            return None;
        }
        let offset = column.offset as usize + row * stride;
        if offset + stride > self.columns.blob_len() {
            invalid_resident_descriptor();
        }
        Some(offset)
    }

    #[inline(always)]
    const fn byte_at(&self, offset: usize) -> u8 {
        if offset >= self.columns.blob_len() {
            invalid_resident_descriptor();
        }
        self.blob.byte_at(offset)
    }

    #[inline(always)]
    const fn read_u8(&self, column: ColumnRange, row: usize, stride: usize) -> Option<u8> {
        match self.column_offset(column, row, stride) {
            Some(offset) => Some(self.byte_at(offset)),
            None => None,
        }
    }

    #[inline(always)]
    const fn read_u16_at(&self, offset: usize) -> u16 {
        self.byte_at(offset) as u16 | ((self.byte_at(offset + 1) as u16) << 8)
    }

    #[inline(always)]
    const fn read_u16(&self, column: ColumnRange, row: usize, stride: usize) -> Option<u16> {
        match self.column_offset(column, row, stride) {
            Some(offset) => Some(self.read_u16_at(offset)),
            None => None,
        }
    }

    #[inline(always)]
    const fn read_u32_at(&self, offset: usize) -> u32 {
        self.read_u16_at(offset) as u32 | ((self.read_u16_at(offset + 2) as u32) << 16)
    }

    #[inline(always)]
    const fn read_u32(&self, column: ColumnRange, row: usize, stride: usize) -> Option<u32> {
        match self.column_offset(column, row, stride) {
            Some(offset) => Some(self.read_u32_at(offset)),
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn local_step_event(&self, step_idx: usize) -> Option<PackedLocalEventRow> {
        match self.column_offset(self.columns.events, step_idx, ROLE_IMAGE_EVENT_STRIDE) {
            Some(offset) => Some(PackedLocalEventRow::from_packed_parts(
                self.read_u16_at(offset),
                self.read_u16_at(offset + 2),
                self.read_u16_at(offset + 4),
                self.read_u16_at(offset + 6),
                self.byte_at(offset + 8),
                self.byte_at(offset + 9),
            )),
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn local_step_lane(
        &self,
        step_idx: usize,
        logical_lane_count: usize,
    ) -> Option<u8> {
        match self.read_u8(self.columns.lanes, step_idx, ROLE_IMAGE_LANE_STRIDE) {
            Some(raw) => match decode_resident_local_step_lane(raw, logical_lane_count) {
                Some(lane) => Some(lane),
                None => invalid_resident_descriptor(),
            },
            None => None,
        }
    }

    #[inline]
    pub(super) const fn lane_bit_view(
        &self,
        range: PackedLaneRange,
        word_len: usize,
    ) -> LaneSetView<'static> {
        if range.is_absent_or_zero_len() {
            /* SAFETY: an empty descriptor lane set has no backing bytes; the
            word span is metadata used only to synthesize zero words. */
            unsafe { LaneSetView::from_bytes(core::ptr::null(), 0, word_len) }
        } else {
            if range.end() > self.columns.lane_bits.len as usize {
                invalid_resident_descriptor();
            }
            let offset = self.columns.lane_bits.offset as usize + range.start();
            if offset + range.len() > self.columns.blob_len() {
                invalid_resident_descriptor();
            }
            /* SAFETY: the column directory bounds above cover the immutable
            static descriptor allocation for the complete returned view. */
            unsafe {
                LaneSetView::from_bytes(self.blob.as_ptr().add(offset), range.len(), word_len)
            }
        }
    }

    #[inline(always)]
    pub(super) const fn resident_row_count(&self) -> usize {
        if self.columns.resident_boundaries.len == 0 {
            0
        } else {
            self.columns.resident_boundaries.len as usize - 1
        }
    }

    #[inline(always)]
    pub(super) const fn resident_row_min_start(&self, idx: usize) -> Option<u16> {
        if idx >= self.resident_row_count() {
            return None;
        }
        let row = self.resident_row_range(idx);
        Some(row.start() as u16)
    }

    #[inline(always)]
    pub(crate) const fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        let event = match self.local_step_event(current_idx) {
            Some(event) => event,
            None => invalid_resident_descriptor(),
        };
        if event.dependency_row == u16::MAX {
            return None;
        }
        let row = event.dependency_row as usize;
        if row >= self.columns.dependencies.len as usize {
            invalid_resident_descriptor();
        } else {
            match self
                .packed_dependency_row(row)
                .to_dependency(self.columns.events.len as usize)
            {
                Some(dependency) => Some(dependency),
                None => invalid_resident_descriptor(),
            }
        }
    }

    #[inline]
    pub(crate) const fn event_conflict_for_index(&self, current_idx: usize) -> PackedEventConflict {
        let event = match self.local_step_event(current_idx) {
            Some(event) => event,
            None => invalid_resident_descriptor(),
        };
        if event.conflict_row == u16::MAX {
            return PackedEventConflict::none();
        }
        let row = event.conflict_row as usize;
        if row >= self.columns.conflicts.len as usize {
            invalid_resident_descriptor();
        } else {
            match self.read_u16(self.columns.conflicts, row, ROLE_IMAGE_CONFLICT_STRIDE) {
                Some(raw) => {
                    let conflict = PackedEventConflict::from_raw(raw);
                    if conflict.is_none() {
                        invalid_resident_descriptor();
                    }
                    conflict
                }
                None => invalid_resident_descriptor(),
            }
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        match self.read_u16(
            self.columns.route_scope_conflicts,
            slot,
            ROLE_IMAGE_CONFLICT_STRIDE,
        ) {
            Some(raw) => PackedEventConflict::from_raw(raw),
            None => invalid_resident_descriptor(),
        }
    }

    pub(crate) const fn route_commit_range_by_slot(&self, slot: usize, arm: u8) -> PackedLaneRange {
        let row_idx = route_arm_row_index(slot, arm);
        let row = self.lane_range_row(self.columns.route_commit_ranges, row_idx);
        if row.is_zero_len() || row.end() > self.columns.route_commit_rows.len as usize {
            invalid_resident_descriptor();
        }
        let Some(scope) = self.route_scope_row(slot) else {
            invalid_resident_descriptor();
        };
        let mut expected = PackedEventConflict::route_arm(scope, arm);
        let start = row.start();
        let mut pos = row.end();
        while pos > start {
            pos -= 1;
            let current = self.route_commit_row_at(pos);
            if !route_commit_decisions_match(current, expected) {
                invalid_resident_descriptor();
            }
            let Some(LocalConflict::RouteArm { scope, .. }) = current.to_conflict() else {
                invalid_resident_descriptor();
            };
            let parent = self.route_commit_parent(scope);
            if pos == start {
                if !parent.is_none() {
                    invalid_resident_descriptor();
                }
                return row;
            }
            if parent.is_none() {
                invalid_resident_descriptor();
            }
            expected = parent;
        }
        invalid_resident_descriptor()
    }

    #[inline(always)]
    pub(crate) const fn route_commit_row_at(&self, idx: usize) -> PackedEventConflict {
        match self.read_u16(
            self.columns.route_commit_rows,
            idx,
            ROLE_IMAGE_CONFLICT_STRIDE,
        ) {
            Some(raw) => PackedEventConflict::from_raw(raw),
            None => invalid_resident_descriptor(),
        }
    }

    #[inline(always)]
    pub(crate) const fn roll_scope_row(&self, slot: usize) -> Option<PackedRollScopeRow> {
        match self.column_offset(self.columns.roll_scopes, slot, ROLE_IMAGE_ROLL_SCOPE_STRIDE) {
            Some(offset) => {
                let Some(_) = decode_resident_roll_scope(self.read_u16_at(offset)) else {
                    invalid_resident_descriptor();
                };
                let row = PackedRollScopeRow::from_packed_parts(
                    self.read_u16_at(offset),
                    self.read_u32_at(offset + 2),
                );
                let event_row = row.event_row();
                if event_row.is_zero_len() || event_row.end() > self.columns.events.len as usize {
                    invalid_resident_descriptor();
                }
                Some(row)
            }
            None => None,
        }
    }

    #[inline(always)]
    const fn route_scope_row(&self, slot: usize) -> Option<ScopeId> {
        match self.column_offset(
            self.columns.route_scopes,
            slot,
            ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
        ) {
            Some(offset) => match decode_resident_route_scope(self.read_u16_at(offset)) {
                Some(scope) => Some(scope),
                None => invalid_resident_descriptor(),
            },
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_by_slot(&self, slot: usize) -> Option<ScopeId> {
        self.route_scope_row(slot)
    }

    pub(crate) const fn route_scope_slot(&self, scope: ScopeId) -> Option<usize> {
        let mut found = usize::MAX;
        let mut slot = 0usize;
        while slot < self.columns.route_scopes.len as usize {
            let Some(candidate) = self.route_scope_row(slot) else {
                invalid_resident_descriptor();
            };
            if candidate.same(scope) {
                if found != usize::MAX {
                    invalid_resident_descriptor();
                }
                found = slot;
            }
            slot += 1;
        }
        if found == usize::MAX {
            None
        } else {
            Some(found)
        }
    }

    const fn route_commit_parent(&self, scope: ScopeId) -> PackedEventConflict {
        let Some(slot) = self.route_scope_slot(scope) else {
            invalid_resident_descriptor();
        };
        let parent = self.route_scope_conflict_by_slot(slot);
        match parent.to_conflict() {
            Some(LocalConflict::RouteArm {
                scope: parent_scope,
                ..
            }) if parent_scope.same(scope) => PackedEventConflict::none(),
            Some(_) | None => parent,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_reentry_by_slot(&self, slot: usize) -> bool {
        if self.route_scope_by_slot(slot).is_none() {
            invalid_resident_descriptor();
        }
        self.route_scope_conflict_by_slot(slot).route_reentry()
    }

    #[inline]
    const fn packed_route_arm_row(&self, row_idx: usize) -> PackedRouteArmRow {
        match self.column_offset(
            self.columns.route_arms,
            row_idx,
            ROLE_IMAGE_ROUTE_ARM_STRIDE,
        ) {
            Some(offset) => {
                let row = PackedRouteArmRow::from_packed_parts(
                    self.read_u32_at(offset),
                    self.read_u32_at(offset + 4),
                );
                if row.is_empty() {
                    invalid_resident_descriptor();
                }
                row
            }
            None => invalid_resident_descriptor(),
        }
    }

    #[inline]
    const fn route_arm_row(&self, row_idx: usize) -> PackedRouteArmRow {
        let row = self.packed_route_arm_row(row_idx);
        let event_row = row.event_row();
        let mut lane_step_start = 0usize;
        let mut idx = 0usize;
        while idx < row_idx {
            lane_step_start += self.packed_route_arm_row(idx).lane_step_len();
            idx += 1;
        }
        let lane_step_end = lane_step_start + row.lane_step_len();
        if event_row.end() > self.columns.events.len as usize
            || (event_row.is_zero_len() != (row.lane_step_len() == 0))
            || lane_step_end > self.columns.route_arm_lane_step_rows.len as usize
        {
            invalid_resident_descriptor();
        }
        row
    }

    #[inline(always)]
    const fn packed_dependency_row(&self, row: usize) -> PackedLocalDependency {
        match self.column_offset(self.columns.dependencies, row, ROLE_IMAGE_DEPENDENCY_STRIDE) {
            Some(offset) => PackedLocalDependency::from_packed_parts(
                self.read_u16_at(offset),
                self.read_u16_at(offset + 2),
                self.read_u16_at(offset + 4),
                self.read_u16_at(offset + 6),
            ),
            None => invalid_resident_descriptor(),
        }
    }

    pub(crate) const fn passive_arm_child_ordinal_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<u16> {
        let row_idx = route_arm_row_index(slot, arm);
        let Some(parent_scope) = self.route_scope_by_slot(slot) else {
            invalid_resident_descriptor();
        };
        match self.route_arm_row(row_idx).child_slot() {
            Some(child_slot) => {
                let child_slot = child_slot as usize;
                if child_slot <= slot {
                    invalid_resident_descriptor();
                }
                let Some(scope) = self.route_scope_by_slot(child_slot) else {
                    invalid_resident_descriptor();
                };
                if scope.same(parent_scope) {
                    invalid_resident_descriptor();
                }
                match self.route_scope_conflict_by_slot(child_slot).to_conflict() {
                    Some(LocalConflict::RouteArm {
                        scope: recorded_parent,
                        arm: recorded_arm,
                    }) if recorded_parent.same(parent_scope) && recorded_arm == arm => {}
                    Some(_) | None => invalid_resident_descriptor(),
                }
                Some(scope.local_ordinal())
            }
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_arm_event_row_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> PackedLaneRange {
        let row_idx = route_arm_row_index(slot, arm);
        self.route_arm_row(row_idx).event_row()
    }

    #[inline(always)]
    const fn boundary_at(&self, idx: usize) -> u16 {
        match self.read_u16(self.columns.resident_boundaries, idx, ROLE_IMAGE_U16_STRIDE) {
            Some(value) => value,
            None => invalid_resident_descriptor(),
        }
    }

    #[inline(always)]
    pub(super) const fn resident_row_range(&self, idx: usize) -> PackedLaneRange {
        if idx >= self.resident_row_count() {
            invalid_resident_descriptor();
        }
        let start = self.boundary_at(idx) as usize;
        let end = self.boundary_at(idx + 1) as usize;
        if start >= end || end > self.columns.lanes.len as usize {
            invalid_resident_descriptor();
        }
        PackedLaneRange::new(start, end - start)
    }

    #[inline(always)]
    const fn lane_range_row(&self, column: ColumnRange, row_idx: usize) -> PackedLaneRange {
        match self.read_u32(column, row_idx, ROLE_IMAGE_LANE_RANGE_STRIDE) {
            Some(raw) => {
                let row = PackedLaneRange::from_raw(raw);
                if row.is_empty() {
                    invalid_resident_descriptor();
                }
                row
            }
            None => invalid_resident_descriptor(),
        }
    }

    #[inline(always)]
    pub(super) const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane_word_count: usize,
    ) -> LaneSetView<'static> {
        let row_idx = route_arm_row_index(slot, arm);
        let row = self.lane_range_row(self.columns.route_arm_lane_rows, row_idx);
        self.lane_bit_view(row, lane_word_count)
    }

    #[inline(always)]
    pub(super) const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
        lane_word_count: usize,
    ) -> LaneSetView<'static> {
        let row = self.lane_range_row(self.columns.route_offer_lane_rows, slot);
        self.lane_bit_view(row, lane_word_count)
    }

    const fn route_arm_lane_step_row_at(
        &self,
        row: usize,
        logical_lane_count: usize,
        event_row: PackedLaneRange,
    ) -> RouteArmLaneStepRow {
        match self.column_offset(
            self.columns.route_arm_lane_step_rows,
            row,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        ) {
            Some(offset) => match decode_resident_route_arm_lane_step(
                RouteArmLaneStepRow::from_packed_parts(
                    self.byte_at(offset),
                    self.read_u16_at(offset + 1),
                    self.read_u16_at(offset + 3),
                ),
                logical_lane_count,
                event_row,
            ) {
                Some(row) => row,
                None => invalid_resident_descriptor(),
            },
            None => invalid_resident_descriptor(),
        }
    }

    const fn route_arm_lane_step_row(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
        logical_lane_count: usize,
    ) -> Option<RouteArmLaneStepRow> {
        let arm_row_idx = route_arm_row_index(slot, arm);
        if lane as usize >= logical_lane_count {
            return None;
        }
        let arm_row = self.route_arm_row(arm_row_idx);
        let range = self.route_arm_lane_step_range(arm_row_idx);
        let event_row = arm_row.event_row();
        if range.end() > self.columns.route_arm_lane_step_rows.len as usize {
            invalid_resident_descriptor();
        }
        let mut pos = range.start();
        let end = range.end();
        while pos < end {
            let row = self.route_arm_lane_step_row_at(pos, logical_lane_count, event_row);
            if row.lane() == lane {
                return Some(row);
            }
            pos += 1;
        }
        None
    }

    const fn route_arm_lane_step_range(&self, row_idx: usize) -> PackedLaneRange {
        let mut start = 0usize;
        let mut idx = 0usize;
        while idx < row_idx {
            start += self.packed_route_arm_row(idx).lane_step_len();
            idx += 1;
        }
        let len = self.packed_route_arm_row(row_idx).lane_step_len();
        let row = PackedLaneRange::new(start, len);
        if row.end() > self.columns.route_arm_lane_step_rows.len as usize {
            invalid_resident_descriptor();
        }
        row
    }

    #[inline(always)]
    pub(super) const fn route_arm_lane_first_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
        logical_lane_count: usize,
    ) -> Option<u16> {
        let row = match self.route_arm_lane_step_row(slot, arm, lane, logical_lane_count) {
            Some(row) => row,
            None => return None,
        };
        Some(row.first_step())
    }

    #[inline(always)]
    pub(super) const fn route_arm_lane_last_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
        logical_lane_count: usize,
    ) -> Option<u16> {
        let row = match self.route_arm_lane_step_row(slot, arm, lane, logical_lane_count) {
            Some(row) => row,
            None => return None,
        };
        Some(row.last_step())
    }
}
