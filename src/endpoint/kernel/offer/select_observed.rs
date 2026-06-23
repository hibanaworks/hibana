use super::{
    CursorEndpoint, OfferScopeSelection, RecvError, RecvResult, Transport, state_index_to_usize,
};
use crate::endpoint::kernel::frontier::checked_state_index;

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
        &mut self,
        carried_lane: Option<u8>,
        carried_frame_label: Option<u8>,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let (Some(lane), Some(frame_label)) = (carried_lane, carried_frame_label) else {
            return Ok(None);
        };
        let current_idx = self.cursor.index();
        let reentry_target = {
            let endpoint = &*self;
            let mut selected_arm_for_scope =
                |scope| endpoint.preview_live_selected_arm_for_scope(scope);
            endpoint.cursor.roll_reentry_recv_index_for_frame(
                lane,
                frame_label,
                &mut selected_arm_for_scope,
            )
        };
        let (scope_id, target_idx, reentry_observed_target) = if let Some(target_idx) =
            reentry_target
        {
            let node_scope = self.cursor.node_scope_id_at(target_idx);
            let scope_id = self
                .cursor
                .route_scope_for_offer_node(node_scope, target_idx)
                .ok_or(RecvError::PhaseInvariant)?;
            (scope_id, target_idx, true)
        } else if let Some(scope_id) = self
            .active_reentry_scope_for_observed_frame(lane, frame_label)
            .or_else(|| {
                self.cursor
                    .enclosing_passive_route_scope_for_exact_frame_label(
                        current_idx,
                        lane,
                        frame_label,
                    )
            })
        {
            let target_idx = self
                .cursor
                .passive_descendant_target_index_from_exact_frame_label(scope_id, lane, frame_label)
                .ok_or(RecvError::PhaseInvariant)?;
            (scope_id, target_idx, false)
        } else {
            return Ok(None);
        };
        let observed_arm = if reentry_observed_target {
            self.cursor.route_arm_for_index(scope_id, target_idx)
        } else {
            self.cursor
                .passive_descendant_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label)
        };
        let selected_arm_for_observed = {
            let preview_conflict = self.cursor.event_conflict_for_index(target_idx);
            let endpoint = &*self;
            let mut selected_arm_for_scope =
                |scope| endpoint.preview_live_selected_arm_for_scope(scope);
            self.cursor.selected_arm_for_reentry_preview_conflict(
                scope_id,
                preview_conflict,
                &mut selected_arm_for_scope,
            )
        };
        if let (Some(selected), Some(observed)) = (selected_arm_for_observed, observed_arm)
            && selected != observed
        {
            return Ok(None);
        }
        if target_idx != current_idx {
            self.commit_cursor_realign_index(target_idx)
                .map_err(|_| RecvError::PhaseInvariant)?;
            self.sync_lane_offer_state();
        }
        let mut selection = self.offer_scope_selection_for_scope_lane_with_selected_arm(
            scope_id,
            target_idx,
            lane,
            selected_arm_for_observed.or(observed_arm),
        )?;
        selection.observed_target =
            checked_state_index(target_idx).ok_or(RecvError::PhaseInvariant)?;
        Ok(Some(selection))
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
}
