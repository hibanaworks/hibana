#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub(in super::super) enum ProgressEvidence {
    Absent,
    Present,
}

impl ProgressEvidence {
    #[inline]
    pub(in super::super) const fn is_absent(self) -> bool {
        matches!(self, Self::Absent)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum CandidateAuthority {
    Passive,
    Controller,
    DynamicController,
}

impl CandidateAuthority {
    pub(super) const fn from_observation(controller: bool, dynamic: bool) -> Self {
        if !controller {
            Self::Passive
        } else if dynamic {
            Self::DynamicController
        } else {
            Self::Controller
        }
    }

    pub(super) const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::DynamicController, Self::DynamicController)
            | (Self::DynamicController, Self::Controller)
            | (Self::DynamicController, Self::Passive)
            | (Self::Controller, Self::DynamicController)
            | (Self::Passive, Self::DynamicController) => Self::DynamicController,
            (Self::Controller, Self::Controller)
            | (Self::Controller, Self::Passive)
            | (Self::Passive, Self::Controller) => Self::Controller,
            (Self::Passive, Self::Passive) => Self::Passive,
        }
    }
}

#[derive(Clone, Copy)]
pub(in super::super) enum CurrentOfferEntry {
    RouteWithOfferLanes,
    RouteWithoutOfferLanes,
    NonRoute,
}

impl CurrentOfferEntry {
    #[inline]
    pub(in super::super) const fn is_route_entry(self) -> bool {
        matches!(
            self,
            Self::RouteWithOfferLanes | Self::RouteWithoutOfferLanes
        )
    }

    #[inline]
    pub(in super::super) const fn has_offer_lanes(self) -> bool {
        matches!(self, Self::RouteWithOfferLanes)
    }

    #[inline]
    pub(in super::super) const fn is_unrunnable_route(self) -> bool {
        matches!(self, Self::RouteWithoutOfferLanes)
    }
}

#[derive(Clone, Copy)]
pub(in super::super) enum CurrentOfferAuthority {
    Controller,
    Passive,
}

impl CurrentOfferAuthority {
    #[inline]
    pub(in super::super) const fn is_controller(self) -> bool {
        matches!(self, Self::Controller)
    }
}

#[derive(Clone, Copy)]
pub(in super::super) enum ProgressSiblingPresence {
    Absent,
    Present,
}

impl ProgressSiblingPresence {
    #[inline]
    pub(in super::super) const fn from_observed_progress_sibling(observed: bool) -> Self {
        if observed {
            Self::Present
        } else {
            Self::Absent
        }
    }

    #[inline]
    pub(in super::super) const fn exists(self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy)]
pub(in super::super) struct OfferAlignmentCandidateInput {
    pub(in super::super) current_idx: usize,
    pub(in super::super) current_entry: CurrentOfferEntry,
    pub(in super::super) current_authority: CurrentOfferAuthority,
    pub(in super::super) progress_sibling_presence: ProgressSiblingPresence,
    pub(in super::super) current_observation: super::current::CurrentOfferObservation,
}
