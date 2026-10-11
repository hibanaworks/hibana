use super::{FrontierKind, LaneOfferState, ScopeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveOfferEntry {
    representative_lane: u8,
    representative: LaneOfferState,
}

impl ActiveOfferEntry {
    #[inline]
    pub(crate) fn new(representative_lane: u8, representative: LaneOfferState) -> Option<Self> {
        representative.key()?;
        Some(Self {
            representative_lane,
            representative,
        })
    }

    #[inline]
    pub(crate) fn accepts_lane(self, info: LaneOfferState) -> bool {
        info == self.representative
    }

    #[inline]
    pub(crate) const fn representative_lane(self) -> u8 {
        self.representative_lane
    }

    #[inline]
    pub(crate) const fn representative(self) -> LaneOfferState {
        self.representative
    }

    #[inline]
    pub(crate) const fn scope(self) -> ScopeId {
        self.representative.scope
    }

    #[inline]
    pub(crate) const fn parallel_root(self) -> ScopeId {
        self.representative.parallel_root
    }

    #[inline]
    pub(crate) const fn frontier(self) -> FrontierKind {
        self.representative.frontier
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
