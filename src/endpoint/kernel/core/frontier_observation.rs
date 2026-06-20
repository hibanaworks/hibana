use super::super::frontier::{
    FrontierKind, OfferEntryObservedState, frontier_global_active_entries_view_from_storage,
    frontier_observed_entries_view_from_storage,
};
use super::{
    ActiveEntrySet, CursorEndpoint, FrontierScratchLayout, FrontierScratchState, ObservedEntrySet,
    Transport, lane_port,
};
use crate::endpoint::kernel::offer::CurrentReentryControllerEvidence;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn global_frontier_scratch_parts(
        &self,
    ) -> (*mut [u8], FrontierScratchLayout, usize) {
        let port = self.port_for_lane(self.primary_lane);
        (
            lane_port::frontier_scratch_ptr(port),
            self.cursor.frontier_scratch_layout(),
            self.cursor.max_frontier_entries(),
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn init_global_frontier_scratch_if_needed(&mut self) {
        if self
            .frontier_state
            .global_frontier_scratch_state
            .is_initialized()
        {
            return;
        }
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        let mut active_entries = frontier_global_active_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        );
        active_entries.clear();
        self.frontier_state.global_frontier_scratch_state = FrontierScratchState::Initialized;
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn global_active_entries(&self) -> ActiveEntrySet {
        if !self
            .frontier_state
            .global_frontier_scratch_state
            .is_initialized()
        {
            return ActiveEntrySet::EMPTY;
        }
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        frontier_global_active_entries_view_from_storage(
            scratch_ptr,
            layout,
            frontier_entry_capacity,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn empty_observed_entries_scratch(
        &mut self,
    ) -> ObservedEntrySet {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        let mut observed = frontier_observed_entries_view_from_storage(
            scratch_ptr,
            layout,
            self.cursor.max_frontier_entries(),
        );
        observed.clear();
        observed
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn recompute_offer_entry_observed_state_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return None;
        }
        let evidence = self.preview_offer_entry_evidence_non_consuming(entry_idx);
        let (observed, _) = self.offer_entry_candidate_from_observation(entry_idx, evidence);
        Some(observed)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_frontier_progress_sibling_exists(
        &self,
        observed_entries: ObservedEntrySet,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        reentry_controller_evidence: CurrentReentryControllerEvidence,
    ) -> bool {
        let mut sibling_mask = observed_entries.progress_mask;
        sibling_mask &= !observed_entries.entry_bit(current_entry_idx);
        if !reentry_controller_evidence.allows_cross_frontier_progress_sibling() {
            sibling_mask &= observed_entries.frontier_mask(current_frontier);
        }
        sibling_mask != 0
    }
}
