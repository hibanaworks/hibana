use crate::eff::EffIndex;
use crate::global::role_program::{LaneWord, RoleFootprint};
use crate::global::typestate::{
    LocalNode, RoleTypestateValue, RouteDispatchEntry, RouteDispatchShape, RouteScopeRecord,
    ScopeRecord, StateIndex,
};
use core::ptr::NonNull;

use super::super::images::role::{
    CompiledRoleImage, CompiledRoleSegmentHeader, PhaseImageHeader, PhaseLaneEntry,
};

pub(in crate::global::compiled) struct CompiledRoleScopeStorage {
    pub(super) typestate: *mut RoleTypestateValue,
    pub(super) segment_headers: *mut CompiledRoleSegmentHeader,
    pub(super) segment_header_cap: usize,
    pub(super) typestate_nodes: *mut LocalNode,
    pub(super) typestate_node_cap: usize,
    pub(super) phase_headers: *mut PhaseImageHeader,
    pub(super) phase_header_cap: usize,
    pub(super) phase_lane_entries: *mut PhaseLaneEntry,
    pub(super) phase_lane_entry_cap: usize,
    pub(super) phase_lane_words: *mut LaneWord,
    pub(super) phase_lane_word_cap: usize,
    pub(super) records: *mut ScopeRecord,
    pub(super) scope_lane_first_eff: *mut EffIndex,
    pub(super) scope_lane_last_eff: *mut EffIndex,
    pub(super) slots_by_scope: *mut u16,
    pub(super) route_dense_by_slot: *mut u16,
    pub(super) route_records: *mut RouteScopeRecord,
    pub(super) route_offer_lane_words: *mut LaneWord,
    pub(super) route_arm0_lane_words: *mut LaneWord,
    pub(super) route_arm1_lane_words: *mut LaneWord,
    pub(super) route_arm0_lane_last_eff_by_slot: *mut EffIndex,
    pub(super) route_dispatch_shapes: *mut RouteDispatchShape,
    pub(super) route_dispatch_shape_cap: usize,
    pub(super) route_dispatch_entries: *mut RouteDispatchEntry,
    pub(super) route_dispatch_entry_cap: usize,
    pub(super) route_dispatch_targets: *mut StateIndex,
    pub(super) route_dispatch_target_cap: usize,
    pub(super) route_scope_cap: usize,
    pub(super) scope_cap: usize,
}

