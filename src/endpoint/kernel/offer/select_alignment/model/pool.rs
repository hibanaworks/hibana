use super::super::super::{ObservedEntrySet, OfferSelectPriority};
use super::entry::{CurrentOfferEntry, OfferAlignmentCandidateInput};
use super::set::OfferEntrySet;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct CurrentOfferObservation {
    present: bool,
    ready: bool,
    progress_evidence: bool,
    observed_progress_evidence: bool,
    controller_progress_sibling_exists: bool,
}

impl CurrentOfferObservation {
    #[inline]
    const fn new(
        present: bool,
        ready: bool,
        progress_evidence: bool,
        observed_progress_evidence: bool,
        controller_progress_sibling_exists: bool,
    ) -> Self {
        Self {
            present,
            ready,
            progress_evidence,
            observed_progress_evidence,
            controller_progress_sibling_exists,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn present(self) -> bool {
        self.present
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn ready(self) -> bool {
        self.ready
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn progress_evidence(
        self,
    ) -> bool {
        self.progress_evidence
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn accumulated_progress_evidence(
        self,
        state_evidence: bool,
    ) -> bool {
        self.observed_progress_evidence || state_evidence
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn controller_progress_sibling_exists(
        self,
    ) -> bool {
        self.controller_progress_sibling_exists
    }
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

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum OfferAlignmentOutcome {
    NoCandidate,
    AmbiguousCandidates,
    UniqueDynamicController(usize),
    UniqueController(usize),
    UniqueCandidate(usize),
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel::offer::select_alignment) struct OfferAlignmentSelection {
    hint_filter: Option<usize>,
    outcome: OfferAlignmentOutcome,
}

impl OfferAlignmentSelection {
    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn has_candidate(self) -> bool {
        !matches!(self.outcome, OfferAlignmentOutcome::NoCandidate)
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
        current_is_candidate: bool,
        current_idx: usize,
    ) -> Option<(OfferSelectPriority, usize)> {
        if current_is_candidate {
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
            OfferAlignmentOutcome::NoCandidate | OfferAlignmentOutcome::AmbiguousCandidates => None,
        }
    }
}

struct ClassifiedOfferCandidateSets {
    candidates: OfferEntrySet,
    controllers: OfferEntrySet,
    dynamic_controllers: OfferEntrySet,
}

impl ClassifiedOfferCandidateSets {
    #[inline]
    const fn new(
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

    fn outcome(self, observed_entries: ObservedEntrySet) -> OfferAlignmentOutcome {
        if self.dynamic_controllers.has_one() {
            return self
                .dynamic_controllers
                .first_entry_idx(observed_entries)
                .map(OfferAlignmentOutcome::UniqueDynamicController)
                .unwrap_or(OfferAlignmentOutcome::AmbiguousCandidates);
        }
        if self.controllers.has_one() {
            return self
                .controllers
                .first_entry_idx(observed_entries)
                .map(OfferAlignmentOutcome::UniqueController)
                .unwrap_or(OfferAlignmentOutcome::AmbiguousCandidates);
        }
        if self.candidates.is_empty() {
            return OfferAlignmentOutcome::NoCandidate;
        }
        if self.candidates.has_one() {
            return self
                .candidates
                .first_entry_idx(observed_entries)
                .map(OfferAlignmentOutcome::UniqueCandidate)
                .unwrap_or(OfferAlignmentOutcome::AmbiguousCandidates);
        }
        OfferAlignmentOutcome::AmbiguousCandidates
    }
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
        CurrentOfferObservation::new(
            !self.current_entry_slot.is_empty(),
            self.current_entry_slot.intersects(self.ready_entries),
            self.current_entry_slot.intersects(self.progress_entries),
            self.current_entry_slot.intersects(raw_progress_entries),
            !self
                .progress_entries
                .intersect(self.controller_entries)
                .without(self.current_entry_slot)
                .is_empty(),
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn suppresses_current_controller_without_evidence(
        self,
        observed_entries: ObservedEntrySet,
        input: OfferAlignmentCandidateInput,
    ) -> bool {
        input.current_authority.is_controller()
            && self.current_matches_route(input.current_entry)
            && self
                .current_entry_slot
                .intersect(OfferEntrySet::from_bits(observed_entries.ready_arm_mask))
                .is_empty()
            && self
                .current_entry_slot
                .intersect(OfferEntrySet::from_bits(observed_entries.progress_mask))
                .is_empty()
            && input.progress_sibling_exists
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn static_controller_frontier(
        self,
        suppress_current: bool,
    ) -> OfferEntrySet {
        let mut ready = self.observed_entries.without(self.controller_entries);
        ready = ready.union(self.current_entry_slot.intersect(self.controller_entries));
        ready = ready.union(self.progress_entries.intersect(self.controller_entries));
        if suppress_current {
            ready = ready.without(self.current_entry_slot);
        }
        ready
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn arbitration_frontier(
        self,
        input: OfferAlignmentCandidateInput,
        static_controller_frontier: OfferEntrySet,
    ) -> OfferEntrySet {
        let mut candidates = self.progress_entries;
        if self.current_matches_route(input.current_entry) {
            candidates = candidates.union(self.current_entry_slot);
        }
        if input.current_entry.is_unrunnable_route() {
            candidates = candidates.union(self.observed_entries.without(self.current_entry_slot));
        }
        candidates.intersect(static_controller_frontier)
    }

    #[inline]
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn hinted_frontier(
        self,
        candidates: OfferEntrySet,
    ) -> OfferEntrySet {
        candidates
            .intersect(self.ready_arm_entries)
            .unique_or_empty()
    }

    pub(in crate::endpoint::kernel::offer::select_alignment) fn selection(
        self,
        observed_entries: ObservedEntrySet,
        input: OfferAlignmentCandidateInput,
        static_controller_frontier: OfferEntrySet,
    ) -> OfferAlignmentSelection {
        let candidates = self.arbitration_frontier(input, static_controller_frontier);
        let hinted = self.hinted_frontier(candidates);
        let hint_filter = hinted.first_entry_idx(observed_entries);
        let candidates = if !hinted.is_empty() {
            hinted
        } else {
            candidates
        };
        let controllers = candidates.intersect(self.controller_entries);
        let dynamic_controllers = controllers.intersect(self.dynamic_controller_entries);
        let outcome =
            ClassifiedOfferCandidateSets::new(candidates, controllers, dynamic_controllers)
                .outcome(observed_entries);

        OfferAlignmentSelection {
            hint_filter,
            outcome,
        }
    }
}
