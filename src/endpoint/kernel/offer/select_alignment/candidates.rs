use super::super::{CurrentFrontierSelectionState, ObservedEntrySet};
use super::model::{
    CurrentOfferAuthority, CurrentOfferCandidateStatus, CurrentOfferEntry, CurrentOfferObservation,
    OfferAlignmentCandidateInput, OfferAlignmentCandidatePool, OfferAlignmentDecision,
    OfferAlignmentSelection, ProgressEvidence, ProgressSiblingPresence,
};
use super::{CursorEndpoint, Transport};
use crate::endpoint::kernel::frontier::FrontierKind;

#[derive(Clone, Copy)]
pub(super) struct OfferAlignmentCandidates {
    current: OfferAlignmentCurrent,
    selection: OfferAlignmentSelection,
}

#[derive(Clone, Copy)]
struct OfferAlignmentCurrent {
    idx: usize,
    entry: CurrentOfferEntry,
    authority: CurrentOfferAuthority,
    progress_sibling_presence: ProgressSiblingPresence,
    observation: CurrentOfferObservation,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CurrentEntryAdmission {
    Eligible,
    Excluded,
}

impl OfferAlignmentCandidates {
    pub(super) fn merge_current_observation(
        &self,
        mut state: CurrentFrontierSelectionState,
    ) -> CurrentFrontierSelectionState {
        self.current.merge_into_state(&mut state);
        state
    }

    pub(super) fn select(
        &self,
        current_state: CurrentFrontierSelectionState,
    ) -> Option<OfferAlignmentDecision> {
        let current = self.current.candidate_status(current_state, self.selection);
        self.selection.select(current, self.current.idx)
    }

    #[inline]
    pub(super) const fn current_entry(&self) -> CurrentOfferEntry {
        self.current.entry
    }

    #[inline]
    pub(super) const fn current_can_remain_after_alignment(
        &self,
        state: CurrentFrontierSelectionState,
    ) -> bool {
        self.current.can_remain_after_alignment(state)
    }
}

impl OfferAlignmentCurrent {
    #[inline]
    const fn new(
        idx: usize,
        entry: CurrentOfferEntry,
        authority: CurrentOfferAuthority,
        progress_sibling_presence: ProgressSiblingPresence,
        observation: CurrentOfferObservation,
    ) -> Self {
        Self {
            idx,
            entry,
            authority,
            progress_sibling_presence,
            observation,
        }
    }

    #[inline]
    const fn admitted(self) -> bool {
        self.observation.selectable()
    }

    #[inline]
    fn merge_into_state(self, state: &mut CurrentFrontierSelectionState) {
        if !self.admitted() {
            return;
        }
        if self.observation.ready() {
            state.record_ready();
        }
        if self.observation.progress_evidence() {
            state.record_progress_evidence();
        }
    }

    #[inline]
    fn candidate_status(
        &self,
        current_state: CurrentFrontierSelectionState,
        selection: OfferAlignmentSelection,
    ) -> CurrentOfferCandidateStatus {
        if !self.admitted() {
            return CurrentOfferCandidateStatus::NotSelectable;
        }

        let current_progress = self
            .observation
            .accumulated_progress_evidence(current_state);
        if !self.current_entry_survives_ready_filter(
            self.current_passive_admission(current_state.frontier, current_progress),
            selection,
        ) {
            return CurrentOfferCandidateStatus::NotSelectable;
        }
        if self.authority.is_controller()
            && current_progress.is_absent()
            && self.progress_sibling_presence.exists()
            && selection.has_candidate()
        {
            return CurrentOfferCandidateStatus::NotSelectable;
        }
        CurrentOfferCandidateStatus::Selectable
    }

    #[inline]
    fn current_entry_survives_ready_filter(
        &self,
        admission: CurrentEntryAdmission,
        selection: OfferAlignmentSelection,
    ) -> bool {
        if !self.entry.is_route_entry()
            || !self.entry.has_offer_lanes()
            || !self.admitted()
            || admission == CurrentEntryAdmission::Excluded
        {
            return false;
        }
        selection.allows_current(self.idx)
    }

    #[inline]
    fn current_passive_admission(
        &self,
        current_frontier: FrontierKind,
        current_progress: ProgressEvidence,
    ) -> CurrentEntryAdmission {
        if current_frontier == FrontierKind::PassiveObserver
            && !self.authority.is_controller()
            && current_progress.is_absent()
            && self.observation.controller_progress_sibling_exists()
        {
            CurrentEntryAdmission::Excluded
        } else {
            CurrentEntryAdmission::Eligible
        }
    }

    #[inline]
    const fn can_remain_after_alignment(self, state: CurrentFrontierSelectionState) -> bool {
        self.observation.permits_retention()
            && self.entry.has_offer_lanes()
            && (self.entry.is_route_entry() || state.ready() || state.has_progress_evidence())
    }
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(super) fn offer_alignment_candidates(
        &self,
        observed_entries: ObservedEntrySet<'_>,
        input: OfferAlignmentCandidateInput,
    ) -> OfferAlignmentCandidates {
        let candidates = OfferAlignmentCandidatePool::from_observed(observed_entries, input);
        let selection = candidates.selection();
        let current = OfferAlignmentCurrent::new(
            input.current_idx,
            input.current_entry,
            input.current_authority,
            input.progress_sibling_presence,
            candidates.current_observation(),
        );

        OfferAlignmentCandidates { current, selection }
    }
}
