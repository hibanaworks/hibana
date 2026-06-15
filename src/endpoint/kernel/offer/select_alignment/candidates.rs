use super::super::{CurrentFrontierSelectionState, ObservedEntrySet, OfferSelectPriority};
use super::model::{
    CurrentOfferAuthority, CurrentOfferCandidateStatus, CurrentOfferEntry, CurrentOfferObservation,
    OfferAlignmentCandidateInput, OfferAlignmentCandidatePool, OfferAlignmentSelection,
    OfferEntrySet, ProgressEvidence, ProgressSiblingPresence,
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
    ) -> Option<(OfferSelectPriority, usize)> {
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
    const fn present(self) -> bool {
        self.observation.present()
    }

    #[inline]
    fn merge_into_state(self, state: &mut CurrentFrontierSelectionState) {
        if !self.present() {
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
        if !self.present() {
            return CurrentOfferCandidateStatus::NotSelectable;
        }

        let current_progress = self
            .observation
            .accumulated_progress_evidence(current_state);
        if !self.current_entry_survives_hint_filter(
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
    fn current_entry_survives_hint_filter(
        &self,
        admission: CurrentEntryAdmission,
        selection: OfferAlignmentSelection,
    ) -> bool {
        if !self.entry.is_route_entry()
            || !self.entry.has_offer_lanes()
            || !self.present()
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
        self.entry.has_offer_lanes()
            && (self.entry.is_route_entry() || state.ready() || state.has_progress_evidence())
    }
}

impl<'r, const ROLE: u8, T, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    pub(super) fn offer_alignment_candidates(
        &self,
        observed_entries: ObservedEntrySet,
        input: OfferAlignmentCandidateInput,
    ) -> OfferAlignmentCandidates {
        let candidates = OfferAlignmentCandidatePool::from_observed(
            observed_entries,
            self.selectable_observed_offer_entries(observed_entries),
            input,
        );
        let current_controller_frontier =
            candidates.current_controller_frontier(observed_entries, input);
        let intrinsic_controller_frontier =
            candidates.intrinsic_controller_frontier(current_controller_frontier);
        let selection =
            candidates.selection(observed_entries, input, intrinsic_controller_frontier);
        let current = OfferAlignmentCurrent::new(
            input.current_idx,
            input.current_entry,
            input.current_authority,
            input.progress_sibling_presence,
            candidates.current_observation(observed_entries),
        );

        OfferAlignmentCandidates { current, selection }
    }

    fn selectable_observed_offer_entries(
        &self,
        observed_entries: ObservedEntrySet,
    ) -> OfferEntrySet {
        let mut selectable = OfferEntrySet::empty();
        let mut slot_idx = 0usize;
        let observed_len = observed_entries.len();
        while slot_idx < observed_len {
            let slot = OfferEntrySet::slot(slot_idx);
            if let Some(entry_idx) = slot.first_entry_idx(observed_entries)
                && self.entry_has_route_scope(entry_idx)
            {
                selectable = selectable.union(slot);
            }
            slot_idx += 1;
        }
        selectable
    }
}
