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
            let slot = crate::invariant_some(active_entries.slot_at(slot_idx));
            let (active, observed, _) = self.scan_active_offer_entry_non_consuming(slot);
            let entry_idx = active.entry().as_usize();
            composed.push_exact_observation(
                observed,
                if self
                    .cursor
                    .route_scope_present_for_entry(entry_idx, Some(active.scope()))
                {
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
