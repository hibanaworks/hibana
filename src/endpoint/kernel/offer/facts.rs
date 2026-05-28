//! Offer frontier fact derivation.

mod evidence;
mod planner;

use super::{
    BindingSlot, Clock, CursorEndpoint, EpochTable, FrontierVisitSet, LabelUniverse,
    MintConfigMarker, OfferFrontierFacts, OfferScopeSelection, RecvResult, Transport, lane_port,
    profile::OfferEntryPosition,
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(super) fn prepare_frontier_facts(
        &mut self,
        pending_recv: &lane_port::PendingRecv,
        selection: OfferScopeSelection,
        frontier_visited: &mut FrontierVisitSet,
    ) -> RecvResult<OfferFrontierFacts> {
        let scope_id = selection.scope_id;
        frontier_visited.record(scope_id);
        let offer_lane_idx = selection.offer_lane as usize;
        let entry = OfferEntryPosition::from_route_entry(selection.at_route_offer_entry);
        let profile = self.offer_scope_profile(scope_id);
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        {
            let frame_label_meta = self.selection_frame_label_meta(selection);
            self.ingest_scope_evidence_for_offer(
                pending_recv,
                scope_id,
                offer_lane_idx,
                offer_lanes,
                profile.suppresses_scope_frame_hint(),
                frame_label_meta,
            );
        }
        let evidence = self.offer_ingress_evidence(selection, entry, profile, offer_lanes);

        Ok(OfferFrontierFacts {
            selection,
            profile: evidence.profile(),
            ingress_mode: evidence.ingress_mode(),
        })
    }
}
