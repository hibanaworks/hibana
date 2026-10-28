//! Shared caller-supplied workspace belongs to the rendezvous, not a wire port.

use super::{Rendezvous, RendezvousAccessState, ScratchLease, Transport};

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
        let start = self.image_frontier.get() as usize;
        let end = self.endpoint_storage_floor();
        if start > end {
            crate::invariant();
        }
        // SAFETY: this is the unique scratch-lease borrow. The checked range
        // lies between the current image frontier and live actor allocations.
        unsafe { core::slice::from_raw_parts_mut(self.slab_ptr.add(start), end - start) }
    }
}
