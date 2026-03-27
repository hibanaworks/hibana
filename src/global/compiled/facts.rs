use crate::{
    control::{
        cluster::effects::{CpEffect, EffectEnvelope},
        lease::planner::LeaseGraphBudget,
    },
    eff::{EffAtom, EffIndex, EffKind},
    global::{
        ControlLabelSpec,
        const_dsl::{EffList, PolicyMode},
    },
};

/// Precomputed dynamic policy site discovered during program lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicPolicySite {
    eff_index: EffIndex,
    label: u8,
    resource_tag: Option<u8>,
    policy: PolicyMode,
}

impl DynamicPolicySite {
    const EMPTY: Self = Self {
        eff_index: EffIndex::ZERO,
        label: 0,
        resource_tag: None,
        policy: PolicyMode::Static,
    };

    #[inline(always)]
    const fn new(
        eff_index: EffIndex,
        label: u8,
        resource_tag: Option<u8>,
        policy: PolicyMode,
    ) -> Self {
        Self {
            eff_index,
            label,
            resource_tag,
            policy,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(&self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    pub(crate) const fn label(&self) -> u8 {
        self.label
    }

    #[inline(always)]
    pub(crate) const fn resource_tag(&self) -> Option<u8> {
        self.resource_tag
    }

    #[inline(always)]
    pub(crate) const fn policy(&self) -> PolicyMode {
        self.policy
    }

    #[inline(always)]
    pub(crate) const fn policy_id(&self) -> u16 {
        match self.policy {
            PolicyMode::Dynamic { policy_id, .. } => policy_id,
            PolicyMode::Static => 0,
        }
    }
}

const MAX_DYNAMIC_POLICY_SITES: usize = crate::eff::meta::MAX_EFF_NODES;

/// Crate-private owner for program-level lowering facts.
#[derive(Clone, Copy)]
pub(crate) struct ProgramFacts {
    lease_budget: LeaseGraphBudget,
    effect_envelope: EffectEnvelope,
    dynamic_policy_sites: [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
    dynamic_policy_sites_len: usize,
}

impl ProgramFacts {
    pub(crate) const fn from_eff_list(eff_list: &EffList) -> Self {
        let lease_budget = LeaseGraphBudget::from_eff_list(eff_list);
        lease_budget.validate();
        let (effect_envelope, dynamic_policy_sites, dynamic_policy_sites_len) =
            Self::compile(eff_list);
        Self {
            lease_budget,
            effect_envelope,
            dynamic_policy_sites,
            dynamic_policy_sites_len,
        }
    }

    #[inline(always)]
    pub(crate) const fn lease_budget(&self) -> LeaseGraphBudget {
        self.lease_budget
    }

    #[inline(always)]
    pub(crate) const fn effect_envelope(&self) -> &EffectEnvelope {
        &self.effect_envelope
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites(&self) -> &[DynamicPolicySite] {
        &self.dynamic_policy_sites[..self.dynamic_policy_sites_len]
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = &DynamicPolicySite> + '_ {
        self.dynamic_policy_sites()
            .iter()
            .filter(move |site| site.policy_id() == policy_id)
    }

    const fn compile(
        eff_list: &EffList,
    ) -> (
        EffectEnvelope,
        [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
        usize,
    ) {
        let mut effect_envelope = EffectEnvelope::empty();
        let mut dynamic_policy_sites = [DynamicPolicySite::EMPTY; MAX_DYNAMIC_POLICY_SITES];
        let mut dynamic_policy_sites_len = 0usize;

        let nodes = eff_list.as_slice();
        let mut offset = 0usize;
        while offset < nodes.len() {
            let node = nodes[offset];
            if matches!(node.kind, EffKind::Atom) {
                let policy = match eff_list.policy_with_scope(offset) {
                    Some((policy, _scope)) => policy,
                    None => PolicyMode::Static,
                };
                let atom = node.atom_data();
                let control_spec = eff_list.control_spec_at(offset);
                Self::emit_atom(&mut effect_envelope, atom, offset, policy, control_spec);
                if policy.is_dynamic() {
                    Self::push_dynamic_policy_site(
                        &mut dynamic_policy_sites,
                        &mut dynamic_policy_sites_len,
                        DynamicPolicySite::new(
                            EffIndex::from_usize(offset),
                            atom.label,
                            atom.resource,
                            policy,
                        ),
                    );
                }
            }
            offset += 1;
        }

        let control_markers = eff_list.control_markers();
        let mut control_idx = 0usize;
        while control_idx < control_markers.len() {
            let marker = control_markers[control_idx];
            effect_envelope.push_control_marker(marker);
            if marker.tap_id != 0 {
                effect_envelope.push_tap_event(marker.tap_id);
            }
            control_idx += 1;
        }

        let scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        while scope_idx < scope_markers.len() {
            effect_envelope.push_scope_marker(scope_markers[scope_idx]);
            scope_idx += 1;
        }

        (
            effect_envelope,
            dynamic_policy_sites,
            dynamic_policy_sites_len,
        )
    }

    const fn emit_atom(
        effect_envelope: &mut EffectEnvelope,
        atom: EffAtom,
        offset: usize,
        policy: PolicyMode,
        control_spec: Option<ControlLabelSpec>,
    ) {
        use crate::eff::EffDirection;

        if atom.is_control {
            if let Some(resource_kind_tag) = atom.resource {
                if let Some(effect) = CpEffect::from_resource_tag(resource_kind_tag) {
                    effect_envelope.push_cp_effect(effect);
                    effect_envelope.push_tap_event(effect.to_tap_event_id());
                } else {
                    let tap_id = 0x0300 + atom.label as u16;
                    effect_envelope.push_tap_event(tap_id);
                }

                if let Some(rule) = control_spec {
                    effect_envelope.push_resource(
                        EffIndex::from_usize(offset),
                        atom.label,
                        rule.scope_kind,
                        resource_kind_tag,
                        rule.shot,
                        policy,
                    );
                } else {
                    effect_envelope.push_resource(
                        EffIndex::from_usize(offset),
                        atom.label,
                        crate::global::const_dsl::ControlScopeKind::None,
                        resource_kind_tag,
                        crate::control::cap::mint::CapShot::One,
                        policy,
                    );
                }
            } else {
                let tap_id = 0x0300 + atom.label as u16;
                effect_envelope.push_tap_event(tap_id);
            }
        } else {
            if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
                panic!("static policy attached to non-control atom");
            }
            match atom.direction {
                EffDirection::Send => {
                    let tap_id = 0x0200 + atom.label as u16;
                    effect_envelope.push_tap_event(tap_id);
                }
                EffDirection::Recv => {
                    let tap_id = 0x0210 + atom.label as u16;
                    effect_envelope.push_tap_event(tap_id);
                }
            }
        }
    }

    const fn push_dynamic_policy_site(
        dynamic_policy_sites: &mut [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
        dynamic_policy_sites_len: &mut usize,
        site: DynamicPolicySite,
    ) {
        if *dynamic_policy_sites_len >= MAX_DYNAMIC_POLICY_SITES {
            panic!("ProgramFacts: MAX_DYNAMIC_POLICY_SITES exceeded");
        }
        dynamic_policy_sites[*dynamic_policy_sites_len] = site;
        *dynamic_policy_sites_len += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::cap::{mint::GenericCapToken, mint::ResourceKind, resource_kinds::CheckpointKind},
        g::{self, Msg, Role},
        global::CanonicalControl,
    };

    #[test]
    fn program_facts_builds_effect_envelope() {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { crate::runtime::consts::LABEL_CHECKPOINT },
                GenericCapToken<CheckpointKind>,
                CanonicalControl<CheckpointKind>,
            >,
            0,
        >();
        let eff = program.into_eff();
        let facts = ProgramFacts::from_eff_list(&eff);

        assert_eq!(facts.effect_envelope().cp_effects().count(), 1);
        assert_eq!(facts.effect_envelope().resources().count(), 1);
    }

    #[test]
    fn program_facts_records_dynamic_policy_sites() {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { crate::runtime::consts::LABEL_CHECKPOINT },
                GenericCapToken<CheckpointKind>,
                CanonicalControl<CheckpointKind>,
            >,
            0,
        >()
        .policy::<77>();
        let eff = program.into_eff();
        let facts = ProgramFacts::from_eff_list(&eff);

        let sites = facts.dynamic_policy_sites();
        assert_eq!(sites.len(), 1);
        assert_eq!(facts.dynamic_policy_sites_for(77).count(), 1);

        let site = sites[0];
        assert_eq!(site.eff_index(), EffIndex::ZERO);
        assert_eq!(site.label(), crate::runtime::consts::LABEL_CHECKPOINT);
        assert_eq!(site.resource_tag(), Some(CheckpointKind::TAG));
        assert_eq!(
            site.policy(),
            PolicyMode::Dynamic {
                policy_id: 77,
                scope: crate::global::const_dsl::ScopeId::none(),
            }
        );
    }
}
