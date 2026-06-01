use super::super::{CurrentFrontierSelectionState, ObservedEntrySet, OfferSelectPriority};
use super::model::{
    CurrentOfferAuthority, CurrentOfferEntry, CurrentOfferObservation,
    OfferAlignmentCandidateInput, OfferAlignmentCandidatePool, OfferAlignmentSelection,
    OfferEntrySet,
};
use super::{
    Clock, CursorEndpoint, EndpointSlot, EpochTable, LabelUniverse, MintConfigMarker, Transport,
};
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
    progress_sibling_exists: bool,
    observation: CurrentOfferObservation,
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
        let current_is_candidate = self.current.is_candidate(current_state, self.selection);
        self.selection.select(
            current_is_candidate && self.current.present(),
            self.current.idx,
        )
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
        progress_sibling_exists: bool,
        observation: CurrentOfferObservation,
    ) -> Self {
        Self {
            idx,
            entry,
            authority,
            progress_sibling_exists,
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
        state.ready |= self.observation.ready();
        state.has_progress_evidence |= self.observation.progress_evidence();
    }

    #[inline]
    fn is_candidate(
        &self,
        current_state: CurrentFrontierSelectionState,
        selection: OfferAlignmentSelection,
    ) -> bool {
        let current_has_evidence = self
            .observation
            .accumulated_progress_evidence(current_state.has_progress_evidence);
        if !self.current_entry_survives_hint_filter(
            self.suppresses_current_passive_without_evidence(
                current_state.frontier,
                current_has_evidence,
            ),
            selection,
        ) {
            return false;
        }
        if self.authority.is_controller()
            && !current_has_evidence
            && self.progress_sibling_exists
            && selection.has_candidate()
        {
            return false;
        }
        true
    }

    #[inline]
    fn current_entry_survives_hint_filter(
        &self,
        suppress_current: bool,
        selection: OfferAlignmentSelection,
    ) -> bool {
        if !self.entry.is_route_entry()
            || !self.entry.has_offer_lanes()
            || !self.present()
            || suppress_current
        {
            return false;
        }
        selection.allows_current(self.idx)
    }

    #[inline]
    fn suppresses_current_passive_without_evidence(
        &self,
        current_frontier: FrontierKind,
        current_has_evidence: bool,
    ) -> bool {
        current_frontier == FrontierKind::PassiveObserver
            && !self.authority.is_controller()
            && !current_has_evidence
            && self.observation.controller_progress_sibling_exists()
    }

    #[inline]
    const fn can_remain_after_alignment(self, state: CurrentFrontierSelectionState) -> bool {
        self.entry.has_offer_lanes()
            && (self.entry.is_route_entry() || state.ready || state.has_progress_evidence)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
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
        let suppress_current_controller =
            candidates.suppresses_current_controller_without_evidence(observed_entries, input);
        let static_controller_frontier =
            candidates.static_controller_frontier(suppress_current_controller);
        let selection = candidates.selection(observed_entries, input, static_controller_frontier);
        let current = OfferAlignmentCurrent::new(
            input.current_idx,
            input.current_entry,
            input.current_authority,
            input.progress_sibling_exists,
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
