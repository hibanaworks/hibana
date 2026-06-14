use super::super::super::{CurrentFrontierSelectionState, ObservedEntrySet, OfferSelectPriority};
use super::entry::{CurrentOfferEntry, OfferAlignmentCandidateInput};
use super::set::OfferEntrySet;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::endpoint::kernel::offer::select_alignment) enum CurrentControllerFrontier {
    IncludeCurrent,
    ExcludeCurrent,
}

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
    const fn empty() -> Self {
        Self { flags: 0 }
    }

    #[inline]
    const fn with_present(self) -> Self {
        Self {
            flags: self.flags | Self::PRESENT,
        }
    }

    #[inline]
    const fn with_ready(self) -> Self {
        Self {
            flags: self.flags | Self::READY,
        }
    }

    #[inline]
    const fn with_progress_evidence(self) -> Self {
        Self {
            flags: self.flags | Self::PROGRESS_EVIDENCE,
        }
    }

    #[inline]
    const fn with_observed_progress_evidence(self) -> Self {
        Self {
            flags: self.flags | Self::OBSERVED_PROGRESS_EVIDENCE,
        }
    }

    #[inline]
    const fn with_controller_progress_sibling(self) -> Self {
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
    hint_filter: Option<usize>,
    outcome: OfferAlignmentOutcome,
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
    pub(in crate::endpoint::kernel::offer::select_alignment) const fn hinted_frontier(
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
