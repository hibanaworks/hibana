use super::super::images::role::{
    CompiledRoleImage, MACHINE_NO_STEP, RoleResidentFacts, encode_compact_count_u16,
    encode_compact_offset_u16,
};
use super::super::materialize::RoleLoweringScratch;
use super::LoweringSummary;
use super::role_image_lowering::{
    build_local_steps_into, build_phase_image_from_steps, build_step_index_to_state_into,
};
use super::role_scope_storage::{CompiledRoleScopeStorage, compact_route_scope_tail};
use crate::global::role_program::RoleFootprint;
use crate::global::typestate::{RoleTypestateInitStorage, StateIndex};

#[inline(always)]
unsafe fn write_offset(field: *mut u16, base: usize, ptr: usize) {
    unsafe {
        field.write(encode_compact_offset_u16(ptr.saturating_sub(base)));
    }
}

#[inline(never)]
unsafe fn init_empty_compiled_role_image(dst: *mut CompiledRoleImage, role: u8) {
    unsafe {
        core::ptr::addr_of_mut!((*dst).typestate_offset).write(0);
        core::ptr::addr_of_mut!((*dst).eff_index_to_step_offset).write(0);
        core::ptr::addr_of_mut!((*dst).phase_headers_offset).write(0);
        core::ptr::addr_of_mut!((*dst).phase_lane_entries_offset).write(0);
        core::ptr::addr_of_mut!((*dst).phase_lane_words_offset).write(0);
        core::ptr::addr_of_mut!((*dst).step_index_to_state_offset).write(0);
        core::ptr::addr_of_mut!((*dst).role).write(role);
        core::ptr::addr_of_mut!((*dst).role_facts).write(RoleResidentFacts::EMPTY);
    }
}

#[inline(never)]
unsafe fn finalize_compiled_role_image_from_typestate(
    dst: *mut CompiledRoleImage,
    scratch: &mut RoleLoweringScratch<'_>,
) {
    let role = unsafe { (*dst).role };
    let image = unsafe { &*dst };
    let typed_typestate = unsafe { &*image.typestate_ptr() };
    let (by_eff_index, present, steps, eff_index_to_step) = scratch.local_step_build_slices_mut();
    let len = build_local_steps_into(
        role,
        typed_typestate,
        by_eff_index,
        present,
        steps,
        eff_index_to_step,
    );
    let (steps, eff_index_to_step, step_index_to_state) = scratch.step_state_build_slices_mut();
    build_step_index_to_state_into(
        typed_typestate,
        steps,
        len,
        eff_index_to_step,
        step_index_to_state,
    );
    let step_state_cap = unsafe { (*dst).role_facts.step_index_to_state_len() };
    if len > step_state_cap {
        panic!("compiled role local step count exceeds allocated step-state capacity");
    }
    unsafe {
        (*dst).role_facts.step_index_to_state_len = encode_compact_count_u16(len);
    }
    let (steps, step_index_to_state, route_guards, parallel_ranges) =
        scratch.phase_build_slices_mut();
    let phase_cap = unsafe { (*dst).role_facts.phase_len() };
    let phase_lane_entry_cap = unsafe { (*dst).role_facts.phase_lane_entry_len() };
    let phase_lane_word_cap = unsafe { (*dst).role_facts.phase_lane_word_len() };
    let (phase_len, phase_lane_entry_len, phase_lane_word_len) = unsafe {
        build_phase_image_from_steps(
            role,
            steps,
            len,
            typed_typestate,
            step_index_to_state,
            route_guards,
            parallel_ranges,
            image.phase_headers_ptr().cast_mut(),
            phase_cap,
            image.phase_lane_entries_ptr().cast_mut(),
            phase_lane_entry_cap,
            image.phase_lane_words_ptr().cast_mut(),
            phase_lane_word_cap,
        )
    };
    unsafe {
        (*dst).role_facts.phase_len = encode_compact_count_u16(phase_len);
        (*dst).role_facts.phase_lane_entry_len = encode_compact_count_u16(phase_lane_entry_len);
        (*dst).role_facts.phase_lane_word_len = encode_compact_count_u16(phase_lane_word_len);
    }
    let eff_index_len = unsafe { (*dst).role_facts.eff_index_to_step_len() };
    let eff_index_to_step_ptr = image.eff_index_to_step_ptr().cast_mut();
    let mut eff_idx = 0usize;
    while eff_idx < eff_index_len {
        unsafe {
            eff_index_to_step_ptr.add(eff_idx).write(MACHINE_NO_STEP);
        }
        eff_idx += 1;
    }
    let eff_index_to_step = scratch.eff_index_to_step();
    if eff_index_len > eff_index_to_step.len() {
        panic!("compiled role eff-index map exceeds lowering scratch capacity");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            eff_index_to_step.as_ptr(),
            eff_index_to_step_ptr,
            eff_index_len,
        );
    }
    let step_state_len = unsafe { (*dst).role_facts.step_index_to_state_len() };
    let step_index_to_state_ptr = image.step_index_to_state_ptr().cast_mut();
    let mut step_idx = 0usize;
    while step_idx < step_state_len {
        unsafe {
            step_index_to_state_ptr.add(step_idx).write(StateIndex::MAX);
        }
        step_idx += 1;
    }
    let step_index_to_state = scratch.step_index_to_state();
    if step_state_len > step_index_to_state.len() {
        panic!("compiled role step-state map exceeds lowering scratch capacity");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            step_index_to_state.as_ptr(),
            step_index_to_state_ptr,
            step_state_len,
        );
        let image_base = dst.cast::<u8>() as usize;
        let image_end = (step_index_to_state_ptr as usize)
            .saturating_add(step_state_len.saturating_mul(core::mem::size_of::<StateIndex>()))
            .saturating_sub(image_base);
        (*dst).role_facts.persistent_bytes = encode_compact_count_u16(image_end);
    }
}