impl CompiledRoleScopeStorage {
    #[inline(always)]
    pub(in crate::global::compiled) const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn scope_cap(scope_count: usize) -> usize {
        scope_count
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn route_scope_cap(route_scope_count: usize) -> usize {
        route_scope_count
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn segment_header_cap(eff_count: usize) -> usize {
        if eff_count == 0 {
            0
        } else {
            eff_count.div_ceil(crate::eff::meta::MAX_SEGMENT_EFFS)
        }
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn typestate_node_cap(
        scope_count: usize,
        passive_linger_route_scope_count: usize,
        local_step_count: usize,
    ) -> usize {
        // Local nodes cover the projected local steps plus at most one boundary
        // node per structured scope. Passive linger route scopes may additionally need
        // one arm-navigation jump beyond that base budget, plus one terminal slot.
        let capped = local_step_count
            .saturating_add(scope_count)
            .saturating_add(passive_linger_route_scope_count)
            .saturating_add(1);
        if capped == 0 { 1 } else { capped }
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn phase_cap(footprint: RoleFootprint) -> usize {
        footprint.phase_count
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn phase_lane_entry_cap(
        footprint: RoleFootprint,
    ) -> usize {
        footprint.phase_lane_entry_count
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn phase_lane_word_cap(
        footprint: RoleFootprint,
    ) -> usize {
        footprint.phase_lane_word_count
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn scope_lane_matrix_cap(
        footprint: RoleFootprint,
    ) -> usize {
        Self::scope_cap(footprint.scope_count).saturating_mul(footprint.logical_lane_count)
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn route_scope_lane_word_cap(
        footprint: RoleFootprint,
    ) -> usize {
        Self::route_scope_cap(footprint.route_scope_count)
            .saturating_mul(footprint.logical_lane_word_count)
    }

    #[inline(always)]
    pub(super) unsafe fn from_image_ptr_with_layout(
        image: *mut CompiledRoleImage,
        footprint: RoleFootprint,
    ) -> Self {
        let scope_cap = Self::scope_cap(footprint.scope_count);
        let route_scope_cap = Self::route_scope_cap(footprint.route_scope_count);
        let segment_header_cap = Self::segment_header_cap(footprint.eff_count);
        let typestate_node_cap = Self::typestate_node_cap(
            footprint.scope_count,
            footprint.passive_linger_route_scope_count,
            footprint.local_step_count,
        );
        let phase_header_cap = Self::phase_cap(footprint);
        let phase_lane_entry_cap = Self::phase_lane_entry_cap(footprint);
        let phase_lane_word_cap = Self::phase_lane_word_cap(footprint);
        let scope_lane_matrix_cap = Self::scope_lane_matrix_cap(footprint);
        let route_scope_lane_word_cap = Self::route_scope_lane_word_cap(footprint);
        let route_dispatch_shape_cap = route_scope_cap;
        let route_dispatch_entry_cap =
            route_scope_cap.saturating_mul(crate::global::typestate::MAX_FIRST_RECV_DISPATCH);
        let route_dispatch_target_cap = route_dispatch_entry_cap;
        let base = image.cast::<u8>() as usize;
        let header_end = base + core::mem::size_of::<CompiledRoleImage>();
        let segment_headers_start = Self::align_up(
            header_end,
            core::mem::align_of::<CompiledRoleSegmentHeader>(),
        );
        let segment_headers_end = segment_headers_start
            + segment_header_cap.saturating_mul(core::mem::size_of::<CompiledRoleSegmentHeader>());
        let typestate_start = Self::align_up(
            segment_headers_end,
            core::mem::align_of::<RoleTypestateValue>(),
        );
        let typestate_end = typestate_start + core::mem::size_of::<RoleTypestateValue>();
        let typestate_nodes_start =
            Self::align_up(typestate_end, core::mem::align_of::<LocalNode>());
        let typestate_nodes_end = typestate_nodes_start
            + typestate_node_cap.saturating_mul(core::mem::size_of::<LocalNode>());
        let phase_headers_start = Self::align_up(
            typestate_nodes_end,
            core::mem::align_of::<PhaseImageHeader>(),
        );
        let phase_headers_end = phase_headers_start
            + phase_header_cap.saturating_mul(core::mem::size_of::<PhaseImageHeader>());
        let phase_lane_entries_start =
            Self::align_up(phase_headers_end, core::mem::align_of::<PhaseLaneEntry>());
        let phase_lane_entries_end = phase_lane_entries_start
            + phase_lane_entry_cap.saturating_mul(core::mem::size_of::<PhaseLaneEntry>());
        let phase_lane_words_start =
            Self::align_up(phase_lane_entries_end, core::mem::align_of::<LaneWord>());
        let phase_lane_words_end = phase_lane_words_start
            + phase_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
        let records_start =
            Self::align_up(phase_lane_words_end, core::mem::align_of::<ScopeRecord>());
        let records_end =
            records_start + scope_cap.saturating_mul(core::mem::size_of::<ScopeRecord>());
        let scope_lane_first_start = Self::align_up(records_end, core::mem::align_of::<EffIndex>());
        let scope_lane_first_end = scope_lane_first_start
            + scope_lane_matrix_cap.saturating_mul(core::mem::size_of::<EffIndex>());
        let scope_lane_last_start =
            Self::align_up(scope_lane_first_end, core::mem::align_of::<EffIndex>());
        let scope_lane_last_end = scope_lane_last_start
            + scope_lane_matrix_cap.saturating_mul(core::mem::size_of::<EffIndex>());
        let slots_start = Self::align_up(scope_lane_last_end, core::mem::align_of::<u16>());
        let slots_end = slots_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
        let route_dense_start = Self::align_up(slots_end, core::mem::align_of::<u16>());
        let route_dense_end =
            route_dense_start + scope_cap.saturating_mul(core::mem::size_of::<u16>());
        let route_records_start =
            Self::align_up(route_dense_end, core::mem::align_of::<RouteScopeRecord>());
        let route_records_end = route_records_start
            + route_scope_cap.saturating_mul(core::mem::size_of::<RouteScopeRecord>());
        let route_offer_lane_words_start =
            Self::align_up(route_records_end, core::mem::align_of::<LaneWord>());
        let route_offer_lane_words_end = route_offer_lane_words_start
            + route_scope_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
        let route_arm0_lane_words_start = Self::align_up(
            route_offer_lane_words_end,
            core::mem::align_of::<LaneWord>(),
        );
        let route_arm0_lane_words_end = route_arm0_lane_words_start
            + route_scope_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
        let route_arm1_lane_words_start =
            Self::align_up(route_arm0_lane_words_end, core::mem::align_of::<LaneWord>());
        let route_arm1_lane_words_end = route_arm1_lane_words_start
            + route_scope_lane_word_cap.saturating_mul(core::mem::size_of::<LaneWord>());
        let route_arm0_lane_last_start =
            Self::align_up(route_arm1_lane_words_end, core::mem::align_of::<EffIndex>());
        let route_arm0_lane_last_end = route_arm0_lane_last_start
            + scope_lane_matrix_cap.saturating_mul(core::mem::size_of::<EffIndex>());
        let route_dispatch_shapes_start = Self::align_up(
            route_arm0_lane_last_end,
            core::mem::align_of::<RouteDispatchShape>(),
        );
        let route_dispatch_shapes_end = route_dispatch_shapes_start
            + route_dispatch_shape_cap.saturating_mul(core::mem::size_of::<RouteDispatchShape>());
        let route_dispatch_entries_start = Self::align_up(
            route_dispatch_shapes_end,
            core::mem::align_of::<RouteDispatchEntry>(),
        );
        let route_dispatch_entries_end = route_dispatch_entries_start
            + route_dispatch_entry_cap.saturating_mul(core::mem::size_of::<RouteDispatchEntry>());
        let route_dispatch_targets_start = Self::align_up(
            route_dispatch_entries_end,
            core::mem::align_of::<StateIndex>(),
        );
        Self {
            typestate: typestate_start as *mut RoleTypestateValue,
            segment_headers: if segment_header_cap == 0 {
                NonNull::<CompiledRoleSegmentHeader>::dangling().as_ptr()
            } else {
                segment_headers_start as *mut CompiledRoleSegmentHeader
            },
            segment_header_cap,
            typestate_nodes: typestate_nodes_start as *mut LocalNode,
            typestate_node_cap,
            phase_headers: phase_headers_start as *mut PhaseImageHeader,
            phase_header_cap,
            phase_lane_entries: phase_lane_entries_start as *mut PhaseLaneEntry,
            phase_lane_entry_cap,
            phase_lane_words: phase_lane_words_start as *mut LaneWord,
            phase_lane_word_cap,
            records: records_start as *mut ScopeRecord,
            scope_lane_first_eff: scope_lane_first_start as *mut EffIndex,
            scope_lane_last_eff: scope_lane_last_start as *mut EffIndex,
            slots_by_scope: slots_start as *mut u16,
            route_dense_by_slot: route_dense_start as *mut u16,
            route_records: route_records_start as *mut RouteScopeRecord,
            route_offer_lane_words: route_offer_lane_words_start as *mut LaneWord,
            route_arm0_lane_words: route_arm0_lane_words_start as *mut LaneWord,
            route_arm1_lane_words: route_arm1_lane_words_start as *mut LaneWord,
            route_arm0_lane_last_eff_by_slot: route_arm0_lane_last_start as *mut EffIndex,
            route_dispatch_shapes: route_dispatch_shapes_start as *mut RouteDispatchShape,
            route_dispatch_shape_cap,
            route_dispatch_entries: route_dispatch_entries_start as *mut RouteDispatchEntry,
            route_dispatch_entry_cap,
            route_dispatch_targets: route_dispatch_targets_start as *mut StateIndex,
            route_dispatch_target_cap,
            route_scope_cap,
            scope_cap,
        }
    }
}

#[inline(always)]
fn used_route_lane_word_words(
    route_records: *const RouteScopeRecord,
    route_scope_count: usize,
    lane_word_len: usize,
) -> (usize, usize) {
    if route_scope_count == 0 || lane_word_len == 0 {
        return (0, 0);
    }
    let mut offer_words = 0usize;
    let mut route_arm_words = 0usize;
    let mut idx = 0usize;
    while idx < route_scope_count {
        let record = unsafe { &*route_records.add(idx) };
        let offer_end = record.offer_lane_word_start as usize + lane_word_len;
        if offer_end > offer_words {
            offer_words = offer_end;
        }
        let route_arm_end = record.route_arm_lane_word_start as usize + lane_word_len;
        if route_arm_end > route_arm_words {
            route_arm_words = route_arm_end;
        }
        idx += 1;
    }
    (offer_words, route_arm_words)
}

#[inline(always)]
pub(super) unsafe fn compact_route_scope_tail(
    storage: &CompiledRoleScopeStorage,
    lane_slot_count: usize,
    lane_word_len: usize,
) -> usize {
    let typestate = unsafe { &mut *storage.typestate };
    let route_scope_count = typestate.route_scope_count();
    let dispatch_shape_count = typestate.route_dispatch_shape_count();
    let dispatch_entry_count = typestate.route_dispatch_entry_count();
    let dispatch_target_count = typestate.route_dispatch_target_count();
    let route_records_start = storage.route_records as usize;
    let route_records_end = route_records_start
        .saturating_add(route_scope_count.saturating_mul(core::mem::size_of::<RouteScopeRecord>()));
    let (offer_lane_word_words, arm1_lane_word_words) = used_route_lane_word_words(
        storage.route_records.cast_const(),
        route_scope_count,
        lane_word_len,
    );
    let arm0_lane_word_words = arm1_lane_word_words;

    let offer_lane_words_start =
        CompiledRoleScopeStorage::align_up(route_records_end, core::mem::align_of::<LaneWord>());
    let offer_lane_words_end = offer_lane_words_start
        .saturating_add(offer_lane_word_words.saturating_mul(core::mem::size_of::<LaneWord>()));
    let arm0_lane_words_start =
        CompiledRoleScopeStorage::align_up(offer_lane_words_end, core::mem::align_of::<LaneWord>());
    let arm0_lane_words_end = arm0_lane_words_start
        .saturating_add(arm0_lane_word_words.saturating_mul(core::mem::size_of::<LaneWord>()));
    let arm1_lane_words_start =
        CompiledRoleScopeStorage::align_up(arm0_lane_words_end, core::mem::align_of::<LaneWord>());
    let arm1_lane_words_end = arm1_lane_words_start
        .saturating_add(arm1_lane_word_words.saturating_mul(core::mem::size_of::<LaneWord>()));
    let arm0_lane_last_start =
        CompiledRoleScopeStorage::align_up(arm1_lane_words_end, core::mem::align_of::<EffIndex>());
    let arm0_lane_last_end = arm0_lane_last_start.saturating_add(
        route_scope_count
            .saturating_mul(lane_slot_count)
            .saturating_mul(core::mem::size_of::<EffIndex>()),
    );
    let dispatch_shapes_start = CompiledRoleScopeStorage::align_up(
        arm0_lane_last_end,
        core::mem::align_of::<RouteDispatchShape>(),
    );
    let dispatch_entries_start = CompiledRoleScopeStorage::align_up(
        dispatch_shapes_start.saturating_add(
            dispatch_shape_count.saturating_mul(core::mem::size_of::<RouteDispatchShape>()),
        ),
        core::mem::align_of::<RouteDispatchEntry>(),
    );
    let dispatch_targets_start = CompiledRoleScopeStorage::align_up(
        dispatch_entries_start.saturating_add(
            dispatch_entry_count.saturating_mul(core::mem::size_of::<RouteDispatchEntry>()),
        ),
        core::mem::align_of::<StateIndex>(),
    );
    let dispatch_shapes_dst = dispatch_shapes_start as *mut RouteDispatchShape;
    let dispatch_entries_dst = dispatch_entries_start as *mut RouteDispatchEntry;
    let dispatch_targets_dst = dispatch_targets_start as *mut StateIndex;
    let offer_lane_words_dst = offer_lane_words_start as *mut LaneWord;
    let arm0_lane_words_dst = arm0_lane_words_start as *mut LaneWord;
    let arm1_lane_words_dst = arm1_lane_words_start as *mut LaneWord;
    let arm0_lane_last_dst = arm0_lane_last_start as *mut EffIndex;

    if offer_lane_word_words != 0 {
        unsafe {
            core::ptr::copy(
                storage.route_offer_lane_words,
                offer_lane_words_dst,
                offer_lane_word_words,
            );
        }
    }
    if arm1_lane_word_words != 0 {
        unsafe {
            core::ptr::copy(
                storage.route_arm0_lane_words,
                arm0_lane_words_dst,
                arm0_lane_word_words,
            );
        }
        unsafe {
            core::ptr::copy(
                storage.route_arm1_lane_words,
                arm1_lane_words_dst,
                arm1_lane_word_words,
            );
        }
    }
    if route_scope_count != 0 && lane_slot_count != 0 {
        unsafe {
            core::ptr::copy(
                storage.route_arm0_lane_last_eff_by_slot,
                arm0_lane_last_dst,
                route_scope_count.saturating_mul(lane_slot_count),
            );
        }
    }
    if dispatch_shape_count != 0 {
        unsafe {
            core::ptr::copy(
                storage.route_dispatch_shapes,
                dispatch_shapes_dst,
                dispatch_shape_count,
            );
        }
    }
    if dispatch_entry_count != 0 {
        unsafe {
            core::ptr::copy(
                storage.route_dispatch_entries,
                dispatch_entries_dst,
                dispatch_entry_count,
            );
        }
    }
    if dispatch_target_count != 0 {
        unsafe {
            core::ptr::copy(
                storage.route_dispatch_targets,
                dispatch_targets_dst,
                dispatch_target_count,
            );
        }
    }

    unsafe {
        typestate.relocate_compact_route_payload(
            storage.route_records.cast_const(),
            offer_lane_words_dst.cast_const(),
            arm0_lane_words_dst.cast_const(),
            arm1_lane_words_dst.cast_const(),
            dispatch_shapes_dst.cast_const(),
            dispatch_shape_count,
            dispatch_entries_dst.cast_const(),
            dispatch_entry_count,
            dispatch_targets_dst.cast_const(),
            dispatch_target_count,
            arm0_lane_last_dst.cast_const(),
        );
    }

    dispatch_targets_start
        .saturating_add(dispatch_target_count.saturating_mul(core::mem::size_of::<StateIndex>()))
}
