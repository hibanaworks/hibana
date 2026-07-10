use super::super::super::ObservedEntrySet;
use super::entry::{CurrentOfferEntry, OfferAlignmentCandidateInput};
use super::selection::{
    ClassifiedOfferCandidateSets, CurrentOfferObservation, OfferAlignmentSelection,
};
use super::set::OfferEntrySet;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentControllerFrontier {
    IncludeCurrent,
    ExcludeCurrent,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentCandidatePool {
    observed_entries: OfferEntrySet,
    ready_entries: OfferEntrySet,
    ready_arm_entries: OfferEntrySet,
    controller_entries: OfferEntrySet,
    dynamic_controller_entries: OfferEntrySet,
    progress_entries: OfferEntrySet,
    current_entry_slot: OfferEntrySet,
}

impl OfferAlignmentCandidatePool {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) fn from_observed(
        observed_entries: ObservedEntrySet,
        selectable: OfferEntrySet,
        input: OfferAlignmentCandidateInput,
    ) -> Self {
        let observed =
            OfferEntrySet::from_bits(observed_entries.occupancy_mask()).intersect(selectable);
        let current_entry_slot =
            OfferEntrySet::from_bits(observed_entries.entry_bit(input.current_idx));
        let raw_progress = OfferEntrySet::from_bits(observed_entries.progress_mask);
        let progress_entries = if input.current_entry.is_route_entry() {
            raw_progress.intersect(observed)
        } else {
            raw_progress.intersect(observed).without(current_entry_slot)
        };
        Self {
            observed_entries: observed,
            ready_entries: OfferEntrySet::from_bits(observed_entries.ready_mask)
                .intersect(observed),
            ready_arm_entries: OfferEntrySet::from_bits(observed_entries.ready_arm_mask)
                .intersect(observed),
            controller_entries: OfferEntrySet::from_bits(observed_entries.controller_mask)
                .intersect(observed),
            dynamic_controller_entries: OfferEntrySet::from_bits(
                observed_entries.dynamic_controller_mask,
            )
            .intersect(observed),
            progress_entries,
            current_entry_slot,
        }
    }

    #[inline]
    const fn current_matches_route(self, entry: CurrentOfferEntry) -> bool {
        !self.current_entry_slot.is_empty() && entry.is_route_entry()
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn current_observation(
        self,
        observed_entries: ObservedEntrySet,
    ) -> CurrentOfferObservation {
        let raw_progress_entries = OfferEntrySet::from_bits(observed_entries.progress_mask);
        let mut observation = CurrentOfferObservation::empty();
        if !self.current_entry_slot.is_empty() {
            observation = observation.with_present();
        }
        if self.current_entry_slot.intersects(self.ready_entries) {
            observation = observation.with_ready();
        }
        if self.current_entry_slot.intersects(self.progress_entries) {
            observation = observation.with_progress_evidence();
        }
        if self.current_entry_slot.intersects(raw_progress_entries) {
            observation = observation.with_observed_progress_evidence();
        }
        if !self
            .progress_entries
            .intersect(self.controller_entries)
            .without(self.current_entry_slot)
            .is_empty()
        {
            observation = observation.with_controller_progress_sibling();
        }
        observation
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn current_controller_frontier(
        self,
        observed_entries: ObservedEntrySet,
        input: OfferAlignmentCandidateInput,
    ) -> CurrentControllerFrontier {
        if input.current_authority.is_controller()
            && self.current_matches_route(input.current_entry)
            && self
                .current_entry_slot
                .intersect(OfferEntrySet::from_bits(observed_entries.ready_arm_mask))
                .is_empty()
            && self
                .current_entry_slot
                .intersect(OfferEntrySet::from_bits(observed_entries.progress_mask))
                .is_empty()
            && input.progress_sibling_presence.exists()
        {
            CurrentControllerFrontier::ExcludeCurrent
        } else {
            CurrentControllerFrontier::IncludeCurrent
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn intrinsic_controller_frontier(
        self,
        current_controller: CurrentControllerFrontier,
    ) -> OfferEntrySet {
        let mut ready = self.observed_entries.without(self.controller_entries);
        ready = ready.union(self.current_entry_slot.intersect(self.controller_entries));
        ready = ready.union(self.progress_entries.intersect(self.controller_entries));
        match current_controller {
            CurrentControllerFrontier::IncludeCurrent => ready,
            CurrentControllerFrontier::ExcludeCurrent => {
                ready = ready.without(self.current_entry_slot);
                ready
            }
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn arbitration_frontier(
        self,
        input: OfferAlignmentCandidateInput,
        intrinsic_controller_frontier: OfferEntrySet,
    ) -> OfferEntrySet {
        let mut candidates = self.progress_entries;
        if self.current_matches_route(input.current_entry) {
            candidates = candidates.union(self.current_entry_slot);
        }
        if input.current_entry.is_unrunnable_route() {
            candidates = candidates.union(self.observed_entries.without(self.current_entry_slot));
        }
        candidates.intersect(intrinsic_controller_frontier)
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn ready_frontier(
        self,
        candidates: OfferEntrySet,
    ) -> OfferEntrySet {
        candidates
            .intersect(self.ready_arm_entries)
            .retain_singleton()
    }

    pub(in crate::endpoint::kernel::offer::select_alignment) fn selection(
        self,
        observed_entries: ObservedEntrySet,
        input: OfferAlignmentCandidateInput,
        intrinsic_controller_frontier: OfferEntrySet,
    ) -> OfferAlignmentSelection {
        let candidates = self.arbitration_frontier(input, intrinsic_controller_frontier);
        let ready = self.ready_frontier(candidates);
        let ready_entry_filter = ready.first_entry_idx(observed_entries);
        let candidates = if !ready.is_empty() { ready } else { candidates };
        let controllers = candidates.intersect(self.controller_entries);
        let dynamic_controllers = controllers.intersect(self.dynamic_controller_entries);
        let outcome =
            ClassifiedOfferCandidateSets::new(candidates, controllers, dynamic_controllers)
                .outcome(observed_entries);

        OfferAlignmentSelection {
            ready_entry_filter,
            outcome,
        }
    }
}
