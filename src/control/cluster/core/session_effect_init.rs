use super::*;

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) fn init_session_effects_for_lane(
        rv: &mut crate::rendezvous::core::Rendezvous<'_, 'cfg, T, U, C>,
        sid: SessionId,
        lane: Lane,
        effect_envelope: EffectEnvelopeRef<'_>,
    ) -> Result<(), CpError> {
        rv.ensure_core_lane_storage_for_lane_slots((lane.raw() as usize).saturating_add(1))
            .ok_or(CpError::resource_exhausted(ResourceScope::Generic))?;
        let mut has_resources = false;
        let effects_already_installed = effect_envelope.resources().all(|descriptor| {
            has_resources = true;
            rv.policy(lane, descriptor.eff_index(), descriptor.tag())
                == Some(effect_envelope.resource_policy(&descriptor))
        });
        if has_resources && effects_already_installed {
            return Ok(());
        }

        rv.reset_policy(lane);
        let mut control_marker_count = 0u32;
        for scope_kind in effect_envelope.control_scopes() {
            if matches!(scope_kind, ControlScopeKind::Topology) {
                rv.prepare_topology_control_scope(lane)
                    .ok_or(CpError::resource_exhausted(ResourceScope::Generic))?;
            } else {
                rv.initialise_control_scope(lane, scope_kind);
            }
            control_marker_count = control_marker_count.saturating_add(1);
        }

        let mut applied_effects = 0u32;
        let mut resource_events = 0u32;
        for descriptor in effect_envelope.resources() {
            resource_events = resource_events.saturating_add(1);
            rv.register_policy(
                lane,
                descriptor.eff_index(),
                descriptor.tag(),
                effect_envelope.resource_policy(&descriptor),
            )?;
        }

        if resource_events > 0 {
            applied_effects = applied_effects.saturating_add(resource_events);
        }

        if applied_effects == 0 && control_marker_count > 0 {
            applied_effects = control_marker_count.max(1);
        }

        if applied_effects > 0 {
            let ts = rv.now32();
            crate::observe::core::push(crate::observe::events::EffectInit::new(
                ts,
                sid.raw(),
                applied_effects,
            ));
        }

        Ok(())
    }

    pub(crate) fn after_local_effect(&self, envelope: CpCommand) -> Result<PendingEffect, CpError> {
        match envelope.effect {
            ControlOp::TopologyBegin => {
                let Some(operands) = envelope.topology else {
                    return Ok(PendingEffect::None);
                };
                let sid = envelope
                    .sid
                    .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                self.with_control_mut(|core| {
                    let (_intent, _ack) = core
                        .topology_state
                        .begin(sid, operands)
                        .expect("topology begin bookkeeping was preflighted before local mutation");
                    Ok(PendingEffect::None)
                })
            }
            _ => Ok(PendingEffect::None),
        }
    }
}
