#[cfg(test)]
use super::super::CompiledProgramImage;
use super::super::{
    LaneSetView, LaneSteps, PackedLaneRange, RoleCompiledCounts, RoleImageColumns, RoleImageRef,
    RoleImageSource, RoleLaneImage, RuntimeRoleFacts, RuntimeRoleFootprint,
};
#[cfg(test)]
use super::super::{RoleDebugFacts, RoleDebugFootprint};
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
    const PASSIVE_LINGER_ROUTE_SCOPE_COUNT: usize = 3;
    const ACTIVE_LANE_COUNT: usize = 4;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 5;
    const LOGICAL_LANE_COUNT: usize = 6;

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                compact_count(counts.max_route_stack_depth),
                compact_count(counts.local_step_count),
                compact_count(counts.route_scope_count),
                compact_count(counts.passive_linger_route_scope_count),
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
            passive_linger_route_scope_count: self.words[Self::PASSIVE_LINGER_ROUTE_SCOPE_COUNT]
                as usize,
            active_lane_count: self.words[Self::ACTIVE_LANE_COUNT] as usize,
            endpoint_lane_slot_count: self.words[Self::ENDPOINT_LANE_SLOT_COUNT] as usize,
            logical_lane_count: self.words[Self::LOGICAL_LANE_COUNT] as usize,
        }
    }
}

#[cfg(test)]
impl RoleDebugFacts {
    const SCOPE_COUNT: usize = 0;
    const MAX_ACTIVE_SCOPE_DEPTH: usize = 1;
    const EFF_COUNT: usize = 2;
    const RESIDENT_ROW_COUNT: usize = 3;
    const RESIDENT_ROW_LANE_ENTRY_COUNT: usize = 4;
    const RESIDENT_ROW_LANE_WORD_COUNT: usize = 5;
    const PARALLEL_ENTER_COUNT: usize = 6;

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                compact_count(counts.scope_count),
                compact_count(counts.max_active_scope_depth),
                compact_count(counts.eff_count),
                compact_count(counts.resident_row_count),
                compact_count(counts.resident_row_lane_entry_count),
                compact_count(counts.resident_row_lane_word_count),
                compact_count(counts.parallel_enter_count),
            ],
        }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RoleDebugFootprint {
        RoleDebugFootprint {
            scope_count: self.words[Self::SCOPE_COUNT] as usize,
            max_active_scope_depth: self.words[Self::MAX_ACTIVE_SCOPE_DEPTH] as usize,
            eff_count: self.words[Self::EFF_COUNT] as usize,
            resident_row_count: self.words[Self::RESIDENT_ROW_COUNT] as usize,
            resident_row_lane_entry_count: self.words[Self::RESIDENT_ROW_LANE_ENTRY_COUNT] as usize,
            resident_row_lane_word_count: self.words[Self::RESIDENT_ROW_LANE_WORD_COUNT] as usize,
            parallel_enter_count: self.words[Self::PARALLEL_ENTER_COUNT] as usize,
        }
    }
}

impl RoleImageRef {
    #[inline(always)]
    pub(crate) const fn new(
        program: crate::global::compiled::images::CompiledProgramRef,
        role: u8,
        facts: RuntimeRoleFacts,
        source: RoleImageSource,
        columns: RoleImageColumns,
        blob: &'static [u8],
        active_lane_row: PackedLaneRange,
        first_active_lane: u16,
    ) -> Self {
        let _ = source;
        Self {
            program,
            role,
            facts,
            #[cfg(test)]
            source,
            columns,
            blob,
            active_lane_row,
            first_active_lane,
        }
    }

    #[inline(always)]
    pub(crate) const fn lanes(self) -> RoleLaneImage {
        if self.blob.len() != self.columns.blob_len() {
            panic!("role image");
        }
        RoleLaneImage::new(
            self.columns,
            self.blob,
            self.active_lane_row,
            self.first_active_lane,
        )
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RuntimeRoleFootprint {
        self.facts.footprint()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn debug_footprint(self) -> RoleDebugFootprint {
        self.source.debug_facts().footprint()
    }

    #[inline(always)]
    pub(crate) const fn local_step_count(self) -> usize {
        self.footprint().local_step_count
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[inline(always)]
    pub(crate) const fn compact_blob_len(self) -> usize {
        self.blob.len()
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[inline(always)]
    pub(crate) const fn largest_section_bytes(self) -> usize {
        let columns = self.columns;
        let mut largest = columns.events.byte_len();
        if columns.lanes.byte_len() > largest {
            largest = columns.lanes.byte_len();
        }
        if columns.dependencies.byte_len() > largest {
            largest = columns.dependencies.byte_len();
        }
        if columns.conflicts.byte_len() > largest {
            largest = columns.conflicts.byte_len();
        }
        if columns.route_scopes.byte_len() > largest {
            largest = columns.route_scopes.byte_len();
        }
        if columns.route_scope_conflicts.byte_len() > largest {
            largest = columns.route_scope_conflicts.byte_len();
        }
        if columns.route_arms.byte_len() > largest {
            largest = columns.route_arms.byte_len();
        }
        if columns.resident_boundaries.byte_len() > largest {
            largest = columns.resident_boundaries.byte_len();
        }
        if columns.lane_bits.byte_len() > largest {
            largest = columns.lane_bits.byte_len();
        }
        if columns.route_arm_lane_rows.byte_len() > largest {
            largest = columns.route_arm_lane_rows.byte_len();
        }
        if columns.route_offer_lane_rows.byte_len() > largest {
            largest = columns.route_offer_lane_rows.byte_len();
        }
        if columns.route_arm_lane_step_rows.byte_len() > largest {
            largest = columns.route_arm_lane_step_rows.byte_len();
        }
        if columns.route_commit_ranges.byte_len() > largest {
            largest = columns.route_commit_ranges.byte_len();
        }
        if columns.route_commit_rows.byte_len() > largest {
            largest = columns.route_commit_rows.byte_len();
        }
        largest
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn program_image(self) -> &'static CompiledProgramImage {
        self.source.program_image()
    }

    #[inline(always)]
    pub(crate) const fn active_lane_set(self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.lanes().active_lane_set(footprint.lane_word_count())
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
    pub(crate) const fn route_scope_ordinal_by_slot(self, slot: usize) -> Option<u16> {
        self.lanes().route_scope_ordinal_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_linger_by_slot(self, slot: usize) -> bool {
        self.lanes().route_scope_linger_by_slot(slot)
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
        self.lanes().first_active_lane()
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
