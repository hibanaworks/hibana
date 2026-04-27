use super::super::images::role::{
    CompiledRoleImage, CompiledRoleSegmentHeader, MACHINE_NO_STEP, RoleResidentFacts,
    encode_compact_count_u16, encode_compact_offset_u16,
};
use super::super::layout::compiled_role_image_bytes_for_layout;
use super::super::materialize::RoleLoweringScratch;
use super::LoweringSummary;
use super::role_image_lowering::{
    build_local_steps_into, build_phase_image_from_steps, build_step_index_to_state_into,
};
use super::role_scope_storage::{CompiledRoleScopeStorage, compact_route_scope_tail};
use crate::global::ControlDesc;
use crate::global::role_program::RoleFootprint;
use crate::global::typestate::{RoleTypestateRowDestinations, RoleTypestateWalkRows, StateIndex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompiledRoleImageInitError {
    SegmentHeaderCapacity,
    RoleCountsUnavailable,
    TypestateNodeCapacity,
    ScopeRowCapacity,
    RouteRowCapacity,
    PhaseHeaderCapacity,
    PhaseLaneEntryCapacity,
    PhaseLaneWordCapacity,
    EffIndexCapacity,
    StepIndexCapacity,
    LaneMatrixCapacity,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoleImageStreamFault {
    None,
    AfterTypestateNode(usize),
    AfterScopeRow(usize),
    AfterRouteRecord(usize),
    AfterRouteSlot(usize),
    AfterLaneMask(usize),
    AfterPhaseHeader(usize),
    AfterPhaseLaneEntry(usize),
    AfterPhaseLaneWord(usize),
    AfterEffIndexRow(usize),
    AfterStepIndexRow(usize),
    AfterControlByEffRow(usize),
}

#[cfg(test)]
impl RoleImageStreamFault {
    #[inline(always)]
    fn after_typestate_node(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterTypestateNode(target) if target == idx => {
                Err(CompiledRoleImageInitError::TypestateNodeCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_scope_row(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterScopeRow(target) if target == idx => {
                Err(CompiledRoleImageInitError::ScopeRowCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_route_record(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterRouteRecord(target) if target == idx => {
                Err(CompiledRoleImageInitError::RouteRowCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_route_slot(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterRouteSlot(target) if target == idx => {
                Err(CompiledRoleImageInitError::RouteRowCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_lane_mask(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterLaneMask(target) if target == idx => {
                Err(CompiledRoleImageInitError::LaneMatrixCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_phase_header(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterPhaseHeader(target) if target == idx => {
                Err(CompiledRoleImageInitError::PhaseHeaderCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_phase_lane_entry(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterPhaseLaneEntry(target) if target == idx => {
                Err(CompiledRoleImageInitError::PhaseLaneEntryCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_phase_lane_word(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterPhaseLaneWord(target) if target == idx => {
                Err(CompiledRoleImageInitError::PhaseLaneWordCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_eff_index(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterEffIndexRow(target) if target == idx => {
                Err(CompiledRoleImageInitError::EffIndexCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_step_index(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterStepIndexRow(target) if target == idx => {
                Err(CompiledRoleImageInitError::StepIndexCapacity)
            }
            _ => Ok(()),
        }
    }

    #[inline(always)]
    fn after_control_by_eff(self, idx: usize) -> Result<(), CompiledRoleImageInitError> {
        match self {
            Self::AfterControlByEffRow(target) if target == idx => {
                Err(CompiledRoleImageInitError::EffIndexCapacity)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(not(test))]
#[derive(Clone, Copy)]
enum RoleImageStreamFault {
    None,
}

#[cfg(not(test))]
impl RoleImageStreamFault {
    #[inline(always)]
    fn after_typestate_node(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_scope_row(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_route_record(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_route_slot(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_lane_mask(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_phase_header(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_phase_lane_entry(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_phase_lane_word(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_eff_index(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_step_index(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
    #[inline(always)]
    fn after_control_by_eff(self, _idx: usize) -> Result<(), CompiledRoleImageInitError> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct CompiledRoleIndexRowLayout {
    eff_index_to_step: *mut u16,
    step_index_to_state: *mut StateIndex,
    control_by_eff: *mut ControlDesc,
    persistent_end: usize,
}

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
        core::ptr::addr_of_mut!((*dst).segment_headers_offset).write(0);
        core::ptr::addr_of_mut!((*dst).eff_index_to_step_offset).write(0);
        core::ptr::addr_of_mut!((*dst).phase_headers_offset).write(0);
        core::ptr::addr_of_mut!((*dst).phase_lane_entries_offset).write(0);
        core::ptr::addr_of_mut!((*dst).phase_lane_words_offset).write(0);
        core::ptr::addr_of_mut!((*dst).step_index_to_state_offset).write(0);
        core::ptr::addr_of_mut!((*dst).control_by_eff_offset).write(0);
        core::ptr::addr_of_mut!((*dst).role).write(role);
        core::ptr::addr_of_mut!((*dst).role_facts).write(RoleResidentFacts::EMPTY);
    }
}

#[inline(never)]
unsafe fn rollback_compiled_role_descriptor_stream(
    dst: *mut CompiledRoleImage,
    role: u8,
    footprint: RoleFootprint,
) {
    unsafe {
        let image_bytes = compiled_role_image_bytes_for_layout(footprint);
        if image_bytes > core::mem::size_of::<CompiledRoleImage>() {
            core::ptr::write_bytes(dst.cast::<u8>(), 0, image_bytes);
        }
        init_empty_compiled_role_image(dst, role);
    }
}

#[inline(never)]
unsafe fn stream_local_step_rows_from_typestate(
    dst: *mut CompiledRoleImage,
    scratch: &mut RoleLoweringScratch<'_>,
) -> Result<usize, CompiledRoleImageInitError> {
    let role = unsafe { (*dst).role };
    let image = unsafe { &*dst };
    let typed_typestate = unsafe { &*image.typestate_ptr() };
    let (steps, eff_index_to_step) = scratch.local_step_build_slices_mut();
    let len = build_local_steps_into(role, typed_typestate, steps, eff_index_to_step);
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
        return Err(CompiledRoleImageInitError::StepIndexCapacity);
    }
    unsafe {
        (*dst).role_facts.step_index_to_state_len = encode_compact_count_u16(len);
    }
    Ok(len)
}

#[inline(never)]
unsafe fn stream_phase_descriptor_rows_from_steps(
    dst: *mut CompiledRoleImage,
    scratch: &mut RoleLoweringScratch<'_>,
    local_step_len: usize,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let role = unsafe { (*dst).role };
    let image = unsafe { &*dst };
    let typed_typestate = unsafe { &*image.typestate_ptr() };
    let (steps, step_index_to_state, route_guards, parallel_ranges) =
        scratch.phase_build_slices_mut();
    let phase_cap = unsafe { (*dst).role_facts.phase_len() };
    let phase_lane_entry_cap = unsafe { (*dst).role_facts.phase_lane_entry_len() };
    let phase_lane_word_cap = unsafe { (*dst).role_facts.phase_lane_word_len() };
    let (phase_len, phase_lane_entry_len, phase_lane_word_len) = unsafe {
        build_phase_image_from_steps(
            role,
            steps,
            local_step_len,
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
    if phase_len > phase_cap {
        return Err(CompiledRoleImageInitError::PhaseHeaderCapacity);
    }
    if phase_lane_entry_len > phase_lane_entry_cap {
        return Err(CompiledRoleImageInitError::PhaseLaneEntryCapacity);
    }
    if phase_lane_word_len > phase_lane_word_cap {
        return Err(CompiledRoleImageInitError::PhaseLaneWordCapacity);
    }
    unsafe {
        (*dst).role_facts.phase_len = encode_compact_count_u16(phase_len);
        (*dst).role_facts.phase_lane_entry_len = encode_compact_count_u16(phase_lane_entry_len);
        (*dst).role_facts.phase_lane_word_len = encode_compact_count_u16(phase_lane_word_len);
    }
    let mut idx = 0usize;
    while idx < phase_len {
        fault.after_phase_header(idx)?;
        idx += 1;
    }
    let mut idx = 0usize;
    while idx < phase_lane_entry_len {
        fault.after_phase_lane_entry(idx)?;
        idx += 1;
    }
    let mut idx = 0usize;
    while idx < phase_lane_word_len {
        fault.after_phase_lane_word(idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_eff_index_to_step_rows(
    dst: *mut CompiledRoleImage,
    scratch: &RoleLoweringScratch<'_>,
    index_rows: CompiledRoleIndexRowLayout,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let eff_index_len = unsafe { (*dst).role_facts.eff_index_to_step_len() };
    let eff_index_to_step_ptr = index_rows.eff_index_to_step;
    let mut eff_idx = 0usize;
    while eff_idx < eff_index_len {
        unsafe {
            eff_index_to_step_ptr.add(eff_idx).write(MACHINE_NO_STEP);
        }
        eff_idx += 1;
    }
    let eff_index_to_step = scratch.eff_index_to_step();
    if eff_index_len > eff_index_to_step.len() {
        return Err(CompiledRoleImageInitError::EffIndexCapacity);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            eff_index_to_step.as_ptr(),
            eff_index_to_step_ptr,
            eff_index_len,
        );
    }
    let mut idx = 0usize;
    while idx < eff_index_len {
        fault.after_eff_index(idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_step_index_to_state_rows(
    dst: *mut CompiledRoleImage,
    scratch: &RoleLoweringScratch<'_>,
    index_rows: CompiledRoleIndexRowLayout,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let step_state_len = unsafe { (*dst).role_facts.step_index_to_state_len() };
    let step_index_to_state_ptr = index_rows.step_index_to_state;
    let mut step_idx = 0usize;
    while step_idx < step_state_len {
        unsafe {
            step_index_to_state_ptr.add(step_idx).write(StateIndex::MAX);
        }
        step_idx += 1;
    }
    let step_index_to_state = scratch.step_index_to_state();
    if step_state_len > step_index_to_state.len() {
        return Err(CompiledRoleImageInitError::StepIndexCapacity);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            step_index_to_state.as_ptr(),
            step_index_to_state_ptr,
            step_state_len,
        );
    }
    let mut idx = 0usize;
    while idx < step_state_len {
        fault.after_step_index(idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_control_by_eff_rows(
    dst: *mut CompiledRoleImage,
    summary: &LoweringSummary,
    index_rows: CompiledRoleIndexRowLayout,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let eff_index_len = unsafe { (*dst).role_facts.eff_index_to_step_len() };
    let control_by_eff = index_rows.control_by_eff;
    let mut eff_idx = 0usize;
    while eff_idx < eff_index_len {
        let desc = summary
            .view()
            .control_desc_at(eff_idx)
            .unwrap_or(ControlDesc::EMPTY);
        unsafe {
            control_by_eff.add(eff_idx).write(desc);
        }
        fault.after_control_by_eff(eff_idx)?;
        eff_idx += 1;
    }
    Ok(())
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
        if storage.segment_header_cap == 0 {
            core::ptr::addr_of_mut!((*dst).segment_headers_offset).write(0);
        } else {
            write_offset(
                core::ptr::addr_of_mut!((*dst).segment_headers_offset),
                image_base,
                storage.segment_headers as usize,
            );
        }
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
fn validate_compiled_role_descriptor_row_capacity(
    role: u8,
    summary: &LoweringSummary,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
) -> Result<(), CompiledRoleImageInitError> {
    let counts = summary
        .role_lowering_counts_for_role(role)
        .ok_or(CompiledRoleImageInitError::RoleCountsUnavailable)?;
    let required_segments = if counts.eff_count == 0 {
        0
    } else {
        counts
            .eff_count
            .div_ceil(crate::eff::meta::MAX_SEGMENT_EFFS)
    };
    if required_segments > storage.segment_header_cap {
        return Err(CompiledRoleImageInitError::SegmentHeaderCapacity);
    }
    let required_typestate_nodes = CompiledRoleScopeStorage::typestate_node_cap(
        counts.scope_count,
        counts.passive_linger_route_scope_count,
        counts.local_step_count,
    );
    if required_typestate_nodes > storage.typestate_node_cap {
        return Err(CompiledRoleImageInitError::TypestateNodeCapacity);
    }
    if counts.scope_count > storage.scope_cap {
        return Err(CompiledRoleImageInitError::ScopeRowCapacity);
    }
    if counts.route_scope_count > storage.route_scope_cap {
        return Err(CompiledRoleImageInitError::RouteRowCapacity);
    }
    if counts.phase_count > storage.phase_header_cap {
        return Err(CompiledRoleImageInitError::PhaseHeaderCapacity);
    }
    if counts.phase_lane_entry_count > storage.phase_lane_entry_cap {
        return Err(CompiledRoleImageInitError::PhaseLaneEntryCapacity);
    }
    if counts.phase_lane_word_count > storage.phase_lane_word_cap {
        return Err(CompiledRoleImageInitError::PhaseLaneWordCapacity);
    }
    if counts.eff_count > footprint.eff_count {
        return Err(CompiledRoleImageInitError::EffIndexCapacity);
    }
    if counts.local_step_count > footprint.local_step_count {
        return Err(CompiledRoleImageInitError::StepIndexCapacity);
    }
    let required_scope_lane_entries = counts.scope_count.saturating_mul(counts.logical_lane_count);
    let allocated_scope_lane_entries = storage
        .scope_cap
        .saturating_mul(footprint.logical_lane_count);
    if required_scope_lane_entries > allocated_scope_lane_entries {
        return Err(CompiledRoleImageInitError::LaneMatrixCapacity);
    }
    let required_route_lane_words =
        counts
            .route_scope_count
            .saturating_mul(crate::global::role_program::lane_word_count(
                counts.logical_lane_count,
            ));
    let allocated_route_lane_words = storage
        .route_scope_cap
        .saturating_mul(footprint.logical_lane_word_count);
    if required_route_lane_words > allocated_route_lane_words {
        return Err(CompiledRoleImageInitError::RouteRowCapacity);
    }
    Ok(())
}

#[inline(never)]
unsafe fn init_compiled_role_segment_header_at(
    segment_idx: usize,
    summary: &LoweringSummary,
    storage: &CompiledRoleScopeStorage,
) -> Result<(), CompiledRoleImageInitError> {
    let headers = unsafe {
        core::slice::from_raw_parts_mut(storage.segment_headers, storage.segment_header_cap)
    };
    let view = summary.view();
    if segment_idx >= view.segment_count() || segment_idx >= headers.len() {
        return Err(CompiledRoleImageInitError::SegmentHeaderCapacity);
    }
    let segment = view.segment_at(segment_idx);
    headers[segment_idx] = CompiledRoleSegmentHeader {
        eff_start: encode_compact_count_u16(segment.start()),
        eff_len: encode_compact_count_u16(segment.len()),
        scope_marker_len: encode_compact_count_u16(segment.scope_markers().len()),
        control_marker_len: encode_compact_count_u16(segment.control_markers().len()),
        policy_marker_len: encode_compact_count_u16(segment.summary().policy_marker_len()),
        control_desc_len: encode_compact_count_u16(segment.summary().control_spec_len()),
    };
    Ok(())
}

#[inline(never)]
unsafe fn stream_compiled_role_segment_headers(
    summary: &LoweringSummary,
    storage: &CompiledRoleScopeStorage,
) -> Result<(), CompiledRoleImageInitError> {
    let headers = unsafe {
        core::slice::from_raw_parts_mut(storage.segment_headers, storage.segment_header_cap)
    };
    let view = summary.view();
    if view.segment_count() > headers.len() {
        return Err(CompiledRoleImageInitError::SegmentHeaderCapacity);
    }

    let mut idx = 0usize;
    while idx < headers.len() {
        headers[idx] = CompiledRoleSegmentHeader::EMPTY;
        idx += 1;
    }

    let mut segment_idx = 0usize;
    while segment_idx < view.segment_count() {
        unsafe {
            init_compiled_role_segment_header_at(segment_idx, summary, storage)?;
        }
        segment_idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_compiled_role_descriptor_rows(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    unsafe {
        stream_compiled_role_segment_headers(summary, storage)?;
        stream_typestate_header(storage);
        let walk_rows = stream_typestate_nodes(role, summary, scratch, footprint, storage, fault)?;
        stream_scope_rows(role, summary, scratch, footprint, storage, walk_rows, fault)?;
        stream_route_slot_by_scope_ordinal(
            role, summary, scratch, footprint, storage, walk_rows, fault,
        )?;
        stream_route_records(role, summary, scratch, footprint, storage, walk_rows, fault)?;
        stream_lane_mask_by_scope(role, summary, scratch, footprint, storage, walk_rows, fault)?;
        let local_step_len = stream_local_step_rows_from_typestate(dst, scratch)?;
        stream_phase_descriptor_rows_from_steps(dst, scratch, local_step_len, fault)?;
        let index_rows = compact_compiled_role_route_tail_and_index_rows(dst, footprint, storage);
        stream_eff_index_to_step_rows(dst, scratch, index_rows, fault)?;
        stream_step_index_to_state_rows(dst, scratch, index_rows, fault)?;
        stream_control_by_eff_rows(dst, summary, index_rows, fault)?;
        publish_compiled_role_image_offsets(dst, index_rows);
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_typestate_header(storage: &CompiledRoleScopeStorage) {
    unsafe {
        crate::global::typestate::stream_value_header(storage.typestate, storage.typestate_nodes);
    }
}

#[inline(never)]
unsafe fn stream_typestate_nodes(
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
    fault: RoleImageStreamFault,
) -> Result<RoleTypestateWalkRows, CompiledRoleImageInitError> {
    unsafe {
        let mut typestate_rows = RoleTypestateRowDestinations {
            nodes_ptr: storage.typestate_nodes,
            nodes_cap: storage.typestate_node_cap,
            scope_records: core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
            scope_slots_by_scope: storage.slots_by_scope,
            route_dense_by_slot: storage.route_dense_by_slot,
            route_records: storage.route_records,
            route_offer_lane_words: storage.route_offer_lane_words,
            route_arm0_lane_words: storage.route_arm0_lane_words,
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
        let walk_rows = crate::global::typestate::stream_value_node_rows_from_summary_for_role(
            storage.typestate,
            role,
            &mut typestate_rows,
            summary,
            scratch.typestate_build_mut(),
        );
        let typestate = &*storage.typestate;
        if typestate.len() > storage.typestate_node_cap {
            return Err(CompiledRoleImageInitError::TypestateNodeCapacity);
        }
        let mut idx = 0usize;
        while idx < typestate.len() {
            let _ = typestate.node(idx);
            fault.after_typestate_node(idx)?;
            idx += 1;
        }
        Ok(walk_rows)
    }
}

#[inline(never)]
unsafe fn stream_scope_rows(
    role: u8,
    summary: &LoweringSummary,
    _scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
    walk_rows: RoleTypestateWalkRows,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let counts = summary
        .role_lowering_counts_for_role(role)
        .ok_or(CompiledRoleImageInitError::RoleCountsUnavailable)?;
    if counts.scope_count > storage.scope_cap {
        return Err(CompiledRoleImageInitError::ScopeRowCapacity);
    }
    unsafe {
        let mut typestate_rows = RoleTypestateRowDestinations {
            nodes_ptr: storage.typestate_nodes,
            nodes_cap: storage.typestate_node_cap,
            scope_records: core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
            scope_slots_by_scope: storage.slots_by_scope,
            route_dense_by_slot: storage.route_dense_by_slot,
            route_records: storage.route_records,
            route_offer_lane_words: storage.route_offer_lane_words,
            route_arm0_lane_words: storage.route_arm0_lane_words,
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
        crate::global::typestate::stream_value_scope_rows_from_walk(
            storage.typestate,
            &mut typestate_rows,
            walk_rows,
        );
    }
    let records = unsafe { core::slice::from_raw_parts(storage.records, counts.scope_count) };
    let mut idx = 0usize;
    while idx < records.len() {
        let _ = records[idx];
        fault.after_scope_row(idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_route_records(
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
    walk_rows: RoleTypestateWalkRows,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let counts = summary
        .role_lowering_counts_for_role(role)
        .ok_or(CompiledRoleImageInitError::RoleCountsUnavailable)?;
    if counts.route_scope_count > storage.route_scope_cap {
        return Err(CompiledRoleImageInitError::RouteRowCapacity);
    }
    unsafe {
        let mut typestate_rows = RoleTypestateRowDestinations {
            nodes_ptr: storage.typestate_nodes,
            nodes_cap: storage.typestate_node_cap,
            scope_records: core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
            scope_slots_by_scope: storage.slots_by_scope,
            route_dense_by_slot: storage.route_dense_by_slot,
            route_records: storage.route_records,
            route_offer_lane_words: storage.route_offer_lane_words,
            route_arm0_lane_words: storage.route_arm0_lane_words,
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
        crate::global::typestate::stream_value_route_record_rows_from_walk(
            storage.typestate,
            &mut typestate_rows,
            scratch.typestate_build_mut(),
            walk_rows,
        );
    }
    let records =
        unsafe { core::slice::from_raw_parts(storage.route_records, counts.route_scope_count) };
    let mut idx = 0usize;
    while idx < records.len() {
        let _ = records[idx];
        fault.after_route_record(idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_route_slot_by_scope_ordinal(
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
    walk_rows: RoleTypestateWalkRows,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let counts = summary
        .role_lowering_counts_for_role(role)
        .ok_or(CompiledRoleImageInitError::RoleCountsUnavailable)?;
    if counts.scope_count > storage.scope_cap {
        return Err(CompiledRoleImageInitError::ScopeRowCapacity);
    }
    unsafe {
        let mut typestate_rows = RoleTypestateRowDestinations {
            nodes_ptr: storage.typestate_nodes,
            nodes_cap: storage.typestate_node_cap,
            scope_records: core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
            scope_slots_by_scope: storage.slots_by_scope,
            route_dense_by_slot: storage.route_dense_by_slot,
            route_records: storage.route_records,
            route_offer_lane_words: storage.route_offer_lane_words,
            route_arm0_lane_words: storage.route_arm0_lane_words,
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
        let _ = scratch;
        crate::global::typestate::stream_value_route_slot_rows_from_walk(
            storage.typestate,
            &mut typestate_rows,
            walk_rows,
        );
    }
    let route_dense =
        unsafe { core::slice::from_raw_parts(storage.route_dense_by_slot, counts.scope_count) };
    let mut idx = 0usize;
    while idx < route_dense.len() {
        let _ = route_dense[idx];
        fault.after_route_slot(idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn stream_lane_mask_by_scope(
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
    walk_rows: RoleTypestateWalkRows,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let counts = summary
        .role_lowering_counts_for_role(role)
        .ok_or(CompiledRoleImageInitError::RoleCountsUnavailable)?;
    let lane_matrix_len = counts
        .scope_count
        .saturating_mul(footprint.logical_lane_count);
    let route_lane_word_len = counts
        .route_scope_count
        .saturating_mul(footprint.logical_lane_word_count);
    unsafe {
        let mut typestate_rows = RoleTypestateRowDestinations {
            nodes_ptr: storage.typestate_nodes,
            nodes_cap: storage.typestate_node_cap,
            scope_records: core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
            scope_slots_by_scope: storage.slots_by_scope,
            route_dense_by_slot: storage.route_dense_by_slot,
            route_records: storage.route_records,
            route_offer_lane_words: storage.route_offer_lane_words,
            route_arm0_lane_words: storage.route_arm0_lane_words,
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
        crate::global::typestate::stream_value_lane_mask_rows_from_walk(
            storage.typestate,
            &mut typestate_rows,
            scratch.typestate_build_mut(),
            walk_rows,
        );
    }
    let scope_first =
        unsafe { core::slice::from_raw_parts(storage.scope_lane_first_eff, lane_matrix_len) };
    let scope_last =
        unsafe { core::slice::from_raw_parts(storage.scope_lane_last_eff, lane_matrix_len) };
    let offer_words =
        unsafe { core::slice::from_raw_parts(storage.route_offer_lane_words, route_lane_word_len) };
    let mut idx = 0usize;
    while idx < scope_first.len() {
        let _ = (scope_first[idx], scope_last[idx]);
        fault.after_lane_mask(idx)?;
        idx += 1;
    }
    let mut idx = 0usize;
    while idx < offer_words.len() {
        let _ = offer_words[idx];
        fault.after_lane_mask(scope_first.len() + idx)?;
        idx += 1;
    }
    Ok(())
}

#[inline(never)]
unsafe fn compact_compiled_role_route_tail_and_index_rows(
    dst: *mut CompiledRoleImage,
    footprint: RoleFootprint,
    storage: &CompiledRoleScopeStorage,
) -> CompiledRoleIndexRowLayout {
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
    let step_index_end = step_index_start.saturating_add(
        footprint
            .local_step_count
            .saturating_mul(core::mem::size_of::<StateIndex>()),
    );
    let control_by_eff_start =
        CompiledRoleScopeStorage::align_up(step_index_end, core::mem::align_of::<ControlDesc>());
    let persistent_end = control_by_eff_start
        .saturating_add(
            footprint
                .eff_count
                .saturating_mul(core::mem::size_of::<ControlDesc>()),
        )
        .saturating_sub(dst.cast::<u8>() as usize);
    CompiledRoleIndexRowLayout {
        eff_index_to_step: eff_index_start as *mut u16,
        step_index_to_state: step_index_start as *mut StateIndex,
        control_by_eff: control_by_eff_start as *mut ControlDesc,
        persistent_end,
    }
}

#[inline(never)]
unsafe fn publish_compiled_role_image_offsets(
    dst: *mut CompiledRoleImage,
    index_rows: CompiledRoleIndexRowLayout,
) {
    unsafe {
        let image_base = dst.cast::<u8>() as usize;
        write_offset(
            core::ptr::addr_of_mut!((*dst).eff_index_to_step_offset),
            image_base,
            index_rows.eff_index_to_step as usize,
        );
        write_offset(
            core::ptr::addr_of_mut!((*dst).step_index_to_state_offset),
            image_base,
            index_rows.step_index_to_state as usize,
        );
        write_offset(
            core::ptr::addr_of_mut!((*dst).control_by_eff_offset),
            image_base,
            index_rows.control_by_eff as usize,
        );
        (*dst).role_facts.persistent_bytes = encode_compact_count_u16(index_rows.persistent_end);
    }
}

#[inline(never)]
#[cfg(test)]
pub(crate) unsafe fn init_compiled_role_image_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) {
    unsafe {
        try_init_compiled_role_image_from_summary(dst, role, summary, scratch, footprint)
            .expect("compiled role descriptor streaming initialization failed");
    }
}

#[inline(never)]
pub(crate) unsafe fn try_init_compiled_role_image_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) -> Result<(), CompiledRoleImageInitError> {
    let storage = unsafe { CompiledRoleScopeStorage::from_image_ptr_with_layout(dst, footprint) };
    unsafe {
        init_compiled_role_image_layout(dst, role, footprint, &storage);
        if let Err(err) =
            validate_compiled_role_descriptor_row_capacity(role, summary, footprint, &storage)
        {
            rollback_compiled_role_descriptor_stream(dst, role, footprint);
            return Err(err);
        }
        if let Err(err) = stream_compiled_role_descriptor_rows(
            dst,
            role,
            summary,
            scratch,
            footprint,
            &storage,
            RoleImageStreamFault::None,
        ) {
            rollback_compiled_role_descriptor_stream(dst, role, footprint);
            return Err(err);
        }
    }
    Ok(())
}

#[inline(never)]
pub(crate) unsafe fn validate_compiled_role_image_init_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    footprint: RoleFootprint,
) -> Result<(), CompiledRoleImageInitError> {
    let storage = unsafe { CompiledRoleScopeStorage::from_image_ptr_with_layout(dst, footprint) };
    validate_compiled_role_descriptor_row_capacity(role, summary, footprint, &storage)
}

#[inline(never)]
pub(crate) unsafe fn init_compiled_role_image_from_prevalidated_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) -> usize {
    let storage = unsafe { CompiledRoleScopeStorage::from_image_ptr_with_layout(dst, footprint) };
    unsafe {
        init_compiled_role_image_layout(dst, role, footprint, &storage);
        stream_compiled_role_descriptor_rows(
            dst,
            role,
            summary,
            scratch,
            footprint,
            &storage,
            RoleImageStreamFault::None,
        )
        .expect("prevalidated compiled role descriptor streaming must be infallible");
        (*dst).actual_persistent_bytes()
    }
}

#[cfg(test)]
#[inline(never)]
pub(crate) unsafe fn try_init_compiled_role_image_from_summary_with_fault(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
    fault: RoleImageStreamFault,
) -> Result<(), CompiledRoleImageInitError> {
    let storage = unsafe { CompiledRoleScopeStorage::from_image_ptr_with_layout(dst, footprint) };
    unsafe {
        init_compiled_role_image_layout(dst, role, footprint, &storage);
        if let Err(err) =
            validate_compiled_role_descriptor_row_capacity(role, summary, footprint, &storage)
        {
            rollback_compiled_role_descriptor_stream(dst, role, footprint);
            return Err(err);
        }
        if let Err(err) = stream_compiled_role_descriptor_rows(
            dst, role, summary, scratch, footprint, &storage, fault,
        ) {
            rollback_compiled_role_descriptor_stream(dst, role, footprint);
            return Err(err);
        }
    }
    Ok(())
}
