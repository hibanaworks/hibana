use super::{
    CursorEndpoint, OfferEntryPosition, OfferScopeSelection, RecvError, RecvResult, ScopeId,
    Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::offer) fn offer_scope_selection_for_scope_lane(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        offer_lane: u8,
    ) -> RecvResult<OfferScopeSelection> {
        self.offer_scope_selection_for_scope_lane_with_selected_arm(
            scope_id,
            current_idx,
            offer_lane,
            self.preview_live_selected_arm_for_scope(scope_id),
        )
    }

    pub(in crate::endpoint::kernel::offer) fn offer_scope_selection_for_scope_lane_with_selected_arm(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        offer_lane: u8,
        selected_arm: Option<u8>,
    ) -> RecvResult<OfferScopeSelection> {
        let lane_idx = usize::from(offer_lane);
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        if !self
            .cursor
            .route_offer_entry_allows_current(scope_id, current_idx, selected_arm)
        {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if !offer_lanes.contains(lane_idx) {
            return Err(RecvError::PhaseInvariant);
        }
        let entry_position = if crate::invariant_some(
            self.cursor
                .route_offer_entry_cursor_position(scope_id, current_idx, selected_arm),
        )
        .is_at_entry()
        {
            OfferEntryPosition::RouteEntry
        } else {
            OfferEntryPosition::AfterRouteEntry
        };
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root: CursorEndpoint::<ROLE, T>::parallel_scope_root(
                &self.cursor,
                scope_id,
            ),
            offer_lane,
            entry_position,
            observed_target: crate::global::typestate::StateIndex::ABSENT,
        })
    }
}
