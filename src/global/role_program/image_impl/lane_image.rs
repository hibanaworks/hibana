use super::super::{
    BlobPtr, ColumnRange, LaneSetView, LaneStepLayout, LaneSteps, PackedLaneRange,
    PackedLocalEventRow, PackedRollScopeRow, PackedRouteArmRow, ROLE_IMAGE_CONFLICT_STRIDE,
    ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_LANE_RANGE_STRIDE,
    ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROLL_SCOPE_STRIDE, ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_ROUTE_SCOPE_STRIDE, ROLE_IMAGE_U16_STRIDE,
    RoleLaneImage, RouteArmLaneStepRow,
};
use crate::global::const_dsl::ScopeId;
use crate::global::typestate::{LocalDependency, PackedEventConflict, PackedLocalDependency};

impl RoleLaneImage {
    #[inline(always)]
    pub(crate) const fn new(columns: super::super::RoleImageColumns, blob: BlobPtr) -> Self {
        Self { columns, blob }
    }

    #[inline(always)]
    const fn column_offset(&self, column: ColumnRange, row: usize, stride: usize) -> Option<usize> {
        if row >= column.len as usize {
            return None;
        }
        let offset = column.offset as usize + row * stride;
        if offset + stride > self.columns.blob_len() {
            crate::invariant();
        }
        Some(offset)
    }

    #[inline(always)]
    const fn byte_at(&self, offset: usize) -> u8 {
        if offset >= self.columns.blob_len() {
            crate::invariant();
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
    pub(crate) const fn local_step_lane(&self, step_idx: usize) -> Option<u8> {
        self.read_u8(self.columns.lanes, step_idx, ROLE_IMAGE_LANE_STRIDE)
    }

    #[inline(always)]
    pub(super) const fn lane_bit_view(
        &self,
        range: PackedLaneRange,
        word_len: usize,
    ) -> LaneSetView<'static> {
        if range.is_absent_or_zero_len() {
            LaneSetView::from_bytes(core::ptr::null(), 0, word_len)
        } else {
            if range.end() > self.columns.lane_bits.len as usize {
                crate::invariant();
            }
            let offset = self.columns.lane_bits.offset as usize + range.start();
            if offset + range.len() > self.columns.blob_len() {
                crate::invariant();
            }
            LaneSetView::from_bytes(
                /* SAFETY: the column directory bounds above cover the backing allocation. */
                unsafe { self.blob.as_ptr().add(offset) },
                range.len(),
                word_len,
            )
        }
    }

    #[inline(always)]
    const fn resident_row_count(&self) -> usize {
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
        if row.is_absent_or_zero_len() {
            None
        } else if row.start() > u16::MAX as usize {
            crate::invariant();
        } else {
            Some(row.start() as u16)
        }
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_steps(
        &self,
        idx: usize,
        lane_idx: usize,
    ) -> Option<LaneSteps> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.resident_row_count() {
            return None;
        }
        let row = self.resident_row_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut first = usize::MAX;
        let mut len = 0usize;
        let mut layout = LaneStepLayout::Contiguous;
        while pos < end && pos < self.columns.lanes.len as usize {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                if first == usize::MAX {
                    first = pos;
                } else if pos != first + len {
                    layout = LaneStepLayout::Sparse;
                }
                len += 1;
            }
            pos += 1;
        }
        if len == 0 {
            None
        } else if first > u16::MAX as usize || len > u16::MAX as usize {
            crate::invariant();
        } else {
            Some(LaneSteps {
                start: first as u16,
                len: len as u16,
                layout,
            })
        }
    }

