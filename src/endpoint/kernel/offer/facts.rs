//! Offer frontier fact derivation.

mod evidence;
mod planner;

use super::{
    Clock, CursorEndpoint, FrontierVisitSet, OfferFrontierFacts, OfferScopeSelection, Transport,
};

impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: Clock,
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
        {
            let frame_label_meta = self.selection_frame_label_meta(selection);
            self.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lanes,
                profile.frame_hint_ingestion(),
                frame_label_meta,
            );
        }
        let evidence = self.offer_ingress_evidence(selection, entry, profile, offer_lanes);

        OfferFrontierFacts {
            selection,
            profile: evidence.profile(),
            ingress_mode: evidence.ingress_mode(),
        }
    }
}
