use crate::{
    eff,
    g::ProgramSourceError,
    global::{
        compiled::lowering::CompiledProgramImage,
        const_dsl::{
            EffList, ReentryMark, ScopeEvent, ScopeKind, ScopeMarker,
            first_visible_controller_mask, first_visible_endpoint_selector_conflicts_from_markers,
            local_route_observer_paths_mergeable, route_arm_ranges_from_first_enter,
            validate_parallel_endpoint_selectors, validate_roll_reentry_endpoint_selectors,
        },
        role_program::{LaneWord, lane_word_count, logical_lane_count_for_role},
    },
};

mod passive_child;

const LANE_FACT_WORDS: usize = lane_word_count(u8::MAX as usize + 1);
const SCOPE_ORDINAL_BYTES: usize = eff::meta::MAX_EFF_NODES.div_ceil(8);

#[inline(always)]
const fn validate_route_stack_depth(summary: &CompiledProgramImage) -> Option<ProgramSourceError> {
    if summary.max_route_stack_depth_for_projection() > u8::MAX as usize {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    None
}

#[derive(Clone, Copy)]
pub(super) struct ExactRoleResidentRowFacts {
    pub(super) resident_row_count: u16,
    pub(super) resident_row_lane_entry_count: u16,
    pub(super) resident_row_lane_word_count: u16,
    pub(super) active_lane_count: u16,
    pub(super) endpoint_lane_slot_count: u16,
    pub(super) logical_lane_count: u16,
}

#[inline(always)]
const fn lane_word_parts(lane: usize) -> (usize, LaneWord) {
    let bits = LaneWord::BITS as usize;
    (lane / bits, 1usize << (lane % bits))
}

#[inline(always)]
const fn encode_u16_count(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role count overflow");
    }
    value as u16
}

#[inline(always)]
const fn insert_lane(words: &mut [LaneWord; LANE_FACT_WORDS], lane: usize) -> bool {
    let (word_idx, bit) = lane_word_parts(lane);
    let seen = (words[word_idx] & bit) != 0;
    if !seen {
        words[word_idx] |= bit;
    }
    !seen
}

#[inline(always)]
const fn insert_scope_ordinal(words: &mut [u8; SCOPE_ORDINAL_BYTES], ordinal: usize) -> bool {
    let byte = ordinal >> 3;
    let bit = ordinal & 7;
    if byte >= words.len() {
        panic!("scope ordinal table capacity exceeded");
    }
    let mask = 1u8 << bit;
    let seen = (words[byte] & mask) != 0;
    if !seen {
        words[byte] |= mask;
    }
    !seen
}

#[derive(Clone, Copy)]
struct ResidentRowRange {
    start_eff: usize,
    end_eff: usize,
}

struct ResidentRowFactTotals {
    row_count: usize,
    lane_entry_count: usize,
    lane_word_count: usize,
}

const fn accumulate_resident_row_range_facts(
    eff_list: &EffList,
    role: u8,
    range: ResidentRowRange,
    totals: &mut ResidentRowFactTotals,
) {
    if range.start_eff >= range.end_eff {
        return;
    }
    let mut seen_lanes = [0usize; LANE_FACT_WORDS];
    let mut resident_row_max_lane_plus_one = 0usize;
    let mut any = false;
    let mut distinct_lane_count = 0usize;
    let mut eff_idx = range.start_eff;
    while eff_idx < range.end_eff && eff_idx < eff_list.len() {
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.atom_data();
            if atom.from == role || atom.to == role {
                any = true;
                let lane = atom.lane as usize;
                let lane_plus_one = lane + 1;
                if lane_plus_one > resident_row_max_lane_plus_one {
                    resident_row_max_lane_plus_one = lane_plus_one;
                }
                if insert_lane(&mut seen_lanes, lane) {
                    distinct_lane_count += 1;
                }
            }
        }
        eff_idx += 1;
    }
    if !any {
        return;
    }
    totals.row_count += 1;
    totals.lane_entry_count += distinct_lane_count;
    totals.lane_word_count += lane_word_count(resident_row_max_lane_plus_one);
    if totals.row_count > u16::MAX as usize
        || totals.lane_entry_count > u16::MAX as usize
        || totals.lane_word_count > u16::MAX as usize
    {
        panic!("compiled role resident-row capacity exceeded");
    }
}

