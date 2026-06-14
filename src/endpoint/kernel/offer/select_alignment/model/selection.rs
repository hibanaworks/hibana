use super::super::super::{CurrentFrontierSelectionState, ObservedEntrySet, OfferSelectPriority};
use super::set::OfferEntrySet;

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum ProgressEvidence {
    Absent,
    Present,
}

impl ProgressEvidence {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn is_absent(self) -> bool {
        matches!(self, Self::Absent)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct CurrentOfferObservation {
    flags: u8,
}

impl CurrentOfferObservation {
    const PRESENT: u8 = 1;
    const READY: u8 = 1 << 1;
    const PROGRESS_EVIDENCE: u8 = 1 << 2;
    const OBSERVED_PROGRESS_EVIDENCE: u8 = 1 << 3;
    const CONTROLLER_PROGRESS_SIBLING_EXISTS: u8 = 1 << 4;

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn empty() -> Self {
        Self { flags: 0 }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn with_present(self) -> Self {
        Self {
            flags: self.flags | Self::PRESENT,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn with_ready(self) -> Self {
        Self {
            flags: self.flags | Self::READY,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn with_progress_evidence(
        self,
    ) -> Self {
        Self {
            flags: self.flags | Self::PROGRESS_EVIDENCE,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn with_observed_progress_evidence(
        self,
    ) -> Self {
        Self {
            flags: self.flags | Self::OBSERVED_PROGRESS_EVIDENCE,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn with_controller_progress_sibling(
        self,
    ) -> Self {
        Self {
            flags: self.flags | Self::CONTROLLER_PROGRESS_SIBLING_EXISTS,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn present(self) -> bool {
        (self.flags & Self::PRESENT) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn ready(self) -> bool {
        (self.flags & Self::READY) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn progress_evidence(
        self,
    ) -> bool {
        (self.flags & Self::PROGRESS_EVIDENCE) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn accumulated_progress_evidence(
        self,
        state: CurrentFrontierSelectionState,
    ) -> ProgressEvidence {
        if (self.flags & Self::OBSERVED_PROGRESS_EVIDENCE) != 0 || state.has_progress_evidence() {
            ProgressEvidence::Present
        } else {
            ProgressEvidence::Absent
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn controller_progress_sibling_exists(
        self,
    ) -> bool {
        (self.flags & Self::CONTROLLER_PROGRESS_SIBLING_EXISTS) != 0
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum OfferAlignmentOutcome {
    CandidateAbsent,
    CandidateSetAmbiguous,
    UniqueDynamicController(usize),
    UniqueController(usize),
    UniqueCandidate(usize),
}

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentOfferCandidateStatus {
    NotSelectable,
    Selectable,
}

impl CurrentOfferCandidateStatus {
    #[inline]
    const fn is_selectable(self) -> bool {
        matches!(self, Self::Selectable)
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentSelection {
    pub(in crate::endpoint::kernel::offer::select_alignment) hint_filter: Option<usize>,
    pub(in crate::endpoint::kernel::offer::select_alignment) outcome: OfferAlignmentOutcome,
}

impl OfferAlignmentSelection {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_candidate(self) -> bool {
        !matches!(self.outcome, OfferAlignmentOutcome::CandidateAbsent)
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn allows_current(
        self,
        current_idx: usize,
    ) -> bool {
        match self.hint_filter {
            Some(filtered_idx) => current_idx == filtered_idx,
            None => true,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn select(
        self,
        current: CurrentOfferCandidateStatus,
        current_idx: usize,
    ) -> Option<(OfferSelectPriority, usize)> {
        if current.is_selectable() {
            return Some((OfferSelectPriority::CurrentOfferEntry, current_idx));
        }
        match self.outcome {
            OfferAlignmentOutcome::UniqueDynamicController(idx) => {
                Some((OfferSelectPriority::DynamicControllerUnique, idx))
            }
            OfferAlignmentOutcome::UniqueController(idx) => {
                Some((OfferSelectPriority::ControllerUnique, idx))
            }
            OfferAlignmentOutcome::UniqueCandidate(idx) => {
                Some((OfferSelectPriority::CandidateUnique, idx))
            }
            OfferAlignmentOutcome::CandidateAbsent
            | OfferAlignmentOutcome::CandidateSetAmbiguous => None,
        }
    }
}

pub(in crate::endpoint::kernel::offer::select_alignment) struct ClassifiedOfferCandidateSets {
    candidates: OfferEntrySet,
    controllers: OfferEntrySet,
    dynamic_controllers: OfferEntrySet,
}

impl ClassifiedOfferCandidateSets {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn new(
        candidates: OfferEntrySet,
        controllers: OfferEntrySet,
        dynamic_controllers: OfferEntrySet,
    ) -> Self {
        Self {
            candidates,
            controllers,
            dynamic_controllers,
        }
    }

    pub(in crate::endpoint::kernel::offer::select_alignment) fn outcome(
        self,
        observed_entries: ObservedEntrySet,
    ) -> OfferAlignmentOutcome {
        if self.dynamic_controllers.has_one() {
            let Some(entry_idx) = self.dynamic_controllers.first_entry_idx(observed_entries) else {
                crate::invariant();
            };
            return OfferAlignmentOutcome::UniqueDynamicController(entry_idx);
        }
        if self.controllers.has_one() {
            let Some(entry_idx) = self.controllers.first_entry_idx(observed_entries) else {
                crate::invariant();
            };
            return OfferAlignmentOutcome::UniqueController(entry_idx);
        }
        if self.candidates.is_empty() {
            return OfferAlignmentOutcome::CandidateAbsent;
        }
        if self.candidates.has_one() {
            let Some(entry_idx) = self.candidates.first_entry_idx(observed_entries) else {
                crate::invariant();
            };
            return OfferAlignmentOutcome::UniqueCandidate(entry_idx);
        }
        OfferAlignmentOutcome::CandidateSetAmbiguous
    }
}
