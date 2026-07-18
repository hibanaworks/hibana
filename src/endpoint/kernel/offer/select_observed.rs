use super::{
    CursorEndpoint, OfferScopeSelection, RecvError, RecvResult, Transport, state_index_to_usize,
};
use crate::endpoint::kernel::frontier::checked_state_index;
use crate::global::typestate::InboundFrameKey;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::offer) fn select_current_materialized_ingress_scope(
        &self,
        carried_key: Option<InboundFrameKey>,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let Some(key) = carried_key else {
            return Ok(None);
        };
        let current_idx = self.cursor.index();
        let Some(meta) = self.cursor.try_recv_meta_at(current_idx) else {
            return Ok(None);
        };
        if !key.matches_recv(meta) {
            return Ok(None);
        }
        if self.cursor.node_event_done_for_lane(current_idx, key.lane) {
            return Ok(None);
        }
        let node_scope = self.cursor.node_scope_id_at(current_idx);
        let Some(scope_id) = self
            .cursor
            .route_scope_for_offer_node(node_scope, current_idx)
        else {
            return Ok(None);
        };
        self.offer_scope_selection_for_scope_lane(scope_id, current_idx, key.lane)
            .map(Some)
    }

    pub(in crate::endpoint::kernel::offer) fn select_observed_ingress_route_scope(
        &mut self,
        carried_key: Option<InboundFrameKey>,
        carried_observation: Option<super::lane_port::FrameObservation>,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let Some(key) = carried_key else {
            return Ok(None);
        };
        let current_idx = self.cursor.index();
        let reentry_target = {
            let endpoint = &*self;
            let mut live_arm_for_scope =
                |scope| endpoint.preview_live_selected_arm_for_scope(scope);
            // A completed arm still owns the enclosing iteration whose
            // completion admits reentry; unrelated inner scopes must not leak.
            let mut committed_arm_for_scope = |scope| endpoint.selected_arm_for_scope(scope);
            endpoint.cursor.roll_reentry_recv_index_for_frame(
                key,
                &mut live_arm_for_scope,
                &mut committed_arm_for_scope,
            )
        };
        let (scope_id, target_idx, reentry_observed_target) =
            if let Some(target_idx) = reentry_target {
                let node_scope = self.cursor.node_scope_id_at(target_idx);
                let scope_id = self
                    .cursor
                    .route_scope_for_offer_node(node_scope, target_idx)
                    .ok_or(RecvError::PhaseInvariant)?;
                (scope_id, target_idx, true)
            } else if let Some(scope_id) = match self
                .active_reentry_scope_for_observed_frame(key)
                .map_err(|_| RecvError::PhaseInvariant)?
            {
                Some(active_reentry) => Some(active_reentry),
                None => self
                    .cursor
                    .enclosing_passive_route_scope_for_key(current_idx, key)
                    .map_err(|_| RecvError::PhaseInvariant)?,
            } {
                let target_idx = self
                    .cursor
                    .passive_descendant_target_index_for_key(scope_id, key)
                    .map_err(|_| RecvError::PhaseInvariant)?
                    .ok_or(RecvError::PhaseInvariant)?;
                (scope_id, target_idx, false)
            } else {
                return Ok(None);
            };
        let observed_arm = if reentry_observed_target {
            self.cursor.route_arm_for_index(scope_id, target_idx)
        } else {
            self.cursor
                .passive_descendant_dispatch_arm_for_key(scope_id, key)
                .map_err(|_| RecvError::PhaseInvariant)?
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
            let observation = carried_observation.ok_or(RecvError::PhaseInvariant)?;
            self.emit_materialization_mismatch_observation(
                usize::from(key.lane),
                key.lane,
                super::lane_port::FrameMismatch::label_mismatch(observation),
            );
            return Err(RecvError::PhaseInvariant);
        }
        if target_idx != current_idx {
            self.commit_cursor_realign_index(target_idx)
                .map_err(|_| RecvError::PhaseInvariant)?;
            self.sync_lane_offer_state();
        }
        let effective_arm = match selected_arm_for_observed {
            Some(selected) => Some(selected),
            None => observed_arm,
        };
        let mut selection = self.offer_scope_selection_for_scope_lane_with_selected_arm(
            scope_id,
            target_idx,
            key.lane,
            effective_arm,
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
        let mut info = self.decision_state.lane_offer_state(lane_idx);
        if info.scope.is_none() {
            return Ok(None);
        }
        if !info.entry.is_absent() && state_index_to_usize(info.entry) != self.cursor.index() {
            self.commit_cursor_realign_index(state_index_to_usize(info.entry))
                .map_err(|_| RecvError::PhaseInvariant)?;
            self.sync_lane_offer_state();
            info = self.decision_state.lane_offer_state(lane_idx);
            if info.scope.is_none()
                || info.entry.is_absent()
                || state_index_to_usize(info.entry) != self.cursor.index()
            {
                return Err(RecvError::PhaseInvariant);
            }
        }
        let scope_id = info.scope;
        let current_idx = self.cursor.index();
        self.offer_scope_selection_for_scope_lane(scope_id, current_idx, lane_idx as u8)
            .map(Some)
    }
}
