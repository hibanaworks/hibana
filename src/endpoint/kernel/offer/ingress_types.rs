//! Offer scope selection value types.

use crate::global::const_dsl::ScopeId;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::endpoint::kernel) enum FrameHintResolution {
    Unresolved,
    Resolved,
}

impl FrameHintResolution {
    #[inline]
    pub(in crate::endpoint::kernel) const fn unresolved() -> Self {
        Self::Unresolved
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn resolved() -> Self {
        Self::Resolved
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn is_resolved(self) -> bool {
        matches!(self, Self::Resolved)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record(&mut self, observed: Self) {
        if observed.is_resolved() {
            *self = Self::Resolved;
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
