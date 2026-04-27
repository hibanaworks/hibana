use super::super::images::program::{
    CompiledProgramFacts, CompiledProgramSection, DynamicPolicySite,
};
use super::LoweringSummary;
use super::program_lowering::{
    compiled_program_emit_atom_into_slices, compiled_program_emit_route_controls,
    compiled_program_push_dynamic_policy_site,
};
use super::program_tail_storage::CompiledProgramTailStorage;
use crate::control::cluster::effects::ResourceDescriptor;
use crate::eff::{EffIndex, EffKind};
use crate::global::const_dsl::PolicyMode;
use core::ptr;

pub(crate) unsafe fn init_compiled_program_image_from_summary(
    dst: *mut CompiledProgramFacts,
    summary: &LoweringSummary,
) {
    let counts = summary.compiled_program_counts();
    let storage = unsafe { CompiledProgramTailStorage::from_image_ptr(dst, counts) };
    let base = dst.cast::<u8>().cast_const();
    let resources_section = unsafe {
        CompiledProgramSection::from_ptr(
            base,
            storage.resources.cast_const(),
            storage.resources_len,
        )
    };
    let dynamic_policy_sites_section = unsafe {
        CompiledProgramSection::from_ptr(base, storage.sites.cast_const(), storage.sites_len)
    };
    let route_controls_section = unsafe {
        CompiledProgramSection::from_ptr(
            base,
            storage.route_controls.cast_const(),
            storage.route_controls_len,
        )
    };
    unsafe {
        ptr::addr_of_mut!((*dst).resources).write(resources_section);
        ptr::addr_of_mut!((*dst).dynamic_policy_sites).write(dynamic_policy_sites_section);
        ptr::addr_of_mut!((*dst).route_controls).write(route_controls_section);
        ptr::addr_of_mut!((*dst).role_count).write(summary.compiled_program_role_count() as u8);
        ptr::addr_of_mut!((*dst).control_scope_mask).write(0);
    }

    let resources =
        unsafe { core::slice::from_raw_parts_mut(storage.resources, storage.resources_len) };
    let mut resources_len = 0usize;
    let dynamic_policy_sites =
        unsafe { core::slice::from_raw_parts_mut(storage.sites, storage.sites_len) };
    let mut dynamic_policy_sites_len = 0usize;
    let route_controls = unsafe {
        core::slice::from_raw_parts_mut(storage.route_controls, storage.route_controls_len)
    };
    let mut route_controls_len = 0usize;

    let view = summary.view();
    let mut segment_idx = 0usize;
    while segment_idx < view.segment_count() {
        let segment = view.segment_at(segment_idx);
        let mut local = 0usize;
        while local < segment.len() {
            let offset = segment.start() + local;
            let node = segment.node_at_local(local);
            if matches!(node.kind, EffKind::Atom) {
                let policy = match segment.policy_at_local(local) {
                    Some(policy) => policy,
                    None => PolicyMode::Static,
                };
                let atom = node.atom_data();
                let control_desc = segment.control_desc_at_local(local);
                let resource_policy_site = if policy.is_dynamic() {
                    compiled_program_push_dynamic_policy_site(
                        dynamic_policy_sites,
                        &mut dynamic_policy_sites_len,
                        DynamicPolicySite::new(
                            EffIndex::from_dense_ordinal(offset),
                            atom.label,
                            atom.resource,
                            control_desc.map(crate::global::ControlDesc::op),
                            policy,
                        ),
                    )
                } else {
                    ResourceDescriptor::STATIC_POLICY_SITE
                };
                compiled_program_emit_atom_into_slices(
                    resources,
                    &mut resources_len,
                    atom,
                    offset,
                    policy,
                    resource_policy_site,
                    control_desc,
                );
            }
            local += 1;
        }
        segment_idx += 1;
    }

    unsafe {
        ptr::addr_of_mut!((*dst).resources).write(resources_section.with_len(resources_len));
        ptr::addr_of_mut!((*dst).dynamic_policy_sites)
            .write(dynamic_policy_sites_section.with_len(dynamic_policy_sites_len));
        compiled_program_emit_route_controls(route_controls, &mut route_controls_len, summary);
        ptr::addr_of_mut!((*dst).route_controls)
            .write(route_controls_section.with_len(route_controls_len));
        ptr::addr_of_mut!((*dst).control_scope_mask)
            .write(summary.compiled_program_control_scope_mask());
    }
}
