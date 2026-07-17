use super::super::super::ObservedEntrySet;
use super::current::CurrentOfferObservation;
use super::entry::{CandidateAuthority, OfferAlignmentCandidateInput};
use super::selection::{ClassifiedCandidates, OfferAlignmentSelection};

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentCandidatePool<'a> {
    observed_entries: ObservedEntrySet<'a>,
    input: OfferAlignmentCandidateInput,
}

impl<'a> OfferAlignmentCandidatePool<'a> {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn from_observed(
        observed_entries: ObservedEntrySet<'a>,
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
    fn current_matches_route(self) -> bool {
        self.input.current_observation.selectable() && self.input.current_entry.is_route_entry()
    }

    #[inline]
    fn slot_has_progress(self, slot_idx: usize) -> bool {
        let Some(slot) = self.observed_entries.slot(slot_idx) else {
            return false;
        };
        slot.is_selectable() && slot.has_progress() && !self.is_current_slot(slot_idx)
    }

    #[inline]
    fn current_yields_to_progress_sibling(self) -> bool {
        self.input.current_authority.is_controller()
            && self.current_matches_route()
            && !self.input.current_observation.has_authority_evidence()
            && self.input.progress_sibling_presence.exists()
    }

    #[inline]
    fn in_intrinsic_controller_frontier(self, slot_idx: usize) -> bool {
        let Some(slot) = self.observed_entries.slot(slot_idx) else {
            return false;
        };
        if !slot.is_selectable() || self.is_current_slot(slot_idx) {
            return false;
        }
        !slot.is_controller() || self.slot_has_progress(slot_idx)
    }

    #[inline]
    fn is_candidate(self, slot_idx: usize) -> bool {
        if !self.in_intrinsic_controller_frontier(slot_idx) {
            return false;
        }
        self.slot_has_progress(slot_idx)
            || (self.input.current_entry.is_unrunnable_route() && !self.is_current_slot(slot_idx))
    }

    pub(in crate::endpoint::kernel::offer::select_alignment) fn current_observation(
        self,
    ) -> CurrentOfferObservation {
        let observation = self.input.current_observation;
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
        let mut ready_count = 0usize;
        let mut ready_entry_filter = None;
        if self.current_matches_route()
            && !self.current_yields_to_progress_sibling()
            && self.input.current_observation.has_ready_arm_evidence()
        {
            ready_count = 1;
            ready_entry_filter = Some(self.input.current_idx);
        }
        let mut slot_idx = 0usize;
        while slot_idx < self.observed_entries.len() {
            let entry_idx = crate::invariant_some(self.observed_entries.entry_idx(slot_idx));
            let group_end = crate::invariant_some(self.observed_entries.entry_group_end(slot_idx));
            let mut current = slot_idx;
            let mut has_ready_candidate = false;
            while current < group_end {
                let slot = crate::invariant_some(self.observed_entries.slot(current));
                if self.is_candidate(current) && slot.has_ready_arm() {
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
                if self.is_candidate(current) {
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
