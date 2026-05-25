use super::*;

impl<'rv, 'cfg, T, U, C, E> EffectRunner for Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn run_effect(&mut self, envelope: CpCommand) -> Result<(), CpError> {
        let envelope = match envelope.effect {
            ControlOp::CapDelegate => envelope.canonicalize_delegate()?,
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                envelope.canonicalize_topology()?
            }
            _ => envelope,
        };
        if matches!(
            envelope.effect,
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit
        ) {
            return Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState,
            ));
        }
        let lane_opt = envelope.lane.map(|lane| Lane::new(lane.raw()));
        let sid_opt = envelope.sid.map(|sid| SessionId::new(sid.raw()));

        let policy_event =
            RawEvent::new(self.clock.now32(), control_op_tap_event_id(envelope.effect))
                .with_arg0(sid_opt.map_or(0, |sid| sid.raw()))
                .with_arg1(lane_opt.map_or(0, |lane| lane.raw()));

        let _ = self.flush_transport_events();
        let policy_attrs = self.transport.metrics().attrs();
        let policy_input = crate::policy_runtime::slot_default_input(
            crate::policy_runtime::PolicySlot::Rendezvous,
        );
        let policy_digest = self.policy_digest(crate::policy_runtime::PolicySlot::Rendezvous);
        let event_hash = crate::policy_runtime::hash_tap_event(&policy_event);
        let signals_input_hash = crate::policy_runtime::hash_policy_input(policy_input);
        let signals_attrs_hash = policy_attrs.hash32();
        let transport_snapshot_hash = crate::policy_runtime::hash_transport_attrs(&policy_attrs);
        let replay_transport = crate::policy_runtime::replay_transport_inputs(&policy_attrs);
        let replay_transport_presence =
            crate::policy_runtime::replay_transport_presence(&policy_attrs);
        let mode_id = crate::policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT,
            lane_opt,
            policy_digest,
            event_hash,
            signals_input_hash,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_EXT,
            lane_opt,
            signals_attrs_hash,
            transport_snapshot_hash,
            ((crate::policy_runtime::slot_tag(crate::policy_runtime::PolicySlot::Rendezvous)
                as u32)
                << 24)
                | ((mode_id as u32) << 16),
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_EVENT,
            lane_opt,
            policy_event.ts,
            policy_event.id as u32,
            policy_event.arg0,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_EVENT_EXT,
            lane_opt,
            policy_event.arg1,
            policy_event.arg2,
            policy_event.causal_key as u32,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_INPUT0,
            lane_opt,
            policy_input[0],
            policy_input[1],
            policy_input[2],
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_INPUT1,
            lane_opt,
            policy_input[3],
            0,
            0,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_TRANSPORT0,
            lane_opt,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_TRANSPORT1,
            lane_opt,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
        );
        let verdict = crate::policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((crate::policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((crate::policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_RESULT,
            lane_opt,
            verdict_meta,
            crate::policy_runtime::verdict_reason(verdict) as u32,
            crate::policy_runtime::POLICY_FUEL_NONE as u32,
        );

        self.perform_effect(envelope)
    }
}

// ============================================================================
