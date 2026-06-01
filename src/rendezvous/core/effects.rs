use super::{
    Clock, ControlOp, CpError, EffIndex, Generation, LabelUniverse, Lane, PolicyMode, RawEvent,
    Rendezvous, ResourceScope, SessionId, TapEvent, Transport, control_op_tap_event_id, emit,
};

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub(crate) fn register_policy(
        &mut self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), CpError> {
        if policy.is_dynamic() && self.ensure_policy_table_storage().is_none() {
            return Err(CpError::resource_exhausted(ResourceScope::PolicyTable));
        }
        self.policies
            .register(lane, eff_index, tag, policy)
            .map_err(|_| CpError::resource_exhausted(ResourceScope::PolicyTable))
    }

    pub(crate) fn policy(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        self.policies.get(lane, eff_index, tag)
    }

    pub(crate) fn reset_policy(&self, lane: Lane) {
        self.policies.reset_lane(lane);
    }

    pub(in crate::rendezvous::core) fn emit_effect(
        &self,
        effect: ControlOp,
        sid: SessionId,
        lane: Lane,
        arg: u32,
    ) {
        let event_id = control_op_tap_event_id(effect);
        let causal = TapEvent::make_causal_key(lane.as_wire(), 1);
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), event_id)
                .with_causal_key(causal)
                .with_arg0(sid.raw())
                .with_arg1(arg),
        );
    }

    pub(crate) fn emit_topology_ack(
        &self,
        sid: SessionId,
        from_lane: Lane,
        to_lane: Lane,
        generation: Generation,
    ) {
        let packed = ((from_lane.as_wire() as u32) & 0xFF)
            | (((to_lane.as_wire() as u32) & 0xFF) << 8)
            | ((generation.0 as u32) << 16);
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), crate::observe::ids::TOPOLOGY_ACK)
                .with_arg0(packed)
                .with_arg1(sid.raw()),
        );
    }
}
