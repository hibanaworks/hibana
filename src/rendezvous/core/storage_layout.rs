use super::{
    AssocTable, Clock, EndpointLeaseId, EndpointLeaseSlot, FREE_REGION_CAPACITY, FreeRegion,
    Rendezvous, RouteTable, Transport,
};
mod capacity;

// # Unsafe Owner Contract
//
// This file owns rendezvous slab layout, sidecar allocation, migration, and
// resident table ingress. The slab pointer and endpoint-lease table are created
// by the rendezvous constructor and remain pinned for the rendezvous lifetime.
// Every raw allocation returned here is aligned, range-checked against the
// current slab frontier, and recorded in rendezvous-owned metadata before typed
// owners bind to it. Migration paths copy initialized table entries into newly
// allocated sidecar storage before rebinding, and source regions are tracked only
// as rendezvous-local free regions for later resident allocation.

impl<'rv, 'cfg, T: Transport, C: Clock> Rendezvous<'rv, 'cfg, T, C>
where
    'cfg: 'rv,
{
    #[inline(always)]
    pub(crate) const fn align_up(value: usize, align: usize) -> usize {
        if align == 0 {
            crate::invariant();
        }
        let mask = align - 1;
        if value > usize::MAX - mask {
            crate::invariant();
        }
        (value + mask) & !mask
    }

    #[inline(always)]
    pub(crate) const fn align_down(value: usize, align: usize) -> usize {
        if align == 0 {
            crate::invariant();
        }
        let mask = align - 1;
        value & !mask
    }

    #[inline(always)]
    pub(crate) const fn frontier_workspace_guard_bytes(
        layout: crate::endpoint::kernel::FrontierScratchLayout,
    ) -> usize {
        let align = layout.total_align();
        if align == 0 {
            crate::invariant();
        }
        let pad = align - 1;
        if layout.total_bytes() > usize::MAX - pad {
            crate::invariant();
        }
        layout.total_bytes() + pad
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
    pub(crate) fn endpoint_storage_floor(&self) -> usize {
        let (_, slab_len) = self.slab_ptr_and_len();
        let mut floor = slab_len;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.is_live() && slot.len != 0 && (slot.offset as usize) < floor {
                floor = slot.offset as usize;
            }
            idx += 1;
        }
        floor
    }

    #[inline]
    pub(crate) fn endpoint_lease_floor(&self) -> usize {
        crate::invariant_some(
            (self.image_frontier as usize).checked_add(self.frontier_workspace_bytes as usize),
        )
    }

    #[inline]
    fn set_image_frontier(&mut self, frontier: u32) {
        self.image_frontier = frontier;
    }

    #[inline]
    fn set_frontier_workspace_bytes(&mut self, bytes: u32) {
        self.frontier_workspace_bytes = bytes;
    }
}
