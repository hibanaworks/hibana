use core::cell::Cell;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub(crate) enum RendezvousAccessState {
    Available = 0,
    RegistryLease = 1,
    ScratchLease = 2,
    EndpointOperation = 3,
    EndpointScratchLease = 4,
}

impl RendezvousAccessState {
    #[inline]
    pub(crate) const fn begin_endpoint_operation(self) -> Option<Self> {
        match self {
            Self::Available => Some(Self::EndpointOperation),
            Self::RegistryLease
            | Self::ScratchLease
            | Self::EndpointOperation
            | Self::EndpointScratchLease => None,
        }
    }

    #[inline]
    pub(crate) const fn finish_endpoint_operation(self) -> Option<Self> {
        match self {
            Self::EndpointOperation => Some(Self::Available),
            Self::Available
            | Self::RegistryLease
            | Self::ScratchLease
            | Self::EndpointScratchLease => None,
        }
    }

    #[inline]
    pub(crate) const fn begin_scratch(self) -> Option<(Self, Self)> {
        match self {
            Self::Available => Some((Self::ScratchLease, Self::Available)),
            Self::EndpointOperation => Some((Self::EndpointScratchLease, Self::EndpointOperation)),
            Self::RegistryLease | Self::ScratchLease | Self::EndpointScratchLease => None,
        }
    }

    #[inline]
    pub(crate) const fn finish_scratch(self) -> Option<Self> {
        match self {
            Self::ScratchLease => Some(Self::Available),
            Self::EndpointScratchLease => Some(Self::EndpointOperation),
            Self::Available | Self::RegistryLease | Self::EndpointOperation => None,
        }
    }
}

pub(crate) struct EndpointOperationLease<'r> {
    state: &'r Cell<RendezvousAccessState>,
}

impl<'r> EndpointOperationLease<'r> {
    pub(super) const fn new(state: &'r Cell<RendezvousAccessState>) -> Self {
        Self { state }
    }
}

impl Drop for EndpointOperationLease<'_> {
    #[inline]
    fn drop(&mut self) {
        let next = crate::invariant_some(self.state.get().finish_endpoint_operation());
        self.state.set(next);
    }
}
