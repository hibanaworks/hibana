#[cfg(test)]
use super::require_route_arm_commit_proof_from_parts;
use super::{
    Arm, ControlSemanticKind, ControlSemanticsTable, CursorEndpoint, DeferReason, DeferSource,
    EndpointSlot, EpochTable, FrameFlags, FrameLabelMask, FrontierKind, LabelUniverse, Lane,
    MintConfigMarker, PassiveArmNavigation, PolicySlot, RecvError, RecvResult, RouteArmCommitProof,
    ScopeFrameLabelMeta, ScopeId, ScopeKind, ScopeTrace, TapEvent, TapFrameMeta, Transport,
    TryFrom, emit, events, ids, policy_runtime,
    preflight_route_arm_commit_after_clearing_other_lanes_from_parts,
    preflight_route_arm_commit_from_parts, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    #[inline(always)]
    pub(crate) fn set_cursor_index(&mut self, idx: usize) {
        self.cursor.set_index(idx);
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn control_semantics(&self) -> ControlSemanticsTable {
        self.cursor.control_semantics()
    }

    #[inline]
    pub(crate) fn scope_trace(&self, scope: ScopeId) -> Option<ScopeTrace> {
        if scope.is_none() {
            return None;
        }
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| ScopeTrace::new(region.range, region.nest))
    }

    #[inline]
    pub(crate) const fn control_semantic_kind(
        &self,
        semantic: ControlSemanticKind,
    ) -> ControlSemanticKind {
        semantic
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn loop_control_drop_frame_label_mask(
        &self,
        frame_label_meta: ScopeFrameLabelMeta,
    ) -> FrameLabelMask {
        if frame_label_meta.loop_meta().loop_label_scope() {
            frame_label_meta.frame_hint_mask()
        } else {
            FrameLabelMask::EMPTY
        }
    }

    pub(in crate::endpoint::kernel) fn preflight_route_arm_commit(
        &self,
        lane: u8,
        scope: ScopeId,
        arm: u8,
    ) -> Option<RouteArmCommitProof> {
        preflight_route_arm_commit_from_parts(&self.decision_state, &self.cursor, lane, scope, arm)
    }

    pub(in crate::endpoint::kernel) fn preflight_route_arm_commit_after_clearing_other_lanes(
        &self,
        lane: u8,
        scope: ScopeId,
        arm: u8,
    ) -> Option<RouteArmCommitProof> {
        preflight_route_arm_commit_after_clearing_other_lanes_from_parts(
            &self.decision_state,
            &self.cursor,
            lane,
            scope,
            arm,
        )
    }

    pub(in crate::endpoint::kernel) fn commit_route_arm_after_preflight(
        &mut self,
        proof: RouteArmCommitProof,
    ) {
        let lane_idx = proof.lane_idx() as usize;
        self.decision_state.commit_route_arm_after_preflight(proof);
        self.refresh_lane_offer_state(lane_idx);
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn require_route_arm_commit_proof(
        &self,
        lane: u8,
        scope: ScopeId,
        arm: u8,
    ) -> RecvResult<RouteArmCommitProof> {
        require_route_arm_commit_proof_from_parts(
            &self.decision_state,
            &self.cursor,
            lane,
            scope,
            arm,
        )
    }

    #[cfg(test)]
    pub(crate) fn test_commit_route_arm(
        &mut self,
        lane: u8,
        scope: ScopeId,
        arm: u8,
    ) -> RecvResult<()> {
        let proof = self.require_route_arm_commit_proof(lane, scope, arm)?;
        self.commit_route_arm_after_preflight(proof);
        Ok(())
    }

    pub(crate) fn pop_route_arm(&mut self, lane: u8, scope: ScopeId) {
        if scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        debug_assert!(
            lane_idx < self.cursor.logical_lane_count(),
            "pop_route_arm: lane {} exceeds logical lane count {}",
            lane_idx,
            self.cursor.logical_lane_count()
        );
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        let is_linger = self.is_linger_route(scope);
        let Some(scope_slot) = self.scope_slot_for_route(scope) else {
            return;
        };
        if self
            .decision_state
            .pop_route_arm(lane_idx, scope, scope_slot, is_linger)
        {
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    fn scope_is_descendant_of(&self, scope: ScopeId, ancestor: ScopeId) -> bool {
        self.route_ancestor_arm(scope, ancestor).is_some()
    }

    fn route_ancestor_arm(&self, scope: ScopeId, ancestor: ScopeId) -> Option<u8> {
        if scope.is_none() || ancestor.is_none() || scope == ancestor {
            return None;
        }
        let mut current = scope;
        while let Some(parent) = self.cursor.route_parent_scope(current) {
            if parent == current {
                return None;
            }
            let arm = self.cursor.route_parent_arm(current)?;
            if parent == ancestor {
                return Some(arm);
            }
            current = parent;
        }
        None
    }

    pub(crate) fn clear_descendant_route_state_for_lane(
        &mut self,
        lane: u8,
        ancestor_scope: ScopeId,
    ) {
        if ancestor_scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        if self.decision_state.lane_route_arm_len(lane_idx) == 0 {
            return;
        }
        while let Some(scope) = self.decision_state.last_lane_scope(lane_idx) {
            if scope.is_none()
                || scope.kind() != ScopeKind::Route
                || !self.scope_is_descendant_of(scope, ancestor_scope)
            {
                break;
            }
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    pub(crate) fn prune_route_state_to_cursor_path_for_lane(&mut self, lane: u8) {
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        if self.decision_state.lane_route_arm_len(lane_idx) == 0 {
            return;
        }
        let cursor_scope = self.cursor.node_scope_id();
        while let Some(scope) = self.decision_state.last_lane_scope(lane_idx) {
            let keep = !scope.is_none()
                && (scope == cursor_scope || self.scope_is_descendant_of(cursor_scope, scope));
            if keep || scope.is_none() {
                break;
            }
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    pub(in crate::endpoint::kernel) fn clear_scope_route_state_for_other_lanes(
        &mut self,
        scope: ScopeId,
        keep_lane: u8,
    ) {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut start = 0usize;
        let mut next = {
            self.decision_state
                .active_route_lanes()
                .next_set_from(start, lane_limit)
        };
        while let Some(lane_idx) = next {
            if lane_idx != keep_lane as usize {
                let lane_wire = lane_idx as u8;
                self.clear_descendant_route_state_for_lane(lane_wire, scope);
                self.pop_route_arm(lane_wire, scope);
            }
            start = lane_idx.saturating_add(1);
            next = {
                self.decision_state
                    .active_route_lanes()
                    .next_set_from(start, lane_limit)
            };
        }
    }

    pub(crate) fn is_linger_route(&self, scope: ScopeId) -> bool {
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| {
                if region.kind == ScopeKind::Loop {
                    return true;
                }
                region.kind == ScopeKind::Route && region.linger
            })
            .unwrap_or(false)
    }

    pub(crate) fn route_arm_for(&self, lane: u8, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return None;
        }
        self.decision_state.route_arm_for(lane_idx, scope)
    }

    pub(crate) fn selected_arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let scope_slot = self.scope_slot_for_route(scope)?;
        self.decision_state.selected_arm_for_scope_slot(scope_slot)
    }

    pub(crate) fn route_scope_offer_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
        let offer_entry = self.cursor.route_scope_offer_entry(scope_id)?;
        Some(if offer_entry.is_max() {
            self.cursor.index()
        } else {
            state_index_to_usize(offer_entry)
        })
    }

    #[inline]
    pub(crate) fn route_scope_depth_bound(&self) -> usize {
        self.cursor.route_scope_count().saturating_add(1)
    }

    #[inline]
    pub(crate) fn typestate_step_bound(&self) -> u32 {
        self.cursor.local_steps_len().saturating_add(1) as u32
    }

    pub(crate) fn preview_passive_materialization_index_for_selected_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        let mut scope = scope_id;
        let mut selected_arm = arm;
        let mut depth = 0usize;
        let depth_bound = self.route_scope_depth_bound();
        while depth < depth_bound {
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope, selected_arm) {
                return Some(entry);
            }
            let PassiveArmNavigation::WithinArm { entry } = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope, selected_arm)?;
            let entry_idx = state_index_to_usize(entry);
            if self.cursor.is_recv_at(entry_idx)
                || self.cursor.is_send_at(entry_idx)
                || self.cursor.is_local_action_at(entry_idx)
                || self.cursor.is_jump_at(entry_idx)
            {
                return Some(entry_idx);
            }
            let child_scope = self
                .cursor
                .passive_arm_scope_by_arm(scope, selected_arm)
                .or_else(|| {
                    let node_scope = self.cursor.node_scope_id_at(entry_idx);
                    (node_scope != scope && node_scope.kind() == ScopeKind::Route)
                        .then_some(node_scope)
                })?;
            selected_arm = self.preview_selected_arm_for_scope(child_scope)?;
            scope = child_scope;
            depth += 1;
        }
        None
    }

    pub(crate) fn preview_selected_arm_for_scope(&self, scope_id: ScopeId) -> Option<u8> {
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let Some(summary_lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) else {
            return None;
        };
        self.preview_scope_ack_token_non_consuming(scope_id, summary_lane_idx, offer_lanes)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    fn structural_arm_for_child_scope(
        &self,
        parent_scope: ScopeId,
        child_scope: ScopeId,
    ) -> Option<u8> {
        self.route_ancestor_arm(child_scope, parent_scope)
    }

    #[inline]
    pub(crate) fn current_offer_scope_id(&self) -> ScopeId {
        let node_scope = self.cursor.node_scope_id();
        if node_scope.is_none() {
            return node_scope;
        }
        if node_scope.kind() == ScopeKind::Route
            && self.selected_arm_for_scope(node_scope).is_some()
        {
            return node_scope;
        }
        let mut child_scope = node_scope;
        while let Some(parent_scope) = self.cursor.route_parent_scope(child_scope) {
            if parent_scope == child_scope {
                return parent_scope;
            }
            let child_selected_arm = self.selected_arm_for_scope(child_scope);
            let Some(parent_arm) = self
                .selected_arm_for_scope(parent_scope)
                .or_else(|| {
                    // Once we have descended into a selected child route, the
                    // ancestor arm is derivable from the structural placement
                    // of that child. Do not invent ancestor authority before
                    // the child itself has become selected.
                    if child_selected_arm.is_some() {
                        self.structural_arm_for_child_scope(parent_scope, child_scope)
                    } else {
                        None
                    }
                })
                .or_else(|| self.preview_selected_arm_for_scope(parent_scope))
            else {
                return parent_scope;
            };
            if self.route_ancestor_arm(child_scope, parent_scope) != Some(parent_arm) {
                return parent_scope;
            }
            child_scope = parent_scope;
        }
        node_scope
    }

    pub(crate) fn rebase_passive_descendant_scope(
        &self,
        stop_scope: ScopeId,
        initial_scope: ScopeId,
    ) -> ScopeId {
        let mut target_scope = initial_scope;
        let mut attempts = 0usize;
        let depth_bound = self.route_scope_depth_bound();
        'rebase: while attempts < depth_bound {
            let mut child_scope = target_scope;
            let mut depth = 0usize;
            while depth < depth_bound {
                let Some(parent_scope) = self.cursor.route_parent_scope(child_scope) else {
                    break 'rebase;
                };
                if parent_scope == child_scope {
                    break 'rebase;
                }
                if parent_scope == stop_scope {
                    break 'rebase;
                }
                if parent_scope.kind() == ScopeKind::Route
                    && let Some(parent_arm) = self
                        .selected_arm_for_scope(parent_scope)
                        .or_else(|| self.preview_selected_arm_for_scope(parent_scope))
                    && self.route_ancestor_arm(child_scope, parent_scope) != Some(parent_arm)
                {
                    if let Some(scope) = self
                        .cursor
                        .passive_arm_scope_by_arm(parent_scope, parent_arm)
                        && scope != child_scope
                    {
                        target_scope = scope;
                        attempts += 1;
                        continue 'rebase;
                    }
                    if let Some(entry_idx) = self
                        .preview_passive_materialization_index_for_selected_arm(
                            parent_scope,
                            parent_arm,
                        )
                    {
                        let scope = self.cursor.node_scope_id_at(entry_idx);
                        if scope.kind() == ScopeKind::Route
                            && scope != parent_scope
                            && scope != child_scope
                        {
                            target_scope = scope;
                            attempts += 1;
                            continue 'rebase;
                        }
                    }
                    break 'rebase;
                }
                child_scope = parent_scope;
                depth += 1;
            }
            break;
        }
        target_scope
    }

    pub(crate) fn current_route_arm_authorized(&self) -> RecvResult<Option<bool>> {
        let Some(region) = self.cursor.scope_region() else {
            return Ok(None);
        };
        if region.kind != ScopeKind::Route {
            return Ok(None);
        }
        let Some(current_arm) = self.cursor.typestate_node(self.cursor.index()).route_arm() else {
            return Ok(None);
        };
        if self.cursor.index() == region.start && self.cursor.is_route_controller(region.scope_id) {
            return Ok(None);
        }
        if let Some(selected_arm) = self.selected_arm_for_scope(region.scope_id) {
            return Ok((selected_arm == current_arm).then_some(false));
        }
        if let Some(preview_arm) = self.preview_selected_arm_for_scope(region.scope_id) {
            return Ok((preview_arm == current_arm).then_some(false));
        }
        if !self.cursor.is_route_controller(region.scope_id) {
            // Passive projections can be positioned at an arm entry before the
            // controller's route decision or materializing payload has been
            // observed. That state is not progress and must not fault the
            // generation here; offer() will wait for projected route evidence
            // before producing a continuation.
            return Ok(None);
        }
        Err(RecvError::PhaseInvariant)
    }

    #[inline]
    pub(crate) fn endpoint_policy_args(lane: Lane, label: u8, flags: FrameFlags) -> u32 {
        ((ROLE as u32) << 24)
            | ((lane.as_wire() as u32) << 16)
            | ((label as u32) << 8)
            | flags.bits() as u32
    }

    #[inline]
    pub(crate) fn emit_policy_audit_event(
        &self,
        id: u16,
        arg0: u32,
        arg1: u32,
        arg2: u32,
        lane: Lane,
    ) {
        let port = self.port_for_lane(lane.raw() as usize);
        let causal = TapEvent::make_causal_key(lane.as_wire(), 1);
        let event = events::RawEvent::new(port.now32(), id)
            .with_causal_key(causal)
            .with_arg0(arg0)
            .with_arg1(arg1)
            .with_arg2(arg2);
        emit(port.tap(), event);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn emit_policy_defer_event(
        &self,
        source: DeferSource,
        reason: DeferReason,
        scope_id: ScopeId,
        frontier: FrontierKind,
        selected_arm: Option<u8>,
        hint: Option<u8>,
        ready_arm_mask: u8,
        binding_ready: bool,
        pending: bool,
        lane: u8,
    ) {
        let source_tag = u32::from(source.as_audit_tag());
        let scope_slot = self
            .scope_slot_for_route(scope_id)
            .and_then(|slot| u16::try_from(slot).ok())
            .unwrap_or(u16::MAX) as u32;
        let arm = selected_arm.unwrap_or(u8::MAX) as u32;
        let hint = hint.unwrap_or(0) as u32;
        let arg0 = (source_tag << 24) | u32::from(pending);
        let arg1 = (scope_slot << 16) | (arm << 8) | (ready_arm_mask as u32);
        let arg2 = ((reason as u32) << 16)
            | (hint << 8)
            | ((frontier.as_audit_tag() as u32) << 4)
            | ((u32::from(binding_ready)) << 1)
            | u32::from(pending);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_DEFER,
            arg0,
            arg1,
            arg2,
            Lane::new(lane as u32),
        );
    }

    pub(crate) fn emit_endpoint_event(
        &self,
        id: u16,
        meta: TapFrameMeta,
        scope_trace: Option<ScopeTrace>,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let packed = ((ROLE as u32) << 24)
            | ((meta.lane as u32) << 16)
            | ((meta.label as u32) << 8)
            | meta.flags.bits() as u32;
        let mut event = events::RawEvent::new(port.now32(), id)
            .with_arg0(meta.sid)
            .with_arg1(packed);
        if let Some(scope) = scope_trace {
            event = event.with_arg2(scope.pack());
        }
        emit(port.tap(), event);
    }

    pub(crate) fn emit_endpoint_policy_audit(
        &self,
        slot: PolicySlot,
        event_id: u16,
        arg0: u32,
        arg1: u32,
        lane: Lane,
    ) {
        let port = self.port_for_lane(lane.raw() as usize);
        let event = events::RawEvent::new(port.now32(), event_id)
            .with_arg0(arg0)
            .with_arg1(arg1);
        let signals = self.policy_signals_for_slot(slot);
        let policy_attrs = *signals.attrs();
        let policy_input = signals.input();
        let policy_words = policy_input.replay_words();
        let policy_digest = port.policy_digest(slot);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_attrs.hash32();
        let policy_attrs_replay_hash = policy_runtime::hash_policy_attrs(&policy_attrs);
        let replay_attrs = policy_runtime::replay_policy_attr_words(&policy_attrs);
        let replay_policy_attr_presence =
            policy_runtime::replay_policy_attr_presence(&policy_attrs);
        let slot_id = policy_runtime::slot_tag(slot);
        let mode_id = policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            policy_attrs_hash,
            policy_attrs_replay_hash,
            ((slot_id as u32) << 24) | ((mode_id as u32) << 16),
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_words[0],
            policy_words[1],
            policy_words[2],
            lane,
        );
        self.emit_policy_audit_event(ids::POLICY_REPLAY_INPUT1, policy_words[3], 0, 0, lane);
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_ATTRS0,
            replay_attrs[0],
            replay_attrs[1],
            replay_attrs[2],
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_ATTRS1,
            replay_attrs[3],
            replay_policy_attr_presence as u32,
            0,
            lane,
        );
        let verdict = policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            policy_runtime::verdict_reason(verdict) as u32,
            policy_runtime::POLICY_FUEL_NONE as u32,
            lane,
        );
    }
}
