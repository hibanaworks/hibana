use crate::{
    eff,
    g::ProgramSourceError,
    global::{
        compiled::lowering::CompiledProgramImage,
        const_dsl::{EffList, ReentryMark, RouteResolver, ScopeEvent, ScopeKind, ScopeMarker},
        role_program::{LaneWord, lane_word_count, logical_lane_count_for_role},
    },
};

mod first_recv_dispatch;
mod passive_child;

const LANE_FACT_WORDS: usize = lane_word_count(u8::MAX as usize + 1);

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

#[derive(Clone, Copy)]
struct ResidentRowLocalFacts<'a> {
    effs: &'a [usize; eff::meta::MAX_EFF_NODES],
    lanes: &'a [u8; eff::meta::MAX_EFF_NODES],
    len: usize,
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

#[inline(always)]
const fn accumulate_resident_row_range_facts(
    local: ResidentRowLocalFacts<'_>,
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
    let mut idx = 0usize;
    while idx < local.len {
        let eff_idx = local.effs[idx];
        if eff_idx >= range.start_eff && eff_idx < range.end_eff {
            any = true;
            let lane = local.lanes[idx] as usize;
            let lane_plus_one = lane + 1;
            if lane_plus_one > resident_row_max_lane_plus_one {
                resident_row_max_lane_plus_one = lane_plus_one;
            }
            if insert_lane(&mut seen_lanes, lane) {
                distinct_lane_count += 1;
            }
        }
        idx += 1;
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

pub(super) const fn exact_role_resident_row_facts(
    eff_list: &EffList,
    scope_markers: &[ScopeMarker],
    role: u8,
) -> ExactRoleResidentRowFacts {
    let mut local_effs = [0usize; eff::meta::MAX_EFF_NODES];
    let mut local_lanes = [0u8; eff::meta::MAX_EFF_NODES];
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
                if local_len >= eff::meta::MAX_EFF_NODES {
                    panic!("compiled role local step capacity exceeded");
                }
                local_effs[local_len] = idx;
                local_lanes[local_len] = atom.lane;
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

    let mut ranges = [(usize::MAX, usize::MAX); eff::meta::MAX_EFF_NODES];
    let mut range_len = 0usize;
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            && matches!(marker.event, ScopeEvent::Enter)
        {
            let mut exit_offset = usize::MAX;
            let mut exit_idx = marker_idx + 1;
            while exit_idx < scope_markers.len() {
                let exit_marker = scope_markers[exit_idx];
                if matches!(exit_marker.scope_id.kind(), Some(ScopeKind::Parallel))
                    && matches!(exit_marker.event, ScopeEvent::Exit)
                    && exit_marker.scope_id.raw() == marker.scope_id.raw()
                {
                    exit_offset = exit_marker.offset;
                    break;
                }
                exit_idx += 1;
            }
            if exit_offset == usize::MAX {
                panic!("parallel scope exit missing");
            }
            if range_len >= eff::meta::MAX_EFF_NODES {
                panic!("compiled role resident-row capacity exceeded");
            }
            ranges[range_len] = (marker.offset, exit_offset);
            range_len += 1;
        }
        marker_idx += 1;
    }

    let local = ResidentRowLocalFacts {
        effs: &local_effs,
        lanes: &local_lanes,
        len: local_len,
    };
    let mut totals = ResidentRowFactTotals {
        row_count: 0,
        lane_entry_count: 0,
        lane_word_count: 0,
    };
    let mut current_eff = 0usize;
    let mut range_idx = 0usize;
    while range_idx < range_len {
        let (enter_eff, exit_eff) = ranges[range_idx];
        accumulate_resident_row_range_facts(
            local,
            ResidentRowRange {
                start_eff: current_eff,
                end_eff: enter_eff,
            },
            &mut totals,
        );
        let parallel_start = if enter_eff > current_eff {
            enter_eff
        } else {
            current_eff
        };
        accumulate_resident_row_range_facts(
            local,
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
        range_idx += 1;
    }
    accumulate_resident_row_range_facts(
        local,
        ResidentRowRange {
            start_eff: current_eff,
            end_eff: eff::meta::MAX_EFF_NODES,
        },
        &mut totals,
    );
    if totals.row_count == 0 {
        accumulate_resident_row_range_facts(
            local,
            ResidentRowRange {
                start_eff: 0,
                end_eff: eff::meta::MAX_EFF_NODES,
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

pub(super) const fn exact_resident_row_count_for_role(
    eff_list: &EffList,
    scope_markers: &[ScopeMarker],
    role: u8,
) -> u16 {
    exact_role_resident_row_facts(eff_list, scope_markers, role).resident_row_count
}

#[derive(Clone, Copy)]
struct LocalSig {
    kind: u8,
    peer: u8,
    label: u8,
    lane: u8,
}

impl LocalSig {
    const SEND: u8 = 0;
    const RECV: u8 = 1;
    const LOCAL: u8 = 2;

    const EMPTY: Self = Self {
        kind: u8::MAX,
        peer: 0,
        label: 0,
        lane: 0,
    };
}

#[inline(always)]
const fn validate_compiled_layout<const ROLE: u8>(
    view: &super::CompiledProgramView<'_>,
    eff_list: &EffList,
) -> Option<ProgramSourceError> {
    validate_resident_row_capacity::<ROLE>(view, eff_list);
    if let Some(error) =
        first_recv_dispatch::validate_first_recv_dispatch_capacity::<ROLE>(view, eff_list)
    {
        return Some(error);
    }
    validate_route_projection_guarantees::<ROLE>(view, eff_list)
}

#[inline(always)]
const fn validate_scope_capacity(view: &super::CompiledProgramView<'_>) {
    let scope_markers = view.scope_markers();
    let mut seen_route_like = [false; eff::meta::MAX_EFF_NODES];
    let mut seen_route_like_len = 0usize;
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
            if ordinal >= seen_route_like.len() {
                panic!("controller arm table capacity exceeded");
            }
            if seen_route_like[ordinal] {
                idx += 1;
                continue;
            }
            if seen_route_like_len >= seen_route_like.len() {
                panic!("controller arm table capacity exceeded");
            }
            seen_route_like[ordinal] = true;
            seen_route_like_len += 1;
        }
        idx += 1;
    }
}

#[inline(always)]
const fn validate_resident_row_capacity<const ROLE: u8>(
    view: &super::CompiledProgramView<'_>,
    eff_list: &EffList,
) {
    exact_resident_row_count_for_role(eff_list, view.scope_markers(), ROLE);
}

#[inline(always)]
const fn validate_route_projection_guarantees<const ROLE: u8>(
    view: &super::CompiledProgramView<'_>,
    eff_list: &EffList,
) -> Option<ProgramSourceError> {
    let scope_markers = view.scope_markers();
    let mut seen_routes = [false; eff::meta::MAX_EFF_NODES];
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && matches!(marker.event, ScopeEvent::Enter)
        {
            let ordinal = marker.scope_id.local_ordinal() as usize;
            if ordinal >= seen_routes.len() {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            if !seen_routes[ordinal] {
                seen_routes[ordinal] = true;
                if let Some(error) = validate_route_scope::<ROLE>(
                    view,
                    eff_list,
                    scope_markers,
                    marker_idx,
                    marker.reentry,
                ) {
                    return Some(error);
                }
            }
        }
        marker_idx += 1;
    }
    None
}

#[inline(always)]
const fn validate_route_scope<const ROLE: u8>(
    view: &super::CompiledProgramView<'_>,
    eff_list: &EffList,
    scope_markers: &[crate::global::const_dsl::ScopeMarker],
    route_enter_marker_idx: usize,
    reentry: ReentryMark,
) -> Option<ProgramSourceError> {
    let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
        route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
    let route_scope = scope_markers[route_enter_marker_idx].scope_id;
    let Some(frontier) = view.route_frontier_summary(route_scope) else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if frontier.is_invalid() || frontier.has_duplicate_label() {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let has_dynamic_resolver = scope_has_dynamic_resolver(view, route_scope);
    let controller_mask = frontier.controller_mask();
    if !has_dynamic_resolver {
        if frontier.has_branch_label_overlap() {
            return Some(ProgramSourceError::ProjectionRouteUnprojectable);
        }
        if !has_exactly_one_bit(controller_mask) {
            return Some(ProgramSourceError::RouteControllerMismatch);
        }
    }
    if reentry.is_reentrant() {
        return None;
    }

    if matches!(unique_controller_role(controller_mask), Some(role) if role == ROLE) {
        return None;
    }

    let mut left = [LocalSig::EMPTY; eff::meta::MAX_EFF_NODES];
    let mut right = [LocalSig::EMPTY; eff::meta::MAX_EFF_NODES];
    let left_len = collect_local_sigs::<ROLE>(eff_list, arm0_start, arm0_end, &mut left);
    let right_len = collect_local_sigs::<ROLE>(eff_list, arm1_start, arm1_end, &mut right);

    if left_len == 0 && right_len == 0 {
        return None;
    }
    if left_len == 0 || right_len == 0 {
        return None;
    }
    if local_sequences_equal(&left, left_len, &right, right_len) {
        return None;
    }
    if dispatchable_after_shared_prefix(&left, left_len, &right, right_len) {
        return None;
    }
    if has_dynamic_resolver {
        return None;
    }
    Some(ProgramSourceError::ProjectionRouteUnprojectable)
}

#[inline(always)]
const fn route_arm_ranges_from_first_enter(
    scope_markers: &[crate::global::const_dsl::ScopeMarker],
    enter_idx: usize,
) -> (usize, usize, usize, usize, usize, usize) {
    if enter_idx >= scope_markers.len() {
        panic!("route enter marker index out of bounds");
    }
    let scope_id = scope_markers[enter_idx].scope_id;
    let mut enter_marker_indices = [usize::MAX; 2];
    let mut enter_offsets = [usize::MAX; 2];
    let mut exit_offsets = [usize::MAX; 2];
    let mut enter_len = 1usize;
    let mut exit_len = 0usize;
    enter_marker_indices[0] = enter_idx;
    enter_offsets[0] = scope_markers[enter_idx].offset;
    let mut idx = enter_idx + 1;
    while idx < scope_markers.len() && (enter_len < 2 || exit_len < 2) {
        let marker = scope_markers[idx];
        if marker.scope_id.same(scope_id)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            match marker.event {
                ScopeEvent::Enter => {
                    if enter_len < 2 {
                        enter_marker_indices[enter_len] = idx;
                        enter_offsets[enter_len] = marker.offset;
                    }
                    enter_len += 1;
                }
                ScopeEvent::Exit => {
                    if exit_len < 2 {
                        exit_offsets[exit_len] = marker.offset;
                    }
                    exit_len += 1;
                }
            }
        }
        idx += 1;
    }

    if enter_len != 2 || exit_len != 2 {
        panic!("route must have exactly 2 arms");
    }
    (
        enter_marker_indices[0],
        enter_offsets[0],
        exit_offsets[0],
        enter_marker_indices[1],
        enter_offsets[1],
        exit_offsets[1],
    )
}

const fn scope_has_dynamic_resolver(
    view: &super::CompiledProgramView<'_>,
    route_scope: crate::global::const_dsl::ScopeId,
) -> bool {
    match view.resolver_for_scope(route_scope) {
        Some(RouteResolver::Dynamic { .. }) => true,
        Some(RouteResolver::Intrinsic) | None => false,
    }
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

const fn collect_local_sigs<const ROLE: u8>(
    eff_list: &EffList,
    start: usize,
    end: usize,
    out: &mut [LocalSig; eff::meta::MAX_EFF_NODES],
) -> usize {
    let mut len = 0usize;
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        let node = eff_list.node_at(idx);
        if matches!(node.kind, eff::EffKind::Atom) {
            let atom = node.atom_data();
            let sig = if atom.from == ROLE && atom.to == ROLE {
                Some(LocalSig {
                    kind: LocalSig::LOCAL,
                    peer: ROLE,
                    label: atom.label,
                    lane: atom.lane,
                })
            } else if atom.from == ROLE {
                Some(LocalSig {
                    kind: LocalSig::SEND,
                    peer: atom.to,
                    label: atom.label,
                    lane: atom.lane,
                })
            } else if atom.to == ROLE {
                Some(LocalSig {
                    kind: LocalSig::RECV,
                    peer: atom.from,
                    label: atom.label,
                    lane: atom.lane,
                })
            } else {
                None
            };
            if let Some(sig) = sig {
                if len >= eff::meta::MAX_EFF_NODES {
                    panic!("projection local signature capacity exceeded");
                }
                out[len] = sig;
                len += 1;
            }
        }
        idx += 1;
    }
    len
}

const fn local_sequences_equal(
    left: &[LocalSig; eff::meta::MAX_EFF_NODES],
    left_len: usize,
    right: &[LocalSig; eff::meta::MAX_EFF_NODES],
    right_len: usize,
) -> bool {
    if left_len != right_len {
        return false;
    }
    let mut idx = 0usize;
    while idx < left_len {
        let lhs = left[idx];
        let rhs = right[idx];
        if lhs.kind != rhs.kind
            || lhs.peer != rhs.peer
            || lhs.label != rhs.label
            || lhs.lane != rhs.lane
        {
            return false;
        }
        idx += 1;
    }
    true
}

const fn dispatchable_after_shared_prefix(
    left: &[LocalSig; eff::meta::MAX_EFF_NODES],
    left_len: usize,
    right: &[LocalSig; eff::meta::MAX_EFF_NODES],
    right_len: usize,
) -> bool {
    let mut prefix = 0usize;
    while prefix < left_len && prefix < right_len {
        let lhs = left[prefix];
        let rhs = right[prefix];
        if lhs.kind != rhs.kind
            || lhs.peer != rhs.peer
            || lhs.label != rhs.label
            || lhs.lane != rhs.lane
        {
            break;
        }
        prefix += 1;
    }
    if prefix >= left_len || prefix >= right_len {
        return false;
    }
    let lhs = left[prefix];
    let rhs = right[prefix];
    lhs.kind == LocalSig::RECV
        && rhs.kind == LocalSig::RECV
        && (lhs.label != rhs.label || lhs.lane != rhs.lane || lhs.peer != rhs.peer)
}
#[inline(always)]
pub(crate) const fn projection_error_all_roles(
    summary: &CompiledProgramImage,
    eff_list: &EffList,
) -> Option<ProgramSourceError> {
    if let Some(error) = validate_route_stack_depth(summary) {
        return Some(error);
    }
    let view = summary.view();
    validate_scope_capacity(&view);
    if let Some(error) = passive_child::validate_passive_child_projection_guarantees(&view) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<0>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<1>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<2>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<3>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<4>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<5>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<6>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<7>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<8>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<9>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<10>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<11>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<12>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<13>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<14>(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = validate_compiled_layout::<15>(&view, eff_list) {
        return Some(error);
    }
    None
}
