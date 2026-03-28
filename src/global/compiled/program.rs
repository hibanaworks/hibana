use core::ptr;

use crate::{
    control::{
        cap::mint::ResourceKind,
        cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, RerouteKind, RouteDecisionKind, SpliceAckKind,
            SpliceIntentKind,
        },
        cluster::effects::{CpEffect, EffectEnvelope},
        lease::planner::LeaseGraphBudget,
    },
    eff::{EffAtom, EffIndex, EffKind},
    global::{
        ControlLabelSpec,
        const_dsl::{ControlScopeKind, PolicyMode},
    },
};

#[cfg(test)]
use crate::global::const_dsl::EffList;

use super::LoweringSummary;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlSemanticKind {
    Other,
    RouteArm,
    LoopContinue,
    LoopBreak,
    SpliceIntent,
    SpliceAck,
    Reroute,
}

impl ControlSemanticKind {
    #[inline(always)]
    pub(crate) const fn is_loop(self) -> bool {
        matches!(self, Self::LoopContinue | Self::LoopBreak)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlSemanticsTable {
    by_label: [ControlSemanticKind; 256],
    by_resource_tag: [ControlSemanticKind; 256],
}

impl ControlSemanticsTable {
    pub(crate) const EMPTY: Self = Self {
        by_label: [ControlSemanticKind::Other; 256],
        by_resource_tag: [ControlSemanticKind::Other; 256],
    };

    #[inline(always)]
    pub(crate) const fn semantic_for_label(&self, label: u8) -> ControlSemanticKind {
        self.by_label[label as usize]
    }

    #[inline(always)]
    pub(crate) const fn semantic_for_resource_tag(
        &self,
        resource_tag: Option<u8>,
    ) -> ControlSemanticKind {
        match resource_tag {
            Some(tag) => self.by_resource_tag[tag as usize],
            None => ControlSemanticKind::Other,
        }
    }

    #[inline(always)]
    pub(crate) const fn semantic_for(
        &self,
        label: u8,
        resource_tag: Option<u8>,
    ) -> ControlSemanticKind {
        let by_resource = self.semantic_for_resource_tag(resource_tag);
        if matches!(by_resource, ControlSemanticKind::Other) {
            self.semantic_for_label(label)
        } else {
            by_resource
        }
    }

    #[inline(always)]
    pub(crate) const fn is_loop_label(&self, label: u8) -> bool {
        self.semantic_for_label(label).is_loop()
    }

    #[inline(always)]
    const fn with_label(mut self, label: u8, kind: ControlSemanticKind) -> Self {
        self.by_label[label as usize] = kind;
        self
    }

    #[inline(always)]
    const fn with_resource_tag(mut self, tag: u8, kind: ControlSemanticKind) -> Self {
        self.by_resource_tag[tag as usize] = kind;
        self
    }
}

const MAX_DYNAMIC_POLICY_SITES: usize = crate::eff::meta::MAX_EFF_NODES;

/// Crate-private owner for program-level lowering facts.
#[derive(Clone)]
pub(crate) struct CompiledProgram {
    effect_envelope: EffectEnvelope,
    dynamic_policy_sites: [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
    dynamic_policy_sites_len: usize,
    control_semantics: ControlSemanticsTable,
}

impl CompiledProgram {
    pub(crate) const fn budget_for_role_program<'prog, const ROLE: u8, LocalSteps, Mint>(
        program: &crate::g::advanced::RoleProgram<'prog, ROLE, LocalSteps, Mint>,
    ) -> LeaseGraphBudget
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let summary = LoweringSummary::scan_const(program.lowering_input());
        summary.lease_budget()
    }

    #[cfg(test)]
    pub(crate) fn compile(eff_list: &EffList) -> Self {
        let summary = LoweringSummary::scan_const(eff_list);
        Self::from_summary(&summary)
    }

    #[cfg(test)]
    pub(crate) fn from_summary(summary: &LoweringSummary) -> Self {
        let mut compiled = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_from_summary(compiled.as_mut_ptr(), summary);
            compiled.assume_init()
        }
    }

