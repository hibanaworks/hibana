use super::{ActiveEntrySet, CursorEndpoint, ObservedEntrySet, Transport};
use crate::endpoint::kernel::frontier::OfferEntryAdmission;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn compose_frontier_observed_entries(
        &mut self,
        active_entries: ActiveEntrySet,
    ) -> ObservedEntrySet {
        let mut composed = self.empty_observed_entries_scratch();
        let mut slot_idx = 0usize;
        while slot_idx < active_entries.len() {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                slot_idx += 1;
                continue;
            };
            if !self.offer_entry_has_active_lanes(entry_idx) {
                slot_idx += 1;
                continue;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                slot_idx += 1;
                continue;
            };
            let (observed_slot, _) = crate::invariant_some(composed.insert_entry(entry_idx));
            composed.record_observation(
                observed_slot,
                observed,
                self.offer_entry_frontier_mask(entry_idx),
                if self.entry_has_route_scope(entry_idx) {
                    OfferEntryAdmission::Selectable
                } else {
                    OfferEntryAdmission::Excluded
                },
            );
            slot_idx += 1;
        }
        composed.seal()
    }
}
