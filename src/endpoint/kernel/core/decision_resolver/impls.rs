mod select;

use super::super::{
    CursorEndpoint, DynamicResolverResolution, EventSemanticKind, ResolverSlot, ScopeId, SendError,
    SendMeta, SendResult, Transport, events, ids, resolver_audit,
};
impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    pub(crate) fn evaluate_dynamic_resolver(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
    ) -> SendResult<()> {
        if !meta.resolver().is_dynamic() {
            return Ok(());
        }
        let dynamic_kind = meta.semantic;
        match dynamic_kind {
            EventSemanticKind::DecisionArm => {
                self.evaluate_arm_decision_resolver(meta, target_label)
            }
            EventSemanticKind::Other => {
                if !meta.scope.is_none() {
                    self.cursor
                        .route_scope_controller_resolver(meta.scope)
                        .ok_or(SendError::PhaseInvariant)?;
                }
                self.evaluate_arm_decision_resolver(meta, target_label)
            }
        }
    }

    fn emit_decision_resolver_audit(
        &self,
        scope_id: ScopeId,
        lane: u8,
        resolver_id: u16,
    ) -> SendResult<()> {
        let port = self.port_for_lane(lane as usize);
        let resolver_words = resolver_audit::EMPTY_RESOLVER_INPUT_WORDS;
        let arg0 = resolver_audit::EMPTY_RESOLVER_INPUT_ARG0;
        let mut event = events::raw_event(port.now32(), ids::ROUTE_ARM_SELECTION)
            .with_arg0(arg0)
            .with_arg1(resolver_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let resolver_digest = port.resolver_digest(ResolverSlot::Decision);
        let event_hash = resolver_audit::hash_tap_event(&event);
        let signals_input_hash = resolver_audit::hash_empty_resolver_input();
        let resolver_attrs_hash = resolver_audit::hash_empty_resolver_attrs();
        let resolver_attrs_replay_hash = resolver_audit::hash_empty_resolver_replay_attrs();
        let replay_attrs = resolver_audit::EMPTY_RESOLVER_ATTR_WORDS;
        let replay_resolver_attr_presence = resolver_audit::EMPTY_RESOLVER_ATTR_PRESENCE;
        let mode_id = resolver_audit::RESOLVER_MODE_AUDIT_ONLY_TAG;
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT,
            resolver_digest,
            event_hash,
            signals_input_hash,
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT_EXT,
            resolver_attrs_hash,
            resolver_attrs_replay_hash,
            ((resolver_audit::slot_tag(ResolverSlot::Decision) as u32) << 24)
                | ((mode_id as u32) << 16),
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_INPUT0,
            resolver_words[0],
            resolver_words[1],
            resolver_words[2],
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_INPUT1,
            resolver_words[3],
            0,
            0,
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_ATTRS0,
            replay_attrs[0],
            replay_attrs[1],
            replay_attrs[2],
            port.lane(),
        );
        self.emit_resolver_audit_event(
            ids::RESOLVER_REPLAY_ATTRS1,
            replay_attrs[3],
            replay_resolver_attr_presence as u32,
            0,
            port.lane(),
        );
        let verdict_meta = ((resolver_audit::RESOLVER_RESULT_NO_ENGINE_TAG as u32) << 24)
            | ((resolver_audit::RESOLVER_RESULT_NO_ENGINE_ARM as u32) << 16);
        self.emit_resolver_audit_event(
            ids::RESOLVER_AUDIT_RESULT,
            verdict_meta,
            resolver_audit::RESOLVER_REASON_NO_ENGINE as u32,
            resolver_audit::RESOLVER_FUEL_NONE as u32,
            port.lane(),
        );
        Ok(())
    }

    fn evaluate_arm_decision_resolver(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
    ) -> SendResult<()> {
        let resolver = meta.resolver();
        let resolver_id = resolver
            .dynamic_resolver_id()
            .ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != resolver.scope() {
            return Err(SendError::PhaseInvariant);
        }

        self.emit_decision_resolver_audit(scope_id, meta.lane, resolver_id)?;

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
            DynamicResolverResolution::NoAuthority => Err(SendError::PhaseInvariant),
        }
    }

    #[inline]
    fn send_error_from_cluster(err: crate::session::cluster::error::ClusterError) -> SendError {
        match err {
            crate::session::cluster::error::ClusterError::ResolverReject { resolver_id } => {
                SendError::ResolverReject { resolver_id }
            }
            crate::session::cluster::error::ClusterError::RendezvousMismatch { .. }
            | crate::session::cluster::error::ClusterError::RendezvousMissing { .. }
            | crate::session::cluster::error::ClusterError::RendezvousBusy { .. }
            | crate::session::cluster::error::ClusterError::ResourceExhausted { .. }
            | crate::session::cluster::error::ClusterError::UnsupportedEffect(_)
            | crate::session::cluster::error::ClusterError::DynamicResolverInvariant { .. } => {
                SendError::PhaseInvariant
            }
        }
    }
}
