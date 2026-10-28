//! Offer frontier fact derivation.

mod evidence;
mod planner;

use super::{
    FrontierVisitSet, OfferFrontierFacts, OfferScopeSelection, OperationKernel, Transport,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(super) fn prepare_frontier_facts(
        &mut self,
        selection: OfferScopeSelection,
        frontier_visited: &mut FrontierVisitSet,
    ) -> OfferFrontierFacts {
        let scope_id = selection.scope_id;
        frontier_visited.record(selection.entry_index.as_usize());
        let profile = self.offer_scope_profile(scope_id);
        let evidence = self.offer_ingress_evidence(selection, profile);

        OfferFrontierFacts {
            selection,
            profile: evidence.profile(),
            ingress_mode: evidence.ingress_mode(),
        }
    }
}
