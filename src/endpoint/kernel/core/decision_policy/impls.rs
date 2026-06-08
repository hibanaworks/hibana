mod select;

use super::super::{
    ControlDesc, ControlOp, ControlSemanticKind, CursorEndpoint, DecisionSubject,
    DynamicPolicyResolution, EpochTable, LabelUniverse, MintConfigMarker, PolicySlot, ScopeId,
    SendError, SendMeta, SendResult, Transport,
    control_policy_is_validated_during_handle_preparation, decision_policy_input_arg0, events, ids,
    policy_runtime,
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
        let decision_signals = self.policy_signals_for_slot(PolicySlot::Decision);
        match dynamic_kind {
            ControlSemanticKind::LoopContinue | ControlSemanticKind::LoopBreak => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_loop_decision_policy(meta, op, &decision_signals)
            }
            ControlSemanticKind::DecisionArm => {
                if control.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                self.evaluate_arm_decision_policy(
                    meta,
                    target_label,
                    DecisionSubject::RouteArm,
                    &decision_signals,
                )
            }
            ControlSemanticKind::Other => {
                if control.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                let subject = if meta.scope.is_none() {
                    DecisionSubject::RouteArm
                } else {
                    self.cursor
                        .route_scope_controller_policy(meta.scope)
                        .map(|(_, _, _, subject)| subject)
                        .unwrap_or(DecisionSubject::RouteArm)
                };
                self.evaluate_arm_decision_policy(meta, target_label, subject, &decision_signals)
            }
        }
    }

    fn emit_decision_policy_audit(
        &self,
        scope_id: ScopeId,
        lane: u8,
        policy_id: u16,
        signals: &crate::transport::context::PolicySignals,
    ) -> SendResult<()> {
        let port = self.port_for_lane(lane as usize);
        let policy_input = signals.input();
        let policy_words = policy_input.replay_words();
        let arg0 = decision_policy_input_arg0(policy_input);
        let mut event = events::RawEvent::new(port.now32(), ids::ROUTE_ARM_SELECTION)
            .with_arg0(arg0)
            .with_arg1(policy_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let policy_digest = port.policy_digest(PolicySlot::Decision);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_runtime::hash_empty_policy_attrs();
        let policy_attrs_replay_hash = policy_runtime::hash_empty_policy_replay_attrs();
        let replay_attrs = policy_runtime::EMPTY_POLICY_ATTR_WORDS;
        let replay_policy_attr_presence = policy_runtime::EMPTY_POLICY_ATTR_PRESENCE;
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
            policy_attrs_replay_hash,
            ((policy_runtime::slot_tag(PolicySlot::Decision) as u32) << 24)
                | ((mode_id as u32) << 16),
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
            policy_words[0],
            policy_words[1],
            policy_words[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT1,
            policy_words[3],
            0,
            0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_ATTRS0,
            replay_attrs[0],
            replay_attrs[1],
            replay_attrs[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_ATTRS1,
            replay_attrs[3],
            replay_policy_attr_presence as u32,
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
        Ok(())
    }

    fn evaluate_arm_decision_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        subject: DecisionSubject,
        signals: &crate::transport::context::PolicySignals,
    ) -> SendResult<()> {
        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        self.emit_decision_policy_audit(scope_id, meta.lane, policy_id, signals)?;

        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let resolution = cluster
            .resolve_dynamic_policy(self.rendezvous_id(), meta.eff_index, subject)
            .map_err(Self::map_cp_error)?;

        match resolution {
            DynamicPolicyResolution::DecisionArm { arm } if arm == arm_index => Ok(()),
            DynamicPolicyResolution::DecisionArm { .. } => {
                Err(SendError::PolicyAbort { reason: policy_id })
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    fn evaluate_loop_decision_policy(
        &mut self,
        meta: &SendMeta,
        op: ControlOp,
        signals: &crate::transport::context::PolicySignals,
    ) -> SendResult<()> {
        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;
        if meta.scope.is_none() || meta.scope != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        self.emit_decision_policy_audit(meta.scope, meta.lane, policy_id, signals)?;

        let subject = DecisionSubject::from_loop_control(op).ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let resolution = cluster
            .resolve_dynamic_policy(self.rendezvous_id(), meta.eff_index, subject)
            .map_err(Self::map_cp_error)?;
        let expected_arm = match op {
            ControlOp::LoopContinue => 0,
            ControlOp::LoopBreak => 1,
            _ => return Err(SendError::PhaseInvariant),
        };
        match resolution {
            DynamicPolicyResolution::DecisionArm { arm } if arm == expected_arm => Ok(()),
            DynamicPolicyResolution::DecisionArm { .. } | DynamicPolicyResolution::Defer => {
                Err(SendError::PolicyAbort { reason: policy_id })
            }
        }
    }
}
