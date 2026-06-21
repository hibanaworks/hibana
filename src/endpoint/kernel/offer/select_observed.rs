use super::{
    CursorEndpoint, OfferEntryPosition, OfferScopeSelection, RecvError, RecvResult, ScopeId,
    Transport, state_index_to_usize,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::offer) fn select_current_materialized_ingress_scope(
        &self,
        carried_lane: Option<u8>,
        carried_frame_label: Option<u8>,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let (Some(lane), Some(frame_label)) = (carried_lane, carried_frame_label) else {
            return Ok(None);
        };
        let current_idx = self.cursor.index();
        let Some(meta) = self.cursor.try_recv_meta_at(current_idx) else {
            return Ok(None);
        };
        if meta.lane != lane || meta.frame_label != frame_label {
            return Ok(None);
        }
        let node_scope = self.cursor.node_scope_id_at(current_idx);
        let Some(scope_id) = self
            .cursor
            .route_scope_for_offer_node(node_scope, current_idx)
        else {
            return Ok(None);
        };
        self.offer_scope_selection_for_scope_lane(scope_id, current_idx, lane)
            .map(Some)
    }

    pub(in crate::endpoint::kernel::offer) fn select_observed_ingress_route_scope(
        &self,
        carried_lane: Option<u8>,
        carried_frame_label: Option<u8>,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let (Some(lane), Some(frame_label)) = (carried_lane, carried_frame_label) else {
            return Ok(None);
        };
        let current_idx = self.cursor.index();
        let Some(scope_id) = self
            .cursor
            .enclosing_passive_route_scope_for_exact_frame_label(current_idx, lane, frame_label)
        else {
            return Ok(None);
        };
        self.offer_scope_selection_for_scope_lane(scope_id, current_idx, lane)
            .map(Some)
    }

    pub(in crate::endpoint::kernel::offer) fn select_carried_ingress_scope(
        &mut self,
        carried_lane: Option<u8>,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let Some(lane_idx) = carried_lane.map(usize::from) else {
            return Ok(None);
        };
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let info = self.decision_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            return Ok(None);
        }
        if !info.entry.is_absent() && state_index_to_usize(info.entry) != self.cursor.index() {
            self.commit_cursor_realign_index(state_index_to_usize(info.entry))
                .map_err(|_| RecvError::PhaseInvariant)?;
            self.sync_lane_offer_state();
            return self.select_carried_ingress_scope(carried_lane);
        }
        let scope_id = info.scope;
        let current_idx = self.cursor.index();
        self.offer_scope_selection_for_scope_lane(scope_id, current_idx, lane_idx as u8)
            .map(Some)
    }

    pub(in crate::endpoint::kernel::offer) fn offer_scope_selection_for_scope_lane(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        offer_lane: u8,
    ) -> RecvResult<OfferScopeSelection> {
        let lane_idx = usize::from(offer_lane);
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        if !self.cursor.route_offer_entry_allows_current(
            scope_id,
            current_idx,
            self.selected_arm_for_scope(scope_id),
        ) {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if !offer_lanes.contains(lane_idx) {
            return Err(RecvError::PhaseInvariant);
        }
        let route_offer_entry_matches = crate::invariant_some(
            self.cursor
                .route_offer_entry_cursor_position(scope_id, current_idx),
        )
        .is_at_entry();
        let entry_position = if route_offer_entry_matches {
            OfferEntryPosition::RouteEntry
        } else {
            OfferEntryPosition::AfterRouteEntry
        };
        let frontier_parallel_root =
            CursorEndpoint::<ROLE, T>::parallel_scope_root(&self.cursor, scope_id);
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root,
            offer_lane,
            entry_position,
        })
    }
}
