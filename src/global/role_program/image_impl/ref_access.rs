use super::{
    CompiledProgramImage, LaneSetView, LaneSteps, LocalDependency, RoleCompiledCounts, RoleFacts,
    RoleFootprint, RoleImage, RoleImageRef, RoleImageSource, RoleLaneImage, lane_word_count,
};

impl RoleImage {
    #[inline(always)]
    pub(crate) const fn new(
        facts: RoleFacts,
        source: RoleImageSource,
        lanes: RoleLaneImage,
    ) -> Self {
        Self {
            facts,
            source,
            lanes,
        }
    }
}

impl RoleFacts {
    #[cfg(test)]
    const SCOPE_COUNT: usize = 0;
    #[cfg(test)]
    const MAX_ACTIVE_SCOPE_DEPTH: usize = 1;
    const MAX_ROUTE_STACK_DEPTH: usize = 2;
    #[cfg(test)]
    const EFF_COUNT: usize = 3;
    const LOCAL_STEP_COUNT: usize = 4;
    #[cfg(test)]
    const RESIDENT_ROW_COUNT: usize = 5;
    #[cfg(test)]
    const RESIDENT_ROW_LANE_ENTRY_COUNT: usize = 6;
    #[cfg(test)]
    const RESIDENT_ROW_LANE_WORD_COUNT: usize = 7;
    #[cfg(test)]
    const PARALLEL_ENTER_COUNT: usize = 8;
    const ROUTE_SCOPE_COUNT: usize = 9;
    const PASSIVE_LINGER_ROUTE_SCOPE_COUNT: usize = 10;
    const ACTIVE_LANE_COUNT: usize = 11;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 12;
    const LOGICAL_LANE_COUNT: usize = 13;

    #[inline(always)]
    const fn compact_count(value: usize) -> u16 {
        if value > u16::MAX as usize {
            panic!("role descriptor fact overflow");
        }
        value as u16
    }

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                Self::compact_count(counts.scope_count),
                Self::compact_count(counts.max_active_scope_depth),
                Self::compact_count(counts.max_route_stack_depth),
                Self::compact_count(counts.eff_count),
                Self::compact_count(counts.local_step_count),
                Self::compact_count(counts.resident_row_count),
                Self::compact_count(counts.resident_row_lane_entry_count),
                Self::compact_count(counts.resident_row_lane_word_count),
                Self::compact_count(counts.parallel_enter_count),
                Self::compact_count(counts.route_scope_count),
                Self::compact_count(counts.passive_linger_route_scope_count),
                Self::compact_count(counts.active_lane_count),
                Self::compact_count(counts.endpoint_lane_slot_count),
                Self::compact_count(counts.logical_lane_count),
            ],
        }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RoleFootprint {
        RoleFootprint {
            #[cfg(test)]
            scope_count: self.words[Self::SCOPE_COUNT] as usize,
            #[cfg(test)]
            max_active_scope_depth: self.words[Self::MAX_ACTIVE_SCOPE_DEPTH] as usize,
            max_route_stack_depth: self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
            #[cfg(test)]
            eff_count: self.words[Self::EFF_COUNT] as usize,
            #[cfg(test)]
            resident_row_count: self.words[Self::RESIDENT_ROW_COUNT] as usize,
            #[cfg(test)]
            resident_row_lane_entry_count: self.words[Self::RESIDENT_ROW_LANE_ENTRY_COUNT] as usize,
            #[cfg(test)]
            resident_row_lane_word_count: self.words[Self::RESIDENT_ROW_LANE_WORD_COUNT] as usize,
            #[cfg(test)]
            parallel_enter_count: self.words[Self::PARALLEL_ENTER_COUNT] as usize,
            route_scope_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            local_step_count: self.words[Self::LOCAL_STEP_COUNT] as usize,
            passive_linger_route_scope_count: self.words[Self::PASSIVE_LINGER_ROUTE_SCOPE_COUNT]
                as usize,
            active_lane_count: self.words[Self::ACTIVE_LANE_COUNT] as usize,
            endpoint_lane_slot_count: self.words[Self::ENDPOINT_LANE_SLOT_COUNT] as usize,
            logical_lane_count: self.words[Self::LOGICAL_LANE_COUNT] as usize,
            logical_lane_word_count: lane_word_count(self.words[Self::LOGICAL_LANE_COUNT] as usize),
            scope_evidence_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            frontier_entry_count: RoleFootprint::frontier_entry_count_for_route_depth(
                self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
            ),
        }
    }
}

impl RoleImageRef {
    #[inline(always)]
    pub(crate) const fn new(image: &'static RoleImage) -> Self {
        Self { image }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RoleFootprint {
        self.image.facts.footprint()
    }

    #[inline(always)]
    pub(crate) fn program_image(self) -> &'static CompiledProgramImage {
        self.image.source.program_image()
    }

    #[inline(always)]
    pub(crate) const fn active_lane_set(self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.image
            .lanes
            .active_lane_set(footprint.logical_lane_word_count)
    }

    #[inline(always)]
    pub(crate) const fn resident_row_min_start(self, idx: usize) -> Option<u16> {
        self.image.lanes.resident_row_min_start(idx)
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_steps(
        self,
        idx: usize,
        lane_idx: usize,
    ) -> Option<LaneSteps> {
        self.image.lanes.resident_row_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    pub(crate) const fn dependency_for_index(self, current_idx: usize) -> Option<LocalDependency> {
        self.image.lanes.dependency_for_index(current_idx)
    }

    #[inline(always)]
    pub(crate) const fn local_step_lane(self, step_idx: usize) -> Option<u8> {
        self.image.lanes.local_step_lane(step_idx)
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_step_at(
        self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        self.image
            .lanes
            .resident_row_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_step_ordinal(
        self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        self.image
            .lanes
            .resident_row_lane_step_ordinal(idx, lane_idx, step_idx)
    }

    #[inline(always)]
    pub(crate) const fn first_active_lane(self) -> Option<usize> {
        self.image.lanes.first_active_lane()
    }

    #[inline(always)]
    pub(crate) const fn route_scope_arm_lane_set_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.image.lanes.route_scope_arm_lane_set_by_slot(
            slot,
            arm,
            self.footprint().logical_lane_word_count,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_scope_offer_lane_set_by_slot(
        self,
        slot: usize,
    ) -> Option<LaneSetView<'static>> {
        self.image
            .lanes
            .route_scope_offer_lane_set_by_slot(slot, self.footprint().logical_lane_word_count)
    }
}
