//! Offer frontier fact derivation.

mod evidence;
mod planner;

use super::{CursorEndpoint, FrontierVisitSet, OfferFrontierFacts, OfferScopeSelection, Transport};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(super) fn prepare_frontier_facts(
        &mut self,
        selection: OfferScopeSelection,
        frontier_visited: &mut FrontierVisitSet,
    ) -> OfferFrontierFacts {
        let scope_id = selection.scope_id;
        frontier_visited.record(scope_id);
        let entry = selection.entry_position;
        let profile = self.offer_scope_profile(scope_id);
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let evidence = self.offer_ingress_evidence(selection, entry, profile, offer_lanes);

        OfferFrontierFacts {
            selection,
            profile: evidence.profile(),
            ingress_mode: evidence.ingress_mode(),
        }
    }
}
