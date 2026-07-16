//! Compact observation records streamed by offer arbitration.

use super::{FrontierKind, ScopeId, StateIndex};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrontierObservationSlot {
    pub(crate) entry: StateIndex,
    pub(crate) frontier_mask: u8,
    flags: u8,
}

impl FrontierObservationSlot {
    const FLAG_CONTROLLER: u8 = 1;
    const FLAG_DYNAMIC: u8 = 1 << 1;
    const FLAG_PROGRESS: u8 = 1 << 2;
    const FLAG_READY_ARM: u8 = 1 << 3;
    const FLAG_READY: u8 = 1 << 4;
    const FLAG_SELECTABLE: u8 = 1 << 5;

    pub(crate) const EMPTY: Self = Self {
        entry: StateIndex::ABSENT,
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(crate) const fn new(entry: StateIndex) -> Self {
        Self {
            entry,
            frontier_mask: 0,
            flags: 0,
        }
    }

    #[inline]
    pub(crate) fn record(
        &mut self,
        observed: OfferEntryObservedState,
        frontier_mask: u8,
        admission: OfferEntryAdmission,
    ) {
        if frontier_mask & !FrontierKind::ALL_BITS != 0 {
            crate::invariant();
        }
        let mut flags = 0u8;
        if observed.is_controller() {
            flags |= Self::FLAG_CONTROLLER;
        }
        if observed.is_dynamic() {
            flags |= Self::FLAG_DYNAMIC;
        }
        if observed.has_progress_evidence() {
            flags |= Self::FLAG_PROGRESS;
        }
        if observed.has_ready_arm_evidence() {
            flags |= Self::FLAG_READY_ARM;
        }
        if observed.is_ready() {
            flags |= Self::FLAG_READY;
        }
        if admission.is_selectable() {
            flags |= Self::FLAG_SELECTABLE;
        }
        self.frontier_mask = frontier_mask;
        self.flags = flags;
    }

    #[inline]
    pub(crate) const fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(crate) const fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(crate) const fn has_progress(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS) != 0
    }

    #[inline]
    pub(crate) const fn has_ready_arm(self) -> bool {
        (self.flags & Self::FLAG_READY_ARM) != 0
    }

    #[inline]
    pub(crate) const fn is_ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }

    #[inline]
    pub(crate) const fn is_selectable(self) -> bool {
        (self.flags & Self::FLAG_SELECTABLE) != 0
    }

    #[inline]
    pub(crate) const fn is_in_frontier(self, frontier: FrontierKind) -> bool {
        (self.frontier_mask & frontier.bit()) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfferEntryAdmission {
    Excluded,
    Selectable,
}

impl OfferEntryAdmission {
    #[inline]
    pub(crate) const fn is_selectable(self) -> bool {
        matches!(self, Self::Selectable)
    }
}

#[inline]
pub(crate) fn cached_frontier_observation_slots_len(slots: &[FrontierObservationSlot]) -> usize {
    let mut len = 0usize;
    while len < slots.len() {
        if slots[len].entry.is_absent() {
            break;
        }
        len += 1;
    }
    len
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

    #[inline]
    pub(crate) fn is_ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }
}
