use super::{
    AssocTable, CapTable, Clock, ControlScopeKind, EndpointLeaseId, EndpointLeaseSlot,
    FREE_REGION_CAPACITY, FreeRegion, GenTable, LabelUniverse, Lane, LoopTable, PolicyTable,
    Rendezvous, RouteTable, StateSnapshotTable, TopologyStateTable, Transport,
};
mod capacity;

// # Unsafe Owner Contract
//
// This file owns rendezvous slab layout, sidecar allocation, migration, and
// resident table binding. The slab pointer and endpoint-lease table are created
// by the rendezvous constructor and remain pinned for the rendezvous lifetime.
// Every raw allocation returned here is aligned, range-checked against the
// current slab frontier, and recorded in rendezvous-owned metadata before typed
// owners bind to it. Migration paths copy initialized table entries into newly
// allocated sidecar storage before rebinding, and old regions are tracked only
// as rendezvous-local free regions for later resident allocation.

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[inline(always)]
    pub(crate) const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    pub(crate) const fn align_down(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        value & !mask
    }

    #[inline(always)]
    pub(crate) const fn frontier_workspace_guard_bytes(
        layout: crate::endpoint::kernel::FrontierScratchLayout,
    ) -> usize {
        layout
            .total_bytes()
            .saturating_add(layout.total_align().saturating_sub(1))
    }

    #[inline]
    pub(crate) fn slab_ptr_and_len(&self) -> (*mut u8, usize) {
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            let slab = &mut *self.slab;
            (slab.as_mut_ptr(), slab.len())
        }
    }

    #[inline]
    fn endpoint_storage_floor(&self) -> usize {
        let (_, slab_len) = self.slab_ptr_and_len();
        let mut floor = slab_len;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied && slot.len != 0 && (slot.offset as usize) < floor {
                floor = slot.offset as usize;
            }
            idx += 1;
        }
        floor
    }

    #[inline]
    pub(crate) fn endpoint_lease_floor(&self) -> usize {
        (self.image_frontier as usize).saturating_add(self.frontier_workspace_bytes as usize)
    }

    #[cfg(test)]
    #[inline]
    fn update_runtime_frontier(&mut self) {
        let frontier = self
            .image_frontier
            .saturating_add(self.frontier_workspace_bytes);
        if frontier > self.runtime_frontier {
            self.runtime_frontier = frontier;
        }
    }

    #[inline]
    fn set_image_frontier(&mut self, frontier: u32) {
        self.image_frontier = frontier;
        #[cfg(test)]
        self.update_runtime_frontier();
    }

    #[inline]
    fn set_frontier_workspace_bytes(&mut self, bytes: u32) {
        self.frontier_workspace_bytes = bytes;
        #[cfg(test)]
        self.update_runtime_frontier();
    }

    #[cfg(all(test, feature = "std"))]
    #[inline]
    pub(crate) fn runtime_sidecar_high_water_bytes(&self) -> usize {
        self.runtime_frontier as usize
    }

    #[cfg(all(test, feature = "std"))]
    #[inline]
    pub(crate) fn runtime_image_frontier_bytes(&self) -> usize {
        self.image_frontier as usize
    }

    #[cfg(all(test, feature = "std"))]
    #[inline]
    pub(crate) fn runtime_frontier_workspace_bytes(&self) -> usize {
        self.frontier_workspace_bytes as usize
    }
}
