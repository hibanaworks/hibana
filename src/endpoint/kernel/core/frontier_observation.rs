use super::super::frontier::{
    ActiveEntrySlot, FrontierKind, FrontierObservationSlot, FrontierScratchSectionLease,
    frontier_global_active_entries_view, frontier_observed_entries_view,
};
use super::{ActiveEntrySet, CursorEndpoint, ObservedEntrySet, ObservedEntrySetBuilder, Transport};
use crate::endpoint::kernel::offer::CurrentReentryControllerEvidence;

#[inline]
fn is_selectable_progress_sibling(
    slot: crate::endpoint::kernel::frontier::FrontierObservationSlot,
    entry_idx: Option<usize>,
    current_entry_idx: usize,
    current_frontier: FrontierKind,
    reentry_controller_evidence: CurrentReentryControllerEvidence,
) -> bool {
    entry_idx != Some(current_entry_idx)
        && slot.is_selectable()
        && slot.has_progress()
        && (reentry_controller_evidence.allows_cross_frontier_progress_sibling()
            || slot.is_in_frontier(current_frontier))
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn global_active_entries<'a>(
        &self,
        scratch: &'a mut FrontierScratchSectionLease<'_, ActiveEntrySlot>,
    ) -> ActiveEntrySet<'a> {
        let mut active_entries = frontier_global_active_entries_view(scratch);
        active_entries.clear();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if info.entry.is_absent() || info.scope.is_none() {
                crate::invariant();
            }
            let key = crate::invariant_some(info.key());
            active_entries.insert_key(key, lane_idx as u8);
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        active_entries.seal()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn empty_observed_entries_scratch<'a>(
        &self,
        scratch: &'a mut FrontierScratchSectionLease<'_, FrontierObservationSlot>,
    ) -> ObservedEntrySetBuilder<'a> {
        let mut observed = frontier_observed_entries_view(scratch);
        observed.clear();
        observed
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_frontier_progress_sibling_exists(
        &self,
        observed_entries: ObservedEntrySet<'_>,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        reentry_controller_evidence: CurrentReentryControllerEvidence,
    ) -> bool {
        let mut slot_idx = 0usize;
        while slot_idx < observed_entries.len() {
            let Some(slot) = observed_entries.slot(slot_idx) else {
                crate::invariant();
            };
            if is_selectable_progress_sibling(
                slot,
                observed_entries.entry_idx(slot_idx),
                current_entry_idx,
                current_frontier,
                reentry_controller_evidence,
            ) {
                return true;
            }
            slot_idx += 1;
        }
        false
    }
}

#[cfg(any(kani, all(test, hibana_repo_tests)))]
mod tests {
    use super::*;
    use crate::endpoint::kernel::frontier::{
        FrontierObservationSlot, OfferEntryAdmission, OfferEntryKey, OfferEntryObservedState,
    };
    use crate::global::{const_dsl::ScopeId, typestate::StateIndex};

    fn excluded_progress() -> FrontierObservationSlot {
        FrontierObservationSlot::from_exact_observation(
            OfferEntryObservedState {
                key: OfferEntryKey::new(ScopeId::route(1), StateIndex::new(7))
                    .expect("exact route key"),
                frontier_mask: FrontierKind::Reentry.bit(),
                flags: OfferEntryObservedState::FLAG_PROGRESS,
            },
            OfferEntryAdmission::Excluded,
        )
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[test]
    fn excluded_scope_cannot_supply_progress_sibling_authority() {
        assert!(!is_selectable_progress_sibling(
            excluded_progress(),
            Some(7),
            9,
            FrontierKind::Reentry,
            CurrentReentryControllerEvidence::ProgressEvidenceAbsent,
        ));
    }

    #[cfg(kani)]
    #[kani::proof]
    fn excluded_scope_never_supplies_progress_sibling_authority() {
        let current_entry: usize = kani::any();
        let observed_entry: usize = kani::any();
        let frontier = if kani::any() {
            FrontierKind::Route
        } else {
            FrontierKind::Reentry
        };
        let controller_evidence = if kani::any() {
            CurrentReentryControllerEvidence::ProgressSatisfiedOrNotController
        } else {
            CurrentReentryControllerEvidence::ProgressEvidenceAbsent
        };
        assert!(!is_selectable_progress_sibling(
            excluded_progress(),
            Some(observed_entry),
            current_entry,
            frontier,
            controller_evidence,
        ));
    }
}
