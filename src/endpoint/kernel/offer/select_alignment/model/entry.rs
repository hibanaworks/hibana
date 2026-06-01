#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentOfferEntry {
    Route { has_offer_lanes: bool },
    NonRoute,
}

impl CurrentOfferEntry {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_meta(
        is_route_entry: bool,
        has_offer_lanes: bool,
    ) -> Self {
        if is_route_entry {
            Self::Route { has_offer_lanes }
        } else {
            Self::NonRoute
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_route_entry(self) -> bool {
        matches!(self, Self::Route { .. })
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_offer_lanes(
        self,
    ) -> bool {
        match self {
            Self::Route { has_offer_lanes } => has_offer_lanes,
            Self::NonRoute => false,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_unrunnable_route(
        self,
    ) -> bool {
        matches!(
            self,
            Self::Route {
                has_offer_lanes: false
            }
        )
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentOfferAuthority {
    Controller,
    Passive,
}

impl CurrentOfferAuthority {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_meta(
        is_controller: bool,
    ) -> Self {
        if is_controller {
            Self::Controller
        } else {
            Self::Passive
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_controller(self) -> bool {
        matches!(self, Self::Controller)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentCandidateInput {
    pub(in crate::endpoint::kernel::offer::select_alignment) current_idx: usize,
    pub(in crate::endpoint::kernel::offer::select_alignment) current_entry: CurrentOfferEntry,
    pub(in crate::endpoint::kernel::offer::select_alignment) current_authority:
        CurrentOfferAuthority,
    pub(in crate::endpoint::kernel::offer::select_alignment) progress_sibling_exists: bool,
}