const fn parallel_exit_offset(
    scope_markers: &[ScopeMarker],
    enter_marker_idx: usize,
) -> Option<usize> {
    let marker = scope_markers[enter_marker_idx];
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
    {
        return None;
    }
    let mut exit_idx = enter_marker_idx + 1;
    while exit_idx < scope_markers.len() {
        let exit_marker = scope_markers[exit_idx];
        if matches!(exit_marker.scope_id.kind(), Some(ScopeKind::Parallel))
            && matches!(exit_marker.event, ScopeEvent::Exit)
            && exit_marker.scope_id.raw() == marker.scope_id.raw()
        {
            return Some(exit_marker.offset());
        }
        exit_idx += 1;
    }
    None
}

pub(super) const fn exact_role_resident_row_facts(
    eff_list: &EffList,
    scope_markers: &[ScopeMarker],
    role: u8,
) -> ExactRoleResidentRowFacts {
    let mut active_lanes = [0usize; LANE_FACT_WORDS];
    let mut local_len = 0usize;
    let mut active_lane_count = 0usize;
    let mut endpoint_lane_slot_count = 0usize;
    let mut idx = 0usize;
    while idx < eff_list.len() {
        let node = eff_list.node_at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.atom_data();
            if atom.from == role || atom.to == role {
                local_len += 1;
                let lane = atom.lane as usize;
                let lane_slot_count = lane + 1;
                if lane_slot_count > endpoint_lane_slot_count {
                    endpoint_lane_slot_count = lane_slot_count;
                }
                if insert_lane(&mut active_lanes, lane) {
                    active_lane_count += 1;
                }
            }
        }
        idx += 1;
    }

    if endpoint_lane_slot_count == 0 {
        endpoint_lane_slot_count = 1;
    }
    let logical_lane_count =
        logical_lane_count_for_role(active_lane_count, endpoint_lane_slot_count);

    if local_len == 0 {
        return ExactRoleResidentRowFacts {
            resident_row_count: 0,
            resident_row_lane_entry_count: 0,
            resident_row_lane_word_count: 0,
            active_lane_count: encode_u16_count(active_lane_count),
            endpoint_lane_slot_count: encode_u16_count(endpoint_lane_slot_count),
            logical_lane_count: encode_u16_count(logical_lane_count),
        };
    }

    let mut totals = ResidentRowFactTotals {
        row_count: 0,
        lane_entry_count: 0,
        lane_word_count: 0,
    };
    let mut current_eff = 0usize;
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            && matches!(marker.event, ScopeEvent::Enter)
        {
            let Some(exit_eff) = parallel_exit_offset(scope_markers, marker_idx) else {
                panic!("parallel scope exit missing");
            };
            accumulate_resident_row_range_facts(
                eff_list,
                role,
                ResidentRowRange {
                    start_eff: current_eff,
                    end_eff: marker.offset(),
                },
                &mut totals,
            );
            let parallel_start = if marker.offset() > current_eff {
                marker.offset()
            } else {
                current_eff
            };
            accumulate_resident_row_range_facts(
                eff_list,
                role,
                ResidentRowRange {
                    start_eff: parallel_start,
                    end_eff: exit_eff,
                },
                &mut totals,
            );
            current_eff = if exit_eff > current_eff {
                exit_eff
            } else {
                current_eff
            };
        }
        marker_idx += 1;
    }
    accumulate_resident_row_range_facts(
        eff_list,
        role,
        ResidentRowRange {
            start_eff: current_eff,
            end_eff: eff_list.len(),
        },
        &mut totals,
    );
    if totals.row_count == 0 {
        accumulate_resident_row_range_facts(
            eff_list,
            role,
            ResidentRowRange {
                start_eff: 0,
                end_eff: eff_list.len(),
            },
            &mut totals,
        );
    }

    ExactRoleResidentRowFacts {
        resident_row_count: encode_u16_count(totals.row_count),
        resident_row_lane_entry_count: encode_u16_count(totals.lane_entry_count),
        resident_row_lane_word_count: encode_u16_count(totals.lane_word_count),
        active_lane_count: encode_u16_count(active_lane_count),
        endpoint_lane_slot_count: encode_u16_count(endpoint_lane_slot_count),
        logical_lane_count: encode_u16_count(logical_lane_count),
    }
}

#[inline(always)]
const fn validate_compiled_layout(role: u8, eff_list: &EffList) -> Option<ProgramSourceError> {
    validate_route_projection_guarantees(role, eff_list)
}

