use crate::{
    eff,
    g::ProgramSourceError,
    global::{
        compiled::lowering::CompiledProgramImage,
        const_dsl::{EffList, ScopeEvent, ScopeKind, ScopeMarker},
        role_program::{LaneWord, lane_word_count, logical_lane_count_for_role},
    },
};

mod first_recv_dispatch;
mod passive_child;

pub(crate) struct ProjectionSeal<const ROLE: u8>;

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

#[inline(always)]
const fn accumulate_resident_row_range_facts(
    local_effs: &[usize; eff::meta::MAX_EFF_NODES],
    local_lanes: &[u8; eff::meta::MAX_EFF_NODES],
    local_len: usize,
    start_eff: usize,
    end_eff: usize,
    resident_row_count: &mut usize,
    resident_row_lane_entry_count: &mut usize,
    resident_row_lane_word_count: &mut usize,
) {
    if start_eff >= end_eff {
        return;
    }
    let mut seen_lanes = [0usize; LANE_FACT_WORDS];
    let mut resident_row_max_lane_plus_one = 0usize;
    let mut any = false;
    let mut distinct_lane_count = 0usize;
    let mut idx = 0usize;
    while idx < local_len {
        let eff_idx = local_effs[idx];
        if eff_idx >= start_eff && eff_idx < end_eff {
            any = true;
            let lane = local_lanes[idx] as usize;
            let lane_plus_one = lane.saturating_add(1);
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
    *resident_row_count += 1;
    *resident_row_lane_entry_count += distinct_lane_count;
    *resident_row_lane_word_count += lane_word_count(resident_row_max_lane_plus_one);
    if *resident_row_count > u16::MAX as usize
        || *resident_row_lane_entry_count > u16::MAX as usize
        || *resident_row_lane_word_count > u16::MAX as usize
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
                let lane_slot_count = lane.saturating_add(1);
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
        if matches!(marker.scope_kind, ScopeKind::Parallel)
            && matches!(marker.event, ScopeEvent::Enter)
        {
            let mut exit_offset = usize::MAX;
            let mut exit_idx = marker_idx + 1;
            while exit_idx < scope_markers.len() {
                let exit_marker = scope_markers[exit_idx];
                if matches!(exit_marker.scope_kind, ScopeKind::Parallel)
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

    let mut resident_row_count = 0usize;
    let mut resident_row_lane_entry_count = 0usize;
    let mut resident_row_lane_word_count = 0usize;
    let mut current_eff = 0usize;
    let mut range_idx = 0usize;
    while range_idx < range_len {
        let (enter_eff, exit_eff) = ranges[range_idx];
        accumulate_resident_row_range_facts(
            &local_effs,
            &local_lanes,
            local_len,
            current_eff,
            enter_eff,
            &mut resident_row_count,
            &mut resident_row_lane_entry_count,
            &mut resident_row_lane_word_count,
        );
        let parallel_start = if enter_eff > current_eff {
            enter_eff
        } else {
            current_eff
        };
        accumulate_resident_row_range_facts(
            &local_effs,
            &local_lanes,
            local_len,
            parallel_start,
            exit_eff,
            &mut resident_row_count,
            &mut resident_row_lane_entry_count,
            &mut resident_row_lane_word_count,
        );
        current_eff = if exit_eff > current_eff {
            exit_eff
        } else {
            current_eff
        };
        range_idx += 1;
    }
    accumulate_resident_row_range_facts(
        &local_effs,
        &local_lanes,
        local_len,
        current_eff,
        eff::meta::MAX_EFF_NODES,
        &mut resident_row_count,
        &mut resident_row_lane_entry_count,
        &mut resident_row_lane_word_count,
    );
    if resident_row_count == 0 {
        accumulate_resident_row_range_facts(
            &local_effs,
            &local_lanes,
            local_len,
            0,
            eff::meta::MAX_EFF_NODES,
            &mut resident_row_count,
            &mut resident_row_lane_entry_count,
            &mut resident_row_lane_word_count,
        );
    }

    ExactRoleResidentRowFacts {
        resident_row_count: encode_u16_count(resident_row_count),
        resident_row_lane_entry_count: encode_u16_count(resident_row_lane_entry_count),
        resident_row_lane_word_count: encode_u16_count(resident_row_lane_word_count),
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

impl<const ROLE: u8> ProjectionSeal<ROLE> {
    #[inline(always)]
    const fn validate_compiled_layout(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
    ) -> Option<ProgramSourceError> {
        Self::validate_resident_row_capacity(view, eff_list);
        if let Some(error) =
            first_recv_dispatch::validate_first_recv_dispatch_capacity::<ROLE>(view, eff_list)
        {
            return Some(error);
        }
        Self::validate_route_projection_guarantees(view, eff_list)
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
                && matches!(marker.scope_kind, ScopeKind::Route | ScopeKind::Loop)
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
    const fn validate_resident_row_capacity(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
    ) {
        let _ = exact_resident_row_count_for_role(eff_list, view.scope_markers(), ROLE);
    }

    #[inline(always)]
    const fn validate_route_projection_guarantees(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
    ) -> Option<ProgramSourceError> {
        let scope_markers = view.scope_markers();
        let mut seen_routes = [false; eff::meta::MAX_EFF_NODES];
        let mut marker_idx = 0usize;
        while marker_idx < scope_markers.len() {
            let marker = scope_markers[marker_idx];
            if matches!(marker.scope_kind, ScopeKind::Route)
                && matches!(marker.event, ScopeEvent::Enter)
            {
                let ordinal = marker.scope_id.local_ordinal() as usize;
                if ordinal >= seen_routes.len() {
                    return Some(ProgramSourceError::ProjectionRouteUnprojectable);
                }
                if !seen_routes[ordinal] {
                    seen_routes[ordinal] = true;
                    if let Some(error) = Self::validate_route_scope(
                        view,
                        eff_list,
                        scope_markers,
                        marker_idx,
                        marker.controller_role,
                        marker.linger,
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
    const fn validate_route_scope(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
        scope_markers: &[crate::global::const_dsl::ScopeMarker],
        route_enter_marker_idx: usize,
        controller_role: Option<u8>,
        linger: bool,
    ) -> Option<ProgramSourceError> {
        let (
            arm0_enter_marker_idx,
            arm0_start,
            arm0_end,
            arm1_enter_marker_idx,
            arm1_start,
            arm1_end,
        ) = Self::route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
        if let Some(error) = Self::validate_decision_policy_consistency(
            view,
            eff_list,
            arm0_enter_marker_idx,
            arm0_end,
            arm1_enter_marker_idx,
            arm1_end,
        ) {
            return Some(error);
        }
        if linger {
            return None;
        }

        if matches!(controller_role, Some(role) if role == ROLE) {
            return None;
        }

        let mut left = [LocalSig::EMPTY; eff::meta::MAX_EFF_NODES];
        let mut right = [LocalSig::EMPTY; eff::meta::MAX_EFF_NODES];
        let left_len = Self::collect_local_sigs(eff_list, arm0_start, arm0_end, &mut left);
        let right_len = Self::collect_local_sigs(eff_list, arm1_start, arm1_end, &mut right);

        if left_len == 0 && right_len == 0 {
            return None;
        }
        if left_len == 0 || right_len == 0 {
            return None;
        }
        if Self::local_sequences_equal(&left, left_len, &right, right_len) {
            return None;
        }
        if Self::dispatchable_after_shared_prefix(&left, left_len, &right, right_len) {
            return None;
        }
        if Self::scope_has_dynamic_policy(
            view,
            eff_list,
            arm0_enter_marker_idx,
            arm0_end,
            arm1_enter_marker_idx,
            arm1_end,
        ) {
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
            if marker.scope_id.canonical().raw() == scope_id.canonical().raw()
                && matches!(marker.scope_kind, ScopeKind::Route)
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

    const fn scope_has_dynamic_policy(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
        arm0_enter_marker_idx: usize,
        arm0_end: usize,
        arm1_enter_marker_idx: usize,
        arm1_end: usize,
    ) -> bool {
        Self::first_route_head_decision_policy_id_in_range(
            view,
            eff_list,
            arm0_enter_marker_idx,
            arm0_end,
        )
        .is_some()
            || Self::first_route_head_decision_policy_id_in_range(
                view,
                eff_list,
                arm1_enter_marker_idx,
                arm1_end,
            )
            .is_some()
    }

    const fn validate_decision_policy_consistency(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
        arm0_enter_marker_idx: usize,
        arm0_end: usize,
        arm1_enter_marker_idx: usize,
        arm1_end: usize,
    ) -> Option<ProgramSourceError> {
        let left = Self::first_route_head_decision_policy_id_in_range(
            view,
            eff_list,
            arm0_enter_marker_idx,
            arm0_end,
        );
        let right = Self::first_route_head_decision_policy_id_in_range(
            view,
            eff_list,
            arm1_enter_marker_idx,
            arm1_end,
        );
        match (left, right) {
            (Some(left_id), Some(right_id)) => {
                if left_id != right_id {
                    return Some(ProgramSourceError::ProjectionRoutePolicyMismatch);
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                return Some(ProgramSourceError::ProjectionRoutePolicyMissing);
            }
            (None, None) => {}
        }
        None
    }

    const fn first_route_head_decision_policy_id_in_range(
        view: &super::CompiledProgramView<'_>,
        eff_list: &EffList,
        route_enter_marker_idx: usize,
        _end: usize,
    ) -> Option<u16> {
        let scope_markers = view.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            return None;
        }
        let route_enter = scope_markers[route_enter_marker_idx];
        if !matches!(route_enter.event, ScopeEvent::Enter)
            || !matches!(route_enter.scope_kind, ScopeKind::Route)
        {
            return None;
        }
        let mut marker_idx = route_enter_marker_idx + 1;
        let mut nested_non_policy_enter = false;
        while marker_idx < scope_markers.len() {
            let marker = scope_markers[marker_idx];
            if marker.offset != route_enter.offset {
                break;
            }
            if matches!(marker.event, ScopeEvent::Enter)
                && !matches!(marker.scope_kind, ScopeKind::Generic)
            {
                nested_non_policy_enter = true;
            }
            marker_idx += 1;
        }
        if nested_non_policy_enter {
            return None;
        }
        if let Some((policy, _scope)) = eff_list.policy_with_scope(route_enter.offset)
            && policy.dynamic_policy_id().is_some()
        {
            return policy.dynamic_policy_id();
        }
        None
    }

    const fn collect_local_sigs(
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
    ProjectionSeal::<0>::validate_scope_capacity(&view);
    if let Some(error) = passive_child::validate_passive_child_projection_guarantees(&view) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<0>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<1>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<2>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<3>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<4>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<5>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<6>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<7>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<8>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<9>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<10>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<11>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<12>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<13>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<14>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    if let Some(error) = ProjectionSeal::<15>::validate_compiled_layout(&view, eff_list) {
        return Some(error);
    }
    None
}

#[cfg(all(test, hibana_repo_tests))]
#[inline(always)]
pub(crate) const fn validate_all_roles(summary: &CompiledProgramImage, eff_list: &EffList) {
    if let Some(error) = projection_error_all_roles(summary, eff_list) {
        error.panic_repo_test();
    }
}
