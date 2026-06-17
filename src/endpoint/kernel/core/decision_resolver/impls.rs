mod select;

use super::super::{
    CursorEndpoint, EventSemanticKind, SendError, SendMeta, SendResult, Transport, emit, events,
};
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
    pub(crate) fn decide_dynamic_resolver(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
    ) -> SendResult<()> {
        let crate::global::const_dsl::RouteResolver::Dynamic { .. } = meta.resolver() else {
            return Ok(());
        };
        match meta.semantic {
            EventSemanticKind::DecisionArm => self.decide_dynamic_route_arm(meta, target_label),
            EventSemanticKind::ProtocolEvent => {
                if !meta.scope.is_none() {
                    self.cursor
                        .route_scope_controller_resolver(meta.scope)
                        .ok_or(SendError::PhaseInvariant)?;
                }
                self.decide_dynamic_route_arm(meta, target_label)
            }
        }
    }

    fn emit_dynamic_resolver_audit(
        &self,
        lane: u8,
        scope_id: ScopeId,
        resolver_id: u16,
        result: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let event = events::resolver_audit(
            port.now32(),
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

    fn decide_dynamic_route_arm(&mut self, meta: &SendMeta, target_label: u8) -> SendResult<()> {
        let crate::global::const_dsl::RouteResolver::Dynamic {
            resolver_id,
            scope: resolver_scope,
        } = meta.resolver()
        else {
            return Err(SendError::PhaseInvariant);
        };

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != resolver_scope.to_scope_id() {
            return Err(SendError::PhaseInvariant);
        }

        let cluster = self.session.cluster();
        let resolution = match cluster.resolve_dynamic_resolver(
            self.rendezvous_id(),
            meta.eff_index,
            resolver_id,
        ) {
            Ok(resolution) => {
                self.emit_dynamic_resolver_success_audit(
                    meta.lane,
                    scope_id,
                    resolver_id,
                    resolution,
                );
                resolution
            }
            Err(crate::session::cluster::error::ClusterError::ResolverReject { resolver_id }) => {
                self.emit_dynamic_resolver_reject_audit(meta.lane, scope_id, resolver_id);
                return Err(SendError::ResolverReject { resolver_id });
            }
            Err(err) => return Err(Self::send_error_from_cluster(err)),
        };

        if resolution.index() == arm_index {
            Ok(())
        } else {
            Err(SendError::ResolverReject { resolver_id })
        }
    }

    #[inline]
    fn send_error_from_cluster(err: crate::session::cluster::error::ClusterError) -> SendError {
        match err {
            crate::session::cluster::error::ClusterError::ResolverReject { resolver_id } => {
                SendError::ResolverReject { resolver_id }
            }
            crate::session::cluster::error::ClusterError::RendezvousMismatch { .. }
            | crate::session::cluster::error::ClusterError::RendezvousUnregistered { .. }
            | crate::session::cluster::error::ClusterError::RendezvousBusy { .. }
            | crate::session::cluster::error::ClusterError::ResourceExhausted { .. }
            | crate::session::cluster::error::ClusterError::DynamicResolverInvariant { .. } => {
                SendError::PhaseInvariant
            }
        }
    }
}
