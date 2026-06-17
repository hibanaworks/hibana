use super::{
    Arm, CursorEndpoint, RecvError, RecvResult, ScopeId, TapFrameMeta, Transport, emit, events,
    state_index_to_usize,
};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) enum IngressEvidenceState {
    Absent = 0,
    Ready = 1,
}

impl IngressEvidenceState {
    #[inline]
    pub(in crate::endpoint::kernel) const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(crate) fn is_reentry_route(&self, scope: ScopeId) -> bool {
        self.cursor.route_scope_reentry(scope)
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
        Some(if offer_entry.is_absent() {
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

    pub(crate) fn current_route_arm_authorized(&self) -> RecvResult<bool> {
        self.cursor
            .current_route_arm_authorization(
                |scope| self.selected_arm_for_scope(scope),
                |scope| self.preview_selected_arm_for_scope(scope),
            )
            .map(|authorization| authorization.authorizes_current_arm())
            .map_err(|_| RecvError::PhaseInvariant)
    }

    pub(crate) fn emit_endpoint_event(&self, id: u16, meta: TapFrameMeta, lane: u8) {
        let port = self.port_for_lane(lane as usize);
        let packed =
            ((ROLE as u32) << 24) | ((meta.lane as u32) << 16) | ((meta.label as u32) << 8);
        let event = events::raw_event(port.now32(), id)
            .with_arg0(meta.sid)
            .with_arg1(packed);
        emit(port.tap(), event);
    }
}
