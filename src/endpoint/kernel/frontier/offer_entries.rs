use super::{EntryBuffer, FrontierKind, LaneOfferState, ScopeId, StateIndex};

#[derive(Clone, Copy)]
pub(crate) struct RootFrontierState {
    pub(crate) root: ScopeId,
    pub(crate) active_start: u8,
    pub(crate) active_len: u8,
}

impl RootFrontierState {
    pub(crate) const EMPTY: Self = Self {
        root: ScopeId::none(),
        active_start: 0,
        active_len: 0,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrontierObservationSlot {
    pub(crate) entry: StateIndex,
    pub(crate) frontier_mask: u8,
}

impl FrontierObservationSlot {
    pub(crate) const EMPTY: Self = Self {
        entry: StateIndex::ABSENT,
        frontier_mask: 0,
    };
}

#[inline]
pub(crate) fn cached_frontier_observation_slots_len(
    slots: EntryBuffer<FrontierObservationSlot>,
) -> usize {
    let mut len = 0usize;
    while len < slots.capacity() {
        if slots[len].entry.is_absent() {
            break;
        }
        len += 1;
    }
    len
}

#[derive(Clone, Copy)]
pub(crate) struct OfferEntrySummary {
    pub(crate) frontier_mask: u8,
    pub(crate) flags: u8,
}

impl OfferEntrySummary {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_INTRINSIC_READY: u8 = 1 << 2;

    pub(crate) const EMPTY: Self = Self {
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(crate) fn observe_lane(&mut self, info: LaneOfferState) {
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
pub(crate) struct OfferEntryObservedState {
    pub(crate) scope_id: ScopeId,
    pub(crate) frontier_mask: u8,
    pub(crate) flags: u8,
}

impl OfferEntryObservedState {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_PROGRESS: u8 = 1 << 2;
    pub(crate) const FLAG_READY_ARM: u8 = 1 << 3;
    pub(crate) const FLAG_BINDING_READY: u8 = 1 << 4;
    pub(crate) const FLAG_READY: u8 = 1 << 5;

    #[inline]
    pub(crate) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(crate) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(crate) fn has_progress_evidence(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS) != 0
    }

    #[inline]
    pub(crate) fn has_ready_arm_evidence(self) -> bool {
        (self.flags & Self::FLAG_READY_ARM) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrontierCandidate {
    pub(crate) scope_id: ScopeId,
    pub(crate) entry_idx: u16,
    pub(crate) parallel_root: ScopeId,
    pub(crate) frontier: FrontierKind,
    pub(crate) flags: u8,
}

impl FrontierCandidate {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_HAS_EVIDENCE: u8 = 1 << 2;
    pub(crate) const FLAG_READY: u8 = 1 << 3;

    pub(crate) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        entry_idx: u16::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: 0,
    };

    #[inline]
    pub(crate) const fn flags_from_observed(observed: OfferEntryObservedState) -> u8 {
        (if (observed.flags & OfferEntryObservedState::FLAG_CONTROLLER) != 0 {
            Self::FLAG_CONTROLLER
        } else {
            0
        }) | (if (observed.flags & OfferEntryObservedState::FLAG_DYNAMIC) != 0 {
            Self::FLAG_DYNAMIC
        } else {
            0
        }) | (if (observed.flags & OfferEntryObservedState::FLAG_PROGRESS) != 0 {
            Self::FLAG_HAS_EVIDENCE
        } else {
            0
        }) | (if (observed.flags & OfferEntryObservedState::FLAG_READY) != 0 {
            Self::FLAG_READY
        } else {
            0
        })
    }

    #[inline]
    pub(crate) const fn has_evidence(self) -> bool {
        (self.flags & Self::FLAG_HAS_EVIDENCE) != 0
    }

    #[inline]
    pub(crate) const fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }
}
