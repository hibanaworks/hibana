use super::super::{
    BlobPtr, LaneSetView, LaneStepLayout, LaneSteps, PackedLaneRange, PackedRollScopeRow,
    RoleCompiledCounts, RoleImageColumns, RoleImageRef, RoleLaneImage, RuntimeRoleFacts,
    RuntimeRoleFootprint,
};
use super::lane_image::invalid_resident_descriptor;
use crate::global::typestate::{LocalAction, LocalDependency, LocalNode, PackedEventConflict};

#[inline(always)]
const fn compact_count(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("role descriptor fact overflow");
    }
    value as u16
}

impl RuntimeRoleFacts {
    const MAX_ROUTE_COMMIT_COUNT: usize = 0;
    const LOCAL_STEP_COUNT: usize = 1;
    const ROUTE_SCOPE_COUNT: usize = 2;
    const ACTIVE_LANE_COUNT: usize = 3;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 4;
    const LOGICAL_LANE_COUNT: usize = 5;

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                compact_count(counts.max_route_commit_count),
                compact_count(counts.local_step_count),
                compact_count(counts.route_scope_count),
                compact_count(counts.active_lane_count),
                compact_count(counts.endpoint_lane_slot_count),
                compact_count(counts.logical_lane_count),
            ],
        }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RuntimeRoleFootprint {
        RuntimeRoleFootprint {
            max_route_commit_count: self.words[Self::MAX_ROUTE_COMMIT_COUNT] as usize,
            route_arm_state_capacity: 0,
            local_step_count: self.words[Self::LOCAL_STEP_COUNT] as usize,
            route_scope_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            active_lane_count: self.words[Self::ACTIVE_LANE_COUNT] as usize,
            endpoint_lane_slot_count: self.words[Self::ENDPOINT_LANE_SLOT_COUNT] as usize,
            logical_lane_count: self.words[Self::LOGICAL_LANE_COUNT] as usize,
        }
    }
}

impl RoleImageRef {
    #[inline(always)]
    pub(crate) const fn new<const N: usize>(
        program: &'static crate::global::compiled::images::CompiledProgramRef,
        role: u8,
        facts: RuntimeRoleFacts,
        columns: RoleImageColumns,
        bytes: &'static [u8; N],
        active_lane_row: PackedLaneRange,
        first_active_lane: u16,
    ) -> Self {
        let blob = BlobPtr::from_array(bytes, columns.blob_len());
        Self {
            program,
            role,
            facts,
            columns,
            blob,
            active_lane_row,
            first_active_lane,
        }
    }

