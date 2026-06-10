use super::{
    Arm, CursorEndpoint, DeferReason, DeferSource, EpochTable, FrameFlags, FrontierKind,
    LabelUniverse, Lane, MintConfigMarker, PolicySlot, RecvError, RecvResult, ScopeId, ScopeTrace,
    TapEvent, TapFrameMeta, Transport, TryFrom, emit, events, ids, policy_runtime,
    state_index_to_usize,
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
    pub(crate) fn scope_trace(&self, scope: ScopeId) -> Option<ScopeTrace> {
        if scope.is_none() {
            return None;
        }
        Some(ScopeTrace::new(scope.range_ordinal(), scope.nest_ordinal()))
    }

    pub(crate) fn is_linger_route(&self, scope: ScopeId) -> bool {
        self.cursor.route_scope_linger(scope)
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

    pub(crate) fn preview_passive_materialization_index_for_selected_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        self.cursor
            .passive_materialization_index_for_selected_arm(scope_id, arm, |scope| {
                self.preview_selected_arm_for_scope(scope)
            })
    }

    pub(crate) fn preview_selected_arm_for_scope(&self, scope_id: ScopeId) -> Option<u8> {
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if offer_lanes
            .first_set(self.cursor.logical_lane_count())
            .is_none()
        {
            return None;
        }
        self.preview_scope_ack_token_non_consuming(scope_id, offer_lanes)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    #[inline]
    pub(crate) fn current_offer_scope_id(&self) -> ScopeId {
        self.cursor.current_offer_scope_id(
            |scope| self.selected_arm_for_scope(scope),
            |scope| self.preview_selected_arm_for_scope(scope),
        )
    }

    pub(crate) fn rebase_passive_descendant_scope(
        &self,
        stop_scope: ScopeId,
        initial_scope: ScopeId,
    ) -> ScopeId {
        self.cursor
            .rebase_passive_descendant_scope(stop_scope, initial_scope, |scope| {
                self.selected_arm_for_scope(scope)
                    .or_else(|| self.preview_selected_arm_for_scope(scope))
            })
    }

    pub(crate) fn current_route_arm_authorized(&self) -> RecvResult<Option<bool>> {
        self.cursor
            .current_route_arm_authorization(
                |scope| self.selected_arm_for_scope(scope),
                |scope| self.preview_selected_arm_for_scope(scope),
            )
            .map_err(|_| RecvError::PhaseInvariant)
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
        ingress_ready: bool,
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
            | ((u32::from(ingress_ready)) << 1)
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
        let policy_input = signals.input();
        let policy_words = policy_input.replay_words();
        let policy_digest = port.policy_digest(slot);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_runtime::hash_empty_policy_attrs();
        let policy_attrs_replay_hash = policy_runtime::hash_empty_policy_replay_attrs();
        let replay_attrs = policy_runtime::EMPTY_POLICY_ATTR_WORDS;
        let replay_policy_attr_presence = policy_runtime::EMPTY_POLICY_ATTR_PRESENCE;
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
