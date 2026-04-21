use super::super::images::program::{
    DynamicPolicySite, MAX_COMPILED_PROGRAM_ROUTE_CONTROLS, MAX_DYNAMIC_POLICY_SITES,
    RouteControlRecord, compiled_program_lookup_route_control,
};
use super::LoweringSummary;
use super::program_lowering::{
    compiled_program_emit_atom, compiled_program_emit_route_controls,
    compiled_program_push_dynamic_policy_site,
};
use crate::control::cluster::effects::{EffectEnvelope, EffectEnvelopeRef, ResourceDescriptor};
use crate::control::lease::planner::LeaseGraphBudget;
use crate::eff::{EffIndex, EffKind};
use crate::global::StaticControlDesc;
use crate::global::const_dsl::{PolicyMode, ScopeId};
use core::ptr;

#[derive(Clone)]
pub(crate) struct CompiledProgram {
    pub(crate) lease_budget: LeaseGraphBudget,
    effect_envelope: EffectEnvelope,
    control_scope_mask: u8,
    dynamic_policy_sites: [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
    route_controls: [RouteControlRecord; MAX_COMPILED_PROGRAM_ROUTE_CONTROLS],
}

impl CompiledProgram {
    #[inline(always)]
    unsafe fn init_array_copy<T: Copy, const N: usize>(dst: *mut [T; N], value: T) {
        let ptr = dst.cast::<T>();
        let mut idx = 0usize;
        while idx < N {
            unsafe {
                ptr.add(idx).write(value);
            }
            idx += 1;
        }
    }

    pub(crate) fn from_summary(summary: &LoweringSummary) -> Self {
        let mut compiled = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_from_summary(compiled.as_mut_ptr(), summary);
            compiled.assume_init()
        }
    }

    #[inline(never)]
    pub(crate) unsafe fn init_from_summary(dst: *mut Self, summary: &LoweringSummary) {
        unsafe {
            ptr::addr_of_mut!((*dst).lease_budget).write(LeaseGraphBudget::new());
            EffectEnvelope::init_empty(ptr::addr_of_mut!((*dst).effect_envelope));
            ptr::addr_of_mut!((*dst).control_scope_mask).write(0);
            Self::init_array_copy(
                ptr::addr_of_mut!((*dst).dynamic_policy_sites),
                DynamicPolicySite::EMPTY,
            );
            Self::init_array_copy(
                ptr::addr_of_mut!((*dst).route_controls),
                RouteControlRecord::EMPTY,
            );
        }

        let effect_envelope = unsafe { &mut *ptr::addr_of_mut!((*dst).effect_envelope) };
        let dynamic_policy_sites = unsafe { &mut *ptr::addr_of_mut!((*dst).dynamic_policy_sites) };
        let mut dynamic_policy_sites_len = 0usize;
        let route_controls = unsafe { &mut *ptr::addr_of_mut!((*dst).route_controls) };
        let mut route_controls_len = 0usize;

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
                lease_budget = lease_budget.include_atom(control_spec, policy);
                let resource_policy_site = if policy.is_dynamic() {
                    compiled_program_push_dynamic_policy_site(
                        dynamic_policy_sites,
                        &mut dynamic_policy_sites_len,
                        DynamicPolicySite::new(
                            EffIndex::from_usize(offset),
                            atom.label,
                            atom.resource,
                            control_spec.map(StaticControlDesc::op),
                            policy,
                        ),
                    )
                } else {
                    ResourceDescriptor::STATIC_POLICY_SITE
                };
                compiled_program_emit_atom(
                    effect_envelope,
                    atom,
                    offset,
                    policy,
                    resource_policy_site,
                    control_spec,
                );
            }
            offset += 1;
        }

        lease_budget.validate();
        unsafe {
            ptr::addr_of_mut!((*dst).lease_budget).write(lease_budget);
        }

        let control_markers = summary.control_markers();
        let mut control_idx = 0usize;
        while control_idx < control_markers.len() {
            let marker = control_markers[control_idx];
            if marker.tap_id != 0 {
                effect_envelope.push_tap_event(marker.tap_id);
            }
            control_idx += 1;
        }
        unsafe {
            ptr::addr_of_mut!((*dst).control_scope_mask)
                .write(summary.compiled_program_control_scope_mask());
        }
        compiled_program_emit_route_controls(route_controls, &mut route_controls_len, summary);
    }

    #[inline(always)]
    fn dynamic_policy_sites_len(&self) -> usize {
        let mut len = 0usize;
        while len < self.dynamic_policy_sites.len() {
            if self.dynamic_policy_sites[len] == DynamicPolicySite::EMPTY {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline(always)]
    fn route_controls_len(&self) -> usize {
        let mut len = 0usize;
        while len < self.route_controls.len() {
            if self.route_controls[len] == RouteControlRecord::EMPTY {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        self.effect_envelope.as_ref_with_controls(
            self.control_scope_mask,
            &self.dynamic_policy_sites[..self.dynamic_policy_sites_len()],
        )
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites(&self) -> &[DynamicPolicySite] {
        &self.dynamic_policy_sites[..self.dynamic_policy_sites_len()]
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

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        compiled_program_lookup_route_control(
            &self.route_controls[..self.route_controls_len()],
            scope_id,
        )
        .and_then(|record| record.controller_role())
    }

    #[inline(always)]
    pub(crate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(
        PolicyMode,
        EffIndex,
        u8,
        crate::control::cap::mint::ControlOp,
    )> {
        compiled_program_lookup_route_control(
            &self.route_controls[..self.route_controls_len()],
            scope_id,
        )
        .and_then(|record| record.route_controller())
    }
}
