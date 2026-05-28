//! Offer ingress and scope selection value types.

use crate::global::const_dsl::ScopeId;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct LaneIngressEvidence {
    pub(in crate::endpoint::kernel) lane_idx: usize,
    pub(in crate::endpoint::kernel) evidence: crate::binding::IngressEvidence,
}

impl LaneIngressEvidence {
    #[inline]
    pub(in crate::endpoint::kernel) const fn new(
        lane_idx: usize,
        evidence: crate::binding::IngressEvidence,
    ) -> Self {
        Self { lane_idx, evidence }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn frame_label(self) -> u8 {
        self.evidence.frame_label.raw()
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn lane(self) -> u8 {
        self.lane_idx as u8
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn into_parts(
        self,
    ) -> (usize, crate::binding::IngressEvidence) {
        (self.lane_idx, self.evidence)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct ResolvedFrameHint {
    pub(in crate::endpoint::kernel) lane: u8,
    pub(in crate::endpoint::kernel) frame_label: u8,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct OfferScopeSelection {
    pub(in crate::endpoint::kernel) scope_id: ScopeId,
    pub(in crate::endpoint::kernel) frontier_parallel_root: Option<ScopeId>,
    pub(in crate::endpoint::kernel) offer_lane: u8,
    pub(in crate::endpoint::kernel) at_route_offer_entry: bool,
}
