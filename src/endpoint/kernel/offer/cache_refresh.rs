use super::{ActiveEntrySet, CursorEndpoint, ObservedEntrySet, OfferEntryKey, Transport};
use crate::endpoint::kernel::frontier::{
    ExactOfferObservation, FrontierObservationSlot, FrontierScratchSectionLease,
    OfferEntryAdmission,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn compose_frontier_observed_entries<'a>(
        &self,
        active_entries: ActiveEntrySet<'_>,
        current_key: Option<OfferEntryKey>,
        scratch: &'a mut FrontierScratchSectionLease<'_, FrontierObservationSlot>,
    ) -> (ObservedEntrySet<'a>, Option<ExactOfferObservation>) {
        let mut composed = self.empty_observed_entries_scratch(scratch);
        let mut current = None;
        let mut slot_idx = 0usize;
        while slot_idx < active_entries.len() {
            let slot = crate::invariant_some(active_entries.slot_at(slot_idx));
            let (active, observed) = self.scan_active_offer_entry_non_consuming(slot);
            let admission = if self.cursor.has_route_scope(active.scope()) {
                OfferEntryAdmission::Selectable
            } else {
                OfferEntryAdmission::Excluded
            };
            if let Some(target) = current_key
                && let Some(exact) = ExactOfferObservation::from_target(target, observed, admission)
                && current.replace(exact).is_some()
            {
                crate::invariant();
            }
            composed.push_exact_observation(observed, admission);
            slot_idx += 1;
        }
        (composed.seal(), current)
    }
}
