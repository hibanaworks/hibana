use super::super::super::{CursorEndpoint, Transport, emit, events};
use crate::global::const_dsl::ScopeId;
use crate::session::cluster::core::DecisionArm;

const RESOLVER_AUDIT_LEFT: u8 = 0;
const RESOLVER_AUDIT_RIGHT: u8 = 1;
const RESOLVER_AUDIT_REJECT: u8 = 0xff;

#[inline]
const fn resolver_audit_result(arm: DecisionArm) -> u8 {
    match arm {
        DecisionArm::Left => RESOLVER_AUDIT_LEFT,
        DecisionArm::Right => RESOLVER_AUDIT_RIGHT,
    }
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn emit_dynamic_resolver_audit(
        &self,
        lane: u8,
        scope_id: ScopeId,
        resolver_id: u16,
        result: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let event = events::resolver_audit(
            port.lane().as_wire(),
            self.sid.raw(),
            scope_id,
            resolver_id,
            result,
        );
        emit(port.tap(), event);
    }

    pub(in crate::endpoint::kernel) fn emit_dynamic_resolver_success_audit(
        &self,
        lane: u8,
        scope_id: ScopeId,
        resolver_id: u16,
        arm: DecisionArm,
    ) {
        self.emit_dynamic_resolver_audit(lane, scope_id, resolver_id, resolver_audit_result(arm));
    }

    pub(in crate::endpoint::kernel) fn emit_dynamic_resolver_reject_audit(
        &self,
        lane: u8,
        scope_id: ScopeId,
        resolver_id: u16,
    ) {
        self.emit_dynamic_resolver_audit(lane, scope_id, resolver_id, RESOLVER_AUDIT_REJECT);
    }
}
