#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentOfferEntry {
    RouteWithOfferLanes,
    RouteWithoutOfferLanes,
    NonRoute,
}

impl CurrentOfferEntry {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_route_entry(self) -> bool {
        matches!(
            self,
            Self::RouteWithOfferLanes | Self::RouteWithoutOfferLanes
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_offer_lanes(
        self,
    ) -> bool {
        matches!(self, Self::RouteWithOfferLanes)
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_unrunnable_route(
        self,
    ) -> bool {
        matches!(self, Self::RouteWithoutOfferLanes)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentOfferAuthority {
    Controller,
    Passive,
}

impl CurrentOfferAuthority {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_controller(self) -> bool {
        matches!(self, Self::Controller)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum ProgressSiblingPresence {
    Absent,
    Present,
}

impl ProgressSiblingPresence {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_observed_progress_sibling(
        observed: bool,
    ) -> Self {
        if observed {
            Self::Present
        } else {
            Self::Absent
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn exists(self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentCandidateInput {
    pub(in crate::endpoint::kernel::offer::select_alignment) current_idx: usize,
    pub(in crate::endpoint::kernel::offer::select_alignment) current_entry: CurrentOfferEntry,
    pub(in crate::endpoint::kernel::offer::select_alignment) current_authority:
        CurrentOfferAuthority,
    pub(in crate::endpoint::kernel::offer::select_alignment) progress_sibling_presence:
        ProgressSiblingPresence,
}
