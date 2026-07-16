use super::super::super::ObservedEntrySet;
use super::entry::OfferAlignmentCandidateInput;
use super::selection::{CurrentOfferObservation, OfferAlignmentOutcome, OfferAlignmentSelection};
use crate::endpoint::kernel::frontier::FrontierObservationSlot;

#[derive(Clone, Copy, Eq, PartialEq)]
enum CurrentControllerFrontier {
    IncludeCurrent,
    ExcludeCurrent,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentCandidatePool {
    observed_entries: ObservedEntrySet,
    input: OfferAlignmentCandidateInput,
}

#[derive(Clone, Copy)]
struct ClassifiedCandidates {
    candidate_count: usize,
    first_candidate: Option<usize>,
    controller_count: usize,
    first_controller: Option<usize>,
    dynamic_controller_count: usize,
    first_dynamic_controller: Option<usize>,
}

impl ClassifiedCandidates {
    const EMPTY: Self = Self {
        candidate_count: 0,
        first_candidate: None,
        controller_count: 0,
        first_controller: None,
        dynamic_controller_count: 0,
        first_dynamic_controller: None,
    };

    fn record(&mut self, entry_idx: usize, slot: FrontierObservationSlot) {
        if self.first_candidate.is_none() {
            self.first_candidate = Some(entry_idx);
        }
        self.candidate_count += 1;
        if slot.is_controller() {
            if self.first_controller.is_none() {
                self.first_controller = Some(entry_idx);
            }
            self.controller_count += 1;
            if slot.is_dynamic() {
                if self.first_dynamic_controller.is_none() {
                    self.first_dynamic_controller = Some(entry_idx);
                }
                self.dynamic_controller_count += 1;
            }
        }
    }

    fn outcome(self) -> OfferAlignmentOutcome {
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

impl OfferAlignmentCandidatePool {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_observed(
        observed_entries: ObservedEntrySet,
        input: OfferAlignmentCandidateInput,
    ) -> Self {
        Self {
            observed_entries,
            input,
        }
    }

    #[inline]
    fn is_current_slot(self, slot_idx: usize) -> bool {
        self.observed_entries.entry_idx(slot_idx) == Some(self.input.current_idx)
    }

    #[inline]
    fn current_slot(self) -> Option<usize> {
        self.observed_entries.slot_for_entry(self.input.current_idx)
    }

    #[inline]
    fn current_matches_route(self) -> bool {
        self.current_slot().is_some() && self.input.current_entry.is_route_entry()
    }

    #[inline]
    fn slot_has_progress(self, slot_idx: usize) -> bool {
        let Some(slot) = self.observed_entries.slot(slot_idx) else {
            return false;
        };
        slot.is_selectable()
            && slot.has_progress()
            && (self.input.current_entry.is_route_entry() || !self.is_current_slot(slot_idx))
    }

    fn current_controller_frontier(self) -> CurrentControllerFrontier {
        let current_has_no_authority_evidence = self.current_slot().is_some_and(|slot_idx| {
            self.observed_entries
                .slot(slot_idx)
                .is_some_and(|slot| !slot.has_ready_arm() && !slot.has_progress())
        });
        if self.input.current_authority.is_controller()
            && self.current_matches_route()
            && current_has_no_authority_evidence
            && self.input.progress_sibling_presence.exists()
        {
            CurrentControllerFrontier::ExcludeCurrent
        } else {
            CurrentControllerFrontier::IncludeCurrent
        }
    }

    #[inline]
    fn in_intrinsic_controller_frontier(
        self,
        slot_idx: usize,
        current_controller: CurrentControllerFrontier,
    ) -> bool {
        let Some(slot) = self.observed_entries.slot(slot_idx) else {
            return false;
        };
        if !slot.is_selectable()
            || (current_controller == CurrentControllerFrontier::ExcludeCurrent
                && self.is_current_slot(slot_idx))
        {
            return false;
        }
        !slot.is_controller() || self.is_current_slot(slot_idx) || self.slot_has_progress(slot_idx)
    }

    #[inline]
    fn is_candidate(self, slot_idx: usize, current_controller: CurrentControllerFrontier) -> bool {
        if !self.in_intrinsic_controller_frontier(slot_idx, current_controller) {
            return false;
        }
        self.slot_has_progress(slot_idx)
            || (self.current_matches_route() && self.is_current_slot(slot_idx))
            || (self.input.current_entry.is_unrunnable_route() && !self.is_current_slot(slot_idx))
    }

    pub(in crate::endpoint::kernel::offer::select_alignment) fn current_observation(
        self,
    ) -> CurrentOfferObservation {
        let Some(current_slot) = self.current_slot() else {
            return CurrentOfferObservation::empty();
        };
        let Some(slot) = self.observed_entries.slot(current_slot) else {
            crate::invariant();
        };
        let mut observation = CurrentOfferObservation::empty().with_present();
        if slot.is_selectable() && slot.is_ready() {
            observation = observation.with_ready();
        }
        if self.slot_has_progress(current_slot) {
            observation = observation.with_progress_evidence();
        }
        if slot.has_progress() {
            observation = observation.with_observed_progress_evidence();
        }

        let mut slot_idx = 0usize;
        while slot_idx < self.observed_entries.len() {
            let Some(sibling) = self.observed_entries.slot(slot_idx) else {
                crate::invariant();
            };
            if !self.is_current_slot(slot_idx)
                && sibling.is_controller()
                && self.slot_has_progress(slot_idx)
            {
                return observation.with_controller_progress_sibling();
            }
            slot_idx += 1;
        }
        observation
    }

    pub(in crate::endpoint::kernel::offer::select_alignment) fn selection(
        self,
    ) -> OfferAlignmentSelection {
        let current_controller = self.current_controller_frontier();
        let mut ready_count = 0usize;
        let mut ready_slot = None;
        let mut slot_idx = 0usize;
        while slot_idx < self.observed_entries.len() {
            let Some(slot) = self.observed_entries.slot(slot_idx) else {
                crate::invariant();
            };
            if self.is_candidate(slot_idx, current_controller) && slot.has_ready_arm() {
                ready_count += 1;
                if ready_slot.is_none() {
                    ready_slot = Some(slot_idx);
                }
            }
            slot_idx += 1;
        }

        let ready_slot = if ready_count == 1 { ready_slot } else { None };
        let ready_entry_filter = ready_slot.and_then(|slot| self.observed_entries.entry_idx(slot));
        let mut classified = ClassifiedCandidates::EMPTY;
        slot_idx = 0;
        while slot_idx < self.observed_entries.len() {
            let selected = match ready_slot {
                Some(ready) => slot_idx == ready,
                None => self.is_candidate(slot_idx, current_controller),
            };
            if selected {
                let Some(slot) = self.observed_entries.slot(slot_idx) else {
                    crate::invariant();
                };
                let Some(entry_idx) = self.observed_entries.entry_idx(slot_idx) else {
                    crate::invariant();
                };
                classified.record(entry_idx, slot);
            }
            slot_idx += 1;
        }

        OfferAlignmentSelection {
            ready_entry_filter,
            outcome: classified.outcome(),
        }
    }
}