    #[inline(always)]
    pub(crate) const fn lanes(&self) -> RoleLaneImage<'_> {
        RoleLaneImage::new(&self.columns, self.blob)
    }

    #[inline(always)]
    pub(crate) const fn footprint(&self) -> RuntimeRoleFootprint {
        let mut footprint = self.facts.footprint();
        footprint.route_arm_state_capacity = self.columns.route_arm_lane_step_rows.len as usize;
        footprint
    }

    #[inline(always)]
    pub(crate) const fn local_step_count(&self) -> usize {
        self.footprint().local_step_count
    }

    #[inline(always)]
    pub(crate) const fn active_lane_set(&self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.lanes()
            .lane_bit_view(self.active_lane_row, footprint.lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn resident_row_min_start(&self, idx: usize) -> Option<u16> {
        self.lanes().resident_row_min_start(idx)
    }

    pub(crate) const fn resident_row_lane_steps(
        &self,
        idx: usize,
        lane_idx: usize,
    ) -> Option<LaneSteps> {
        if lane_idx >= self.footprint().logical_lane_count {
            return None;
        }
        let lanes = self.lanes();
        if idx >= lanes.resident_row_count() {
            return None;
        }
        let row = lanes.resident_row_range(idx);
        if row.end() > self.local_step_count() {
            invalid_resident_descriptor();
        }
        let mut pos = row.start();
        let end = row.end();
        let mut first = usize::MAX;
        let mut len = 0usize;
        let mut layout = LaneStepLayout::Contiguous;
        while pos < end {
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
            invalid_resident_descriptor();
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
        self.lanes().dependency_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) const fn event_conflict_for_index(&self, current_idx: usize) -> PackedEventConflict {
        self.lanes().event_conflict_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        self.lanes().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_commit_range_by_slot(&self, slot: usize, arm: u8) -> PackedLaneRange {
        self.lanes().route_commit_range_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn route_commit_row_at(&self, idx: usize) -> PackedEventConflict {
        self.lanes().route_commit_row_at(idx)
    }

    #[inline(always)]
    pub(crate) const fn roll_scope_row(&self, slot: usize) -> Option<PackedRollScopeRow> {
        self.lanes().roll_scope_row(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_by_slot(
        &self,
        slot: usize,
    ) -> Option<crate::global::const_dsl::ScopeId> {
        self.lanes().route_scope_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_slot(
        &self,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<usize> {
        if self.columns.route_scopes.len as usize != self.footprint().route_scope_count {
            invalid_resident_descriptor();
        }
        self.lanes().route_scope_slot(scope)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_reentry_by_slot(&self, slot: usize) -> bool {
        self.lanes().route_scope_reentry_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn passive_arm_child_ordinal_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<u16> {
        self.lanes().passive_arm_child_ordinal_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn route_arm_event_row_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> PackedLaneRange {
        self.lanes().route_arm_event_row_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn local_step_lane(&self, step_idx: usize) -> Option<u8> {
        if step_idx >= self.local_step_count() {
            return None;
        }
        self.lanes()
            .local_step_lane(step_idx, self.footprint().logical_lane_count)
    }

    pub(crate) const fn local_step_node(&self, step_idx: usize) -> Option<LocalNode> {
        if step_idx >= self.local_step_count() {
            None
        } else {
            let Some(node) = self
                .lanes()
                .local_step_node(step_idx, self.role, self.program)
            else {
                return None;
            };
            let Some(expected_lane) = self.local_step_lane(step_idx) else {
                invalid_resident_descriptor();
            };
            let actual_lane = match node.action() {
                LocalAction::Send { lane, .. }
                | LocalAction::Recv { lane, .. }
                | LocalAction::Local { lane, .. } => lane,
                LocalAction::Terminate => invalid_resident_descriptor(),
            };
            if actual_lane != expected_lane {
                invalid_resident_descriptor();
            }
            Some(node)
        }
    }

    pub(crate) const fn resident_row_lane_step_at(
        &self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        if lane_idx >= self.footprint().logical_lane_count {
            return None;
        }
        let lanes = self.lanes();
        if idx >= lanes.resident_row_count() {
            return None;
        }
        let row = lanes.resident_row_range(idx);
        if row.end() > self.local_step_count() {
            invalid_resident_descriptor();
        }
        let mut pos = row.start();
        let end = row.end();
        let mut seen = 0usize;
        while pos < end {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                if seen == ordinal {
                    if pos > u16::MAX as usize {
                        invalid_resident_descriptor();
                    }
                    return Some(pos as u16);
                }
                seen += 1;
            }
            pos += 1;
        }
        None
    }

    pub(crate) const fn resident_row_lane_step_ordinal(
        &self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx >= self.footprint().logical_lane_count {
            return None;
        }
        let lanes = self.lanes();
        if idx >= lanes.resident_row_count() {
            return None;
        }
        let row = lanes.resident_row_range(idx);
        if row.end() > self.local_step_count() {
            invalid_resident_descriptor();
        }
        if step_idx < row.start() || step_idx >= row.end() {
            return None;
        }
        let mut pos = row.start();
        let end = row.end();
        let mut ordinal = 0usize;
        while pos < end {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                if pos == step_idx {
                    if ordinal > u16::MAX as usize {
                        invalid_resident_descriptor();
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
    pub(crate) const fn first_active_lane(&self) -> Option<usize> {
        if self.first_active_lane == u16::MAX {
            None
        } else {
            Some(self.first_active_lane as usize)
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> LaneSetView<'static> {
        self.lanes()
            .route_scope_arm_lane_set_by_slot(slot, arm, self.footprint().lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
    ) -> LaneSetView<'static> {
        self.lanes()
            .route_scope_offer_lane_set_by_slot(slot, self.footprint().lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn route_arm_lane_first_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.lanes().route_arm_lane_first_step_by_slot(
            slot,
            arm,
            lane,
            self.footprint().logical_lane_count,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_arm_lane_last_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.lanes().route_arm_lane_last_step_by_slot(
            slot,
            arm,
            lane,
            self.footprint().logical_lane_count,
        )
    }
}
