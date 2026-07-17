use super::super::super::ObservedEntrySet;
use super::entry::{CandidateAuthority, OfferAlignmentCandidateInput};
use super::selection::{ClassifiedCandidates, CurrentOfferObservation, OfferAlignmentSelection};

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
            let end = crate::invariant_some(self.observed_entries.entry_group_end(slot_idx));
            let mut current = slot_idx;
            while current < end {
                let slot = crate::invariant_some(self.observed_entries.slot(current));
                if slot.has_ready_arm() || slot.has_progress() {
                    return false;
                }
                current += 1;
            }
            true
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
        let Some(current_start) = self.current_slot() else {
            return CurrentOfferObservation::empty();
        };
        let current_end =
            crate::invariant_some(self.observed_entries.entry_group_end(current_start));
        let mut observation = CurrentOfferObservation::empty().with_present();
        let mut current = current_start;
        while current < current_end {
            let slot = crate::invariant_some(self.observed_entries.slot(current));
            if slot.is_selectable() && slot.is_ready() {
                observation = observation.with_ready();
            }
            if self.slot_has_progress(current) {
                observation = observation.with_progress_evidence();
            }
            if slot.has_progress() {
                observation = observation.with_observed_progress_evidence();
            }
            current += 1;
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
        let mut ready_entry_filter = None;
        let mut slot_idx = 0usize;
        while slot_idx < self.observed_entries.len() {
            let entry_idx = crate::invariant_some(self.observed_entries.entry_idx(slot_idx));
            let group_end = crate::invariant_some(self.observed_entries.entry_group_end(slot_idx));
            let mut current = slot_idx;
            let mut has_ready_candidate = false;
            while current < group_end {
                let slot = crate::invariant_some(self.observed_entries.slot(current));
                if self.is_candidate(current, current_controller) && slot.has_ready_arm() {
                    has_ready_candidate = true;
                }
                current += 1;
            }
            if has_ready_candidate {
                ready_count += 1;
                if ready_entry_filter.is_none() {
                    ready_entry_filter = Some(entry_idx);
                }
            }
            slot_idx = group_end;
        }

        if ready_count != 1 {
            ready_entry_filter = None;
        }
        let mut classified = ClassifiedCandidates::EMPTY;
        slot_idx = 0;
        while slot_idx < self.observed_entries.len() {
            let entry_idx = crate::invariant_some(self.observed_entries.entry_idx(slot_idx));
            let group_end = crate::invariant_some(self.observed_entries.entry_group_end(slot_idx));
            let mut current = slot_idx;
            let mut has_candidate = false;
            let mut authority = CandidateAuthority::Passive;
            while current < group_end {
                let slot = crate::invariant_some(self.observed_entries.slot(current));
                if self.is_candidate(current, current_controller) {
                    has_candidate = true;
                    authority = authority.merge(CandidateAuthority::from_observation(
                        slot.is_controller(),
                        slot.is_dynamic(),
                    ));
                }
                current += 1;
            }
            if has_candidate && ready_entry_filter.is_none_or(|ready| ready == entry_idx) {
                classified.record(entry_idx, authority);
            }
            slot_idx = group_end;
        }

        OfferAlignmentSelection {
            ready_entry_filter,
            outcome: classified.outcome(),
        }
    }
}

#[cfg(any(kani, all(test, hibana_repo_tests)))]
mod tests;
