use core::ptr;

use crate::eff::EffIndex;
use crate::endpoint::kernel::{EndpointArenaLayout, FrontierScratchLayout};
use crate::global::role_program::{
    LaneSetView, LaneSteps, LaneWord, LocalStep, PhaseRouteGuard, RoleFootprint, lane_word_count,
    logical_lane_count_for_role,
};
use crate::global::typestate::{
    LocalAction, LocalNode, RoleTypestateValue, RouteDispatchEntry, RouteDispatchShape,
    RouteScopeRecord, ScopeRecord, StateIndex,
};

use super::{LoweringSummary, lease::RoleLoweringScratch};

const MACHINE_NO_STEP: u16 = u16::MAX;

#[inline(always)]
const fn encode_compact_step_index(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact index overflow");
    }
    value as u16
}

#[inline(always)]
const fn encode_compact_count_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact count overflow");
    }
    value as u16
}

#[inline(always)]
const fn encode_compact_offset_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact offset overflow");
    }
    value as u16
}

/// Crate-private runtime image for role-local immutable facts.
#[derive(Clone, Debug)]
pub(crate) struct CompiledRoleImage {
    typestate_offset: u16,
    phase_headers_offset: u16,
    phase_lane_entries_offset: u16,
    phase_lane_words_offset: u16,
    eff_index_to_step_offset: u16,
    step_index_to_state_offset: u16,
    role: u8,
    role_facts: RoleResidentFacts,
}

#[derive(Clone, Copy, Debug)]
struct PhaseImageHeader {
    lane_entry_start: u16,
    lane_entry_len: u16,
    lane_word_start: u16,
    lane_word_len: u16,
    min_start: u16,
    route_guard: PhaseRouteGuard,
}

