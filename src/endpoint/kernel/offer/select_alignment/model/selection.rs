use super::entry::CandidateAuthority;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum OfferAlignmentDecision {
    KeepCurrent,
    Realign(usize),
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
    ) -> Option<OfferAlignmentDecision> {
        if current.is_selectable() {
            return Some(OfferAlignmentDecision::KeepCurrent);
        }
        let entry_idx = match self.outcome {
            OfferAlignmentOutcome::UniqueDynamicController(idx)
            | OfferAlignmentOutcome::UniqueController(idx)
            | OfferAlignmentOutcome::UniqueCandidate(idx) => idx,
            OfferAlignmentOutcome::CandidateAbsent
            | OfferAlignmentOutcome::CandidateSetAmbiguous => return None,
        };
        if entry_idx == current_idx {
            crate::invariant();
        }
        Some(OfferAlignmentDecision::Realign(entry_idx))
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    #[kani::proof]
    fn erased_candidate_can_only_request_distinct_realign() {
        let current = kani::any::<u8>() as usize;
        let target = kani::any::<u8>() as usize;
        kani::assume(current != target);
        let class = kani::any::<u8>();
        kani::assume(class < 3);
        let outcome = match class {
            0 => OfferAlignmentOutcome::UniqueCandidate(target),
            1 => OfferAlignmentOutcome::UniqueController(target),
            _ => OfferAlignmentOutcome::UniqueDynamicController(target),
        };
        let selection = OfferAlignmentSelection {
            ready_entry_filter: None,
            outcome,
        };

        assert_eq!(
            selection.select(CurrentOfferCandidateStatus::NotSelectable, current),
            Some(OfferAlignmentDecision::Realign(target))
        );
    }

    #[kani::proof]
    fn exact_current_selection_never_becomes_realign() {
        let current = kani::any::<u8>() as usize;
        let target = kani::any::<u8>() as usize;
        let selection = OfferAlignmentSelection {
            ready_entry_filter: None,
            outcome: OfferAlignmentOutcome::UniqueDynamicController(target),
        };

        assert_eq!(
            selection.select(CurrentOfferCandidateStatus::Selectable, current),
            Some(OfferAlignmentDecision::KeepCurrent)
        );
    }
}
