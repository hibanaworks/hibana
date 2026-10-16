//! Shared caller-supplied workspace belongs to the rendezvous, not a wire port.

use super::{Rendezvous, RendezvousAccessState, ScratchLease, Transport};
use crate::endpoint::kernel::FrontierScratchLayout;

#[inline(always)]
fn align_up_absolute_offset(base: usize, offset: usize, align: usize) -> usize {
    crate::runtime_core::layout::align_offset(base, offset, align)
}

impl<'rv, 'cfg: 'rv, T: Transport> Rendezvous<'rv, 'cfg, T> {
    pub(crate) fn try_scratch_lease(&self) -> Option<ScratchLease<'_>> {
        ScratchLease::try_acquire(&self.access_state)
    }

    pub(crate) fn require_access_barrier(&self) {
        match self.access_state.get() {
            RendezvousAccessState::RegistryLease
            | RendezvousAccessState::ScratchLease
            | RendezvousAccessState::EndpointOperation(_)
            | RendezvousAccessState::EndpointScratchLease(_) => {}
            RendezvousAccessState::Available => crate::invariant(),
        }
    }

    fn require_scratch_lease(&self, lease: &ScratchLease<'_>) {
        if !lease.authorizes(&self.access_state)
            || !matches!(
                self.access_state.get(),
                RendezvousAccessState::ScratchLease
                    | RendezvousAccessState::EndpointScratchLease(_)
            )
        {
            crate::invariant();
        }
    }

    pub(crate) fn scratch<'lease>(&self, lease: &'lease mut ScratchLease<'_>) -> &'lease mut [u8] {
        self.require_scratch_lease(lease);
        let base = self.image_frontier.get() as usize;
        let workspace = self.frontier_workspace_bytes.get() as usize;
        let start = crate::runtime_core::layout::add(base, workspace);
        let end = self.endpoint_storage_floor();
        if start > end {
            crate::invariant();
        }
        // SAFETY: this is the unique scratch-lease borrow. The checked range
        // lies between the current image frontier and live actor allocations.
        unsafe { core::slice::from_raw_parts_mut(self.slab_ptr.add(start), end - start) }
    }

    pub(crate) fn frontier_scratch<'lease>(
        &self,
        lease: &'lease mut ScratchLease<'_>,
    ) -> &'lease mut [u8] {
        self.require_scratch_lease(lease);
        let start = self.image_frontier.get() as usize;
        let workspace = self.frontier_workspace_bytes.get() as usize;
        let end = crate::runtime_core::layout::add(start, workspace);
        if end > self.endpoint_storage_floor() {
            crate::invariant();
        }
        let aligned = if workspace == 0 {
            end
        } else {
            align_up_absolute_offset(
                self.slab_ptr as usize,
                start,
                FrontierScratchLayout::new(0).total_align(),
            )
        };
        if aligned > end {
            crate::invariant();
        }
        // SAFETY: the aligned frontier workspace is wholly before actor
        // storage and is borrowed through the unique mutable scratch lease.
        unsafe { core::slice::from_raw_parts_mut(self.slab_ptr.add(aligned), end - aligned) }
    }
}

#[cfg(test)]
mod tests {
    use super::align_up_absolute_offset;

    #[test]
    fn frontier_scratch_offset_aligns_absolute_address_not_offset_only() {
        let base = 3;
        let start = 5;
        let align = 8;
        let aligned = align_up_absolute_offset(base, start, align);
        assert_eq!((base + aligned) % align, 0);
        assert_eq!(aligned, start);
    }
}
