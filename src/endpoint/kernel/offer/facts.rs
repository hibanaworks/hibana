//! Offer frontier fact derivation.

mod evidence;
mod planner;

use super::{
    CursorEndpoint, FrontierVisitSet, OfferFrontierFacts, OfferScopeSelection,
    ScopeFrameLabelScratch, Transport,
};

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
        {
            let mut frame_label_scratch = ScopeFrameLabelScratch::EMPTY;
            self.write_selection_frame_label_meta(selection, &mut frame_label_scratch);
            self.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lanes,
                profile.frame_hint_ingestion(),
                &frame_label_scratch.view(),
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
