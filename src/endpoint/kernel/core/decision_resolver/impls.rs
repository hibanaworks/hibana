mod select;

use super::super::{
    CursorEndpoint, DynamicResolverResolution, EventSemanticKind, ResolverSlot, SendError,
    SendMeta, SendResult, Transport, events, ids,
};
impl<'r, const ROLE: u8, T, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    pub(crate) fn evaluate_dynamic_resolver(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
    ) -> SendResult<()> {
        let crate::global::const_dsl::RouteResolver::Dynamic { .. } = meta.resolver() else {
            return Ok(());
        };
        match meta.semantic {
            EventSemanticKind::DecisionArm => {
                self.evaluate_arm_decision_resolver(meta, target_label)
            }
            EventSemanticKind::ProtocolEvent => {
                if !meta.scope.is_none() {
                    self.cursor
                        .route_scope_controller_resolver(meta.scope)
                        .ok_or(SendError::PhaseInvariant)?;
                }
                self.evaluate_arm_decision_resolver(meta, target_label)
            }
        }
    }

    fn emit_decision_resolver_audit(&self, lane: u8, resolver_id: u16) {
        let port = self.port_for_lane(lane as usize);
        let event = events::raw_event(port.now32(), ids::ROUTE_ARM_SELECTION)
            .with_arg0(self.sid.raw())
            .with_arg1(resolver_id as u32);
        self.emit_resolver_audit_replay(ResolverSlot::Decision, event, port.lane());
    }

    fn evaluate_arm_decision_resolver(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
    ) -> SendResult<()> {
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

        self.emit_decision_resolver_audit(meta.lane, resolver_id);

        let cluster = self.session.cluster();
        let resolution = cluster
            .resolve_dynamic_resolver(self.rendezvous_id(), meta.eff_index, resolver_id)
            .map_err(Self::send_error_from_cluster)?;

        match resolution {
            DynamicResolverResolution::DecisionArm { arm } if arm == arm_index => Ok(()),
            DynamicResolverResolution::DecisionArm { .. } => {
                Err(SendError::ResolverReject { resolver_id })
            }
            DynamicResolverResolution::Defer => Err(SendError::ResolverReject { resolver_id }),
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
