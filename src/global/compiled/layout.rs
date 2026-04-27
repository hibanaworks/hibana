use crate::control::cluster::effects::ResourceDescriptor;
use crate::eff::EffIndex;
use crate::global::ControlDesc;
use crate::global::compiled::images::program::{
    CompiledProgramCounts, CompiledProgramFacts, DynamicPolicySite, RouteControlRecord,
};
use crate::global::compiled::images::role::{
    CompiledRoleImage, CompiledRoleSegmentHeader, PhaseImageHeader, PhaseLaneEntry,
};
use crate::global::const_dsl::ControlScopeKind;
#[cfg(test)]
use crate::global::role_program::lane_word_count;
use crate::global::role_program::{LaneWord, RoleFootprint};
use crate::global::typestate::{
    LocalNode, RoleTypestateValue, RouteDispatchEntry, RouteDispatchShape, RouteScopeRecord,
    ScopeRecord, StateIndex,
};

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[inline(always)]
const fn max_align(current: usize, candidate: usize) -> usize {
    if candidate > current {
        candidate
    } else {
        current
    }
}

#[inline(always)]
const fn section_bytes<T>(count: usize) -> usize {
    count.saturating_mul(core::mem::size_of::<T>())
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_program_tail_bytes_for_counts(
    counts: CompiledProgramCounts,
) -> usize {
    let mut offset = core::mem::size_of::<CompiledProgramFacts>();
    offset = align_up(offset, core::mem::align_of::<ResourceDescriptor>());
    offset = offset.saturating_add(section_bytes::<ResourceDescriptor>(counts.resources));
    offset = align_up(offset, core::mem::align_of::<DynamicPolicySite>());
    offset = offset.saturating_add(section_bytes::<DynamicPolicySite>(
        counts.dynamic_policy_sites,
    ));
    offset = align_up(offset, core::mem::align_of::<RouteControlRecord>());
    offset.saturating_add(section_bytes::<RouteControlRecord>(counts.route_controls))
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_program_tail_align() -> usize {
    let mut align = core::mem::align_of::<CompiledProgramFacts>();
    align = max_align(align, core::mem::align_of::<ResourceDescriptor>());
    align = max_align(align, core::mem::align_of::<ControlScopeKind>());
    align = max_align(align, core::mem::align_of::<DynamicPolicySite>());
    max_align(align, core::mem::align_of::<RouteControlRecord>())
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_scope_cap(scope_count: usize) -> usize {
    scope_count
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_route_scope_cap(
    route_scope_count: usize,
) -> usize {
    route_scope_count
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_segment_header_cap(
    eff_count: usize,
) -> usize {
    if eff_count == 0 {
        0
    } else {
        eff_count.div_ceil(crate::eff::meta::MAX_SEGMENT_EFFS)
    }
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_step_cap(eff_count: usize) -> usize {
    if eff_count == 0 { 1 } else { eff_count }
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_typestate_node_cap(
    scope_count: usize,
    passive_linger_route_scope_count: usize,
    local_step_count: usize,
) -> usize {
    let capped = local_step_count
        .saturating_add(scope_count)
        .saturating_add(passive_linger_route_scope_count)
        .saturating_add(1);
    if capped == 0 { 1 } else { capped }
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_phase_cap(
    footprint: RoleFootprint,
) -> usize {
    footprint.phase_count
}

#[inline(always)]
const fn compiled_role_phase_lane_entry_cap(footprint: RoleFootprint) -> usize {
    footprint.phase_lane_entry_count
}

#[inline(always)]
const fn compiled_role_phase_lane_word_cap(footprint: RoleFootprint) -> usize {
    footprint.phase_lane_word_count
}

#[inline(always)]
const fn compiled_role_scope_lane_matrix_cap(footprint: RoleFootprint) -> usize {
    compiled_role_scope_cap(footprint.scope_count).saturating_mul(footprint.logical_lane_count)
}

#[inline(always)]
const fn compiled_role_route_scope_lane_word_cap(footprint: RoleFootprint) -> usize {
    compiled_role_route_scope_cap(footprint.route_scope_count)
        .saturating_mul(footprint.logical_lane_word_count)
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_image_bytes_for_layout(
    footprint: RoleFootprint,
) -> usize {
    let scope_cap = compiled_role_scope_cap(footprint.scope_count);
    let route_scope_cap = compiled_role_route_scope_cap(footprint.route_scope_count);
    let segment_header_cap = compiled_role_segment_header_cap(footprint.eff_count);
    let eff_index_cap = compiled_role_step_cap(footprint.eff_count);
    let step_index_cap = compiled_role_step_cap(footprint.local_step_count);
    let typestate_node_cap = compiled_role_typestate_node_cap(
        footprint.scope_count,
        footprint.passive_linger_route_scope_count,
        footprint.local_step_count,
    );
    let phase_header_cap = compiled_role_phase_cap(footprint);
    let phase_lane_entry_cap = compiled_role_phase_lane_entry_cap(footprint);
    let phase_lane_word_cap = compiled_role_phase_lane_word_cap(footprint);
    let scope_lane_matrix_cap = compiled_role_scope_lane_matrix_cap(footprint);
    let route_scope_lane_word_cap = compiled_role_route_scope_lane_word_cap(footprint);
    let route_dispatch_shape_cap = route_scope_cap;
    let route_dispatch_entry_cap =
        route_scope_cap.saturating_mul(crate::global::typestate::MAX_FIRST_RECV_DISPATCH);
    let route_dispatch_target_cap = route_dispatch_entry_cap;
    let header = core::mem::size_of::<CompiledRoleImage>();
    let segment_headers_start =
        align_up(header, core::mem::align_of::<CompiledRoleSegmentHeader>());
    let segment_headers_end = segment_headers_start
        + segment_header_cap.saturating_mul(core::mem::size_of::<CompiledRoleSegmentHeader>());
    let typestate_start = align_up(
        segment_headers_end,
        max_align(
            core::mem::align_of::<RoleTypestateValue>(),
            core::mem::align_of::<CompiledRoleImage>(),
        ),
    );
    let typestate_end = typestate_start + core::mem::size_of::<RoleTypestateValue>();
    let typestate_nodes_start = align_up(typestate_end, core::mem::align_of::<LocalNode>());
    let typestate_nodes_end = typestate_nodes_start
        + typestate_node_cap.saturating_mul(core::mem::size_of::<LocalNode>());
    let phase_headers_start = align_up(
        typestate_nodes_end,
        core::mem::align_of::<PhaseImageHeader>(),
    );
    let phase_headers_end = phase_headers_start
        + phase_header_cap.saturating_mul(core::mem::size_of::<PhaseImageHeader>());
    let phase_lane_entries_start =
        align_up(phase_headers_end, core::mem::align_of::<PhaseLaneEntry>());
    let phase_lane_entries_end = phase_lane_entries_start
        + phase_lane_entry_cap.saturating_mul(core::mem::size_of::<PhaseLaneEntry>());
    let phase_lane_words_start =
        align_up(phase_lane_entries_end, core::mem::align_of::<LaneWord>());
    let phase_lane_words_end = phase_lane_words_start
        + phase_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
    let records_start = align_up(phase_lane_words_end, core::mem::align_of::<ScopeRecord>());
    let records_end = records_start + scope_cap.saturating_mul(core::mem::size_of::<ScopeRecord>());
    let scope_lane_first_start = align_up(records_end, core::mem::align_of::<EffIndex>());
    let scope_lane_first_end = scope_lane_first_start
        + scope_lane_matrix_cap.saturating_mul(core::mem::size_of::<EffIndex>());
    let scope_lane_last_start = align_up(scope_lane_first_end, core::mem::align_of::<EffIndex>());
    let scope_lane_last_end = scope_lane_last_start
        + scope_lane_matrix_cap.saturating_mul(core::mem::size_of::<EffIndex>());
    let slots_start = align_up(scope_lane_last_end, core::mem::align_of::<u16>());
    let slots_end = slots_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
    let route_dense_start = align_up(slots_end, core::mem::align_of::<u16>());
    let route_dense_end = route_dense_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
    let route_records_start = align_up(route_dense_end, core::mem::align_of::<RouteScopeRecord>());
    let route_records_end = route_records_start
        + route_scope_cap.saturating_mul(core::mem::size_of::<RouteScopeRecord>());
    let route_offer_lane_words_start =
        align_up(route_records_end, core::mem::align_of::<LaneWord>());
    let route_offer_lane_words_end = route_offer_lane_words_start
        + route_scope_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
    let route_arm0_lane_words_start = align_up(
        route_offer_lane_words_end,
        core::mem::align_of::<LaneWord>(),
    );
    let route_arm0_lane_words_end = route_arm0_lane_words_start
        + route_scope_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
    let route_arm1_lane_words_start =
        align_up(route_arm0_lane_words_end, core::mem::align_of::<LaneWord>());
    let route_arm1_lane_words_end = route_arm1_lane_words_start
        + route_scope_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
    let route_arm0_lane_last_start =
        align_up(route_arm1_lane_words_end, core::mem::align_of::<EffIndex>());
    let route_arm0_lane_last_end = route_arm0_lane_last_start
        + scope_lane_matrix_cap.saturating_mul(core::mem::size_of::<EffIndex>());
    let route_dispatch_shapes_start = align_up(
        route_arm0_lane_last_end,
        core::mem::align_of::<RouteDispatchShape>(),
    );
    let route_dispatch_shapes_end = route_dispatch_shapes_start
        + route_dispatch_shape_cap.saturating_mul(core::mem::size_of::<RouteDispatchShape>());
    let route_dispatch_entries_start = align_up(
        route_dispatch_shapes_end,
        core::mem::align_of::<RouteDispatchEntry>(),
    );
    let route_dispatch_entries_end = route_dispatch_entries_start
        + route_dispatch_entry_cap.saturating_mul(core::mem::size_of::<RouteDispatchEntry>());
    let route_dispatch_targets_start = align_up(
        route_dispatch_entries_end,
        core::mem::align_of::<StateIndex>(),
    );
    let route_dispatch_targets_end = route_dispatch_targets_start
        + route_dispatch_target_cap.saturating_mul(core::mem::size_of::<StateIndex>());
    let eff_index_start = align_up(route_dispatch_targets_end, core::mem::align_of::<u16>());
    let eff_index_end = eff_index_start + eff_index_cap.saturating_mul(core::mem::size_of::<u16>());
    let step_index_start = align_up(eff_index_end, core::mem::align_of::<StateIndex>());
    let step_index_end =
        step_index_start + step_index_cap.saturating_mul(core::mem::size_of::<StateIndex>());
    let control_by_eff_start = align_up(step_index_end, core::mem::align_of::<ControlDesc>());
    control_by_eff_start + eff_index_cap.saturating_mul(core::mem::size_of::<ControlDesc>())
}

#[cfg(test)]
#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_image_bytes_for_counts(
    scope_count: usize,
    route_scope_count: usize,
    eff_count: usize,
) -> usize {
    let phase_count = if eff_count == 0 {
        0
    } else {
        let derived = scope_count.saturating_mul(2).saturating_add(1);
        if derived < eff_count {
            derived
        } else {
            eff_count
        }
    };
    compiled_role_image_bytes_for_layout(RoleFootprint {
        scope_count,
        max_active_scope_depth: scope_count,
        eff_count,
        phase_count,
        phase_lane_entry_count: eff_count,
        phase_lane_word_count: if eff_count == 0 {
            0
        } else {
            phase_count.saturating_mul(lane_word_count(u8::MAX as usize + 1))
        },
        parallel_enter_count: scope_count,
        route_scope_count,
        local_step_count: eff_count,
        passive_linger_route_scope_count: route_scope_count,
        active_lane_count: u8::MAX as usize + 1,
        endpoint_lane_slot_count: u8::MAX as usize + 1,
        logical_lane_count: u8::MAX as usize + 1,
        logical_lane_word_count: lane_word_count(u8::MAX as usize + 1),
        max_route_stack_depth: 0,
        scope_evidence_count: 0,
        frontier_entry_count: 0,
    })
}

#[inline(always)]
pub(in crate::global::compiled) const fn compiled_role_image_align() -> usize {
    let mut align = core::mem::align_of::<CompiledRoleImage>();
    align = max_align(align, core::mem::align_of::<RoleTypestateValue>());
    align = max_align(align, core::mem::align_of::<LocalNode>());
    align = max_align(align, core::mem::align_of::<PhaseImageHeader>());
    align = max_align(align, core::mem::align_of::<PhaseLaneEntry>());
    align = max_align(align, core::mem::align_of::<LaneWord>());
    align = max_align(align, core::mem::align_of::<ScopeRecord>());
    align = max_align(align, core::mem::align_of::<EffIndex>());
    align = max_align(align, core::mem::align_of::<u16>());
    align = max_align(align, core::mem::align_of::<RouteScopeRecord>());
    align = max_align(align, core::mem::align_of::<RouteDispatchShape>());
    align = max_align(align, core::mem::align_of::<RouteDispatchEntry>());
    align = max_align(align, core::mem::align_of::<StateIndex>());
    max_align(align, core::mem::align_of::<ControlDesc>())
}
