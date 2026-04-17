use super::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn bump_scope_evidence_generation(&mut self, slot: usize) {
        self.route_state.scope_evidence.bump_generation(slot);
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
        if scope_id.is_none() || scope_id.kind() != ScopeKind::Route {
            return None;
        }
        self.cursor.route_scope_slot(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_evidence_generation_for_scope(
        &self,
        scope_id: ScopeId,
    ) -> u16 {
        self.route_state
            .scope_evidence
            .generation_for_slot(self.scope_slot_for_route(scope_id))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn peek_scope_ack(
        &self,
        scope_id: ScopeId,
    ) -> Option<RouteDecisionToken> {
        let slot = self.scope_slot_for_route(scope_id)?;
        self.route_state.scope_evidence.peek_ack(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn take_scope_ack(
        &mut self,
        scope_id: ScopeId,
    ) -> Option<RouteDecisionToken> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let token = self.route_state.scope_evidence.take_ack(slot);
        if token.is_some() {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
        token
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_scope_hint(&mut self, scope_id: ScopeId, label: u8) {
        if label == 0 {
            return;
        }
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self.route_state.scope_evidence.record_hint(slot, label)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_scope_hint_dynamic(
        &mut self,
        scope_id: ScopeId,
        label: u8,
    ) {
        if label == 0 {
            return;
        }
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self
                .route_state
                .scope_evidence
                .record_hint_dynamic(slot, label)
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
                .route_state
                .scope_evidence
                .mark_ready_arm(slot, arm, poll_ready)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn loop_control_label_evidence_only(
        &self,
        label_meta: ScopeLabelMeta,
        label: u8,
        arm: u8,
    ) -> bool {
        label_meta.loop_meta().loop_label_scope()
            && self.is_loop_semantic_label(label)
            && label_meta.loop_meta().arm_has_recv(arm)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn static_passive_scope_evidence_materializes_poll(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        !self.cursor.is_route_controller(scope_id)
            && !self
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn static_passive_dispatch_arm_from_exact_label(
        &self,
        scope_id: ScopeId,
        label: u8,
        label_meta: ScopeLabelMeta,
    ) -> Option<u8> {
        if !self.static_passive_scope_evidence_materializes_poll(scope_id) {
            return None;
        }
        let _ = label_meta;
        self.static_passive_descendant_dispatch_arm_from_exact_label(scope_id, label)
    }

    pub(in crate::endpoint::kernel) fn static_passive_descendant_dispatch_arm_from_exact_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<u8> {
        if let Some((dispatch_arm, _)) = self.cursor.first_recv_target(scope_id, label) {
            if dispatch_arm != ARM_SHARED {
                return Some(dispatch_arm);
            }
        }
        let mut matched_arm = None;
        for arm in [0u8, 1u8] {
            let Some(child_scope) = self.cursor.passive_arm_scope_by_arm(scope_id, arm) else {
                continue;
            };
            if self
                .static_passive_descendant_dispatch_arm_from_exact_label(child_scope, label)
                .is_some()
            {
                if matched_arm.is_some_and(|prev| prev != arm) {
                    return None;
                }
                matched_arm = Some(arm);
            }
        }
        matched_arm
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.route_state.scope_evidence.ready_arm_mask(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_poll_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.route_state.scope_evidence.poll_ready_arm_mask(slot)
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
            && self.route_state.scope_evidence.consume_ready_arm(slot, arm)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn peek_scope_hint(&self, scope_id: ScopeId) -> Option<u8> {
        let slot = self.scope_slot_for_route(scope_id)?;
        self.route_state.scope_evidence.peek_hint(slot)
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn take_scope_hint(&mut self, scope_id: ScopeId) -> Option<u8> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let label = self.route_state.scope_evidence.take_hint(slot);
        if label.is_some() {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
        label
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_scope_evidence(&mut self, scope_id: ScopeId) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self.route_state.scope_evidence.clear(slot)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_evidence_conflicted(&self, scope_id: ScopeId) -> bool {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return false;
        };
        self.route_state.scope_evidence.conflicted(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn can_advance_route_scope(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> bool {
        let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
        self.route_arm_for(lane_wire, scope_id).is_some()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn lane_for_label_or_offer(
        &self,
        scope_id: ScopeId,
        target_label: u8,
    ) -> u8 {
        if let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label) {
            lane_idx as u8
        } else {
            self.offer_lane_for_scope(scope_id)
        }
    }

    pub(in crate::endpoint::kernel) fn policy_signals_for_slot(
        &self,
        slot: crate::policy_runtime::PolicySlot,
    ) -> crate::transport::context::PolicySignals<'_> {
        match self.binding.policy_signals_provider() {
            Some(provider) => provider.signals(slot),
            None => crate::transport::context::PolicySignals::ZERO,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn evidence_fingerprint(
        &self,
        scope_id: ScopeId,
        binding_ready: bool,
    ) -> EvidenceFingerprint {
        EvidenceFingerprint::new(
            self.peek_scope_ack(scope_id).is_some(),
            self.scope_has_ready_arm_evidence(scope_id),
            binding_ready,
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

    pub(in crate::endpoint::kernel) fn try_select_lane_for_label(
        &mut self,
        target_label: u8,
    ) -> bool {
        if let Some(meta) = self.cursor.try_recv_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(meta) = self.cursor.try_send_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(meta) = self.cursor.try_local_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(region) = self.cursor.scope_region()
            && region.kind == ScopeKind::Route
            && self.cursor.is_route_controller(region.scope_id)
            && self
                .cursor
                .controller_arm_entry_for_label(region.scope_id, target_label)
                .is_some()
        {
            return true;
        }
        let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label) else {
            return false;
        };
        let Some(idx) = self.cursor.index_for_lane_step(lane_idx) else {
            return false;
        };
        if idx != self.cursor.index() {
            self.set_cursor_index(idx);
        }
        true
    }

    pub(in crate::endpoint::kernel) fn hint_matches_scope(
        label_meta: ScopeLabelMeta,
        label: u8,
        suppress_hint: bool,
    ) -> bool {
        if suppress_hint {
            return false;
        }
        label_meta.matches_hint_label(label)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_has_controller_arm_entry(
        cursor: &PhaseCursor,
        scope_id: ScopeId,
    ) -> bool {
        cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn current_recv_is_scope_local(
        cursor: &PhaseCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
        label: u8,
        resource: Option<u8>,
        arm: u8,
    ) -> bool {
        cursor
            .first_recv_target(scope_id, label)
            .map(|(target_arm, _)| target_arm == arm)
            .unwrap_or(false)
            || (loop_meta.loop_label_scope()
                && is_loop_control_label_or_resource(semantics, label, resource))
    }

    pub(in crate::endpoint::kernel) fn scope_label_to_arm(
        label_meta: ScopeLabelMeta,
        label: u8,
    ) -> Option<u8> {
        label_meta.arm_for_label(label)
    }

    pub(in crate::endpoint::kernel) fn scope_evidence_label_to_arm(
        label_meta: ScopeLabelMeta,
        label: u8,
    ) -> Option<u8> {
        label_meta.evidence_arm_for_label(label)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn binding_scope_evidence_label_to_arm(
        label_meta: ScopeLabelMeta,
        label: u8,
    ) -> Option<u8> {
        label_meta.binding_evidence_arm_for_label(label)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_arm_has_recv(
        cursor: &PhaseCursor,
        scope_id: ScopeId,
        arm: u8,
    ) -> bool {
        if cursor.route_scope_arm_recv_index(scope_id, arm).is_some() {
            return true;
        }
        if let Some((entry, _label)) = cursor.controller_arm_entry_by_arm(scope_id, arm) {
            if cursor.is_recv_at(state_index_to_usize(entry)) {
                return true;
            }
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) =
            cursor.follow_passive_observer_arm_for_scope(scope_id, arm)
        {
            return cursor.is_recv_at(state_index_to_usize(entry));
        }
        (cursor.route_scope_first_recv_dispatch_arm_mask(scope_id) & ScopeEvidence::arm_bit(arm))
            != 0
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
            if let Some((entry, _label)) = self.cursor.controller_arm_entry_by_arm(scope_id, arm) {
                if let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry)) {
                    return recv_meta.peer != ROLE;
                }
                return false;
            }
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
        {
            let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry)) else {
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
                    && self
                        .control_semantic_kind(recv_meta.label, recv_meta.resource)
                        .is_loop()
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

    #[cfg(test)]
    #[inline]
    pub(in crate::endpoint::kernel) fn route_token_has_materialization_evidence(
        &self,
        scope_id: ScopeId,
        token: RouteDecisionToken,
    ) -> bool {
        let arm = token.arm().as_u8();
        if !self.arm_requires_materialization_ready_evidence(scope_id, arm) {
            return true;
        }
        self.scope_has_ready_arm(scope_id, arm)
    }

    pub(in crate::endpoint::kernel) fn take_hint_for_lane(
        &mut self,
        lane_idx: usize,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
        drain_transport_hints: bool,
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
        let hint_label_mask = label_meta.hint_label_mask();
        let taken = if !port.has_route_hint_for_label_mask(hint_label_mask, drain_transport_hints) {
            None
        } else {
            port.take_route_hint_for_label_mask(hint_label_mask, drain_transport_hints)
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
                port.has_pending_route_decision_for_lane(
                    scope_id,
                    ROLE,
                    Lane::new(offer_lane_idx as u32),
                )
            })
            .unwrap_or(false)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn pending_scope_hint_lane_mask(
        &mut self,
        lane_idx: usize,
        offer_lane_idx: usize,
        label_meta: ScopeLabelMeta,
        drain_transport_hints: bool,
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
            label_meta.hint_label_mask(),
            Lane::new(offer_lane_idx as u32),
            drain_transport_hints,
        );
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        pending
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn preview_scope_ack_token_non_consuming(
        &self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: crate::global::role_program::LaneSetView,
    ) -> Option<RouteDecisionToken> {
        if let Some(token) = self.peek_scope_ack(scope_id) {
            return Some(token);
        }
        let lane_limit = self.cursor.logical_lane_count();
        if summary_lane_idx >= lane_limit {
            return None;
        }
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if !offer_lanes.contains(lane_idx)
                || !self.pending_scope_ack_lane_mask(summary_lane_idx, scope_id, lane_idx)
            {
                lane_idx += 1;
                continue;
            }
            let Some(arm) = self
                .port_for_lane(lane_idx)
                .peek_route_decision(scope_id, ROLE)
            else {
                lane_idx += 1;
                continue;
            };
            if let Some(arm) = Arm::new(arm) {
                return Some(RouteDecisionToken::from_ack(arm));
            }
            lane_idx += 1;
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
    pub(in crate::endpoint::kernel) fn ack_route_decision_for_lane(
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
            .ack_route_decision(scope_id, role);
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        arm
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_route_decision_for_lane(
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
            .record_route_decision(scope_id, arm);
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
    }

    pub(in crate::endpoint::kernel) fn arm_has_recv(&self, scope_id: ScopeId, arm: u8) -> bool {
        if Self::scope_arm_has_recv(&self.cursor, scope_id, arm) {
            return true;
        }
        self.preview_passive_materialization_index_for_selected_arm(scope_id, arm)
            .map(|target_idx| self.cursor.try_recv_meta_at(target_idx).is_some())
            .unwrap_or(false)
    }
}
