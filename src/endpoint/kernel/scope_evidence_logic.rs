use super::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
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
    pub(in crate::endpoint::kernel) fn record_scope_ack(
        &mut self,
        scope_id: ScopeId,
        token: RouteDecisionToken,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self.route_state.scope_evidence.record_ack(slot, token)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
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
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, true);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_materialization_ready_arm(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
    ) {
        self.mark_scope_ready_arm_inner(scope_id, arm, false);
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
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_label(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        label_meta: ScopeLabelMeta,
    ) {
        let exact_static_passive_arm =
            self.static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        let arm = Self::scope_evidence_label_to_arm(label_meta, label).or(exact_static_passive_arm);
        if let Some(arm) = arm {
            if self.loop_control_label_evidence_only(label_meta, label, arm) {
                return;
            }
            if self.static_passive_scope_evidence_materializes_poll(scope_id) {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_static_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, label);
            }
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_binding_label(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        label_meta: ScopeLabelMeta,
    ) {
        let exact_static_passive_arm =
            self.static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        let arm = Self::binding_scope_evidence_label_to_arm(label_meta, label)
            .or(exact_static_passive_arm);
        if let Some(arm) = arm {
            if self.loop_control_label_evidence_only(label_meta, label, arm) {
                return;
            }
            if self.static_passive_scope_evidence_materializes_poll(scope_id) {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_static_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, label);
            }
        }
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

    pub(in crate::endpoint::kernel) fn mark_static_passive_descendant_path_ready(
        &mut self,
        scope_id: ScopeId,
        label: u8,
    ) {
        let Some(arm) =
            self.static_passive_descendant_dispatch_arm_from_exact_label(scope_id, label)
        else {
            return;
        };
        self.mark_scope_ready_arm(scope_id, arm);
        if self.selected_arm_for_scope(scope_id).is_none() {
            let lane = self.offer_lane_for_scope(scope_id);
            let _ = self.set_route_arm(lane, scope_id, arm);
        }
        let Some(child_scope) = self.cursor.passive_arm_scope_by_arm(scope_id, arm) else {
            return;
        };
        self.mark_static_passive_descendant_path_ready(child_scope, label);
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
    pub(in crate::endpoint::kernel) fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        is_dynamic_scope: bool,
        is_route_controller: bool,
    ) -> bool {
        if is_dynamic_scope {
            self.clear_scope_evidence(scope_id);
            return true;
        }
        if is_route_controller {
            return false;
        }
        self.clear_scope_evidence(scope_id);
        true
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
        slot: Slot,
    ) -> crate::transport::context::PolicySignals {
        match self.binding.policy_signals_provider() {
            Some(provider) => provider.signals(slot),
            None => crate::transport::context::PolicySignals::ZERO,
        }
    }

    pub(super) async fn await_transport_payload_for_offer_lane(
        &mut self,
        offer_lane: u8,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
    ) -> RecvResult<()> {
        let lane_idx = offer_lane as usize;
        let port = self.port_for_lane(lane_idx);
        let payload = lane_port::recv_future(port)
            .await
            .map_err(RecvError::Transport)?;
        if *transport_payload_len == 0 && !payload.as_bytes().is_empty() {
            *transport_payload_len = lane_port::copy_payload_into_scratch(port, &payload)
                .map_err(|_| RecvError::PhaseInvariant)?;
            *transport_payload_lane = offer_lane;
        }
        Ok(())
    }

    pub(super) async fn await_static_passive_progress(
        &mut self,
        selection: OfferScopeSelection,
        selected_arm: Option<u8>,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
    ) -> RecvResult<()> {
        let materialization_meta = self.selection_materialization_meta(selection);
        if let Some(arm) = selected_arm
            && selection.at_route_offer_entry
            && let Some(entry) = materialization_meta.passive_arm_entry(arm)
        {
            if !self.cursor.is_recv_at(state_index_to_usize(entry)) {
                return Ok(());
            }
        }
        if binding_classification.is_none()
            && let Some((_, classification)) = {
                let label_meta = self.selection_label_meta(selection);
                self.poll_binding_for_offer(
                    selection.scope_id,
                    selection.offer_lane_idx as usize,
                    selection.offer_lane_mask,
                    label_meta,
                    materialization_meta,
                )
            }
        {
            *binding_classification = Some(classification);
            return Ok(());
        }
        if *transport_payload_len == 0 {
            self.await_transport_payload_for_offer_lane(
                selection.offer_lane,
                transport_payload_len,
                transport_payload_lane,
            )
            .await?;
        }
        Ok(())
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

    pub(super) async fn try_poll_route_decision_immediate(
        &self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
    ) -> Option<Arm> {
        let arm = poll_fn(|cx| {
            let mut lane_idx = 0usize;
            while lane_idx < offer_lanes_len {
                let lane = offer_lanes[lane_idx];
                let port = self.port_for_lane(lane as usize);
                if let Poll::Ready(arm) = port.poll_route_decision(scope_id, ROLE, cx) {
                    return Poll::Ready(Some(arm));
                }
                lane_idx += 1;
            }
            Poll::Ready(None)
        })
        .await?;
        Arm::new(arm)
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

    pub(super) async fn try_poll_route_decision_for_offer(
        &self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
    ) -> Option<Arm> {
        self.try_poll_route_decision_immediate(scope_id, offer_lanes, offer_lanes_len)
            .await
            .or_else(|| self.poll_arm_from_ready_mask(scope_id))
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
        let mut dispatch_idx = 0usize;
        while let Some((_label, dispatch_arm, target)) =
            cursor.route_scope_first_recv_dispatch_entry(scope_id, dispatch_idx)
        {
            if (dispatch_arm == arm || dispatch_arm == ARM_SHARED)
                && cursor
                    .try_recv_meta_at(state_index_to_usize(target))
                    .is_some()
            {
                return true;
            }
            dispatch_idx += 1;
        }
        false
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
        let taken = if !port.has_route_hint_for_label_mask(hint_label_mask) {
            None
        } else {
            port.take_route_hint_for_label_mask(hint_label_mask)
        };
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        taken
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn pending_scope_ack_lane_mask(
        &self,
        lane_idx: usize,
        scope_id: ScopeId,
        offer_lane_mask: u8,
    ) -> u8 {
        self.ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(|port| {
                (port.pending_route_decision_lane_mask(scope_id, ROLE) as u8) & offer_lane_mask
            })
            .unwrap_or(0)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn pending_scope_hint_lane_mask(
        &mut self,
        lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
    ) -> u8 {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let Some(port) = self.ports.get(lane_idx).and_then(|port| port.as_ref()) else {
            return 0;
        };
        let lane_mask =
            (port.pending_route_hint_lane_mask_for_label_mask(label_meta.hint_label_mask()) as u8)
                & offer_lane_mask;
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        lane_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn preview_scope_ack_token_non_consuming(
        &self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lane_mask: u8,
    ) -> Option<RouteDecisionToken> {
        if let Some(token) = self.peek_scope_ack(scope_id) {
            return Some(token);
        }
        if summary_lane_idx >= MAX_LANES {
            return None;
        }
        let mut pending_ack_mask =
            self.pending_scope_ack_lane_mask(summary_lane_idx, scope_id, offer_lane_mask);
        while let Some(lane_idx) = Self::next_lane_in_mask(&mut pending_ack_mask) {
            let Some(arm) = self
                .port_for_lane(lane_idx)
                .peek_route_decision(scope_id, ROLE)
            else {
                continue;
            };
            if let Some(arm) = Arm::new(arm) {
                return Some(RouteDecisionToken::from_ack(arm));
            }
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn next_lane_in_mask(lane_mask: &mut u8) -> Option<usize> {
        if *lane_mask == 0 {
            return None;
        }
        let lane_idx = lane_mask.trailing_zeros() as usize;
        *lane_mask &= !(1u8 << lane_idx);
        Some(lane_idx)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn take_preferred_lane_in_mask(
        preferred_lane_idx: usize,
        lane_mask: &mut u8,
    ) -> Option<usize> {
        if preferred_lane_idx < MAX_LANES && (*lane_mask & (1u8 << preferred_lane_idx)) != 0 {
            *lane_mask &= !(1u8 << preferred_lane_idx);
            return Some(preferred_lane_idx);
        }
        Self::next_lane_in_mask(lane_mask)
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

    pub(in crate::endpoint::kernel) fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        let hint_matches_scope = Self::hint_matches_scope(label_meta, label, false);
        let exact_static_passive_arm =
            self.static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        if !hint_matches_scope && exact_static_passive_arm.is_none() {
            return;
        }
        if suppress_hint || !hint_matches_scope {
            self.mark_scope_ready_arm_from_binding_label(scope_id, label, label_meta);
            return;
        }
        self.record_scope_hint(scope_id, label);
        self.mark_scope_ready_arm_from_binding_label(scope_id, label, label_meta);
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        if suppress_hint {
            if let Some(label) = self.take_hint_for_lane(lane_idx, false, label_meta) {
                self.record_scope_hint_dynamic(scope_id, label);
                self.mark_scope_ready_arm_from_label(scope_id, label, label_meta);
            }

            if let Some(arm) = self.ack_route_decision_for_lane(lane_idx, scope_id, ROLE) {
                if let Some(arm) = Arm::new(arm) {
                    self.record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
                }
            }
            return;
        }
        if let Some(arm) = self.ack_route_decision_for_lane(lane_idx, scope_id, ROLE) {
            if let Some(arm) = Arm::new(arm) {
                self.record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
            }
        }
        if let Some(label) = self.take_hint_for_lane(lane_idx, suppress_hint, label_meta) {
            self.record_scope_hint(scope_id, label);
        }
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lane_mask: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        if offer_lane_mask == 0 {
            return;
        }
        let pending_ack_mask =
            self.pending_scope_ack_lane_mask(summary_lane_idx, scope_id, offer_lane_mask);
        let pending_hint_mask =
            self.pending_scope_hint_lane_mask(summary_lane_idx, offer_lane_mask, label_meta);
        let mut pending_evidence_mask = pending_ack_mask | pending_hint_mask;
        if pending_evidence_mask == 0 {
            return;
        }
        while let Some(lane_idx) = Self::next_lane_in_mask(&mut pending_evidence_mask) {
            self.ingest_scope_evidence_for_lane(lane_idx, scope_id, suppress_hint, label_meta);
        }
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