#[inline(never)]
unsafe fn init_compiled_role_image_layout(
    dst: *mut CompiledRoleImage,
    role: u8,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
) {
    let init_empty = core::hint::black_box(
        init_empty_compiled_role_image as unsafe fn(*mut CompiledRoleImage, u8),
    );
    unsafe { init_empty(dst, role) };
    unsafe {
        let image_base = dst.cast::<u8>() as usize;
        write_offset(
            core::ptr::addr_of_mut!((*dst).typestate_offset),
            image_base,
            storage.typestate as usize,
        );
        write_offset(
            core::ptr::addr_of_mut!((*dst).phase_headers_offset),
            image_base,
            storage.phase_headers as usize,
        );
        write_offset(
            core::ptr::addr_of_mut!((*dst).phase_lane_entries_offset),
            image_base,
            storage.phase_lane_entries as usize,
        );
        write_offset(
            core::ptr::addr_of_mut!((*dst).phase_lane_words_offset),
            image_base,
            storage.phase_lane_words as usize,
        );
        (*dst).role_facts.active_lane_count = encode_compact_count_u16(footprint.active_lane_count);
        (*dst).role_facts.endpoint_lane_slot_count =
            encode_compact_count_u16(footprint.endpoint_lane_slot_count);
        (*dst).role_facts.phase_len = encode_compact_count_u16(storage.phase_header_cap);
        (*dst).role_facts.phase_lane_entry_len =
            encode_compact_count_u16(storage.phase_lane_entry_cap);
        (*dst).role_facts.phase_lane_word_len =
            encode_compact_count_u16(storage.phase_lane_word_cap);
        (*dst).role_facts.eff_index_to_step_len = encode_compact_count_u16(footprint.eff_count);
        (*dst).role_facts.step_index_to_state_len =
            encode_compact_count_u16(footprint.local_step_count);
    }
}

#[inline(never)]
unsafe fn init_compiled_role_image_typestate(
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
) {
    unsafe {
        let mut typestate_storage = RoleTypestateInitStorage {
            nodes_ptr: storage.typestate_nodes,
            nodes_cap: storage.typestate_node_cap,
            scope_records: core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
            scope_slots_by_scope: storage.slots_by_scope,
            route_dense_by_slot: storage.route_dense_by_slot,
            route_records: storage.route_records,
            route_offer_lane_words: storage.route_offer_lane_words,
            route_arm1_lane_words: storage.route_arm1_lane_words,
            route_lane_word_len: footprint.logical_lane_word_count,
            route_dispatch_shapes: storage.route_dispatch_shapes,
            route_dispatch_shape_cap: storage.route_dispatch_shape_cap,
            route_dispatch_entries: storage.route_dispatch_entries,
            route_dispatch_entry_cap: storage.route_dispatch_entry_cap,
            route_dispatch_targets: storage.route_dispatch_targets,
            route_dispatch_target_cap: storage.route_dispatch_target_cap,
            lane_slot_count: footprint.logical_lane_count,
            scope_lane_first_eff: storage.scope_lane_first_eff,
            scope_lane_last_eff: storage.scope_lane_last_eff,
            route_arm0_lane_last_eff_by_slot: storage.route_arm0_lane_last_eff_by_slot,
            route_scope_cap: storage.route_scope_cap,
        };
        crate::global::typestate::init_value_from_summary_for_role(
            storage.typestate,
            role,
            &mut typestate_storage,
            summary,
            scratch.typestate_build_mut(),
        );
    }
}

#[inline(never)]
unsafe fn finalize_compiled_role_image_offsets(
    dst: *mut CompiledRoleImage,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
) {
    let compact_route_end = unsafe {
        compact_route_scope_tail(
            storage,
            footprint.logical_lane_count,
            footprint.logical_lane_word_count,
        )
    };
    let eff_index_start =
        CompiledRoleScopeStorage::align_up(compact_route_end, core::mem::align_of::<u16>());
    let step_index_start = CompiledRoleScopeStorage::align_up(
        eff_index_start
            + footprint
                .eff_count
                .saturating_mul(core::mem::size_of::<u16>()),
        core::mem::align_of::<StateIndex>(),
    );
    unsafe {
        let image_base = dst.cast::<u8>() as usize;
        write_offset(
            core::ptr::addr_of_mut!((*dst).eff_index_to_step_offset),
            image_base,
            eff_index_start,
        );
        write_offset(
            core::ptr::addr_of_mut!((*dst).step_index_to_state_offset),
            image_base,
            step_index_start,
        );
    }
}

#[inline(never)]
pub(crate) unsafe fn init_compiled_role_image_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) {
    let storage = unsafe { CompiledRoleScopeStorage::from_image_ptr_with_layout(dst, footprint) };
    unsafe {
        init_compiled_role_image_layout(dst, role, footprint, &storage);
        init_compiled_role_image_typestate(role, summary, scratch, footprint, &storage);
        finalize_compiled_role_image_offsets(dst, footprint, &storage);
    }
    let finalize = core::hint::black_box(finalize_compiled_role_image_from_typestate);
    unsafe { finalize(dst, scratch) };
}
