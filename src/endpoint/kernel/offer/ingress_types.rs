//! Offer scope selection value types.

use crate::global::const_dsl::ScopeId;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct ResolvedFrameHint;

impl ResolvedFrameHint {
    #[inline]
    pub(in crate::endpoint::kernel) const fn scope_evidence() -> Self {
        Self
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn staged_transport() -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct OfferScopeSelection {
    pub(in crate::endpoint::kernel) scope_id: ScopeId,
    pub(in crate::endpoint::kernel) frontier_parallel_root: Option<ScopeId>,
    pub(in crate::endpoint::kernel) offer_lane: u8,
    pub(in crate::endpoint::kernel) at_route_offer_entry: bool,
}
