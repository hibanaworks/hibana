#[path = "impls/select.rs"]
mod select;

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
    pub(crate) fn evaluate_dynamic_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        control: Option<ControlDesc>,
    ) -> SendResult<()> {
        if !meta.policy().is_dynamic() {
            return Ok(());
        }
        if let Some(control) = control
            && control_policy_is_validated_during_handle_preparation(control.op())
        {
            return Ok(());
        }
        let dynamic_kind = self.control_semantic_kind(meta.semantic);
        let route_signals = self.policy_signals_for_slot(PolicySlot::Route).into_owned();
        match dynamic_kind {
            ControlSemanticKind::LoopContinue | ControlSemanticKind::LoopBreak => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_loop_policy(meta, op, &route_signals)
            }
            ControlSemanticKind::RouteArm => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_route_policy(meta, target_label, op, &route_signals)
            }
            ControlSemanticKind::Other => {
                if control.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                let op = if meta.scope.is_none() {
                    ControlOp::RouteDecision
                } else {
                    self.cursor
                        .route_scope_controller_policy(meta.scope)
                        .map(|(_, _, _, op)| op)
                        .unwrap_or(ControlOp::RouteDecision)
                };
                self.evaluate_route_policy(meta, target_label, op, &route_signals)
            }
        }
    }

    fn emit_route_policy_audit(
        &self,
        scope_id: ScopeId,
        lane: u8,
        policy_id: u16,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) {
        let port = self.port_for_lane(lane as usize);
        let _ = port.flush_transport_events();
        let transport_attrs = port.transport().metrics().attrs();
        let mut policy_attrs = *signals.attrs();
        policy_attrs.copy_from(&transport_attrs);
        let policy_input = signals.input;
        let arg0 = route_policy_input_arg0(&policy_input);
        let mut event = events::RawEvent::new(port.now32(), ids::ROUTE_DECISION)
            .with_arg0(arg0)
            .with_arg1(policy_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let policy_digest = port.policy_digest(PolicySlot::Route);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_attrs.hash32();
        let transport_snapshot_hash = policy_runtime::hash_transport_attrs(&policy_attrs);
        let replay_transport = policy_runtime::replay_transport_inputs(&policy_attrs);
        let replay_transport_presence = policy_runtime::replay_transport_presence(&policy_attrs);
        let mode_id = policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            policy_attrs_hash,
            transport_snapshot_hash,
            ((policy_runtime::slot_tag(PolicySlot::Route) as u32) << 24) | ((mode_id as u32) << 16),
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_input[0],
            policy_input[1],
            policy_input[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT1,
            policy_input[3],
            0,
            0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT0,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT1,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
            port.lane(),
        );
        let verdict = policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            policy_runtime::verdict_reason(verdict) as u32,
            policy_runtime::POLICY_FUEL_NONE as u32,
            port.lane(),
        );
    }

    fn evaluate_route_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        op: ControlOp,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        // Route decisions are fixed at the offer/decode decision point.
        // Re-evaluating dynamic route policy for local self-send can diverge from
        // the selected arm and introduce non-deterministic PolicyAbort.
        if meta.peer == ROLE {
            return Ok(());
        }

        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        self.emit_route_policy_audit(scope_id, meta.lane, policy_id, signals);

        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                op,
                signals.input,
                &attrs,
            )
            .map_err(Self::map_cp_error)?;

        match resolution {
            DynamicPolicyResolution::RouteArm { arm } if arm == arm_index => Ok(()),
            DynamicPolicyResolution::RouteArm { .. } => {
                Err(SendError::PolicyAbort { reason: policy_id })
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    fn evaluate_loop_policy(
        &mut self,
        meta: &SendMeta,
        op: ControlOp,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        // For local control (self-send), the caller explicitly chooses continue/break.
        // No resolver validation is needed - the caller's choice is authoritative.
        if meta.peer == ROLE {
            return Ok(());
        }

        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;

        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                op,
                signals.input,
                &attrs,
            )
            .map_err(Self::map_cp_error)?;

        if meta.scope.is_none() || meta.scope != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match resolution {
            DynamicPolicyResolution::Loop { decision } => {
                let disposition = if decision {
                    LoopDisposition::Continue
                } else {
                    LoopDisposition::Break
                };
                if !loop_control_kind_matches_disposition(meta.semantic, disposition) {
                    return Err(SendError::PolicyAbort { reason: policy_id });
                }
                Ok(())
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }
}