const fn validate_scope_capacity(eff_list: &EffList) {
    let scope_markers = eff_list.scope_markers();
    let mut seen_route_like = [0u8; SCOPE_ORDINAL_BYTES];
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers[idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(
                marker.scope_id.kind(),
                Some(ScopeKind::Route) | Some(ScopeKind::Roll)
            )
        {
            let ordinal = marker.scope_id.local_ordinal() as usize;
            if !insert_scope_ordinal(&mut seen_route_like, ordinal) {
                idx += 1;
                continue;
            }
        }
        idx += 1;
    }
}

const fn validate_route_projection_guarantees(
    role: u8,
    eff_list: &EffList,
) -> Option<ProgramSourceError> {
    let scope_markers = eff_list.scope_markers();
    let mut seen_routes = [0u8; SCOPE_ORDINAL_BYTES];
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && matches!(marker.event, ScopeEvent::Enter)
        {
            let ordinal = marker.scope_id.local_ordinal() as usize;
            if ordinal >= eff::meta::MAX_EFF_NODES {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            if insert_scope_ordinal(&mut seen_routes, ordinal)
                && let Some(error) =
                    validate_route_scope(role, eff_list, scope_markers, marker_idx, marker.reentry)
            {
                return Some(error);
            }
        }
        marker_idx += 1;
    }
    None
}

const fn validate_route_scope(
    role: u8,
    eff_list: &EffList,
    scope_markers: &[crate::global::const_dsl::ScopeMarker],
    route_enter_marker_idx: usize,
    reentry: ReentryMark,
) -> Option<ProgramSourceError> {
    let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
        route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
    let route_scope = scope_markers[route_enter_marker_idx].scope_id;
    let has_dynamic_resolver = scope_has_dynamic_resolver(eff_list, route_scope);
    let controller_mask = first_visible_controller_mask(eff_list, arm0_start, arm0_end)
        | first_visible_controller_mask(eff_list, arm1_start, arm1_end);
    if !has_dynamic_resolver {
        if first_visible_endpoint_selector_conflicts_from_markers(
            eff_list,
            arm0_start,
            arm0_end,
            arm1_start,
            arm1_end,
            route_enter_marker_idx + 1,
            route_enter_marker_idx + 1,
        ) {
            return Some(ProgramSourceError::ProjectionRouteUnprojectable);
        }
        if !has_exactly_one_bit(controller_mask) {
            return Some(ProgramSourceError::RouteControllerMismatch);
        }
    }
    if reentry.is_reentrant() {
        return None;
    }

    if matches!(unique_controller_role(controller_mask), Some(controller) if controller == role) {
        return None;
    }

    if local_route_observer_paths_mergeable(
        eff_list, arm0_start, arm0_end, arm1_start, arm1_end, role,
    ) {
        return None;
    }
    if has_dynamic_resolver {
        return None;
    }
    Some(ProgramSourceError::ProjectionRouteUnprojectable)
}

#[inline(always)]
const fn scope_has_dynamic_resolver(
    eff_list: &EffList,
    route_scope: crate::global::const_dsl::ScopeId,
) -> bool {
    eff_list.resolver_for_scope(route_scope).is_some()
}

const fn unique_controller_role(mask: u16) -> Option<u8> {
    if !has_exactly_one_bit(mask) {
        return None;
    }
    let mut role = 0u8;
    while role < u16::BITS as u8 {
        if (mask & (1u16 << role)) != 0 {
            return Some(role);
        }
        role += 1;
    }
    None
}

#[inline(always)]
const fn has_exactly_one_bit(mask: u16) -> bool {
    mask != 0 && (mask & (mask - 1)) == 0
}

pub(crate) const fn projection_error_all_roles(
    summary: &CompiledProgramImage,
    eff_list: &EffList,
) -> Option<ProgramSourceError> {
    if let Some(error) = validate_route_stack_depth(summary) {
        return Some(error);
    }
    validate_scope_capacity(eff_list);
    if !validate_parallel_endpoint_selectors(eff_list) {
        return Some(ProgramSourceError::ParallelAmbiguousEndpointSelector);
    }
    if !validate_roll_reentry_endpoint_selectors(eff_list) {
        return Some(ProgramSourceError::ReentryAmbiguousEndpointSelector);
    }
    if let Some(error) =
        passive_child::validate_passive_child_projection_guarantees(eff_list.scope_markers())
    {
        return Some(error);
    }
    let mut role = 0u8;
    while role < crate::g::ROLE_DOMAIN_SIZE {
        if let Some(error) = validate_compiled_layout(role, eff_list) {
            return Some(error);
        }
        role += 1;
    }
    None
}
