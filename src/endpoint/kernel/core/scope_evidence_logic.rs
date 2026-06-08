use super::{
    Arm, ControlSemanticsTable, CursorEndpoint, EpochTable, EventCursor, EvidenceFingerprint,
    LabelUniverse, Lane, MintConfigMarker, Port, RouteArmToken, ScopeArmMaterializationMeta,
    ScopeEvidence, ScopeFrameLabelMeta, ScopeId, ScopeLoopMeta, Transport, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn bump_scope_evidence_generation(&mut self, slot: usize) {
        self.decision_state.scope_evidence.bump_generation(slot);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn bump_scope_evidence_generation_for_scope(
        &mut self,
        scope_id: ScopeId,
        slot: usize,
    ) {
        self.bump_scope_evidence_generation(slot);
        self.refresh_frontier_observation_cache_for_scope(scope_id);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_slot_for_route(
        &self,
        scope_id: ScopeId,
    ) -> Option<usize> {
        self.cursor.route_scope_slot(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_evidence_generation_for_scope(
        &self,
        scope_id: ScopeId,
    ) -> u16 {
        self.decision_state
            .scope_evidence
            .generation_for_slot(self.scope_slot_for_route(scope_id))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn peek_scope_ack(
        &self,
        scope_id: ScopeId,
    ) -> Option<RouteArmToken> {
        let slot = self.scope_slot_for_route(scope_id)?;
        self.decision_state.scope_evidence.peek_ack(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn take_scope_ack(
        &mut self,
        scope_id: ScopeId,
    ) -> Option<RouteArmToken> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let token = self.decision_state.scope_evidence.take_ack(slot);
        if token.is_some() {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
        token
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_scope_frame_hint(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self
                .decision_state
                .scope_evidence
                .record_frame_hint(slot, lane, frame_label)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_dynamic_scope_frame_hint(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self
                .decision_state
                .scope_evidence
                .record_dynamic_frame_hint(slot, lane, frame_label)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_inner(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
        poll_ready: bool,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self
                .decision_state
                .scope_evidence
                .mark_ready_arm(slot, arm, poll_ready)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn loop_control_evidence_only(
        &self,
        frame_label_meta: ScopeFrameLabelMeta,
        arm: u8,
    ) -> bool {
        frame_label_meta.loop_meta().loop_label_scope()
            && frame_label_meta.loop_meta().arm_has_recv(arm)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn static_passive_scope_evidence_materializes_poll(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        self.cursor
            .static_passive_scope_evidence_materializes_poll(scope_id)
    }

    pub(in crate::endpoint::kernel) fn passive_dispatch_arm_from_exact_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<u8> {
        self.cursor
            .passive_descendant_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label)
    }

    pub(in crate::endpoint::kernel) fn static_passive_descendant_dispatch_arm_from_exact_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<u8> {
        self.cursor
            .passive_descendant_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.decision_state.scope_evidence.ready_arm_mask(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_poll_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.decision_state.scope_evidence.poll_ready_arm_mask(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_has_ready_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> bool {
        (self.scope_ready_arm_mask(scope_id) & ScopeEvidence::arm_bit(arm)) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_has_ready_arm_evidence(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        self.scope_ready_arm_mask(scope_id) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn consume_scope_ready_arm(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self
                .decision_state
                .scope_evidence
                .consume_ready_arm(slot, arm)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn peek_scope_frame_hint(
        &self,
        scope_id: ScopeId,
    ) -> Option<u8> {
        let slot = self.scope_slot_for_route(scope_id)?;
        self.decision_state.scope_evidence.peek_frame_hint(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn peek_scope_frame_hint_with_lane(
        &self,
        scope_id: ScopeId,
    ) -> Option<(u8, u8)> {
        let slot = self.scope_slot_for_route(scope_id)?;
        self.decision_state
            .scope_evidence
            .peek_frame_hint_with_lane(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_scope_evidence(&mut self, scope_id: ScopeId) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self.decision_state.scope_evidence.clear(slot)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_evidence_conflicted(&self, scope_id: ScopeId) -> bool {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return false;
        };
        self.decision_state.scope_evidence.conflicted(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn lane_for_label_or_offer(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> u8 {
        if let Some((lane_idx, _)) = self.cursor.pending_step_for_label(target_label) {
            lane_idx as u8
        } else {
            self.offer_lane_for_scope(scope_id)
        }
    }

    pub(in crate::endpoint::kernel) fn policy_signals_for_slot(
        &self,
        slot: crate::policy_runtime::PolicySlot,
    ) -> crate::transport::context::PolicySignals {
        let _ = slot;
        crate::transport::context::PolicySignals::ZERO
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn evidence_fingerprint(
        &self,
        scope_id: ScopeId,
        ingress_ready: bool,
    ) -> EvidenceFingerprint {
        EvidenceFingerprint::new(
            self.peek_scope_ack(scope_id).is_some(),
            self.scope_has_ready_arm_evidence(scope_id),
            ingress_ready,
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn poll_arm_from_ready_mask(
        &self,
        scope_id: ScopeId,
    ) -> Option<Arm> {
        let mask = self.scope_poll_ready_arm_mask(scope_id);
        if mask.count_ones() != 1 {
            return None;
        }
        Arm::new(mask.trailing_zeros() as u8)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_has_controller_arm_entry(
        cursor: &EventCursor,
        scope_id: ScopeId,
    ) -> bool {
        cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn current_recv_is_scope_local(
        cursor: &EventCursor,
        _semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
        lane: u8,
        frame_label: u8,
        semantic: crate::global::compiled::images::ControlSemanticKind,
        arm: u8,
    ) -> bool {
        cursor.current_recv_matches_scope_arm(
            scope_id,
            lane,
            frame_label,
            loop_meta.loop_label_scope(),
            semantic,
            arm,
        )
    }

    pub(in crate::endpoint::kernel) fn scope_evidence_frame_label_to_arm(
        frame_label_meta: ScopeFrameLabelMeta,
        frame_label: u8,
    ) -> Option<u8> {
        frame_label_meta.evidence_arm_for_frame_label(frame_label)
    }

    #[inline]
    fn scope_arm_has_recv_with_dispatch(
        cursor: &EventCursor,
        scope_id: ScopeId,
        arm: u8,
        dispatch_has_arm: bool,
    ) -> bool {
        if cursor.route_scope_arm_recv_index(scope_id, arm).is_some() {
            return true;
        }
        if let Some((entry, _)) = cursor.controller_arm_entry_by_arm(scope_id, arm) {
            if cursor.is_recv_at(state_index_to_usize(entry)) {
                return true;
            }
        }
        if let Some(entry_idx) = cursor.passive_observer_arm_entry_index(scope_id, arm) {
            return cursor.is_recv_at(entry_idx);
        }
        dispatch_has_arm
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_has_recv_with_materialization(
        &self,
        scope_id: ScopeId,
        arm: u8,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> bool {
        if Self::scope_arm_has_recv_with_dispatch(
            &self.cursor,
            scope_id,
            arm,
            materialization_meta.arm_has_first_recv_dispatch(arm),
        ) {
            return true;
        }
        self.preview_passive_materialization_index_for_selected_arm(scope_id, arm)
            .map(|target_idx| self.cursor.try_recv_meta_at(target_idx).is_some())
            .unwrap_or(false)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_requires_materialization_ready_evidence(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> bool {
        let at_scope_offer_entry = self
            .cursor
            .route_scope_offer_entry(scope_id)
            .map(|entry| entry.is_max() || self.cursor.index() == state_index_to_usize(entry))
            .unwrap_or(true);
        if self.cursor.is_route_controller(scope_id) && at_scope_offer_entry {
            if let Some((entry, _)) = self.cursor.controller_arm_entry_by_arm(scope_id, arm) {
                if let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry)) {
                    return recv_meta.peer != ROLE;
                }
                return false;
            }
        }
        if let Some(entry_idx) = self.cursor.passive_observer_arm_entry_index(scope_id, arm) {
            let Some(recv_meta) = self.cursor.try_recv_meta_at(entry_idx) else {
                return false;
            };
            if recv_meta.peer == ROLE {
                return false;
            }
            if recv_meta.is_control {
                if let Some((_controller_entry, controller_label)) =
                    self.cursor.controller_arm_entry_by_arm(scope_id, arm)
                    && recv_meta.label == controller_label
                {
                    return false;
                }
                if !self.cursor.is_route_controller(scope_id)
                    && self.control_semantic_kind(recv_meta.semantic).is_loop()
                {
                    return false;
                }
            }
            return true;
        }
        self.cursor
            .route_scope_arm_recv_index(scope_id, arm)
            .is_some()
    }

    pub(in crate::endpoint::kernel) fn take_frame_hint_for_lane(
        &mut self,
        lane_idx: usize,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) -> Option<u8> {
        if suppress_hint {
            return None;
        }
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let port = self.port_for_lane(lane_idx);
        let frame_hint_mask = frame_label_meta.frame_hint_mask();
        let taken = if !port.has_route_hint_for_frame_label_mask(self.sid, frame_hint_mask) {
            None
        } else {
            port.take_route_hint_for_frame_label_mask(self.sid, frame_hint_mask)
        };
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        taken
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn pending_scope_ack_lane_mask(
        &self,
        lane_idx: usize,
        scope_id: ScopeId,
        offer_lane_idx: usize,
    ) -> bool {
        self.ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(|port| {
                port.has_pending_route_arm_selection_for_lane(
                    scope_id,
                    ROLE,
                    Lane::new(offer_lane_idx as u32),
                )
            })
            .unwrap_or(false)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn pending_scope_frame_hint_on_lane(
        &mut self,
        lane_idx: usize,
        frame_label_meta: ScopeFrameLabelMeta,
    ) -> bool {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let Some(port) = self.ports.get(lane_idx).and_then(|port| port.as_ref()) else {
            return false;
        };
        let pending = port.has_pending_route_hint_for_lane(
            self.sid,
            frame_label_meta.frame_hint_mask(),
            Lane::new(lane_idx as u32),
        );
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        pending
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn preview_scope_ack_token_non_consuming(
        &self,
        scope_id: ScopeId,
        offer_lanes: crate::global::role_program::LaneSetView,
    ) -> Option<RouteArmToken> {
        if let Some(token) = self.peek_scope_ack(scope_id) {
            return Some(token);
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            if !self.pending_scope_ack_lane_mask(lane_idx, scope_id, lane_idx) {
                next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
                continue;
            }
            let Some(arm) = self
                .port_for_lane(lane_idx)
                .peek_route_arm_selection(scope_id, ROLE)
            else {
                next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
                continue;
            };
            if let Some(arm) = Arm::new(arm) {
                return Some(RouteArmToken::from_ack(arm));
            }
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn next_slot_in_mask(slot_mask: &mut u8) -> Option<usize> {
        if *slot_mask == 0 {
            return None;
        }
        let slot_idx = slot_mask.trailing_zeros() as usize;
        *slot_mask &= !(1u8 << slot_idx);
        Some(slot_idx)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn ack_route_arm_selection_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        role: u8,
    ) -> Option<u8> {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let arm = self
            .port_for_lane(lane_idx)
            .ack_route_arm_selection(scope_id, role);
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        arm
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_route_arm_selection_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        arm: u8,
    ) {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        self.port_for_lane(lane_idx)
            .record_route_arm_selection(scope_id, arm);
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
    }

    pub(in crate::endpoint::kernel) fn arm_has_recv(&self, scope_id: ScopeId, arm: u8) -> bool {
        let materialization_meta = self.compute_scope_arm_materialization_meta(scope_id);
        self.arm_has_recv_with_materialization(scope_id, arm, materialization_meta)
    }
}
