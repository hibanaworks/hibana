use super::{
    Arm, CursorEndpoint, DeferReason, FrameFlags, FrontierKind, Lane, RecvError, RecvResult,
    ResolverSlot, ScopeId, ScopeTrace, TapEvent, TapFrameMeta, Transport, TryFrom, emit, events,
    ids, resolver_audit, state_index_to_usize,
};

const AUDIT_ABSENT_SCOPE_SLOT: u16 = u16::MAX;
const AUDIT_ABSENT_ARM: u8 = u8::MAX;
const AUDIT_HINT_PRESENT: u32 = 1 << 2;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct ResolverDeferAudit {
    pub(in crate::endpoint::kernel) reason: DeferReason,
    pub(in crate::endpoint::kernel) scope_id: ScopeId,
    pub(in crate::endpoint::kernel) frontier: FrontierKind,
    pub(in crate::endpoint::kernel) selected_arm: Option<u8>,
    pub(in crate::endpoint::kernel) hint: Option<u8>,
    pub(in crate::endpoint::kernel) ready_arm_mask: u8,
    pub(in crate::endpoint::kernel) ingress_ready: bool,
    pub(in crate::endpoint::kernel) pending: bool,
    pub(in crate::endpoint::kernel) lane: u8,
}

#[inline]
fn audit_scope_slot(slot: Option<usize>) -> u32 {
    match slot {
        Some(slot) => match u16::try_from(slot) {
            Ok(slot) if slot != AUDIT_ABSENT_SCOPE_SLOT => u32::from(slot),
            _ => crate::invariant(),
        },
        None => u32::from(AUDIT_ABSENT_SCOPE_SLOT),
    }
}

#[inline]
fn audit_arm(arm: Option<u8>) -> u32 {
    match arm {
        Some(arm) if arm != AUDIT_ABSENT_ARM => u32::from(arm),
        Some(_) => crate::invariant(),
        None => u32::from(AUDIT_ABSENT_ARM),
    }
}

#[inline]
fn audit_hint(hint: Option<u8>) -> (u32, u32) {
    match hint {
        Some(hint) => (u32::from(hint), AUDIT_HINT_PRESENT),
        None => (0, 0),
    }
}

impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
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
        offer_lanes.first_set(self.cursor.logical_lane_count())?;
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
    pub(crate) fn endpoint_resolver_args(lane: Lane, label: u8, flags: FrameFlags) -> u32 {
        ((ROLE as u32) << 24)
            | ((lane.as_wire() as u32) << 16)
            | ((label as u32) << 8)
            | flags.bits() as u32
    }

    #[inline]
    pub(crate) fn emit_resolver_audit_event(
        &self,
        id: u16,
        arg0: u32,
        arg1: u32,
        arg2: u32,
        lane: Lane,
    ) {
        let port = self.port_for_lane(lane.raw() as usize);
        let causal = TapEvent::make_causal_key(lane.as_wire(), 1);
        let event = events::raw_event(port.now32(), id)
            .with_causal_key(causal)
            .with_arg0(arg0)
            .with_arg1(arg1)
            .with_arg2(arg2);
        emit(port.tap(), event);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn emit_resolver_defer_event(&self, audit: ResolverDeferAudit) {
        const RESOLVER_DEFER_AUDIT_TAG: u32 = 0x80;
        let scope_slot = audit_scope_slot(self.scope_slot_for_route(audit.scope_id));
        let arm = audit_arm(audit.selected_arm);
        let (hint, hint_present) = audit_hint(audit.hint);
        let arg0 = (RESOLVER_DEFER_AUDIT_TAG << 24) | u32::from(audit.pending);
        let arg1 = (scope_slot << 16) | (arm << 8) | (audit.ready_arm_mask as u32);
        let arg2 = ((audit.reason as u32) << 16)
            | (hint << 8)
            | ((audit.frontier.as_audit_tag() as u32) << 4)
            | hint_present
            | ((u32::from(audit.ingress_ready)) << 1)
            | u32::from(audit.pending);
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT_DEFER,
            arg0,
            arg1,
            arg2,
            Lane::new(audit.lane as u32),
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
        let mut event = events::raw_event(port.now32(), id)
            .with_arg0(meta.sid)
            .with_arg1(packed);
        if let Some(scope) = scope_trace {
            event = event.with_arg2(scope.pack());
        }
        emit(port.tap(), event);
    }

    pub(crate) fn emit_endpoint_resolver_audit(
        &self,
        slot: ResolverSlot,
        event_id: u16,
        arg0: u32,
        arg1: u32,
        lane: Lane,
    ) {
        let port = self.port_for_lane(lane.raw() as usize);
        let event = events::raw_event(port.now32(), event_id)
            .with_arg0(arg0)
            .with_arg1(arg1);
        let resolver_words = resolver_audit::EMPTY_RESOLVER_INPUT_WORDS;
        let resolver_digest = port.resolver_digest(slot);
        let event_hash = resolver_audit::hash_tap_event(&event);
        let signals_input_hash = resolver_audit::hash_empty_resolver_input();
        let resolver_attrs_hash = resolver_audit::hash_empty_resolver_attrs();
        let resolver_attrs_replay_hash = resolver_audit::hash_empty_resolver_replay_attrs();
        let replay_attrs = resolver_audit::EMPTY_RESOLVER_ATTR_WORDS;
        let replay_resolver_attr_presence = resolver_audit::EMPTY_RESOLVER_ATTR_PRESENCE;
        let slot_id = resolver_audit::slot_tag(slot);
        let mode_id = resolver_audit::RESOLVER_MODE_AUDIT_ONLY_TAG;
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT,
            resolver_digest,
            event_hash,
            signals_input_hash,
            lane,
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT_EXT,
            resolver_attrs_hash,
            resolver_attrs_replay_hash,
            ((slot_id as u32) << 24) | ((mode_id as u32) << 16),
            lane,
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            lane,
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            lane,
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_INPUT0,
            resolver_words[0],
            resolver_words[1],
            resolver_words[2],
            lane,
        );
        self.emit_resolver_audit_event(ids::RESOLVER_REPLAY_INPUT1, resolver_words[3], 0, 0, lane);
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_ATTRS0,
            replay_attrs[0],
            replay_attrs[1],
            replay_attrs[2],
            lane,
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_ATTRS1,
            replay_attrs[3],
            replay_resolver_attr_presence as u32,
            0,
            lane,
        );
        let verdict_meta = ((resolver_audit::RESOLVER_RESULT_NO_ENGINE_TAG as u32) << 24)
            | ((resolver_audit::RESOLVER_RESULT_NO_ENGINE_ARM as u32) << 16);
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT_RESULT,
            verdict_meta,
            resolver_audit::RESOLVER_REASON_NO_ENGINE as u32,
            resolver_audit::RESOLVER_FUEL_NONE as u32,
            lane,
        );
    }
}
