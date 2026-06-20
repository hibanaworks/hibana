use super::{ActiveEntrySet, CursorEndpoint, ObservedEntrySet, Transport};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn compose_frontier_observed_entries(
        &mut self,
        active_entries: ActiveEntrySet,
    ) -> ObservedEntrySet {
        let mut composed = self.empty_observed_entries_scratch();
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T>::next_slot_in_mask(&mut remaining_slots)
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            if self.offer_entry_state_snapshot(entry_idx).is_none()
                || !self.offer_entry_has_active_lanes(entry_idx)
            {
                continue;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                continue;
            };
            let Some((observed_bit, _)) = composed.insert_entry(entry_idx) else {
                continue;
            };
            composed.observe_with_frontier_mask(
                observed_bit,
                observed,
                self.offer_entry_frontier_mask(entry_idx),
            );
        }
        composed
    }
}
