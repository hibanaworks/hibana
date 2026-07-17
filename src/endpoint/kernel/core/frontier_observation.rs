use super::super::frontier::{
    FrontierKind, frontier_global_active_entries_view_from_storage,
    frontier_observed_entries_view_from_storage,
};
use super::{
    ActiveEntrySet, CursorEndpoint, FrontierScratchLayout, ObservedEntrySet,
    ObservedEntrySetBuilder, Transport, lane_port, state_index_to_usize,
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
    pub(in crate::endpoint::kernel) fn global_active_entries(&mut self) -> ActiveEntrySet {
        let (scratch_ptr, layout, frontier_entry_capacity) = self.global_frontier_scratch_parts();
        /* SAFETY: the current public endpoint operation owns the port's
        frontier scratch lease until the returned builder is sealed below. */
        let mut active_entries = unsafe {
            frontier_global_active_entries_view_from_storage(
                scratch_ptr,
                layout,
                frontier_entry_capacity,
            )
        };
        active_entries.clear();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if info.entry.is_absent() || info.scope.is_none() {
                crate::invariant();
            }
            let entry_idx = state_index_to_usize(info.entry);
            if active_entries.slot_for_entry(entry_idx).is_none()
                && !active_entries.insert_entry(entry_idx, lane_idx as u8)
            {
                crate::invariant();
            }
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        active_entries.seal()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn empty_observed_entries_scratch(
        &mut self,
    ) -> ObservedEntrySetBuilder {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        /* SAFETY: the current public endpoint operation owns the port's
        observation scratch section until the builder is sealed. */
        let mut observed = unsafe {
            frontier_observed_entries_view_from_storage(
                scratch_ptr,
                layout,
                self.cursor.max_frontier_entries(),
            )
        };
        observed.clear();
        observed
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_frontier_progress_sibling_exists(
        &self,
        observed_entries: ObservedEntrySet,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        reentry_controller_evidence: CurrentReentryControllerEvidence,
    ) -> bool {
        let mut slot_idx = 0usize;
        while slot_idx < observed_entries.len() {
            let Some(slot) = observed_entries.slot(slot_idx) else {
                crate::invariant();
            };
            let sibling = observed_entries.entry_idx(slot_idx) != Some(current_entry_idx);
            let frontier_matches = reentry_controller_evidence
                .allows_cross_frontier_progress_sibling()
                || slot.is_in_frontier(current_frontier);
            if sibling && slot.has_progress() && frontier_matches {
                return true;
            }
            slot_idx += 1;
        }
        false
    }
}
