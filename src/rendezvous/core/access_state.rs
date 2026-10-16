use super::EndpointLeaseId;
use core::cell::Cell;

#[cfg(kani)]
#[path = "access_state/kani.rs"]
mod proofs;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(::kani::Arbitrary))]
pub(crate) enum RendezvousAccessState {
    Available = 0,
    RegistryLease = 1,
    ScratchLease = 2,
    EndpointOperation(EndpointLeaseId) = 3,
    EndpointScratchLease(EndpointLeaseId) = 4,
}

impl RendezvousAccessState {
    #[inline]
    pub(crate) const fn begin_endpoint_operation(self, actor: EndpointLeaseId) -> Option<Self> {
        match self {
            Self::Available => Some(Self::EndpointOperation(actor)),
            Self::RegistryLease
            | Self::ScratchLease
            | Self::EndpointOperation(_)
            | Self::EndpointScratchLease(_) => None,
        }
    }

    #[inline]
    pub(crate) const fn finish_endpoint_operation(self, actor: EndpointLeaseId) -> Option<Self> {
        match self {
            Self::EndpointOperation(owner) if owner.0 == actor.0 => Some(Self::Available),
            Self::Available
            | Self::RegistryLease
            | Self::ScratchLease
            | Self::EndpointOperation(_)
            | Self::EndpointScratchLease(_) => None,
        }
    }

    #[inline]
    pub(crate) const fn begin_scratch(self) -> Option<(Self, Self)> {
        match self {
            Self::Available => Some((Self::ScratchLease, Self::Available)),
            Self::EndpointOperation(actor) => Some((
                Self::EndpointScratchLease(actor),
                Self::EndpointOperation(actor),
            )),
            Self::RegistryLease | Self::ScratchLease | Self::EndpointScratchLease(_) => None,
        }
    }

    #[inline]
    pub(crate) const fn finish_scratch(self) -> Option<Self> {
        match self {
            Self::ScratchLease => Some(Self::Available),
            Self::EndpointScratchLease(actor) => Some(Self::EndpointOperation(actor)),
            Self::Available | Self::RegistryLease | Self::EndpointOperation(_) => None,
        }
    }
}

pub(crate) struct EndpointOperationLease<'r> {
    state: &'r Cell<RendezvousAccessState>,
    actor: EndpointLeaseId,
}

impl<'r> EndpointOperationLease<'r> {
    const fn new(state: &'r Cell<RendezvousAccessState>, actor: EndpointLeaseId) -> Self {
        Self { state, actor }
    }

    pub(crate) fn try_acquire(
        state: &'r Cell<RendezvousAccessState>,
        actor: EndpointLeaseId,
    ) -> Option<Self> {
        let next = state.get().begin_endpoint_operation(actor)?;
        state.set(next);
        Some(Self::new(state, actor))
    }

    pub(crate) fn protects(
        &self,
        state: &Cell<RendezvousAccessState>,
        actor: EndpointLeaseId,
    ) -> bool {
        core::ptr::eq(self.state, state)
            && self.actor == actor
            && state.get() == RendezvousAccessState::EndpointOperation(actor)
    }
}

impl Drop for EndpointOperationLease<'_> {
    #[inline]
    fn drop(&mut self) {
        let next = crate::invariant_some(self.state.get().finish_endpoint_operation(self.actor));
        self.state.set(next);
    }
}

pub(crate) struct ScratchLease<'r> {
    state: &'r Cell<RendezvousAccessState>,
    restore: RendezvousAccessState,
}

impl<'r> ScratchLease<'r> {
    pub(super) fn try_acquire(state: &'r Cell<RendezvousAccessState>) -> Option<Self> {
        let (leased, restore) = state.get().begin_scratch()?;
        state.set(leased);
        Some(Self { state, restore })
    }

    pub(super) fn authorizes(&self, state: &Cell<RendezvousAccessState>) -> bool {
        core::ptr::eq(self.state, state)
    }
}

impl Drop for ScratchLease<'_> {
    fn drop(&mut self) {
        if self.state.get().finish_scratch() != Some(self.restore) {
            crate::invariant();
        }
        self.state.set(self.restore);
    }
}
