use super::super::super::{CurrentFrontierSelectionState, OfferSelectPriority};
use super::entry::{CandidateAuthority, ProgressEvidence};

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

#[derive(Clone, Copy)]
pub(super) struct ClassifiedCandidates {
    candidate_count: usize,
    first_candidate: Option<usize>,
    controller_count: usize,
    first_controller: Option<usize>,
    dynamic_controller_count: usize,
    first_dynamic_controller: Option<usize>,
}

impl ClassifiedCandidates {
    pub(super) const EMPTY: Self = Self {
        candidate_count: 0,
        first_candidate: None,
        controller_count: 0,
        first_controller: None,
        dynamic_controller_count: 0,
        first_dynamic_controller: None,
    };

    pub(super) fn record(&mut self, entry_idx: usize, authority: CandidateAuthority) {
        if self.first_candidate.is_none() {
            self.first_candidate = Some(entry_idx);
        }
        self.candidate_count += 1;
        match authority {
            CandidateAuthority::Passive => {}
            CandidateAuthority::Controller | CandidateAuthority::DynamicController => {
                if self.first_controller.is_none() {
                    self.first_controller = Some(entry_idx);
                }
                self.controller_count += 1;
                if authority == CandidateAuthority::DynamicController {
                    if self.first_dynamic_controller.is_none() {
                        self.first_dynamic_controller = Some(entry_idx);
                    }
                    self.dynamic_controller_count += 1;
                }
            }
        }
    }

    pub(super) fn outcome(self) -> OfferAlignmentOutcome {
        if self.dynamic_controller_count == 1 {
            return OfferAlignmentOutcome::UniqueDynamicController(crate::invariant_some(
                self.first_dynamic_controller,
            ));
        }
        if self.controller_count == 1 {
            return OfferAlignmentOutcome::UniqueController(crate::invariant_some(
                self.first_controller,
            ));
        }
        match self.candidate_count {
            0 => OfferAlignmentOutcome::CandidateAbsent,
            1 => {
                OfferAlignmentOutcome::UniqueCandidate(crate::invariant_some(self.first_candidate))
            }
            _ => OfferAlignmentOutcome::CandidateSetAmbiguous,
        }
    }
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
    pub(in crate::endpoint::kernel::offer::select_alignment) ready_entry_filter: Option<usize>,
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
        match self.ready_entry_filter {
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
