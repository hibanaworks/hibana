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
    lane: u8,
    frame_label: u8,
    source: ResolvedFrameHintSource,
}

#[derive(Clone, Copy)]
enum ResolvedFrameHintSource {
    ScopeEvidence,
    StagedTransport,
}

impl ResolvedFrameHint {
    #[inline]
    pub(in crate::endpoint::kernel) const fn scope_evidence(lane: u8, frame_label: u8) -> Self {
        Self {
            lane,
            frame_label,
            source: ResolvedFrameHintSource::ScopeEvidence,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn staged_transport(lane: u8, frame_label: u8) -> Self {
        Self {
            lane,
            frame_label,
            source: ResolvedFrameHintSource::StagedTransport,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn route_lane(self) -> u8 {
        self.lane
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn route_frame_label(self) -> u8 {
        self.frame_label
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn materialization_frame_label(self) -> Option<u8> {
        match self.source {
            ResolvedFrameHintSource::ScopeEvidence => None,
            ResolvedFrameHintSource::StagedTransport => Some(self.frame_label),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct OfferScopeSelection {
    pub(in crate::endpoint::kernel) scope_id: ScopeId,
    pub(in crate::endpoint::kernel) frontier_parallel_root: Option<ScopeId>,
    pub(in crate::endpoint::kernel) offer_lane: u8,
    pub(in crate::endpoint::kernel) at_route_offer_entry: bool,
}