    pub(crate) unsafe fn init_from_summary(dst: *mut Self, summary: &LoweringSummary) {
        unsafe {
            EffectEnvelope::init_empty(ptr::addr_of_mut!((*dst).effect_envelope));
            ptr::addr_of_mut!((*dst).dynamic_policy_sites)
                .write([DynamicPolicySite::EMPTY; MAX_DYNAMIC_POLICY_SITES]);
            ptr::addr_of_mut!((*dst).dynamic_policy_sites_len).write(0);
            ptr::addr_of_mut!((*dst).control_semantics).write(ControlSemanticsTable::EMPTY);
        }

        let effect_envelope = unsafe { &mut *ptr::addr_of_mut!((*dst).effect_envelope) };
        let dynamic_policy_sites = unsafe { &mut *ptr::addr_of_mut!((*dst).dynamic_policy_sites) };
        let dynamic_policy_sites_len =
            unsafe { &mut *ptr::addr_of_mut!((*dst).dynamic_policy_sites_len) };
        let control_semantics = unsafe { &mut *ptr::addr_of_mut!((*dst).control_semantics) };

        let view = summary.view();
        let mut lease_budget = LeaseGraphBudget::new();

        let nodes = view.as_slice();
        let mut offset = 0usize;
        while offset < nodes.len() {
            let node = nodes[offset];
            if matches!(node.kind, EffKind::Atom) {
                let policy = match view.policy_at(offset) {
                    Some(policy) => policy,
                    None => PolicyMode::Static,
                };
                let atom = node.atom_data();
                let control_spec = view.control_spec_at(offset);
                lease_budget = lease_budget.include_atom(atom.label, atom.resource, policy);
                Self::emit_atom(effect_envelope, atom, offset, policy, control_spec);
                Self::record_semantics(control_semantics, atom.label, atom.resource, control_spec);
                if policy.is_dynamic() {
                    Self::push_dynamic_policy_site(
                        dynamic_policy_sites,
                        dynamic_policy_sites_len,
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

        lease_budget.validate();

        let control_markers = view.control_markers();
        let mut control_idx = 0usize;
        while control_idx < control_markers.len() {
            let marker = control_markers[control_idx];
            effect_envelope.push_control_marker(marker);
            if marker.tap_id != 0 {
                effect_envelope.push_tap_event(marker.tap_id);
            }
            control_idx += 1;
        }

        let scope_markers = view.scope_markers();
        let mut scope_idx = 0usize;
        while scope_idx < scope_markers.len() {
            effect_envelope.push_scope_marker(scope_markers[scope_idx]);
            scope_idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) const fn effect_envelope(&self) -> &EffectEnvelope {
        &self.effect_envelope
    }

    #[inline(always)]
    pub(crate) const fn control_semantics(&self) -> &ControlSemanticsTable {
        &self.control_semantics
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

    const fn emit_atom(
        effect_envelope: &mut EffectEnvelope,
        atom: EffAtom,
        offset: usize,
        policy: PolicyMode,
        control_spec: Option<ControlLabelSpec>,
    ) {
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
                        ControlScopeKind::None,
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
            let tap_id = 0x0200 + atom.label as u16;
            effect_envelope.push_tap_event(tap_id);
        }
    }

    const fn push_dynamic_policy_site(
        dynamic_policy_sites: &mut [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
        dynamic_policy_sites_len: &mut usize,
        site: DynamicPolicySite,
    ) {
        if *dynamic_policy_sites_len >= MAX_DYNAMIC_POLICY_SITES {
            panic!("CompiledProgram: MAX_DYNAMIC_POLICY_SITES exceeded");
        }
        dynamic_policy_sites[*dynamic_policy_sites_len] = site;
        *dynamic_policy_sites_len += 1;
    }

    fn record_semantics(
        table: &mut ControlSemanticsTable,
        label: u8,
        resource_tag: Option<u8>,
        control_spec: Option<ControlLabelSpec>,
    ) {
        let kind = Self::semantic_kind(label, resource_tag, control_spec);
        *table = table.with_label(label, kind);
        if let Some(tag) = resource_tag {
            *table = table.with_resource_tag(tag, kind);
        }
    }

    const fn semantic_kind(
        label: u8,
        resource_tag: Option<u8>,
        control_spec: Option<ControlLabelSpec>,
    ) -> ControlSemanticKind {
        let Some(tag) = resource_tag else {
            return ControlSemanticKind::Other;
        };
        if tag == LoopContinueKind::TAG {
            return ControlSemanticKind::LoopContinue;
        }
        if tag == LoopBreakKind::TAG {
            return ControlSemanticKind::LoopBreak;
        }
        if tag == SpliceIntentKind::TAG {
            return ControlSemanticKind::SpliceIntent;
        }
        if tag == SpliceAckKind::TAG {
            return ControlSemanticKind::SpliceAck;
        }
        if tag == RerouteKind::TAG {
            return ControlSemanticKind::Reroute;
        }
        if tag == RouteDecisionKind::TAG {
            return ControlSemanticKind::RouteArm;
        }
        if let Some(spec) = control_spec
            && matches!(spec.scope_kind, ControlScopeKind::Route)
        {
            return ControlSemanticKind::RouteArm;
        }
        let _ = label;
        ControlSemanticKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::{CompiledProgram, ControlSemanticKind};
    use crate::{
        control::cap::{
            mint::{GenericCapToken, ResourceKind},
            resource_kinds::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        g::{self, Msg, Role},
        global::CanonicalControl,
        runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_ROUTE_DECISION},
    };

    const ROUTE_POLICY_ID: u16 = 4401;

    #[test]
    fn compiled_program_tracks_dynamic_policy_sites_and_lease_budget() {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();

        let summary = crate::global::compiled::LoweringSummary::scan_const(program.eff_list());
        let compiled = CompiledProgram::compile(program.eff_list());
        let sites: std::vec::Vec<_> = compiled.dynamic_policy_sites_for(ROUTE_POLICY_ID).collect();

        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].label(), LABEL_ROUTE_DECISION);
        assert_eq!(sites[0].resource_tag(), Some(RouteDecisionKind::TAG));

        let budget = summary.lease_budget();
        assert!(!budget.requires_caps());
        assert!(!budget.requires_slots());
        assert!(!budget.requires_splice());
        assert!(!budget.requires_delegation());

        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_label(LABEL_ROUTE_DECISION),
            ControlSemanticKind::RouteArm
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_resource_tag(Some(RouteDecisionKind::TAG)),
            ControlSemanticKind::RouteArm
        );
    }

    #[test]
    fn compiled_program_marks_loop_control_semantics_from_control_metadata() {
        let body = g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();
        let continue_arm = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >(),
            body,
        );
        let break_arm = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >();
        let program = g::route(continue_arm, break_arm);

        let compiled = CompiledProgram::compile(program.eff_list());

        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_label(LABEL_LOOP_CONTINUE),
            ControlSemanticKind::LoopContinue
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_label(LABEL_LOOP_BREAK),
            ControlSemanticKind::LoopBreak
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_resource_tag(Some(LoopContinueKind::TAG)),
            ControlSemanticKind::LoopContinue
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_resource_tag(Some(LoopBreakKind::TAG)),
            ControlSemanticKind::LoopBreak
        );
    }
}
