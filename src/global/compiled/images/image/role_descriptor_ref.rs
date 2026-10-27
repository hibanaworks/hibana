use super::CompiledProgramRef;
use crate::{
    endpoint::kernel::EndpointArenaLayout,
    global::{
        event_program::LocalEventProgram,
        role_program::{DENSE_LANE_ABSENT, DenseLaneOrdinal, RoleImageRef},
    },
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
    /// Cold construction of the exact logical views that can reside at this
    /// actor. This set allocates storage; it does not authorize ownership.
    pub(crate) fn logical_views(&self) -> LogicalViews {
        use crate::eff::EventPayload;
        let program = LocalEventProgram::from_rows(self.resident);
        let mut views = LogicalViews { words: [0; 8] };
        views.insert(self.role());
        let mut role = 0usize;
        while role <= self.program().facts.max_role as usize {
            if self.program().initial_actor_for(role as u8) == Some(self.role()) {
                views.insert(role as u8);
            }
            role += 1;
        }
        for step in 0..self.local_len() {
            let row = crate::invariant_some(program.event_row_at(step));
            let atom = row.atom();
            views.insert(row.logical_role());
            if let EventPayload::Endpoint(role) = atom.payload {
                views.insert(role);
            }
        }
        views
    }

    pub(crate) fn has_view_lane(&self, role: u8, lane: usize) -> bool {
        let program = LocalEventProgram::from_rows(self.resident);
        // Existing session membership uses lane zero even for a role with no
        // visible operations. It carries no additional protocol authority.
        if lane == 0 {
            return true;
        }
        for step in 0..self.local_len() {
            let row = crate::invariant_some(program.event_row_at(step));
            if row.atom().lane as usize == lane && row.logical_role() == role {
                return true;
            }
        }
        false
    }

    pub(crate) fn view_lane_slot_count(&self, role: u8) -> usize {
        let program = LocalEventProgram::from_rows(self.resident);
        let mut count = 1;
        for step in 0..self.local_len() {
            let row = crate::invariant_some(program.event_row_at(step));
            if row.logical_role() == role {
                count = count.max(row.atom().lane as usize + 1);
            }
        }
        count
    }

    pub(crate) fn all_view_lane_slots(&self) -> usize {
        self.logical_views()
            .roles()
            .map(|role| self.view_lane_slot_count(role))
            .sum()
    }
    #[inline(always)]
    pub(crate) const fn from_resident(image: &'static RoleImageRef) -> Self {
        Self { resident: image }
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> &'static CompiledProgramRef {
        self.resident.program()
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
        self.resident.role()
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.resident.footprint().local_step_count
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_blob_len(&self) -> usize {
        self.resident.columns().blob_len()
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_byte_at(&self, offset: usize) -> u8 {
        if offset >= self.proof_blob_len() {
            crate::invariant();
        }
        self.resident.blob().byte_at(offset)
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

    #[cfg(all(test, hibana_repo_tests))]
    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.footprint().endpoint_lane_slot_count
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.footprint().logical_lane_count
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> crate::endpoint::kernel::FrontierScratchLayout {
        crate::endpoint::kernel::FrontierScratchLayout::new(self.max_frontier_entries())
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.footprint().frontier_entry_count()
    }

    #[inline(always)]
    pub(crate) fn max_route_commit_count(&self) -> usize {
        self.footprint().max_route_commit_count
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

#[derive(Debug)]
pub(crate) struct LogicalViews {
    words: [u32; 8],
}

impl LogicalViews {
    pub(crate) fn contains(&self, role: u8) -> bool {
        self.words[role as usize / 32] & (1 << (role % 32)) != 0
    }

    fn insert(&mut self, role: u8) {
        self.words[role as usize / 32] |= 1 << (role % 32);
    }

    pub(crate) fn roles(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=u8::MAX).filter(|role| self.contains(*role))
    }

    pub(crate) fn len(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }
}
