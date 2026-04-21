use super::LoweringSummary;
use crate::control::cap::mint::ControlOp;
use crate::control::cluster::effects::ResourceDescriptor;
use crate::eff::{EffAtom, EffIndex};
use crate::global::StaticControlDesc;
use crate::global::const_dsl::{
    ControlScopeKind, PolicyMode, ScopeEvent, ScopeId, ScopeKind, ScopeMarker,
};

use super::super::images::program::{DynamicPolicySite, RouteControlRecord};

#[cfg(test)]
use crate::control::cluster::effects::EffectEnvelope;

#[inline(always)]
pub(super) const fn compiled_program_push_dynamic_policy_site(
    dynamic_policy_sites: &mut [DynamicPolicySite],
    dynamic_policy_sites_len: &mut usize,
    site: DynamicPolicySite,
) -> u16 {
    if *dynamic_policy_sites_len >= dynamic_policy_sites.len() {
        panic!("CompiledProgram: MAX_DYNAMIC_POLICY_SITES exceeded");
    }
    let site_index = *dynamic_policy_sites_len;
    dynamic_policy_sites[site_index] = site;
    *dynamic_policy_sites_len += 1;
    site_index as u16
}

#[inline(always)]
fn compiled_program_push_resource(
    resources: &mut [ResourceDescriptor],
    len: &mut usize,
    descriptor: ResourceDescriptor,
) {
    if *len >= resources.len() {
        panic!("CompiledProgram: MAX_RESOURCES exceeded");
    }
    resources[*len] = descriptor;
    *len += 1;
}

#[inline(always)]
fn compiled_program_route_scope_end(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
    scope: ScopeId,
    default_end: usize,
) -> usize {
    let mut scope_end = default_end;
    let mut scan_idx = enter_idx + 1;
    let mut nest_depth = 1usize;
    while scan_idx < scope_markers.len() {
        let scan_marker = scope_markers[scan_idx];
        if scan_marker.scope_id.local_ordinal() == scope.local_ordinal() {
            match scan_marker.event {
                ScopeEvent::Enter => nest_depth += 1,
                ScopeEvent::Exit => {
                    nest_depth -= 1;
                    if nest_depth == 0 {
                        scope_end = scan_marker.offset;
                        break;
                    }
                }
            }
        }
        scan_idx += 1;
    }
    scope_end
}

#[inline(always)]
fn compiled_program_insert_route_control(
    route_controls: &mut [RouteControlRecord],
    route_controls_len: &mut usize,
    record: RouteControlRecord,
) {
    let target_raw = record.canonical_raw();
    let mut insert_idx = 0usize;
    while insert_idx < *route_controls_len {
        let existing_raw = route_controls[insert_idx].canonical_raw();
        if existing_raw == target_raw {
            return;
        }
        if existing_raw > target_raw {
            break;
        }
        insert_idx += 1;
    }
    if *route_controls_len >= route_controls.len() {
        panic!("CompiledProgram: MAX_ROUTE_CONTROLS exceeded");
    }
    let mut shift_idx = *route_controls_len;
    while shift_idx > insert_idx {
        route_controls[shift_idx] = route_controls[shift_idx - 1];
        shift_idx -= 1;
    }
    route_controls[insert_idx] = record;
    *route_controls_len += 1;
}

#[inline(always)]
pub(super) fn compiled_program_emit_route_controls(
    route_controls: &mut [RouteControlRecord],
    route_controls_len: &mut usize,
    summary: &LoweringSummary,
) {
    let view = summary.view();
    let scope_markers = view.scope_markers();
    let default_end = view.as_slice().len();
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_kind, ScopeKind::Route)
        {
            let scope_end = compiled_program_route_scope_end(
                scope_markers,
                marker_idx,
                marker.scope_id,
                default_end,
            );
            let (route_policy_id, route_policy_eff, route_policy_tag, route_policy_op) = match view
                .first_route_head_dynamic_policy_in_range(marker.scope_id, marker_idx, scope_end)
            {
                Some((policy, eff_offset, tag, op)) => (
                    match policy.dynamic_policy_id() {
                        Some(policy_id) => policy_id,
                        None => u16::MAX,
                    },
                    EffIndex::from_usize(eff_offset),
                    tag,
                    op,
                ),
                None => (u16::MAX, EffIndex::MAX, 0, ControlOp::Fence),
            };
            compiled_program_insert_route_control(
                route_controls,
                route_controls_len,
                RouteControlRecord::new(
                    marker.scope_id,
                    marker.controller_role,
                    route_policy_id,
                    route_policy_eff,
                    route_policy_tag,
                    route_policy_op,
                ),
            );
        }
        marker_idx += 1;
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(super) fn compiled_program_emit_atom_into_slices(
    resources: &mut [ResourceDescriptor],
    resources_len: &mut usize,
    atom: EffAtom,
    offset: usize,
    policy: PolicyMode,
    resource_policy_site: u16,
    control_spec: Option<StaticControlDesc>,
) {
    if atom.is_control {
        let resource_kind_tag = atom
            .resource
            .expect("control atom must carry a resource tag");
        let control_spec = control_spec.expect("control atom missing control descriptor");
        if policy.is_dynamic() && !control_spec.supports_dynamic_policy() {
            panic!("dynamic policy attached to unsupported control op");
        }
        if control_spec.label() != atom.label || control_spec.resource_tag() != resource_kind_tag {
            panic!("control atom/control descriptor mismatch");
        }
        let descriptor = ResourceDescriptor::new(
            EffIndex::from_usize(offset),
            resource_policy_site,
            control_spec,
        );
        compiled_program_push_resource(resources, resources_len, descriptor);
    } else if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
        panic!("static policy attached to non-control atom");
    }
}

#[inline(always)]
pub(in crate::global::compiled) const fn control_scope_mask_bit(
    scope_kind: ControlScopeKind,
) -> u8 {
    match scope_kind {
        ControlScopeKind::None => 0,
        ControlScopeKind::Loop => 1 << 0,
        ControlScopeKind::Checkpoint => 1 << 1,
        ControlScopeKind::Cancel => 0,
        ControlScopeKind::Splice => 1 << 3,
        ControlScopeKind::Reroute => 0,
        ControlScopeKind::Policy => 0,
        ControlScopeKind::Route => 0,
    }
}

#[cfg(test)]
#[inline(always)]
pub(super) fn compiled_program_emit_atom(
    effect_envelope: &mut EffectEnvelope,
    atom: EffAtom,
    offset: usize,
    policy: PolicyMode,
    resource_policy_site: u16,
    control_spec: Option<StaticControlDesc>,
) {
    if atom.is_control {
        let resource_kind_tag = atom
            .resource
            .expect("control atom must carry a resource tag");
        let control_spec = control_spec.expect("control atom missing control descriptor");
        if policy.is_dynamic() && !control_spec.supports_dynamic_policy() {
            panic!("dynamic policy attached to unsupported control op");
        }
        if control_spec.label() != atom.label || control_spec.resource_tag() != resource_kind_tag {
            panic!("control atom/control descriptor mismatch");
        }
        let descriptor = ResourceDescriptor::new(
            EffIndex::from_usize(offset),
            resource_policy_site,
            control_spec,
        );
        let tap_id = if control_spec.tap_id() != 0 {
            control_spec.tap_id()
        } else {
            0x0300 + atom.label as u16
        };
        effect_envelope.push_tap_event(tap_id);
        effect_envelope.push_resource(descriptor);
    } else {
        if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
            panic!("static policy attached to non-control atom");
        }
        let tap_id = 0x0200 + atom.label as u16;
        effect_envelope.push_tap_event(tap_id);
    }
}
