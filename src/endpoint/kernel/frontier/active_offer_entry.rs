use super::{FrontierKind, LaneOfferState, ScopeId, StateIndex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferEntrySummary {
    pub(crate) frontier_mask: u8,
    pub(crate) flags: u8,
}

impl OfferEntrySummary {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_INTRINSIC_READY: u8 = 1 << 2;

    const EMPTY: Self = Self {
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    fn observe_lane(&mut self, info: LaneOfferState) {
        self.frontier_mask |= info.frontier.bit();
        if info.is_controller() {
            self.flags |= Self::FLAG_CONTROLLER;
        }
        if info.is_dynamic() {
            self.flags |= Self::FLAG_DYNAMIC;
        }
        if info.intrinsic_ready() {
            self.flags |= Self::FLAG_INTRINSIC_READY;
        }
    }

    #[inline]
    pub(crate) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(crate) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(crate) fn intrinsic_ready(self) -> bool {
        (self.flags & Self::FLAG_INTRINSIC_READY) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveOfferEntry {
    representative_lane: u8,
    representative: LaneOfferState,
    summary: OfferEntrySummary,
}

impl ActiveOfferEntry {
    #[inline]
    pub(crate) fn new(representative_lane: u8, representative: LaneOfferState) -> Option<Self> {
        if representative.entry.is_absent() || representative.scope.is_none() {
            return None;
        }
        let mut summary = OfferEntrySummary::EMPTY;
        summary.observe_lane(representative);
        Some(Self {
            representative_lane,
            representative,
            summary,
        })
    }

    #[inline]
    pub(crate) fn observe_lane(&mut self, info: LaneOfferState) -> bool {
        if info.entry.is_absent() || info.scope.is_none() || info.entry != self.representative.entry
        {
            return false;
        }
        self.summary.observe_lane(info);
        true
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
    pub(crate) const fn entry(self) -> StateIndex {
        self.representative.entry
    }

    #[inline]
    pub(crate) const fn parallel_root(self) -> ScopeId {
        self.representative.parallel_root
    }

    #[inline]
    pub(crate) const fn frontier(self) -> FrontierKind {
        self.representative.frontier
    }

    #[inline]
    pub(crate) const fn summary(self) -> OfferEntrySummary {
        self.summary
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
