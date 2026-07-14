use super::CompiledProgramRef;
use crate::{
    endpoint::kernel::EndpointArenaLayout,
    global::role_program::{DENSE_LANE_ABSENT, DenseLaneOrdinal, RoleImageRef, lane_word_count},
};

#[derive(Clone, Copy)]
pub(crate) struct RoleDescriptorRef {
    resident: &'static RoleImageRef,
}

impl core::fmt::Debug for RoleDescriptorRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RoleDescriptorRef")
            .field("program", &self.program())
            .field("role", &self.role())
            .finish()
    }
}

impl RoleDescriptorRef {
    #[inline(always)]
    pub(crate) const fn from_resident(image: &'static RoleImageRef) -> Self {
        Self { resident: image }
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> &'static CompiledProgramRef {
        self.resident.program
    }

    #[inline(always)]
    pub(crate) const fn local_event_rows(
        &self,
    ) -> &'static crate::global::role_program::RoleImageRef {
        self.resident
    }

    #[inline(always)]
    fn footprint(&self) -> crate::global::role_program::RuntimeRoleFootprint {
        self.resident.footprint()
    }

    #[inline(always)]
    fn endpoint_layout_footprint(&self) -> crate::global::role_program::RuntimeRoleFootprint {
        self.footprint()
    }

    #[inline(always)]
    pub(crate) fn role(&self) -> u8 {
        self.resident.role
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.resident.footprint().local_step_count
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_blob_len(&self) -> usize {
        self.resident.columns.blob_len()
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_byte_at(&self, offset: usize) -> u8 {
        if offset >= self.proof_blob_len() {
            crate::invariant();
        }
        self.resident.blob.byte_at(offset)
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        if lane_idx >= self.logical_lane_count() {
            return false;
        }
        self.resident.active_lane_set().contains(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        self.resident.first_active_lane()
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.footprint().endpoint_lane_slot_count.max(1)
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.footprint()
            .logical_lane_count
            .max(self.endpoint_lane_slot_count())
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> crate::endpoint::kernel::FrontierScratchLayout {
        crate::endpoint::kernel::FrontierScratchLayout::new(
            self.max_frontier_entries(),
            lane_word_count(self.logical_lane_count()),
        )
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.footprint().frontier_entry_count()
    }

    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.footprint().max_route_stack_depth
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.footprint().route_scope_count
    }

    #[inline(always)]
    pub(crate) fn fill_active_lane_dense_by_lane(&self, dst: &mut [DenseLaneOrdinal]) -> usize {
        dst.fill(DENSE_LANE_ABSENT);
        let active = self.resident.active_lane_set();
        let mut dense = 0usize;
        let mut next = active.first_set(dst.len());
        while let Some(lane_idx) = next {
            dst[lane_idx] = crate::invariant_some(DenseLaneOrdinal::new(dense));
            dense += 1;
            next = active.next_set_from(lane_idx + 1, dst.len());
        }
        dense
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout(&self) -> EndpointArenaLayout {
        EndpointArenaLayout::from_footprint(self.endpoint_layout_footprint())
    }
}
