use super::super::{
    BlobPtr, LaneSetView, LaneSteps, PackedLaneRange, PackedRollScopeRow, RoleCompiledCounts,
    RoleImageColumns, RoleImageRef, RoleLaneImage, RuntimeRoleFacts, RuntimeRoleFootprint,
};
use crate::global::typestate::{LocalDependency, LocalNode, PackedEventConflict};

#[inline(always)]
const fn compact_count(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("role descriptor fact overflow");
    }
    value as u16
}

impl RuntimeRoleFacts {
    const MAX_ROUTE_STACK_DEPTH: usize = 0;
    const LOCAL_STEP_COUNT: usize = 1;
    const ROUTE_SCOPE_COUNT: usize = 2;
    const ACTIVE_LANE_COUNT: usize = 3;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 4;
    const LOGICAL_LANE_COUNT: usize = 5;

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                compact_count(counts.max_route_stack_depth),
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
            max_route_stack_depth: self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
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
    pub(crate) const fn lanes(self) -> RoleLaneImage {
        RoleLaneImage::new(self.columns, self.blob)
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RuntimeRoleFootprint {
        self.facts.footprint()
    }

    #[inline(always)]
    pub(crate) const fn local_step_count(self) -> usize {
        self.footprint().local_step_count
    }

    #[inline(always)]
    pub(crate) const fn active_lane_set(self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.lanes()
            .lane_bit_view(self.active_lane_row, footprint.lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn resident_row_min_start(self, idx: usize) -> Option<u16> {
        self.lanes().resident_row_min_start(idx)
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_steps(
        self,
        idx: usize,
        lane_idx: usize,
    ) -> Option<LaneSteps> {
        self.lanes().resident_row_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    pub(crate) const fn dependency_for_index(self, current_idx: usize) -> Option<LocalDependency> {
        self.lanes().dependency_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) const fn event_conflict_for_index(self, current_idx: usize) -> PackedEventConflict {
        self.lanes().event_conflict_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_conflict_by_slot(self, slot: usize) -> PackedEventConflict {
        self.lanes().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_commit_range_by_slot(self, slot: usize, arm: u8) -> PackedLaneRange {
        self.lanes().route_commit_range_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn route_commit_row_at(self, idx: usize) -> PackedEventConflict {
        self.lanes().route_commit_row_at(idx)
    }

    #[inline(always)]
    pub(crate) const fn roll_scope_row(self, slot: usize) -> Option<PackedRollScopeRow> {
        self.lanes().roll_scope_row(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_ordinal_by_slot(self, slot: usize) -> Option<u16> {
        self.lanes().route_scope_ordinal_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_reentry_by_slot(self, slot: usize) -> bool {
        self.lanes().route_scope_reentry_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn passive_arm_child_ordinal_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<u16> {
        self.lanes().passive_arm_child_ordinal_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn route_arm_event_row_by_slot(self, slot: usize, arm: u8) -> PackedLaneRange {
        self.lanes().route_arm_event_row_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn local_step_lane(self, step_idx: usize) -> Option<u8> {
        self.lanes().local_step_lane(step_idx)
    }

    #[inline(always)]
    pub(crate) const fn local_step_node(self, step_idx: usize) -> Option<LocalNode> {
        if step_idx >= self.local_step_count() {
            None
        } else {
            self.lanes()
                .local_step_node(step_idx, self.role, self.program)
        }
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_step_at(
        self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        self.lanes()
            .resident_row_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_step_ordinal(
        self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        self.lanes()
            .resident_row_lane_step_ordinal(idx, lane_idx, step_idx)
    }

    #[inline(always)]
    pub(crate) const fn first_active_lane(self) -> Option<usize> {
        if self.first_active_lane == u16::MAX {
            None
        } else {
            Some(self.first_active_lane as usize)
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_arm_lane_set_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.lanes()
            .route_scope_arm_lane_set_by_slot(slot, arm, self.footprint().lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn route_scope_offer_lane_set_by_slot(
        self,
        slot: usize,
    ) -> Option<LaneSetView<'static>> {
        self.lanes()
            .route_scope_offer_lane_set_by_slot(slot, self.footprint().lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn route_arm_lane_first_step_by_slot(
        self,
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
        self,
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
