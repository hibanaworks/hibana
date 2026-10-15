use super::{
    CursorEndpoint, ObservedIngressSelection, OfferScopeSelection, RecvError, RecvResult,
    Transport, state_index_to_usize,
};
use crate::{
    endpoint::kernel::frontier::checked_state_index,
    global::typestate::{EventAdmissionError, EventArmView, InboundFrameKey},
    runtime_core::UniqueMatch,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    /// Classify one exact user receive without advancing its local step.
    /// Only a projected first-visible branch receive can select an offer.
    /// Ordinary input stays available to recv; future input waits for its
    /// projected predecessor. Both share recv's identity and progress checks.
    pub(in crate::endpoint::kernel::offer) fn select_observed_ingress(
        &mut self,
        carried_key: Option<InboundFrameKey>,
        carried_observation: Option<super::lane_port::FrameObservation>,
    ) -> RecvResult<Option<ObservedIngressSelection>> {
        let Some(key) = carried_key else {
            return Ok(None);
        };
        let current_idx = self.cursor.index();
        let mut matched = UniqueMatch::NONE;
        let mut pending = UniqueMatch::NONE;
        for idx in 0..self.cursor.local_steps_len() {
            let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
                continue;
            };
            if meta.origin.is_session() || !key.matches_recv(meta) {
                continue;
            }
            let lane = usize::from(meta.lane);
            if lane >= self.cursor.logical_lane_count()
                || self.port_for_lane(lane).lane().as_wire() != meta.lane
            {
                return Err(RecvError::PhaseInvariant);
            }
            let preview_conflict = self.cursor.event_conflict_for_index(idx);
            let mut selected =
                |scope, view| self.selected_arm_for_recv_event(preview_conflict, scope, view);
            match self.cursor.event_enabled(idx, meta.into(), &mut selected) {
                Ok(_) => {}
                Err(EventAdmissionError::PredecessorPending) => {
                    pending = pending.add(idx);
                    continue;
                }
                Err(EventAdmissionError::Invariant) => continue,
            }
            let scope = if let Some(scope) = self
                .cursor
                .route_scope_for_offer_node(self.cursor.node_scope_id_at(idx), idx)
                && let Some(arm) = self.cursor.route_arm_for_index(scope, idx)
                && self
                    .cursor
                    .route_arm_lane_first_step(scope, arm, meta.lane)
                    .and_then(|step| self.cursor.node_index_for_relocatable_step(step))
                    == Some(idx)
            {
                Some(scope)
            } else {
                let scope = self
                    .cursor
                    .enclosing_passive_route_scope_for_key(idx, key)
                    .map_err(|_| RecvError::PhaseInvariant)?;
                match scope {
                    Some(scope)
                        if self
                            .cursor
                            .passive_descendant_target_index_for_key(scope, key)
                            .map_err(|_| RecvError::PhaseInvariant)?
                            == Some(idx) =>
                    {
                        Some(scope)
                    }
                    Some(_) | None => None,
                }
            };
            matched = matched.add((scope, idx));
            if matched.is_ambiguous() {
                break;
            }
        }
        let Some((scope_id, target_idx)) = matched
            .finish_optional()
            .map_err(|_| RecvError::PhaseInvariant)?
        else {
            if pending
                .finish_optional()
                .map_err(|_| RecvError::PhaseInvariant)?
                .is_some()
            {
                return Ok(Some(ObservedIngressSelection::PendingReceive(key.lane)));
            }
            return Ok(None);
        };
        let Some(scope_id) = scope_id else {
            return Ok(Some(ObservedIngressSelection::OrdinaryReceive(key.lane)));
        };
        let observed_arm = self.cursor.route_arm_for_index(scope_id, target_idx);
        let preview_conflict = self.cursor.event_conflict_for_index(target_idx);
        let selected =
            self.selected_arm_for_recv_event(preview_conflict, scope_id, EventArmView::Preview);
        if let (Some(selected), Some(observed)) = (selected, observed_arm)
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
        let mut selection = self.offer_scope_selection_for_scope_lane_with_selected_arm(
            scope_id,
            target_idx,
            key.lane,
            observed_arm,
        )?;
        selection.observed_target =
            checked_state_index(target_idx).ok_or(RecvError::PhaseInvariant)?;
        Ok(Some(ObservedIngressSelection::Branch(selection)))
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