    #[inline(always)]
    pub(crate) const fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        let event = match self.local_step_event(current_idx) {
            Some(event) => event,
            None => return None,
        };
        if event.dependency_row == u16::MAX {
            return None;
        }
        let row = event.dependency_row as usize;
        if row >= self.columns.dependencies.len as usize {
            crate::invariant();
        } else {
            match self.packed_dependency_row(row) {
                Some(row) => row.to_dependency(),
                None => crate::invariant(),
            }
        }
    }

    #[inline(always)]
    pub(crate) const fn event_conflict_for_index(&self, current_idx: usize) -> PackedEventConflict {
        let event = match self.local_step_event(current_idx) {
            Some(event) => event,
            None => return PackedEventConflict::none(),
        };
        if event.conflict_row == u16::MAX {
            return PackedEventConflict::none();
        }
        let row = event.conflict_row as usize;
        if row >= self.columns.conflicts.len as usize {
            crate::invariant();
        } else {
            match self.read_u16(self.columns.conflicts, row, ROLE_IMAGE_CONFLICT_STRIDE) {
                Some(raw) => PackedEventConflict::from_raw(raw),
                None => crate::invariant(),
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
            None => PackedEventConflict::none(),
        }
    }

    #[inline(always)]
    pub(crate) const fn route_commit_range_by_slot(&self, slot: usize, arm: u8) -> PackedLaneRange {
        if arm >= 2 {
            return PackedLaneRange::EMPTY;
        }
        let row_idx = slot * 2 + arm as usize;
        self.lane_range_row(self.columns.route_commit_ranges, row_idx)
    }

    #[inline(always)]
    pub(crate) const fn route_commit_row_at(&self, idx: usize) -> PackedEventConflict {
        match self.read_u16(
            self.columns.route_commit_rows,
            idx,
            ROLE_IMAGE_CONFLICT_STRIDE,
        ) {
            Some(raw) => PackedEventConflict::from_raw(raw),
            None => PackedEventConflict::none(),
        }
    }

    #[inline(always)]
    pub(crate) const fn roll_scope_row(&self, slot: usize) -> Option<PackedRollScopeRow> {
        match self.column_offset(self.columns.roll_scopes, slot, ROLE_IMAGE_ROLL_SCOPE_STRIDE) {
            Some(offset) => {
                let row = PackedRollScopeRow::from_packed_parts(
                    self.read_u16_at(offset),
                    self.read_u32_at(offset + 2),
                );
                if row.is_empty() { None } else { Some(row) }
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
            Some(offset) => Some(ScopeId::from_raw(self.read_u16_at(offset))),
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_by_slot(&self, slot: usize) -> Option<ScopeId> {
        let row = match self.route_scope_row(slot) {
            Some(row) => row,
            None => return None,
        };
        if row.is_none() { None } else { Some(row) }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_reentry_by_slot(&self, slot: usize) -> bool {
        if self.route_scope_by_slot(slot).is_none() {
            return false;
        }
        self.route_scope_conflict_by_slot(slot).route_reentry()
    }

    #[inline(always)]
    const fn route_arm_row(&self, row_idx: usize) -> PackedRouteArmRow {
        match self.column_offset(
            self.columns.route_arms,
            row_idx,
            ROLE_IMAGE_ROUTE_ARM_STRIDE,
        ) {
            Some(offset) => PackedRouteArmRow::from_packed_parts(
                self.read_u32_at(offset),
                self.read_u32_at(offset + 4),
            ),
            None => panic!("role image"),
        }
    }

    #[inline(always)]
    const fn packed_dependency_row(&self, row: usize) -> Option<PackedLocalDependency> {
        match self.column_offset(self.columns.dependencies, row, ROLE_IMAGE_DEPENDENCY_STRIDE) {
            Some(offset) => Some(PackedLocalDependency::from_packed_parts(
                self.read_u16_at(offset),
                self.read_u16_at(offset + 2),
                self.read_u16_at(offset + 4),
                self.read_u16_at(offset + 6),
            )),
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn passive_arm_child_ordinal_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<u16> {
        if arm >= 2 {
            return None;
        }
        let row_idx = slot * 2 + arm as usize;
        match self.route_arm_row(row_idx).child_slot_delta() {
            Some(delta) => match self.route_scope_by_slot(slot + delta as usize) {
                Some(scope) => Some(scope.local_ordinal()),
                None => None,
            },
            None => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_arm_event_row_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> PackedLaneRange {
        if arm >= 2 {
            return PackedLaneRange::EMPTY;
        }
        let row_idx = slot * 2 + arm as usize;
        self.route_arm_row(row_idx).event_row()
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_step_at(
        &self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.resident_row_count() {
            return None;
        }
        let row = self.resident_row_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut seen = 0usize;
        while pos < end && pos < self.columns.lanes.len as usize {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                if seen == ordinal {
                    if pos > u16::MAX as usize {
                        crate::invariant();
                    }
                    return Some(pos as u16);
                }
                seen += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    pub(super) const fn resident_row_lane_step_ordinal(
        &self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.resident_row_count() {
            return None;
        }
        let row = self.resident_row_range(idx);
        if step_idx < row.start()
            || step_idx >= row.end()
            || step_idx >= self.columns.lanes.len as usize
        {
            return None;
        }
        let mut pos = row.start();
        let end = row.end();
        let mut ordinal = 0usize;
        while pos < end && pos < self.columns.lanes.len as usize {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                if pos == step_idx {
                    if ordinal > u16::MAX as usize {
                        crate::invariant();
                    }
                    return Some(ordinal as u16);
                }
                ordinal += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    const fn boundary_at(&self, idx: usize) -> u16 {
        match self.read_u16(self.columns.resident_boundaries, idx, ROLE_IMAGE_U16_STRIDE) {
            Some(value) => value,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    const fn resident_row_range(&self, idx: usize) -> PackedLaneRange {
        if idx >= self.resident_row_count() {
            return PackedLaneRange::EMPTY;
        }
        let start = self.boundary_at(idx) as usize;
        let end = self.boundary_at(idx + 1) as usize;
        PackedLaneRange::new(start, end - start)
    }

    #[inline(always)]
    const fn lane_range_row(&self, column: ColumnRange, row_idx: usize) -> PackedLaneRange {
        match self.read_u32(column, row_idx, ROLE_IMAGE_LANE_RANGE_STRIDE) {
            Some(raw) => PackedLaneRange::from_raw(raw),
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(super) const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane_word_count: usize,
    ) -> Option<LaneSetView<'static>> {
        if arm >= 2 {
            return None;
        }
        let row_idx = slot * 2 + arm as usize;
        let row = self.lane_range_row(self.columns.route_arm_lane_rows, row_idx);
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, lane_word_count))
    }

    #[inline(always)]
    pub(super) const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
        lane_word_count: usize,
    ) -> Option<LaneSetView<'static>> {
        let row = self.lane_range_row(self.columns.route_offer_lane_rows, slot);
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, lane_word_count))
    }

    #[inline(always)]
    const fn route_arm_lane_step_row_at(&self, row: usize) -> Option<RouteArmLaneStepRow> {
        match self.column_offset(
            self.columns.route_arm_lane_step_rows,
            row,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        ) {
            Some(offset) => Some(RouteArmLaneStepRow::new(
                self.byte_at(offset),
                self.read_u16_at(offset + 1) as usize,
                self.read_u16_at(offset + 3) as usize,
            )),
            None => None,
        }
    }

    #[inline(always)]
    const fn route_arm_lane_step_row(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
        logical_lane_count: usize,
    ) -> Option<RouteArmLaneStepRow> {
        if arm >= 2 || lane as usize >= logical_lane_count {
            return None;
        }
        let arm_row_idx = slot * 2 + arm as usize;
        let range = self.route_arm_row(arm_row_idx).lane_step_row();
        if range.end() > self.columns.route_arm_lane_step_rows.len as usize {
            crate::invariant();
        }
        let mut pos = range.start();
        let end = range.end();
        while pos < end {
            let Some(row) = self.route_arm_lane_step_row_at(pos) else {
                return None;
            };
            if row.lane() == lane {
                return Some(row);
            }
            pos += 1;
        }
        None
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