impl PhaseImageHeader {
    const EMPTY: Self = Self {
        lane_entry_start: 0,
        lane_entry_len: 0,
        lane_word_start: 0,
        lane_word_len: 0,
        min_start: 0,
        route_guard: PhaseRouteGuard::EMPTY,
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseLaneEntry {
    pub(crate) lane: u8,
    pub(crate) steps: LaneSteps,
}

impl PhaseLaneEntry {
    const EMPTY: Self = Self {
        lane: 0,
        steps: LaneSteps::EMPTY,
    };
}

#[derive(Clone, Copy, Debug)]
struct RoleResidentFacts {
    active_lane_count: u16,
    endpoint_lane_slot_count: u16,
    phase_len: u16,
    phase_lane_entry_len: u16,
    phase_lane_word_len: u16,
    eff_index_to_step_len: u16,
    step_index_to_state_len: u16,
    persistent_bytes: u16,
}

impl RoleResidentFacts {
    const EMPTY: Self = Self {
        active_lane_count: 0,
        endpoint_lane_slot_count: 0,
        phase_len: 0,
        phase_lane_entry_len: 0,
        phase_lane_word_len: 0,
        eff_index_to_step_len: 0,
        step_index_to_state_len: 0,
        persistent_bytes: 0,
    };

    #[inline(always)]
    const fn active_lane_count(self) -> usize {
        self.active_lane_count as usize
    }

    #[inline(always)]
    const fn endpoint_lane_slot_count(self) -> usize {
        let count = self.endpoint_lane_slot_count as usize;
        if count == 0 { 1 } else { count }
    }

    #[inline(always)]
    const fn phase_len(self) -> usize {
        self.phase_len as usize
    }

    #[inline(always)]
    const fn phase_lane_entry_len(self) -> usize {
        self.phase_lane_entry_len as usize
    }

    #[inline(always)]
    const fn phase_lane_word_len(self) -> usize {
        self.phase_lane_word_len as usize
    }

    #[inline(always)]
    const fn eff_index_to_step_len(self) -> usize {
        self.eff_index_to_step_len as usize
    }

    #[inline(always)]
    const fn step_index_to_state_len(self) -> usize {
        self.step_index_to_state_len as usize
    }

    #[inline(always)]
    const fn persistent_bytes(self) -> usize {
        self.persistent_bytes as usize
    }
}

struct CompiledRoleScopeStorage {
    typestate: *mut RoleTypestateValue,
    typestate_nodes: *mut LocalNode,
    typestate_node_cap: usize,
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
    records: *mut ScopeRecord,
    scope_lane_first_eff: *mut EffIndex,
    scope_lane_last_eff: *mut EffIndex,
    slots_by_scope: *mut u16,
    route_dense_by_slot: *mut u16,
    route_records: *mut RouteScopeRecord,
    route_offer_lane_words: *mut LaneWord,
    route_arm1_lane_words: *mut LaneWord,
    route_arm0_lane_last_eff_by_slot: *mut EffIndex,
    route_dispatch_shapes: *mut RouteDispatchShape,
    route_dispatch_shape_cap: usize,
    route_dispatch_entries: *mut RouteDispatchEntry,
    route_dispatch_entry_cap: usize,
    route_dispatch_targets: *mut StateIndex,
    route_dispatch_target_cap: usize,
    route_scope_cap: usize,
    scope_cap: usize,
}

impl CompiledRoleScopeStorage {
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn scope_cap(scope_count: usize) -> usize {
        scope_count
    }

    #[inline(always)]
    const fn route_scope_cap(route_scope_count: usize) -> usize {
        route_scope_count
    }

    #[inline(always)]
    const fn step_cap(eff_count: usize) -> usize {
        if eff_count == 0 { 1 } else { eff_count }
    }

    #[inline(always)]
    const fn typestate_node_cap(
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
    const fn phase_cap(footprint: RoleFootprint) -> usize {
        footprint.phase_count
    }

    #[inline(always)]
    const fn phase_lane_entry_cap(footprint: RoleFootprint) -> usize {
        footprint.phase_lane_entry_count
    }

    #[inline(always)]
    const fn phase_lane_word_cap(footprint: RoleFootprint) -> usize {
        footprint.phase_lane_word_count
    }

    #[inline(always)]
    const fn scope_lane_matrix_cap(footprint: RoleFootprint) -> usize {
        Self::scope_cap(footprint.scope_count).saturating_mul(footprint.logical_lane_count)
    }

    #[inline(always)]
    const fn route_scope_lane_word_cap(footprint: RoleFootprint) -> usize {
        Self::route_scope_cap(footprint.route_scope_count)
            .saturating_mul(footprint.logical_lane_word_count)
    }

    #[inline(always)]
    const fn total_bytes_for_layout(footprint: RoleFootprint) -> usize {
        let scope_cap = Self::scope_cap(footprint.scope_count);
        let route_scope_cap = Self::route_scope_cap(footprint.route_scope_count);
        let eff_index_cap = Self::step_cap(footprint.eff_count);
        let step_index_cap = Self::step_cap(footprint.local_step_count);
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
        let header = core::mem::size_of::<CompiledRoleImage>();
        let typestate_start = Self::align_up(
            header,
            if core::mem::align_of::<RoleTypestateValue>()
                > core::mem::align_of::<CompiledRoleImage>()
            {
                core::mem::align_of::<RoleTypestateValue>()
            } else {
                core::mem::align_of::<CompiledRoleImage>()
            },
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
        let route_arm1_lane_words_start = Self::align_up(
            route_offer_lane_words_end,
            core::mem::align_of::<LaneWord>(),
        );
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
        let route_dispatch_targets_end = route_dispatch_targets_start
            + route_dispatch_target_cap.saturating_mul(core::mem::size_of::<StateIndex>());
        let eff_index_start =
            Self::align_up(route_dispatch_targets_end, core::mem::align_of::<u16>());
        let eff_index_end =
            eff_index_start + eff_index_cap.saturating_mul(core::mem::size_of::<u16>());
        let step_index_start = Self::align_up(eff_index_end, core::mem::align_of::<StateIndex>());
        step_index_start + step_index_cap.saturating_mul(core::mem::size_of::<StateIndex>())
    }

    #[cfg(test)]
    #[inline(always)]
    const fn total_bytes_for_counts(
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
        Self::total_bytes_for_layout(RoleFootprint {
            scope_count,
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
    const fn overall_align() -> usize {
        let mut align = core::mem::align_of::<CompiledRoleImage>();
        if core::mem::align_of::<RoleTypestateValue>() > align {
            align = core::mem::align_of::<RoleTypestateValue>();
        }
        if core::mem::align_of::<LocalNode>() > align {
            align = core::mem::align_of::<LocalNode>();
        }
        if core::mem::align_of::<PhaseImageHeader>() > align {
            align = core::mem::align_of::<PhaseImageHeader>();
        }
        if core::mem::align_of::<PhaseLaneEntry>() > align {
            align = core::mem::align_of::<PhaseLaneEntry>();
        }
        if core::mem::align_of::<LaneWord>() > align {
            align = core::mem::align_of::<LaneWord>();
        }
        if core::mem::align_of::<ScopeRecord>() > align {
            align = core::mem::align_of::<ScopeRecord>();
        }
        if core::mem::align_of::<RouteScopeRecord>() > align {
            align = core::mem::align_of::<RouteScopeRecord>();
        }
        if core::mem::align_of::<RouteDispatchShape>() > align {
            align = core::mem::align_of::<RouteDispatchShape>();
        }
        if core::mem::align_of::<RouteDispatchEntry>() > align {
            align = core::mem::align_of::<RouteDispatchEntry>();
        }
        if core::mem::align_of::<EffIndex>() > align {
            align = core::mem::align_of::<EffIndex>();
        }
        if core::mem::align_of::<StateIndex>() > align {
            align = core::mem::align_of::<StateIndex>();
        }
        align
    }

    #[inline(always)]
    unsafe fn from_image_ptr_with_layout(
        image: *mut CompiledRoleImage,
        footprint: RoleFootprint,
    ) -> Self {
        let scope_cap = Self::scope_cap(footprint.scope_count);
        let route_scope_cap = Self::route_scope_cap(footprint.route_scope_count);
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
        let typestate_start =
            Self::align_up(header_end, core::mem::align_of::<RoleTypestateValue>());
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
        let route_arm1_lane_words_start = Self::align_up(
            route_offer_lane_words_end,
            core::mem::align_of::<LaneWord>(),
        );
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
    let mut arm1_words = 0usize;
    let mut idx = 0usize;
    while idx < route_scope_count {
        let record = unsafe { &*route_records.add(idx) };
        let offer_end = record.offer_lane_word_start as usize + lane_word_len;
        if offer_end > offer_words {
            offer_words = offer_end;
        }
        let arm1_end = record.arm1_lane_word_start as usize + lane_word_len;
        if arm1_end > arm1_words {
            arm1_words = arm1_end;
        }
        idx += 1;
    }
    (offer_words, arm1_words)
}

#[inline(always)]
unsafe fn compact_route_scope_tail(
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

    let offer_lane_words_start =
        CompiledRoleScopeStorage::align_up(route_records_end, core::mem::align_of::<LaneWord>());
    let offer_lane_words_end = offer_lane_words_start
        .saturating_add(offer_lane_word_words.saturating_mul(core::mem::size_of::<LaneWord>()));
    let arm1_lane_words_start =
        CompiledRoleScopeStorage::align_up(offer_lane_words_end, core::mem::align_of::<LaneWord>());
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

fn build_local_steps_into(
    role: u8,
    typestate: &RoleTypestateValue,
    by_eff_index: &mut [LocalStep],
    present: &mut [bool],
    steps: &mut [LocalStep],
    eff_index_to_step: &mut [u16],
) -> usize {
    if by_eff_index.len() != present.len() || by_eff_index.len() != eff_index_to_step.len() {
        panic!("compiled role local-step scratch shape mismatch");
    }
    let mut idx = 0usize;
    while idx < by_eff_index.len() {
        by_eff_index[idx] = LocalStep::EMPTY;
        present[idx] = false;
        eff_index_to_step[idx] = MACHINE_NO_STEP;
        idx += 1;
    }
    let mut step_idx = 0usize;
    while step_idx < steps.len() {
        steps[step_idx] = LocalStep::EMPTY;
        step_idx += 1;
    }

    let mut node_idx = 0usize;
    while node_idx < typestate.len() {
        match typestate.node(node_idx).action() {
            LocalAction::Send {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                lane,
                ..
            } => {
                let idx = eff_index.as_usize();
                if idx >= by_eff_index.len() {
                    panic!("local step eff_index exceeds lowering scratch capacity");
                }
                if !present[idx] {
                    by_eff_index[idx] =
                        LocalStep::send(eff_index, peer, label, resource, is_control, shot, lane);
                    present[idx] = true;
                }
            }
            LocalAction::Recv {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                lane,
                ..
            } => {
                let idx = eff_index.as_usize();
                if idx >= by_eff_index.len() {
                    panic!("local step eff_index exceeds lowering scratch capacity");
                }
                if !present[idx] {
                    by_eff_index[idx] =
                        LocalStep::recv(eff_index, peer, label, resource, is_control, shot, lane);
                    present[idx] = true;
                }
            }
            LocalAction::Local {
                eff_index,
                label,
                resource,
                is_control,
                shot,
                lane,
                ..
            } => {
                let idx = eff_index.as_usize();
                if idx >= by_eff_index.len() {
                    panic!("local step eff_index exceeds lowering scratch capacity");
                }
                if !present[idx] {
                    by_eff_index[idx] =
                        LocalStep::local(eff_index, role, label, resource, is_control, shot, lane);
                    present[idx] = true;
                }
            }
            LocalAction::Terminate | LocalAction::Jump { .. } => {}
        }
        node_idx += 1;
    }

    let mut len = 0usize;
    let mut idx = 0usize;
    while idx < by_eff_index.len() {
        if present[idx] {
            if len >= steps.len() {
                panic!("compiled role local step count exceeds lowering scratch capacity");
            }
            if len > u16::MAX as usize {
                panic!("compiled role local step count overflow");
            }
            steps[len] = by_eff_index[idx];
            eff_index_to_step[idx] = len as u16;
            len += 1;
        }
        idx += 1;
    }
    len
}

fn build_step_index_to_state_into(
    typestate: &RoleTypestateValue,
    steps: &[LocalStep],
    len: usize,
    eff_index_to_step: &[u16],
    step_index_to_state: &mut [StateIndex],
) {
    if len > steps.len() || len > step_index_to_state.len() {
        panic!("compiled role step-state lowering exceeds scratch capacity");
    }
    let mut idx = 0usize;
    while idx < step_index_to_state.len() {
        step_index_to_state[idx] = StateIndex::MAX;
        idx += 1;
    }
    let mut node_idx = 0usize;
    while node_idx < typestate.len() {
        match typestate.node(node_idx).action() {
            LocalAction::Send {
                eff_index,
                peer,
                label,
                lane,
                ..
            } => record_step_state(
                steps,
                len,
                eff_index_to_step,
                step_index_to_state,
                node_idx,
                eff_index,
                true,
                false,
                label,
                peer,
                lane,
            ),
            LocalAction::Recv {
                eff_index,
                peer,
                label,
                lane,
                ..
            } => record_step_state(
                steps,
                len,
                eff_index_to_step,
                step_index_to_state,
                node_idx,
                eff_index,
                false,
                false,
                label,
                peer,
                lane,
            ),
            LocalAction::Local {
                eff_index,
                label,
                lane,
                ..
            } => record_step_state(
                steps,
                len,
                eff_index_to_step,
                step_index_to_state,
                node_idx,
                eff_index,
                false,
                true,
                label,
                0,
                lane,
            ),
            LocalAction::Terminate | LocalAction::Jump { .. } => {}
        }
        node_idx += 1;
    }
}

fn record_step_state(
    steps: &[LocalStep],
    len: usize,
    eff_index_to_step: &[u16],
    step_index_to_state: &mut [StateIndex],
    node_idx: usize,
    eff_index: crate::eff::EffIndex,
    is_send: bool,
    is_local: bool,
    label: u8,
    peer: u8,
    lane: u8,
) {
    let eff_idx = eff_index.as_usize();
    if eff_idx >= eff_index_to_step.len() {
        panic!("eff_index out of bounds for compiled role mapping scratch");
    }
    let step_idx = eff_index_to_step[eff_idx];
    if step_idx == MACHINE_NO_STEP {
        return;
    }
    let step_idx = step_idx as usize;
    if step_idx >= len || step_idx >= steps.len() || step_idx >= step_index_to_state.len() {
        panic!("compiled role step index out of bounds");
    }
    let step = steps[step_idx];
    let matches = if is_local {
        step.is_local_action() && step.label() == label && step.lane() == lane
    } else if is_send {
        step.is_send() && step.label() == label && step.peer() == peer && step.lane() == lane
    } else {
        step.is_recv() && step.label() == label && step.peer() == peer && step.lane() == lane
    };
    if !matches {
        panic!("compiled role typestate mapping mismatch");
    }
    let mapped = StateIndex::from_usize(node_idx);
    if step_index_to_state[step_idx].is_max() {
        step_index_to_state[step_idx] = mapped;
    } else if step_index_to_state[step_idx].raw() != mapped.raw() {
        panic!("duplicate typestate mapping for step index");
    }
}

unsafe fn build_phase_image_from_steps(
    role: u8,
    steps: &[LocalStep],
    len: usize,
    typestate: &RoleTypestateValue,
    step_index_to_state: &[StateIndex],
    route_guards: &mut [PhaseRouteGuard],
    parallel_ranges: &mut [(usize, usize)],
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
) -> (usize, usize, usize) {
    if len > steps.len() || len > step_index_to_state.len() || len > route_guards.len() {
        panic!("compiled role phase lowering exceeds scratch capacity");
    }
    unsafe {
        initialize_phase_image_storage(
            phase_headers,
            phase_header_cap,
            phase_lane_entries,
            phase_lane_entry_cap,
            phase_lane_words,
            phase_lane_word_cap,
        );
    }
    let mut range_idx = 0usize;
    while range_idx < parallel_ranges.len() {
        parallel_ranges[range_idx] = (0, 0);
        range_idx += 1;
    }
    if len == 0 {
        return (0, 0, 0);
    }

    build_route_guards_for_steps_into(role, len, typestate, step_index_to_state, route_guards);

    let mut phase_count = 0usize;
    let mut phase_lane_entry_len = 0usize;
    let mut phase_lane_word_len = 0usize;

    if !typestate.has_parallel_phase_scope() {
        unsafe {
            push_phase_range_to_image(
                steps,
                0,
                len,
                route_guards,
                phase_headers,
                phase_header_cap,
                phase_lane_entries,
                phase_lane_entry_cap,
                phase_lane_words,
                phase_lane_word_cap,
                &mut phase_count,
                &mut phase_lane_entry_len,
                &mut phase_lane_word_len,
            );
        }
    } else {
        let mut parallel_count = 0usize;
        loop {
            let Some(range) = typestate.parallel_phase_range_at(parallel_count) else {
                break;
            };
            if parallel_count >= parallel_ranges.len() {
                panic!("compiled role phase capacity exceeded");
            }
            parallel_ranges[parallel_count] = range;
            parallel_count += 1;
        }

        if parallel_count == 0 {
            unsafe {
                push_phase_range_to_image(
                    steps,
                    0,
                    len,
                    route_guards,
                    phase_headers,
                    phase_header_cap,
                    phase_lane_entries,
                    phase_lane_entry_cap,
                    phase_lane_words,
                    phase_lane_word_cap,
                    &mut phase_count,
                    &mut phase_lane_entry_len,
                    &mut phase_lane_word_len,
                );
            }
        } else {
            let mut current_step = 0usize;
            let mut range_idx = 0usize;
            while range_idx < parallel_count {
                let (enter_eff, exit_eff) = parallel_ranges[range_idx];

                let seq_start = current_step;
                let mut seq_end = current_step;
                while seq_end < len && steps[seq_end].eff_index().as_usize() < enter_eff {
                    seq_end += 1;
                }
                if seq_end > seq_start {
                    unsafe {
                        push_phase_range_to_image(
                            steps,
                            seq_start,
                            seq_end,
                            route_guards,
                            phase_headers,
                            phase_header_cap,
                            phase_lane_entries,
                            phase_lane_entry_cap,
                            phase_lane_words,
                            phase_lane_word_cap,
                            &mut phase_count,
                            &mut phase_lane_entry_len,
                            &mut phase_lane_word_len,
                        );
                    }
                }

                let par_start = seq_end;
                let mut par_end = par_start;
                while par_end < len && steps[par_end].eff_index().as_usize() < exit_eff {
                    par_end += 1;
                }
                if par_end > par_start {
                    unsafe {
                        push_phase_range_to_image(
                            steps,
                            par_start,
                            par_end,
                            route_guards,
                            phase_headers,
                            phase_header_cap,
                            phase_lane_entries,
                            phase_lane_entry_cap,
                            phase_lane_words,
                            phase_lane_word_cap,
                            &mut phase_count,
                            &mut phase_lane_entry_len,
                            &mut phase_lane_word_len,
                        );
                    }
                }

                current_step = par_end;
                range_idx += 1;
            }

            if current_step < len {
                unsafe {
                    push_phase_range_to_image(
                        steps,
                        current_step,
                        len,
                        route_guards,
                        phase_headers,
                        phase_header_cap,
                        phase_lane_entries,
                        phase_lane_entry_cap,
                        phase_lane_words,
                        phase_lane_word_cap,
                        &mut phase_count,
                        &mut phase_lane_entry_len,
                        &mut phase_lane_word_len,
                    );
                }
            }

            if phase_count == 0 {
                unsafe {
                    push_phase_range_to_image(
                        steps,
                        0,
                        len,
                        route_guards,
                        phase_headers,
                        phase_header_cap,
                        phase_lane_entries,
                        phase_lane_entry_cap,
                        phase_lane_words,
                        phase_lane_word_cap,
                        &mut phase_count,
                        &mut phase_lane_entry_len,
                        &mut phase_lane_word_len,
                    );
                }
            }
        }
    }

    (phase_count, phase_lane_entry_len, phase_lane_word_len)
}

fn build_route_guards_for_steps_into(
    role: u8,
    len: usize,
    typestate: &RoleTypestateValue,
    step_index_to_state: &[StateIndex],
    route_guards: &mut [PhaseRouteGuard],
) {
    let mut idx = 0usize;
    while idx < route_guards.len() {
        route_guards[idx] = PhaseRouteGuard::EMPTY;
        idx += 1;
    }
    let mut step_idx = 0usize;
    while step_idx < len {
        let state = step_index_to_state[step_idx];
        if let Some((scope, arm)) =
            crate::global::typestate::phase_route_guard_for_state_for_role(typestate, role, state)
        {
            route_guards[step_idx] = PhaseRouteGuard::new(scope, arm);
        }
        step_idx += 1;
    }
}

unsafe fn push_phase_range_to_image(
    steps: &[LocalStep],
    start: usize,
    end: usize,
    route_guards: &[PhaseRouteGuard],
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
    phase_count: &mut usize,
    total_lane_entries: &mut usize,
    total_lane_words: &mut usize,
) {
    if *phase_count >= phase_header_cap {
        panic!("compiled role phase capacity exceeded");
    }
    let lane_entry_start = *total_lane_entries;
    let lane_word_start = *total_lane_words;
    let mut min_start = u16::MAX;
    let mut phase_lane_entry_len = 0usize;
    let mut max_lane_plus_one = 0usize;
    let mut step_idx = start;
    while step_idx < end {
        let lane = steps[step_idx].lane();
        let lane_plus_one = lane as usize + 1;
        if lane_plus_one > max_lane_plus_one {
            max_lane_plus_one = lane_plus_one;
        }
        let mut entry_idx = 0usize;
        let mut matched = false;
        while entry_idx < phase_lane_entry_len {
            let entry = unsafe { &mut *phase_lane_entries.add(lane_entry_start + entry_idx) };
            if entry.lane == lane {
                if entry.steps.len == u16::MAX {
                    panic!("phase lane length overflow");
                }
                entry.steps.len += 1;
                matched = true;
                break;
            }
            entry_idx += 1;
        }
        if !matched {
            if *total_lane_entries >= phase_lane_entry_cap {
                panic!("compiled role phase lane-entry capacity exceeded");
            }
            let lane_start = encode_compact_step_index(step_idx);
            unsafe {
                phase_lane_entries
                    .add(*total_lane_entries)
                    .write(PhaseLaneEntry {
                        lane,
                        steps: LaneSteps {
                            start: lane_start,
                            len: 1,
                        },
                    });
            }
            *total_lane_entries += 1;
            phase_lane_entry_len += 1;
            if lane_start < min_start {
                min_start = lane_start;
            }
        }
        step_idx += 1;
    }
    let phase_lane_word_len = lane_word_count(max_lane_plus_one);
    if phase_lane_entry_len > u16::MAX as usize {
        panic!("compiled role phase lane-entry count overflow");
    }
    if lane_entry_start > u16::MAX as usize {
        panic!("compiled role phase lane-entry offset overflow");
    }
    if phase_lane_word_len > u16::MAX as usize {
        panic!("compiled role phase lane-word count overflow");
    }
    if lane_word_start > u16::MAX as usize {
        panic!("compiled role phase lane-word offset overflow");
    }
    if lane_word_start.saturating_add(phase_lane_word_len) > phase_lane_word_cap {
        panic!("compiled role phase lane-word capacity exceeded");
    }
    let mut word_idx = 0usize;
    while word_idx < phase_lane_word_len {
        unsafe {
            phase_lane_words.add(lane_word_start + word_idx).write(0);
        }
        word_idx += 1;
    }
    let mut entry_idx = 0usize;
    while entry_idx < phase_lane_entry_len {
        let lane = unsafe { (*phase_lane_entries.add(lane_entry_start + entry_idx)).lane as usize };
        let word_bits = LaneWord::BITS as usize;
        let word_idx = lane / word_bits;
        let bit = 1usize << (lane % word_bits);
        unsafe {
            let slot = phase_lane_words.add(lane_word_start + word_idx);
            slot.write(slot.read() | bit);
        }
        entry_idx += 1;
    }
    unsafe {
        phase_headers.add(*phase_count).write(PhaseImageHeader {
            lane_entry_start: encode_compact_count_u16(lane_entry_start),
            lane_entry_len: encode_compact_count_u16(phase_lane_entry_len),
            lane_word_start: encode_compact_count_u16(lane_word_start),
            lane_word_len: encode_compact_count_u16(phase_lane_word_len),
            min_start: if phase_lane_entry_len == 0 {
                0
            } else {
                min_start
            },
            route_guard: route_guard_for_range(route_guards, start, end),
        });
    }
    *phase_count += 1;
    *total_lane_words += phase_lane_word_len;
}

fn route_guard_for_range(
    route_guards: &[PhaseRouteGuard],
    start: usize,
    end: usize,
) -> PhaseRouteGuard {
    if start >= end || start >= route_guards.len() {
        return PhaseRouteGuard::EMPTY;
    }
    let guard = route_guards[start];
    let mut idx = start + 1;
    while idx < end && idx < route_guards.len() {
        let candidate = route_guards[idx];
        if !guard.matches(candidate) {
            return PhaseRouteGuard::EMPTY;
        }
        idx += 1;
    }
    guard
}

unsafe fn initialize_phase_image_storage(
    phase_headers: *mut PhaseImageHeader,
    phase_header_cap: usize,
    phase_lane_entries: *mut PhaseLaneEntry,
    phase_lane_entry_cap: usize,
    phase_lane_words: *mut LaneWord,
    phase_lane_word_cap: usize,
) {
    let mut phase_idx = 0usize;
    while phase_idx < phase_header_cap {
        unsafe {
            phase_headers.add(phase_idx).write(PhaseImageHeader::EMPTY);
        }
        phase_idx += 1;
    }
    let mut lane_entry_idx = 0usize;
    while lane_entry_idx < phase_lane_entry_cap {
        unsafe {
            phase_lane_entries
                .add(lane_entry_idx)
                .write(PhaseLaneEntry::EMPTY);
        }
        lane_entry_idx += 1;
    }
    let mut lane_word_idx = 0usize;
    while lane_word_idx < phase_lane_word_cap {
        unsafe {
            phase_lane_words.add(lane_word_idx).write(0);
        }
        lane_word_idx += 1;
    }
}

#[inline(never)]
unsafe fn init_empty_compiled_role_image(dst: *mut CompiledRoleImage, role: u8) {
    unsafe { CompiledRoleImage::init_empty_compiled_role(dst, role) };
}

#[inline(never)]
unsafe fn finalize_compiled_role_image_from_typestate(
    dst: *mut CompiledRoleImage,
    scratch: &mut RoleLoweringScratch<'_>,
) {
    unsafe { CompiledRoleImage::finalize_compiled_role_from_typestate(dst, scratch) };
}

impl CompiledRoleImage {
    #[inline(always)]
    fn base_ptr(&self) -> *const u8 {
        (self as *const Self).cast::<u8>()
    }

    #[inline(always)]
    fn ptr_at<T>(&self, offset: u16) -> *const T {
        if offset == 0 {
            core::ptr::null()
        } else {
            unsafe { self.base_ptr().add(offset as usize).cast::<T>() }
        }
    }

    #[inline(always)]
    fn typestate_ptr(&self) -> *const RoleTypestateValue {
        self.ptr_at(self.typestate_offset)
    }

    #[inline(always)]
    fn phase_headers_ptr(&self) -> *const PhaseImageHeader {
        self.ptr_at(self.phase_headers_offset)
    }

    #[inline(always)]
    fn phase_lane_entries_ptr(&self) -> *const PhaseLaneEntry {
        self.ptr_at(self.phase_lane_entries_offset)
    }

    #[inline(always)]
    fn phase_lane_words_ptr(&self) -> *const LaneWord {
        self.ptr_at(self.phase_lane_words_offset)
    }

    #[inline(always)]
    fn eff_index_to_step_ptr(&self) -> *const u16 {
        self.ptr_at(self.eff_index_to_step_offset)
    }

    #[inline(always)]
    fn step_index_to_state_ptr(&self) -> *const StateIndex {
        self.ptr_at(self.step_index_to_state_offset)
    }

    #[inline(always)]
    unsafe fn write_offset(field: *mut u16, base: usize, ptr: usize) {
        unsafe {
            field.write(encode_compact_offset_u16(ptr.saturating_sub(base)));
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_counts(
        scope_count: usize,
        route_scope_count: usize,
        eff_count: usize,
    ) -> usize {
        CompiledRoleScopeStorage::total_bytes_for_counts(scope_count, route_scope_count, eff_count)
    }

    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_program(footprint: RoleFootprint) -> usize {
        CompiledRoleScopeStorage::total_bytes_for_layout(footprint)
    }

    #[inline(always)]
    pub(crate) const fn persistent_align() -> usize {
        CompiledRoleScopeStorage::overall_align()
    }

    #[inline(always)]
    pub(crate) fn actual_persistent_bytes(&self) -> usize {
        self.role_facts.persistent_bytes()
    }

    #[inline(never)]
    unsafe fn init_empty_compiled_role(dst: *mut Self, role: u8) {
        unsafe {
            ptr::addr_of_mut!((*dst).typestate_offset).write(0);
            ptr::addr_of_mut!((*dst).eff_index_to_step_offset).write(0);
            ptr::addr_of_mut!((*dst).phase_headers_offset).write(0);
            ptr::addr_of_mut!((*dst).phase_lane_entries_offset).write(0);
            ptr::addr_of_mut!((*dst).phase_lane_words_offset).write(0);
            ptr::addr_of_mut!((*dst).step_index_to_state_offset).write(0);
            ptr::addr_of_mut!((*dst).role).write(role);
            ptr::addr_of_mut!((*dst).role_facts).write(RoleResidentFacts::EMPTY);
        }
    }

    #[inline(never)]
    unsafe fn finalize_compiled_role_from_typestate(
        dst: *mut Self,
        scratch: &mut RoleLoweringScratch<'_>,
    ) {
        let role = unsafe { (*dst).role };
        let image = unsafe { &*dst };
        let typed_typestate = unsafe { &*image.typestate_ptr() };
        let (by_eff_index, present, steps, eff_index_to_step) =
            scratch.local_step_build_slices_mut();
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
    pub(crate) unsafe fn init_from_summary_for_program<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
        scratch: &mut RoleLoweringScratch<'_>,
        footprint: RoleFootprint,
    ) {
        unsafe { Self::init_from_summary_with_layout::<ROLE>(dst, summary, scratch, footprint) };
    }

    #[inline(never)]
    unsafe fn init_from_summary_with_layout<const ROLE: u8>(
        dst: *mut Self,
        summary: &LoweringSummary,
        scratch: &mut RoleLoweringScratch<'_>,
        footprint: RoleFootprint,
    ) {
        let init_empty =
            core::hint::black_box(init_empty_compiled_role_image as unsafe fn(*mut Self, u8));
        unsafe { init_empty(dst, ROLE) };
        let storage =
            unsafe { CompiledRoleScopeStorage::from_image_ptr_with_layout(dst, footprint) };
        unsafe {
            let image_base = dst.cast::<u8>() as usize;
            Self::write_offset(
                ptr::addr_of_mut!((*dst).typestate_offset),
                image_base,
                storage.typestate as usize,
            );
            Self::write_offset(
                ptr::addr_of_mut!((*dst).phase_headers_offset),
                image_base,
                storage.phase_headers as usize,
            );
            Self::write_offset(
                ptr::addr_of_mut!((*dst).phase_lane_entries_offset),
                image_base,
                storage.phase_lane_entries as usize,
            );
            Self::write_offset(
                ptr::addr_of_mut!((*dst).phase_lane_words_offset),
                image_base,
                storage.phase_lane_words as usize,
            );
            (*dst).role_facts.active_lane_count =
                encode_compact_count_u16(footprint.active_lane_count);
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
        unsafe {
            crate::global::typestate::init_value_from_summary_for_role(
                storage.typestate,
                storage.typestate_nodes,
                storage.typestate_node_cap,
                ROLE,
                core::slice::from_raw_parts_mut(storage.records, storage.scope_cap),
                storage.slots_by_scope,
                storage.route_dense_by_slot,
                storage.route_records,
                storage.route_offer_lane_words,
                storage.route_arm1_lane_words,
                footprint.logical_lane_word_count,
                storage.route_dispatch_shapes,
                storage.route_dispatch_shape_cap,
                storage.route_dispatch_entries,
                storage.route_dispatch_entry_cap,
                storage.route_dispatch_targets,
                storage.route_dispatch_target_cap,
                footprint.logical_lane_count,
                storage.scope_lane_first_eff,
                storage.scope_lane_last_eff,
                storage.route_arm0_lane_last_eff_by_slot,
                storage.route_scope_cap,
                summary,
                scratch.typestate_build_mut(),
            );
        }
        let compact_route_end = unsafe {
            compact_route_scope_tail(
                &storage,
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
            Self::write_offset(
                ptr::addr_of_mut!((*dst).eff_index_to_step_offset),
                image_base,
                eff_index_start,
            );
            Self::write_offset(
                ptr::addr_of_mut!((*dst).step_index_to_state_offset),
                image_base,
                step_index_start,
            );
        }
        let finalize = core::hint::black_box(finalize_compiled_role_image_from_typestate);
        unsafe { finalize(dst, scratch) };
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.role_facts.step_index_to_state_len()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn phase_count(&self) -> usize {
        self.phase_len()
    }

    #[inline(always)]
    fn phase_header(&self, idx: usize) -> Option<PhaseImageHeader> {
        if idx >= self.phase_len() {
            return None;
        }
        Some(unsafe { *self.phase_headers_ptr().add(idx) })
    }

    #[inline(always)]
    fn phase_lane_entries_for_header(&self, header: PhaseImageHeader) -> &[PhaseLaneEntry] {
        let start = header.lane_entry_start as usize;
        let len = header.lane_entry_len as usize;
        let total = self.role_facts.phase_lane_entry_len();
        if start > total || len > total.saturating_sub(start) {
            debug_assert!(false, "compiled role phase lane-entry bounds out of range");
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.phase_lane_entries_ptr().add(start), len) }
    }

    #[inline(always)]
    pub(crate) fn phase_lane_entries(&self, idx: usize) -> &[PhaseLaneEntry] {
        let Some(header) = self.phase_header(idx) else {
            return &[];
        };
        self.phase_lane_entries_for_header(header)
    }

    #[inline(always)]
    fn phase_lane_words_for_header(&self, header: PhaseImageHeader) -> LaneSetView {
        let start = header.lane_word_start as usize;
        let len = header.lane_word_len as usize;
        let total = self.role_facts.phase_lane_word_len();
        if start > total || len > total.saturating_sub(start) {
            debug_assert!(false, "compiled role phase lane-word bounds out of range");
            return LaneSetView::from_parts(core::ptr::null(), 0);
        }
        LaneSetView::from_parts(unsafe { self.phase_lane_words_ptr().add(start) }, len)
    }

    #[inline(always)]
    pub(crate) fn phase_lane_set(&self, idx: usize) -> Option<LaneSetView> {
        self.phase_header(idx)
            .map(|header| self.phase_lane_words_for_header(header))
    }

    #[inline(always)]
    pub(crate) fn phase_min_start(&self, idx: usize) -> Option<u16> {
        self.phase_header(idx).map(|header| header.min_start)
    }

    #[inline(always)]
    pub(crate) fn phase_route_guard(&self, idx: usize) -> Option<PhaseRouteGuard> {
        self.phase_header(idx).map(|header| header.route_guard)
    }

    #[inline(always)]
    pub(crate) fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        let header = self.phase_header(idx)?;
        let lane_entries = self.phase_lane_entries_for_header(header);
        let mut entry_idx = 0usize;
        while entry_idx < lane_entries.len() {
            let entry = lane_entries[entry_idx];
            if entry.lane as usize == lane_idx {
                return Some(entry.steps);
            }
            entry_idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn typestate_ref(&self) -> &RoleTypestateValue {
        debug_assert!(!self.typestate_ptr().is_null());
        unsafe { &*self.typestate_ptr() }
    }

    #[inline(always)]
    pub(crate) fn eff_index_to_step(&self) -> &[u16] {
        unsafe {
            core::slice::from_raw_parts(
                self.eff_index_to_step_ptr(),
                self.role_facts.eff_index_to_step_len(),
            )
        }
    }

    #[inline(always)]
    pub(crate) fn step_index_to_state(&self) -> &[StateIndex] {
        unsafe {
            core::slice::from_raw_parts(
                self.step_index_to_state_ptr(),
                self.role_facts.step_index_to_state_len(),
            )
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn is_active_lane(&self, lane_idx: usize) -> bool {
        self.has_active_lane(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        let mut phase_idx = 0usize;
        while phase_idx < self.phase_len() {
            let lane_entries = self.phase_lane_entries(phase_idx);
            let mut entry_idx = 0usize;
            while entry_idx < lane_entries.len() {
                if lane_entries[entry_idx].lane as usize == lane_idx {
                    return true;
                }
                entry_idx += 1;
            }
            phase_idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        let mut best = usize::MAX;
        let mut phase_idx = 0usize;
        while phase_idx < self.phase_len() {
            let lane_entries = self.phase_lane_entries(phase_idx);
            let mut entry_idx = 0usize;
            while entry_idx < lane_entries.len() {
                let lane = lane_entries[entry_idx].lane as usize;
                if lane < best {
                    best = lane;
                }
                entry_idx += 1;
            }
            phase_idx += 1;
        }
        if best == usize::MAX { None } else { Some(best) }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.typestate_ref().controller_arm_entry_by_arm(scope, arm)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn first_recv_dispatch_entry(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.typestate_ref().first_recv_dispatch_entry(scope, idx)
    }

    #[inline(always)]
    pub(crate) fn fill_active_lane_dense_by_lane(&self, dst: &mut [u8]) -> usize {
        Self::build_active_lane_dense_map_into(self, dst)
    }

    #[inline(always)]
    pub(crate) fn fill_logical_lane_dense_by_lane(&self, dst: &mut [u8]) -> usize {
        Self::build_logical_lane_dense_map_into(self.logical_lane_count(), dst)
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        logical_lane_count_for_role(self.active_lane_count(), self.endpoint_lane_slot_count())
    }

    #[inline(always)]
    pub(crate) fn logical_lane_word_count(&self) -> usize {
        lane_word_count(self.logical_lane_count())
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.role_facts.endpoint_lane_slot_count()
    }

    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.typestate_ref().max_route_stack_depth()
    }

    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.typestate_ref().max_loop_stack_depth()
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        let lane_slots = self.route_table_lane_slots();
        if lane_slots == 0 {
            return 0;
        }
        lane_slots.saturating_mul(self.max_route_stack_depth().max(1))
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        if self.typestate_ref().route_scope_count() == 0 {
            0
        } else {
            self.endpoint_lane_slot_count()
        }
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.max_loop_stack_depth()
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.active_lane_count().saturating_mul(4).max(4)
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.compiled_frontier_entry_capacity()
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.typestate_ref().route_scope_count()
    }

    #[inline(always)]
    pub(crate) fn scope_evidence_count(&self) -> usize {
        self.typestate_ref().route_scope_count()
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout_for_binding(
        &self,
        binding_enabled: bool,
    ) -> EndpointArenaLayout {
        EndpointArenaLayout::from_footprint_with_binding(
            self.endpoint_layout_footprint(),
            binding_enabled,
        )
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        FrontierScratchLayout::new(
            self.max_frontier_entries(),
            self.logical_lane_count(),
            self.logical_lane_word_count(),
        )
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn compiled_max_frontier_entries(&self) -> usize {
        self.compiled_frontier_entry_capacity()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn compiled_frontier_scratch_layout(&self) -> FrontierScratchLayout {
        FrontierScratchLayout::new(
            self.compiled_max_frontier_entries(),
            self.logical_lane_count(),
            self.logical_lane_word_count(),
        )
    }

    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.role_facts.active_lane_count()
    }

    #[inline(always)]
    pub(crate) fn endpoint_layout_footprint(&self) -> RoleFootprint {
        RoleFootprint::for_endpoint_layout(
            self.active_lane_count(),
            self.endpoint_lane_slot_count(),
            self.logical_lane_count(),
            self.max_route_stack_depth(),
            self.scope_evidence_count(),
            self.max_frontier_entries(),
        )
    }

    #[inline(always)]
    fn phase_len(&self) -> usize {
        self.role_facts.phase_len()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn phase_lane_entry_len(&self) -> usize {
        self.role_facts.phase_lane_entry_len()
    }

    #[inline(always)]
    fn compiled_frontier_entry_capacity(&self) -> usize {
        self.typestate_ref().frontier_entry_capacity()
    }

    fn build_active_lane_dense_map_into(image: &Self, dst: &mut [u8]) -> usize {
        dst.fill(u8::MAX);
        let mut phase_idx = 0usize;
        while phase_idx < image.phase_len() {
            if let Some(header) = image.phase_header(phase_idx) {
                let lane_entries = image.phase_lane_entries_for_header(header);
                let mut entry_idx = 0usize;
                while entry_idx < lane_entries.len() {
                    let lane = lane_entries[entry_idx].lane as usize;
                    if lane < dst.len() {
                        dst[lane] = 0;
                    }
                    entry_idx += 1;
                }
            }
            phase_idx += 1;
        }
        let mut lane_idx = 0usize;
        let mut dense = 0usize;
        while lane_idx < dst.len() {
            if dst[lane_idx] != u8::MAX {
                dst[lane_idx] = dense as u8;
                dense += 1;
            }
            lane_idx += 1;
        }
        dense
    }

    fn build_logical_lane_dense_map_into(logical_lane_count: usize, dst: &mut [u8]) -> usize {
        let mut lane_idx = 0usize;
        while lane_idx < dst.len() {
            dst[lane_idx] = if lane_idx < logical_lane_count {
                lane_idx as u8
            } else {
                u8::MAX
            };
            lane_idx += 1;
        }
        core::cmp::min(logical_lane_count, dst.len())
    }
}

#[cfg(test)]
mod tests {
    extern crate self as hibana;

    mod fanout_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/fanout_program.rs"
        ));
    }
    mod huge_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/huge_program.rs"
        ));
    }
    mod linear_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/linear_program.rs"
        ));
    }
    mod localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/localside.rs"
        ));
    }
    mod route_control_kinds {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_control_kinds.rs"
        ));
    }

    fn drive<F: core::future::Future>(future: F) -> F::Output {
        futures::executor::block_on(future)
    }

    use super::{CompiledRoleImage, LoweringSummary};
    use crate::{
        control::{
            cap::mint::{
                CAP_HANDLE_LEN, CapError, CapShot, CapsMask, ControlMint, ControlResourceKind,
                GenericCapToken, MintConfig, ResourceKind, SessionScopedKind,
            },
            cap::resource_kinds::RouteDecisionKind,
            types::{Lane, SessionId},
        },
        g::{self, Msg, Role},
        global::{
            CanonicalControl, ControlHandling, role_program,
            steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil},
            typestate::{JumpReason, LocalAction},
        },
        runtime::{config::CounterClock, consts::DefaultLabelUniverse},
        substrate::{
            Transport,
            transport::{Outgoing, TransportError, TransportEvent},
            wire::Payload,
        },
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RouteRightKind;

    impl ResourceKind for RouteRightKind {
        type Handle = (u8, u64);
        const TAG: u8 = 241;
        const NAME: &'static str = "RouteRightKind";
        const AUTO_MINT_EXTERNAL: bool = false;

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            let mut out = [0u8; CAP_HANDLE_LEN];
            out[0] = handle.0;
            out[1..9].copy_from_slice(&handle.1.to_le_bytes());
            out
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&data[1..9]);
            Ok((data[0], u64::from_le_bytes(raw)))
        }

        fn zeroize(handle: &mut Self::Handle) {
            handle.0 = 0;
            handle.1 = 0;
        }

        fn caps_mask(_handle: &Self::Handle) -> CapsMask {
            CapsMask::empty()
        }

        fn scope_id(handle: &Self::Handle) -> Option<crate::global::const_dsl::ScopeId> {
            Some(crate::global::const_dsl::ScopeId::from_raw(handle.1))
        }
    }

    impl SessionScopedKind for RouteRightKind {
        fn handle_for_session(_sid: SessionId, _lane: Lane) -> Self::Handle {
            (0, crate::global::const_dsl::ScopeId::none().raw())
        }

        fn shot() -> CapShot {
            CapShot::One
        }
    }

    impl ControlResourceKind for RouteRightKind {
        const LABEL: u8 = 99;
        const SCOPE: crate::global::const_dsl::ControlScopeKind =
            crate::global::const_dsl::ControlScopeKind::Route;
        const TAP_ID: u16 = 0x03ff;
        const SHOT: CapShot = CapShot::One;
        const HANDLING: ControlHandling = ControlHandling::Canonical;
    }

    impl ControlMint for RouteRightKind {
        fn mint_handle(
            _sid: SessionId,
            _lane: Lane,
            scope: crate::global::const_dsl::ScopeId,
        ) -> Self::Handle {
            (1, scope.raw())
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct DummyTransport;

    impl Transport for DummyTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Send<'a>
            = core::future::Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = core::future::Ready<Result<Payload<'a>, Self::Error>>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            core::future::ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            core::future::ready(Err(TransportError::Failed))
        }

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {}

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    fn retain_pico_smoke_fixture_symbols() {
        let _ = fanout_program::ROUTE_SCOPE_COUNT;
        let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = fanout_program::ACK_LABELS;
        let _ = huge_program::ROUTE_SCOPE_COUNT;
        let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = huge_program::ACK_LABELS;
        let _ = linear_program::ROUTE_SCOPE_COUNT;
        let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = linear_program::ACK_LABELS;
        let _ = huge_program::run::<DummyTransport, DefaultLabelUniverse, CounterClock, 2>
            as fn(
                &mut localside::ControllerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
                &mut localside::WorkerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
            );
        let _ = linear_program::run::<DummyTransport, DefaultLabelUniverse, CounterClock, 2>
            as fn(
                &mut localside::ControllerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
                &mut localside::WorkerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
            );
        let _ = fanout_program::run::<DummyTransport, DefaultLabelUniverse, CounterClock, 2>
            as fn(
                &mut localside::ControllerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
                &mut localside::WorkerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
            );
        let _ = localside::worker_offer_decode_u8::<
            0,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            2,
        >
            as fn(
                &mut localside::WorkerEndpoint<
                    '_,
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    2,
                >,
            ) -> u8;
    }

    #[test]
    fn pico_smoke_fixture_symbols_are_reachable() {
        retain_pico_smoke_fixture_symbols();
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CheckpointKind;

    impl ResourceKind for CheckpointKind {
        type Handle = ();
        const TAG: u8 = 242;
        const NAME: &'static str = "CheckpointKind";
        const AUTO_MINT_EXTERNAL: bool = false;

        fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            [0u8; CAP_HANDLE_LEN]
        }

        fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            Ok(())
        }

        fn zeroize(_handle: &mut Self::Handle) {}

        fn caps_mask(_handle: &Self::Handle) -> CapsMask {
            CapsMask::empty()
        }

        fn scope_id(_handle: &Self::Handle) -> Option<crate::global::const_dsl::ScopeId> {
            None
        }
    }

    impl SessionScopedKind for CheckpointKind {
        fn handle_for_session(_sid: SessionId, _lane: Lane) -> Self::Handle {}

        fn shot() -> CapShot {
            CapShot::One
        }
    }

    impl ControlResourceKind for CheckpointKind {
        const LABEL: u8 = 0x52;
        const SCOPE: crate::global::const_dsl::ControlScopeKind =
            crate::global::const_dsl::ControlScopeKind::Checkpoint;
        const TAP_ID: u16 = 0x0400;
        const SHOT: CapShot = CapShot::One;
        const HANDLING: ControlHandling = ControlHandling::Canonical;
    }

    impl ControlMint for CheckpointKind {
        fn mint_handle(
            _sid: SessionId,
            _lane: Lane,
            _scope: crate::global::const_dsl::ScopeId,
        ) -> Self::Handle {
        }
    }

    type SendOnly<const LANE: u8, S, D, M> = StepCons<SendStep<S, D, M, LANE>, StepNil>;
    type BranchSteps<L, R> = RouteSteps<L, R>;

    fn with_compiled_role_image<const ROLE: u8, R>(
        program: &role_program::RoleProgram<'_, ROLE, MintConfig>,
        f: impl FnOnce(&CompiledRoleImage) -> R,
    ) -> R {
        crate::global::compiled::with_compiled_role_image::<ROLE, _>(
            crate::global::lowering_input(program),
            f,
        )
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct TypestateNodeStats {
        node_count: usize,
        send_count: usize,
        recv_count: usize,
        local_count: usize,
        jump_count: usize,
        terminate_count: usize,
        route_arm_end_jumps: usize,
        loop_continue_jumps: usize,
        loop_break_jumps: usize,
        passive_observer_branch_jumps: usize,
    }

    fn typestate_node_stats(image: &CompiledRoleImage) -> TypestateNodeStats {
        let typestate = image.typestate_ref();
        let mut stats = TypestateNodeStats::default();
        let mut idx = 0usize;
        while idx < typestate.len() {
            let node = typestate.node(idx);
            stats.node_count += 1;
            match node.action() {
                LocalAction::Send { .. } => stats.send_count += 1,
                LocalAction::Recv { .. } => stats.recv_count += 1,
                LocalAction::Local { .. } => stats.local_count += 1,
                LocalAction::Terminate => stats.terminate_count += 1,
                LocalAction::Jump { reason } => {
                    stats.jump_count += 1;
                    match reason {
                        JumpReason::RouteArmEnd => stats.route_arm_end_jumps += 1,
                        JumpReason::LoopContinue => stats.loop_continue_jumps += 1,
                        JumpReason::LoopBreak => stats.loop_break_jumps += 1,
                        JumpReason::PassiveObserverBranch => {
                            stats.passive_observer_branch_jumps += 1;
                        }
                    }
                }
            }
            idx += 1;
        }
        stats
    }

    #[test]
    fn compiled_role_image_header_stays_compact() {
        assert!(
            core::mem::size_of::<super::CompiledRoleImage>() <= 32,
            "CompiledRoleImage header regressed back to pointer-rich layout: {} bytes",
            core::mem::size_of::<super::CompiledRoleImage>()
        );
    }

    #[test]
    fn compiled_role_exposes_controller_arm_and_dispatch_tables() {
        type LeftSteps = SeqSteps<
            SendOnly<
                0,
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            SendOnly<0, Role<0>, Role<1>, Msg<41, ()>>,
        >;
        type RightSteps = SeqSteps<
            SendOnly<
                0,
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            SendOnly<0, Role<0>, Role<1>, Msg<47, ()>>,
        >;
        type ProgramSteps = BranchSteps<LeftSteps, RightSteps>;

        const LEFT: g::Program<LeftSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<41, ()>, 0>(),
        );
        const RIGHT: g::Program<RightSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        const PROGRAM: g::Program<ProgramSteps> = g::route(LEFT, RIGHT);
        let program = PROGRAM;

        let controller: role_program::RoleProgram<'_, 0, MintConfig> =
            role_program::project(&program);
        with_compiled_role_image(&controller, |controller_compiled| {
            let controller_scope = controller_compiled.typestate_ref().node(0).scope();
            assert_eq!(controller_compiled.role(), 0);
            assert!(controller_compiled.is_active_lane(0));
            assert_eq!(
                controller_compiled
                    .controller_arm_entry_by_arm(controller_scope, 0)
                    .map(|(_, label)| label),
                Some(crate::runtime::consts::LABEL_ROUTE_DECISION)
            );
            assert_eq!(
                controller_compiled
                    .controller_arm_entry_by_arm(controller_scope, 1)
                    .map(|(_, label)| label),
                Some(99)
            );
            assert!(
                controller_compiled
                    .typestate_ref()
                    .controller_arm_entry_by_arm(controller_scope, 0)
                    .is_some(),
                "compiled role typestate must remain the single source of controller-arm facts"
            );
        });

        let worker: role_program::RoleProgram<'_, 1, MintConfig> = role_program::project(&program);
        with_compiled_role_image(&worker, |worker_compiled| {
            let worker_scope = worker_compiled.typestate_ref().node(0).scope();
            assert_eq!(worker_compiled.role(), 1);
            assert!(worker_compiled.is_active_lane(0));
            assert!(worker_compiled.phase_count() > 0);
            assert!(worker_compiled.local_len() > 0);
            assert_eq!(
                worker_compiled
                    .first_recv_dispatch_entry(worker_scope, 0)
                    .map(|(label, arm, _)| (label, arm)),
                Some((41, 0))
            );
            assert_eq!(
                worker_compiled
                    .first_recv_dispatch_entry(worker_scope, 1)
                    .map(|(label, arm, _)| (label, arm)),
                Some((47, 1))
            );
            assert!(
                worker_compiled
                    .typestate_ref()
                    .first_recv_dispatch_entry(worker_scope, 0)
                    .is_some(),
                "compiled role typestate must remain the single source of first-recv dispatch facts"
            );
            assert!(
                worker_compiled
                    .eff_index_to_step()
                    .iter()
                    .any(|&step_idx| step_idx != u16::MAX),
                "compiled role image must retain at least one eff-index mapping",
            );
            assert!(
                worker_compiled
                    .step_index_to_state()
                    .iter()
                    .any(|state| !state.is_max()),
                "compiled role image must retain at least one step-state mapping",
            );
        });
    }

    #[test]
    fn large_route_prefix_keeps_offer_and_frontier_bounds_local() {
        type Prefix01 = StepCons<SendStep<Role<0>, Role<1>, Msg<1, u8>, 0>, StepNil>;
        type Prefix02 = StepCons<SendStep<Role<1>, Role<0>, Msg<2, u8>, 0>, StepNil>;
        type Prefix03 = StepCons<SendStep<Role<0>, Role<1>, Msg<3, u8>, 0>, StepNil>;
        type Prefix04 = StepCons<SendStep<Role<1>, Role<0>, Msg<4, u8>, 0>, StepNil>;
        type Prefix05 = StepCons<SendStep<Role<0>, Role<1>, Msg<5, u8>, 0>, StepNil>;
        type Prefix06 = StepCons<SendStep<Role<1>, Role<0>, Msg<6, u8>, 0>, StepNil>;
        type Prefix07 = StepCons<SendStep<Role<0>, Role<1>, Msg<7, u8>, 0>, StepNil>;
        type Prefix08 = StepCons<SendStep<Role<1>, Role<0>, Msg<8, u8>, 0>, StepNil>;
        type Prefix09 = StepCons<SendStep<Role<0>, Role<1>, Msg<9, u8>, 0>, StepNil>;
        type Prefix10 = StepCons<SendStep<Role<1>, Role<0>, Msg<10, u8>, 0>, StepNil>;
        type Prefix11 = StepCons<SendStep<Role<0>, Role<1>, Msg<11, u8>, 0>, StepNil>;
        type Prefix12 = StepCons<SendStep<Role<1>, Role<0>, Msg<12, u8>, 0>, StepNil>;
        type Prefix13 = StepCons<SendStep<Role<0>, Role<1>, Msg<13, u8>, 0>, StepNil>;
        type Prefix14 = StepCons<SendStep<Role<1>, Role<0>, Msg<14, u8>, 0>, StepNil>;
        type Prefix15 = StepCons<SendStep<Role<0>, Role<1>, Msg<15, u8>, 0>, StepNil>;
        type Prefix16 = StepCons<SendStep<Role<1>, Role<0>, Msg<16, u8>, 0>, StepNil>;
        type PrefixSteps = SeqSteps<
            Prefix01,
            SeqSteps<
                Prefix02,
                SeqSteps<
                    Prefix03,
                    SeqSteps<
                        Prefix04,
                        SeqSteps<
                            Prefix05,
                            SeqSteps<
                                Prefix06,
                                SeqSteps<
                                    Prefix07,
                                    SeqSteps<
                                        Prefix08,
                                        SeqSteps<
                                            Prefix09,
                                            SeqSteps<
                                                Prefix10,
                                                SeqSteps<
                                                    Prefix11,
                                                    SeqSteps<
                                                        Prefix12,
                                                        SeqSteps<
                                                            Prefix13,
                                                            SeqSteps<
                                                                Prefix14,
                                                                SeqSteps<Prefix15, Prefix16>,
                                                            >,
                                                        >,
                                                    >,
                                                >,
                                            >,
                                        >,
                                    >,
                                >,
                            >,
                        >,
                    >,
                >,
            >,
        >;
        type LeftSteps = SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { crate::runtime::consts::LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                    0,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<41, ()>, 0>, StepNil>,
        >;
        type RightSteps = SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                    0,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<47, ()>, 0>, StepNil>,
        >;
        type ProgramSteps = SeqSteps<PrefixSteps, RouteSteps<LeftSteps, RightSteps>>;

        const PREFIX: crate::g::Program<PrefixSteps> = g::seq(
            g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>(),
            g::seq(
                g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>(),
                        g::seq(
                            g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>(),
                                g::seq(
                                    g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>(),
                                    g::seq(
                                        g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>(),
                                        g::seq(
                                            g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>(),
                                            g::seq(
                                                g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>(),
                                                g::seq(
                                                    g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>(),
                                                    g::seq(
                                                        g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>(
                                                        ),
                                                        g::seq(
                                                            g::send::<
                                                                Role<0>,
                                                                Role<1>,
                                                                Msg<13, u8>,
                                                                0,
                                                            >(
                                                            ),
                                                            g::seq(
                                                                g::send::<
                                                                    Role<1>,
                                                                    Role<0>,
                                                                    Msg<14, u8>,
                                                                    0,
                                                                >(
                                                                ),
                                                                g::seq(
                                                                    g::send::<
                                                                        Role<0>,
                                                                        Role<1>,
                                                                        Msg<15, u8>,
                                                                        0,
                                                                    >(
                                                                    ),
                                                                    g::send::<
                                                                        Role<1>,
                                                                        Role<0>,
                                                                        Msg<16, u8>,
                                                                        0,
                                                                    >(
                                                                    ),
                                                                ),
                                                            ),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        const LEFT: crate::g::Program<LeftSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<41, ()>, 0>(),
        );
        const RIGHT: crate::g::Program<RightSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        const PROGRAM: crate::g::Program<ProgramSteps> = g::seq(PREFIX, g::route(LEFT, RIGHT));
        let program = PROGRAM;

        let worker: role_program::RoleProgram<'_, 1, MintConfig> = role_program::project(&program);
        let lowering = crate::global::lowering_input(&worker);
        let summary = lowering.summary();
        assert!(
            CompiledRoleImage::persistent_bytes_for_program(lowering.footprint())
                < CompiledRoleImage::persistent_bytes_for_counts(
                    summary.stamp().scope_count(),
                    lowering.route_scope_count(),
                    lowering.eff_count(),
                ),
            "role image sizing should use the projected local step count instead of full eff_count"
        );
        with_compiled_role_image(&worker, |image| {
            assert!(
                image.local_len() >= 9,
                "large prefix should still project a substantial local program"
            );
            assert_eq!(
                image.route_scope_count(),
                1,
                "single trailing route should compile to one route scope"
            );
            assert_eq!(
                image.compiled_max_frontier_entries(),
                1,
                "frontier bound must stay tied to the active route frontier"
            );
            assert!(
                image.compiled_frontier_scratch_layout().total_bytes()
                    < image.local_len()
                        * core::mem::size_of::<crate::global::typestate::StateIndex>()
                        * 8,
                "frontier scratch must stay local to route metadata instead of scaling with the full local program"
            );
        });
    }

    fn assert_huge_shape_bounds<Steps>(
        program: &crate::g::Program<Steps>,
        expected_route_scope_count: usize,
        expected_frontier_entries: usize,
    ) where
        Steps: crate::global::program::BuildProgramSource
            + crate::g::advanced::steps::ProjectRole<crate::g::Role<1>>,
    {
        let worker: role_program::RoleProgram<'_, 1, MintConfig> = role_program::project(program);
        with_compiled_role_image(&worker, |image| {
            let active_lane_count = image.active_lane_count();
            let layout = image.endpoint_arena_layout_for_binding(true);
            let no_binding_layout = image.endpoint_arena_layout_for_binding(false);

            assert!(
                image.local_len() >= expected_route_scope_count,
                "huge choreography local length must dominate the route scope count"
            );
            assert_eq!(
                image.route_scope_count(),
                expected_route_scope_count,
                "route scope count must stay tied to the huge choreography shape"
            );
            assert_eq!(
                image.compiled_max_frontier_entries(),
                expected_frontier_entries,
                "frontier bound must stay tied to branch-local fan-out"
            );
            assert_eq!(
                image.max_frontier_entries(),
                image.compiled_max_frontier_entries(),
                "test-visible frontier capacity must match the compiled exact count"
            );
            assert!(
                image.compiled_max_frontier_entries() < image.local_len().max(1),
                "frontier bound must not grow with the full local prefix"
            );
            assert_eq!(
                layout.scope_evidence_slots().count(),
                image.scope_evidence_count(),
                "scope evidence storage must stay exact-bound to the compiled evidence count"
            );
            assert_eq!(
                layout.binding_slots().count(),
                image.logical_lane_count() * 8,
                "binding storage must stay exact-bound to the logical lane count"
            );
            assert_eq!(
                no_binding_layout.binding_slots().count(),
                0,
                "NoBinding layout must not reserve buffered binding slots"
            );
            assert_eq!(
                no_binding_layout.phase_cursor_lane_cursors().count(),
                image.logical_lane_count(),
                "NoBinding layout must still reserve phase cursor lane storage"
            );
            assert_eq!(
                no_binding_layout.route_state_lane_dense_by_lane().count(),
                image.logical_lane_count(),
                "NoBinding layout must still reserve route lane maps"
            );
            assert_eq!(
                no_binding_layout.binding_len().count(),
                0,
                "NoBinding layout must not reserve binding len storage"
            );
            assert_eq!(
                no_binding_layout.binding_label_masks().count(),
                0,
                "NoBinding layout must not reserve binding label masks"
            );
            assert!(
                no_binding_layout.total_bytes() < layout.total_bytes(),
                "NoBinding layout must stay smaller than the binding-capable layout"
            );
            assert_eq!(
                layout.route_arm_stack().count(),
                active_lane_count * image.max_route_stack_depth(),
                "route-arm stack must stay exact-bound to active lanes and route depth"
            );
            assert_eq!(
                layout.frontier_offer_entry_slots().count(),
                image.compiled_max_frontier_entries(),
                "offer entry storage must stay tied to the compiled simultaneous frontier bound"
            );
        });
    }

    fn count_parallel_enter_markers(summary: &LoweringSummary) -> usize {
        let markers = summary.view().scope_markers();
        let mut count = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, crate::global::const_dsl::ScopeEvent::Enter)
                && matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Parallel
                )
            {
                count += 1;
            }
            idx += 1;
        }
        count
    }

    #[test]
    fn huge_shape_phase_counts_stay_bounded_by_parallel_markers() {
        let route_program = huge_program::PROGRAM;
        let route_worker: role_program::RoleProgram<'_, 1, MintConfig> =
            role_program::project(&route_program);
        let route_lowering = crate::global::lowering_input(&route_worker);
        let route_summary = route_lowering.summary();
        let route_parallel_markers = count_parallel_enter_markers(route_summary);
        with_compiled_role_image(&route_worker, |image| {
            let phase_count = image.phase_len();
            let phase_lane_entry_len = image.phase_lane_entry_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                route_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "route-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=route_heavy local_len={} phase_count={} phase_lane_entry_len={} parallel_enter_markers={} phase_header_size={} phase_lane_entry_size={} lane_steps_size={} route_guard_size={}",
                local_len,
                phase_count,
                phase_lane_entry_len,
                route_parallel_markers,
                core::mem::size_of::<super::PhaseImageHeader>(),
                core::mem::size_of::<super::PhaseLaneEntry>(),
                core::mem::size_of::<crate::global::role_program::LaneSteps>(),
                core::mem::size_of::<crate::global::role_program::PhaseRouteGuard>(),
            );
        });

        let linear_program = linear_program::PROGRAM;
        let linear_worker: role_program::RoleProgram<'_, 1, MintConfig> =
            role_program::project(&linear_program);
        let linear_lowering = crate::global::lowering_input(&linear_worker);
        let linear_summary = linear_lowering.summary();
        let linear_parallel_markers = count_parallel_enter_markers(linear_summary);
        with_compiled_role_image(&linear_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                linear_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "linear-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=linear_heavy local_len={} phase_count={} parallel_enter_markers={}",
                local_len,
                phase_count,
                linear_parallel_markers,
            );
        });

        let fanout_program = fanout_program::PROGRAM;
        let fanout_worker: role_program::RoleProgram<'_, 1, MintConfig> =
            role_program::project(&fanout_program);
        let fanout_lowering = crate::global::lowering_input(&fanout_worker);
        let fanout_summary = fanout_lowering.summary();
        let fanout_parallel_markers = count_parallel_enter_markers(fanout_summary);
        with_compiled_role_image(&fanout_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                fanout_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "fanout-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=fanout_heavy local_len={} phase_count={} parallel_enter_markers={}",
                local_len,
                phase_count,
                fanout_parallel_markers,
            );
        });
    }

    #[test]
    fn nested_parallel_exact_facts_match_built_phase_image() {
        type Lane0 = SendOnly<0, Role<0>, Role<1>, Msg<1, ()>>;
        type Lane1 = SendOnly<1, Role<1>, Role<0>, Msg<2, ()>>;
        type Lane2 = SendOnly<2, Role<0>, Role<1>, Msg<3, ()>>;
        type InnerSteps = crate::global::steps::ParSteps<Lane0, Lane1>;
        type ProgramSteps = crate::global::steps::ParSteps<InnerSteps, Lane2>;

        const LANE0_PROGRAM: crate::global::Program<Lane0> =
            g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>();
        const LANE1_PROGRAM: crate::global::Program<Lane1> =
            g::send::<Role<1>, Role<0>, Msg<2, ()>, 1>();
        const LANE2_PROGRAM: crate::global::Program<Lane2> =
            g::send::<Role<0>, Role<1>, Msg<3, ()>, 2>();
        const INNER_PROGRAM: crate::global::Program<InnerSteps> =
            g::par(LANE0_PROGRAM, LANE1_PROGRAM);
        const PROGRAM: crate::global::Program<ProgramSteps> = g::par(INNER_PROGRAM, LANE2_PROGRAM);

        let worker: role_program::RoleProgram<'_, 0, MintConfig> = role_program::project(&PROGRAM);
        let lowering = crate::global::lowering_input(&worker);
        let counts = lowering.summary().role_lowering_counts::<0>();

        with_compiled_role_image(&worker, |image| {
            assert_eq!(counts.phase_count, image.phase_len());
            assert_eq!(counts.phase_lane_entry_count, image.phase_lane_entry_len());
            assert_eq!(
                counts.phase_lane_word_count,
                image.role_facts.phase_lane_word_len()
            );
        });
    }

    fn print_role_tail_breakdown<const ROLE: u8, Steps>(
        name: &str,
        program: &crate::g::Program<Steps>,
    ) where
        Steps: crate::global::program::BuildProgramSource
            + crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>,
        <Steps as crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>>::Output:
            crate::global::steps::StepCount,
    {
        let worker: role_program::RoleProgram<'_, ROLE, MintConfig> =
            role_program::project(program);
        let lowering = crate::global::lowering_input(&worker);
        let summary = lowering.summary();
        let scope_count = summary.stamp().scope_count();
        let eff_count = lowering.eff_count();
        let route_enter_count = summary
            .view()
            .scope_markers()
            .iter()
            .filter(|marker| {
                matches!(marker.event, crate::global::const_dsl::ScopeEvent::Enter)
                    && matches!(
                        marker.scope_kind,
                        crate::global::const_dsl::ScopeKind::Route
                    )
            })
            .count();
        let local_len = lowering.local_step_count();
        let phase_cap = super::CompiledRoleScopeStorage::phase_cap(lowering.footprint());
        let typestate_node_cap = super::CompiledRoleScopeStorage::typestate_node_cap(
            scope_count,
            lowering.passive_linger_route_scope_count(),
            local_len,
        );
        let scope_cap = super::CompiledRoleScopeStorage::scope_cap(scope_count);
        let route_scope_cap =
            super::CompiledRoleScopeStorage::route_scope_cap(lowering.route_scope_count());
        let eff_cap = super::CompiledRoleScopeStorage::step_cap(eff_count);
        let step_cap = super::CompiledRoleScopeStorage::step_cap(local_len);
        let route_stats = with_compiled_role_image(&worker, |image| {
            image.typestate_ref().route_scope_payload_stats()
        });
        let scope_stats =
            with_compiled_role_image(&worker, |image| image.typestate_ref().scope_payload_stats());
        let node_stats = with_compiled_role_image(&worker, typestate_node_stats);
        let actual_total_bytes =
            with_compiled_role_image(&worker, |image| image.actual_persistent_bytes());
        std::println!(
            "role-tail-breakdown name={name} scope_count={} eff_count={} local_len={} phase_cap={} typestate_node_cap={} built_node_len={} typestate_node_slack={} local_node_size={} local_action_size={} policy_mode_size={} scope_record_size={} route_scope_record_size={} state_index_size={} typestate_nodes_bytes={} phases_bytes={} records_bytes={} slots_bytes={} route_dense_bytes={} route_records_bytes={} route_recv_bytes={} eff_index_bytes={} step_index_bytes={} total_bytes={} send_nodes={} recv_nodes={} local_nodes={} jump_nodes={} terminate_nodes={} route_arm_end_jumps={} loop_continue_jumps={} loop_break_jumps={} passive_observer_branch_jumps={} total_lane_first_entries={} max_lane_first_entries={} total_lane_last_entries={} max_lane_last_entries={} total_arm_entries={} max_arm_entries={} total_passive_arm_scopes={} max_passive_arm_scopes={} route_scope_count={} route_enter_count={} total_first_recv_entries={} max_first_recv_entries={} total_arm_lane_last_entries={} max_arm_lane_last_entries={} total_arm_lane_last_override_entries={} max_arm_lane_last_override_entries={} total_offer_lane_entries={} max_offer_lane_entries={}",
            scope_count,
            eff_count,
            local_len,
            phase_cap,
            typestate_node_cap,
            node_stats.node_count,
            typestate_node_cap.saturating_sub(node_stats.node_count),
            core::mem::size_of::<crate::global::typestate::LocalNode>(),
            crate::global::typestate::LocalNode::packed_action_size(),
            core::mem::size_of::<crate::global::const_dsl::PolicyMode>(),
            core::mem::size_of::<crate::global::typestate::ScopeRecord>(),
            core::mem::size_of::<crate::global::typestate::RouteScopeRecord>(),
            core::mem::size_of::<crate::global::typestate::StateIndex>(),
            typestate_node_cap * core::mem::size_of::<crate::global::typestate::LocalNode>(),
            phase_cap * core::mem::size_of::<super::PhaseImageHeader>()
                + step_cap * core::mem::size_of::<super::PhaseLaneEntry>(),
            scope_cap * core::mem::size_of::<crate::global::typestate::ScopeRecord>(),
            scope_cap * core::mem::size_of::<u16>(),
            scope_cap * core::mem::size_of::<u16>(),
            route_scope_cap * core::mem::size_of::<crate::global::typestate::RouteScopeRecord>(),
            0usize,
            eff_cap * core::mem::size_of::<u16>(),
            step_cap * core::mem::size_of::<crate::global::typestate::StateIndex>(),
            actual_total_bytes,
            node_stats.send_count,
            node_stats.recv_count,
            node_stats.local_count,
            node_stats.jump_count,
            node_stats.terminate_count,
            node_stats.route_arm_end_jumps,
            node_stats.loop_continue_jumps,
            node_stats.loop_break_jumps,
            node_stats.passive_observer_branch_jumps,
            scope_stats.total_lane_first_entries,
            scope_stats.max_lane_first_entries,
            scope_stats.total_lane_last_entries,
            scope_stats.max_lane_last_entries,
            scope_stats.total_arm_entries,
            scope_stats.max_arm_entries,
            scope_stats.total_passive_arm_scopes,
            scope_stats.max_passive_arm_scopes,
            route_stats.route_scope_count,
            route_enter_count,
            route_stats.total_first_recv_entries,
            route_stats.max_first_recv_entries,
            route_stats.total_arm_lane_last_entries,
            route_stats.max_arm_lane_last_entries,
            route_stats.total_arm_lane_last_override_entries,
            route_stats.max_arm_lane_last_override_entries,
            route_stats.total_offer_lane_entries,
            route_stats.max_offer_lane_entries,
        );
    }

    #[test]
    fn huge_shape_role_image_tail_breakdown_is_reported() {
        let route_program = huge_program::PROGRAM;
        print_role_tail_breakdown::<1, huge_program::ProgramSteps>("route_heavy", &route_program);

        let linear_program = linear_program::PROGRAM;
        print_role_tail_breakdown::<1, linear_program::ProgramSteps>(
            "linear_heavy",
            &linear_program,
        );

        let fanout_program = fanout_program::PROGRAM;
        print_role_tail_breakdown::<1, fanout_program::ProgramSteps>(
            "fanout_heavy",
            &fanout_program,
        );
    }

    #[test]
    fn offer_regression_role_tail_breakdown_is_reported() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type SessionRequestWireMsg = Msg<0x10, u8>;
        type AdminReplyMsg = Msg<0x50, u8>;
        type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
        type CheckpointMsg = Msg<
            { CheckpointKind::LABEL },
            GenericCapToken<CheckpointKind>,
            CanonicalControl<CheckpointKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { crate::runtime::consts::LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>;
        type ReplyDecisionLeftSteps = SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
        >;
        type SnapshotReplyPathSteps = SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >,
        >;
        type ReplyDecisionRightSteps =
            SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, SnapshotReplyPathSteps>;
        type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
        type RequestExchangeSteps =
            SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
        type ContinueArmSteps =
            SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
        type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
        type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

        const REPLY_DECISION: g::Program<ReplyDecisionSteps> = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                            g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                        ),
                    ),
                ),
            ),
        );
        const REQUEST_EXCHANGE: g::Program<RequestExchangeSteps> = g::seq(
            g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
            REPLY_DECISION,
        );
        const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                REQUEST_EXCHANGE,
            ),
            g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
        );
        let program = LOOP_PROGRAM;

        print_role_tail_breakdown::<0, LoopProgramSteps>("offer_admin_snapshot_client", &program);
        print_role_tail_breakdown::<1, LoopProgramSteps>("offer_admin_snapshot_server", &program);
    }

    #[test]
    fn huge_route_heavy_shape_keeps_resident_bounds_local() {
        let program = huge_program::PROGRAM;
        assert_huge_shape_bounds(&program, huge_program::ROUTE_SCOPE_COUNT, 1);
    }

    #[test]
    fn huge_linear_heavy_shape_keeps_resident_bounds_local() {
        let program = linear_program::PROGRAM;
        assert_huge_shape_bounds(&program, linear_program::ROUTE_SCOPE_COUNT, 0);
    }

    #[test]
    fn huge_fanout_heavy_shape_keeps_resident_bounds_local() {
        let program = fanout_program::PROGRAM;
        assert_huge_shape_bounds(&program, fanout_program::ROUTE_SCOPE_COUNT, 1);
    }
}
