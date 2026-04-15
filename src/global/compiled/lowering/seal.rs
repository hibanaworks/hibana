use crate::{
    eff::{self, EffKind},
    global::{
        compiled::LoweringSummary,
        const_dsl::{ScopeEvent, ScopeId, ScopeKind},
        role_program::{LaneWord, lane_word_count, logical_lane_count_for_role},
    },
};

pub(crate) struct ProjectionSeal<const ROLE: u8>;

const LANE_FACT_WORDS: usize = lane_word_count(u8::MAX as usize + 1);

#[derive(Clone, Copy)]
pub(super) struct ExactRolePhaseFacts {
    pub(super) phase_count: u16,
    pub(super) phase_lane_entry_count: u16,
    pub(super) phase_lane_word_count: u16,
    pub(super) active_lane_count: u16,
    pub(super) endpoint_lane_slot_count: u16,
    pub(super) logical_lane_count: u16,
    pub(super) logical_lane_word_count: u16,
}

#[inline(always)]
const fn lane_word_parts(lane: usize) -> (usize, LaneWord) {
    let bits = LaneWord::BITS as usize;
    (lane / bits, 1usize << (lane % bits))
}

#[inline(always)]
const fn encode_u16_count(value: usize, label: &str) -> u16 {
    if value > u16::MAX as usize {
        panic!("{}", label);
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
const fn accumulate_phase_range_facts(
    local_effs: &[usize; eff::meta::MAX_EFF_NODES],
    local_lanes: &[u8; eff::meta::MAX_EFF_NODES],
    local_len: usize,
    start_eff: usize,
    end_eff: usize,
    phase_count: &mut usize,
    phase_lane_entry_count: &mut usize,
    phase_lane_word_count: &mut usize,
) {
    if start_eff >= end_eff {
        return;
    }
    let mut seen_lanes = [0usize; LANE_FACT_WORDS];
    let mut phase_max_lane_plus_one = 0usize;
    let mut any = false;
    let mut distinct_lane_count = 0usize;
    let mut idx = 0usize;
    while idx < local_len {
        let eff_idx = local_effs[idx];
        if eff_idx >= start_eff && eff_idx < end_eff {
            any = true;
            let lane = local_lanes[idx] as usize;
            let lane_plus_one = lane.saturating_add(1);
            if lane_plus_one > phase_max_lane_plus_one {
                phase_max_lane_plus_one = lane_plus_one;
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
    *phase_count += 1;
    *phase_lane_entry_count += distinct_lane_count;
    *phase_lane_word_count += lane_word_count(phase_max_lane_plus_one);
    if *phase_count > u16::MAX as usize
        || *phase_lane_entry_count > u16::MAX as usize
        || *phase_lane_word_count > u16::MAX as usize
    {
        panic!("compiled role phase capacity exceeded");
    }
}

pub(super) const fn exact_role_phase_facts(
    view: crate::global::compiled::LoweringView<'_>,
    role: u8,
) -> ExactRolePhaseFacts {
    let nodes = view.as_slice();
    let mut local_effs = [0usize; eff::meta::MAX_EFF_NODES];
    let mut local_lanes = [0u8; eff::meta::MAX_EFF_NODES];
    let mut active_lanes = [0usize; LANE_FACT_WORDS];
    let mut local_len = 0usize;
    let mut active_lane_count = 0usize;
    let mut endpoint_lane_slot_count = 0usize;
    let mut idx = 0usize;
    while idx < nodes.len() {
        let node = nodes[idx];
        if matches!(node.kind, EffKind::Atom) {
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
    let logical_lane_word_count = lane_word_count(logical_lane_count);

    if local_len == 0 {
        return ExactRolePhaseFacts {
            phase_count: 0,
            phase_lane_entry_count: 0,
            phase_lane_word_count: 0,
            active_lane_count: encode_u16_count(
                active_lane_count,
                "compiled role active lane count overflow",
            ),
            endpoint_lane_slot_count: encode_u16_count(
                endpoint_lane_slot_count,
                "compiled role endpoint lane slot count overflow",
            ),
            logical_lane_count: encode_u16_count(
                logical_lane_count,
                "compiled role logical lane count overflow",
            ),
            logical_lane_word_count: encode_u16_count(
                logical_lane_word_count,
                "compiled role logical lane word count overflow",
            ),
        };
    }

    let scope_markers = view.scope_markers();
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
                panic!("compiled role phase capacity exceeded");
            }
            ranges[range_len] = (marker.offset, exit_offset);
            range_len += 1;
        }
        marker_idx += 1;
    }

    let mut phase_count = 0usize;
    let mut phase_lane_entry_count = 0usize;
    let mut phase_lane_word_count = 0usize;
    let mut current_eff = 0usize;
    let mut range_idx = 0usize;
    while range_idx < range_len {
        let (enter_eff, exit_eff) = ranges[range_idx];
        accumulate_phase_range_facts(
            &local_effs,
            &local_lanes,
            local_len,
            current_eff,
            enter_eff,
            &mut phase_count,
            &mut phase_lane_entry_count,
            &mut phase_lane_word_count,
        );
        let parallel_start = if enter_eff > current_eff {
            enter_eff
        } else {
            current_eff
        };
        accumulate_phase_range_facts(
            &local_effs,
            &local_lanes,
            local_len,
            parallel_start,
            exit_eff,
            &mut phase_count,
            &mut phase_lane_entry_count,
            &mut phase_lane_word_count,
        );
        current_eff = if exit_eff > current_eff {
            exit_eff
        } else {
            current_eff
        };
        range_idx += 1;
    }
    accumulate_phase_range_facts(
        &local_effs,
        &local_lanes,
        local_len,
        current_eff,
        eff::meta::MAX_EFF_NODES,
        &mut phase_count,
        &mut phase_lane_entry_count,
        &mut phase_lane_word_count,
    );
    if phase_count == 0 {
        accumulate_phase_range_facts(
            &local_effs,
            &local_lanes,
            local_len,
            0,
            eff::meta::MAX_EFF_NODES,
            &mut phase_count,
            &mut phase_lane_entry_count,
            &mut phase_lane_word_count,
        );
    }

    ExactRolePhaseFacts {
        phase_count: encode_u16_count(phase_count, "compiled role phase capacity exceeded"),
        phase_lane_entry_count: encode_u16_count(
            phase_lane_entry_count,
            "compiled role phase lane-entry capacity exceeded",
        ),
        phase_lane_word_count: encode_u16_count(
            phase_lane_word_count,
            "compiled role phase lane-word capacity exceeded",
        ),
        active_lane_count: encode_u16_count(
            active_lane_count,
            "compiled role active lane count overflow",
        ),
        endpoint_lane_slot_count: encode_u16_count(
            endpoint_lane_slot_count,
            "compiled role endpoint lane slot count overflow",
        ),
        logical_lane_count: encode_u16_count(
            logical_lane_count,
            "compiled role logical lane count overflow",
        ),
        logical_lane_word_count: encode_u16_count(
            logical_lane_word_count,
            "compiled role logical lane word count overflow",
        ),
    }
}

pub(super) const fn exact_phase_count_for_role(
    view: crate::global::compiled::LoweringView<'_>,
    role: u8,
) -> u16 {
    exact_role_phase_facts(view, role).phase_count
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
    const fn validate_compiled_layout(view: crate::global::compiled::LoweringView<'_>) {
        Self::validate_phase_capacity(view);
        Self::validate_scope_capacity(view);
        Self::validate_route_projection_guarantees(view);
    }

    const fn validate_scope_capacity(view: crate::global::compiled::LoweringView<'_>) {
        let scope_markers = view.scope_markers();
        let mut seen_route_like = [ScopeId::none(); eff::meta::MAX_EFF_NODES];
        let mut seen_route_like_len = 0usize;
        let mut idx = 0usize;
        while idx < scope_markers.len() {
            let marker = scope_markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route | ScopeKind::Loop)
                && !Self::contains_scope(&seen_route_like, seen_route_like_len, marker.scope_id)
            {
                if seen_route_like_len >= seen_route_like.len() {
                    panic!("controller arm table capacity exceeded");
                }
                seen_route_like[seen_route_like_len] = marker.scope_id;
                seen_route_like_len += 1;
            }
            idx += 1;
        }
    }

    const fn validate_phase_capacity(view: crate::global::compiled::LoweringView<'_>) {
        let _ = exact_phase_count_for_role(view, ROLE);
    }

    const fn contains_scope(
        scopes: &[ScopeId; eff::meta::MAX_EFF_NODES],
        len: usize,
        scope: ScopeId,
    ) -> bool {
        let mut idx = 0usize;
        while idx < len {
            if scopes[idx].raw() == scope.raw() {
                return true;
            }
            idx += 1;
        }
        false
    }

    const fn validate_route_projection_guarantees(view: crate::global::compiled::LoweringView<'_>) {
        let scope_markers = view.scope_markers();
        let mut marker_idx = 0usize;
        while marker_idx < scope_markers.len() {
            let marker = scope_markers[marker_idx];
            if matches!(marker.scope_kind, ScopeKind::Route)
                && matches!(marker.event, ScopeEvent::Enter)
            {
                Self::validate_route_scope(
                    view,
                    scope_markers,
                    marker.scope_id,
                    marker.controller_role,
                    marker.linger,
                );
            }
            marker_idx += 1;
        }
    }

    const fn validate_route_scope(
        view: crate::global::compiled::LoweringView<'_>,
        scope_markers: &[crate::global::const_dsl::ScopeMarker],
        scope_id: ScopeId,
        controller_role: Option<u8>,
        linger: bool,
    ) {
        let (
            arm0_enter_marker_idx,
            arm0_start,
            arm0_end,
            arm1_enter_marker_idx,
            arm1_start,
            arm1_end,
        ) = Self::route_arm_ranges(scope_markers, scope_id);
        Self::validate_route_policy_consistency(
            view,
            arm0_enter_marker_idx,
            arm0_end,
            arm1_enter_marker_idx,
            arm1_end,
        );
        if linger {
            return;
        }

        if matches!(controller_role, Some(role) if role == ROLE) {
            return;
        }

        let mut left = [LocalSig::EMPTY; eff::meta::MAX_EFF_NODES];
        let mut right = [LocalSig::EMPTY; eff::meta::MAX_EFF_NODES];
        let left_len = Self::collect_local_sigs(view, arm0_start, arm0_end, &mut left);
        let right_len = Self::collect_local_sigs(view, arm1_start, arm1_end, &mut right);

        if left_len == 0 && right_len == 0 {
            return;
        }
        if Self::local_sequences_equal(&left, left_len, &right, right_len) {
            return;
        }
        if Self::dispatchable_after_shared_prefix(&left, left_len, &right, right_len) {
            return;
        }
        if Self::scope_has_dynamic_policy(
            view,
            arm0_enter_marker_idx,
            arm0_end,
            arm1_enter_marker_idx,
            arm1_end,
        ) {
            return;
        }
        panic!(
            "Route unprojectable for this role: arms not mergeable, wire dispatch non-deterministic, and no dynamic policy annotation provided"
        );
    }

    const fn route_arm_ranges(
        scope_markers: &[crate::global::const_dsl::ScopeMarker],
        scope_id: ScopeId,
    ) -> (usize, usize, usize, usize, usize, usize) {
        let mut enter_marker_indices = [usize::MAX; 2];
        let mut enter_offsets = [usize::MAX; 2];
        let mut exit_offsets = [usize::MAX; 2];
        let mut enter_len = 0usize;
        let mut exit_len = 0usize;
        let mut idx = 0usize;
        while idx < scope_markers.len() {
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
        view: crate::global::compiled::LoweringView<'_>,
        arm0_enter_marker_idx: usize,
        arm0_end: usize,
        arm1_enter_marker_idx: usize,
        arm1_end: usize,
    ) -> bool {
        Self::first_route_head_dynamic_policy_id_in_range(view, arm0_enter_marker_idx, arm0_end)
            .is_some()
            || Self::first_route_head_dynamic_policy_id_in_range(
                view,
                arm1_enter_marker_idx,
                arm1_end,
            )
            .is_some()
    }

    const fn validate_route_policy_consistency(
        view: crate::global::compiled::LoweringView<'_>,
        arm0_enter_marker_idx: usize,
        arm0_end: usize,
        arm1_enter_marker_idx: usize,
        arm1_end: usize,
    ) {
        let left = Self::first_route_head_dynamic_policy_id_in_range(
            view,
            arm0_enter_marker_idx,
            arm0_end,
        );
        let right = Self::first_route_head_dynamic_policy_id_in_range(
            view,
            arm1_enter_marker_idx,
            arm1_end,
        );
        match (left, right) {
            (Some(left_id), Some(right_id)) => {
                if left_id != right_id {
                    panic!("route scope recorded different controller policy ids across arms");
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                panic!("route scope recorded a controller policy annotation on only one arm");
            }
            (None, None) => {}
        }
    }

    const fn first_route_head_dynamic_policy_id_in_range(
        view: crate::global::compiled::LoweringView<'_>,
        route_enter_marker_idx: usize,
        end: usize,
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
        let nodes = view.as_slice();
        let mut marker_idx = route_enter_marker_idx + 1;
        let mut active_scope_depth = 1usize;
        let mut idx = route_enter.offset;
        while idx < end && idx < nodes.len() {
            let mut scan_marker_idx = marker_idx;
            let mut depth_after_exits = active_scope_depth;
            while scan_marker_idx < scope_markers.len() {
                let marker = scope_markers[scan_marker_idx];
                if marker.offset != idx {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Exit) {
                    depth_after_exits = depth_after_exits.saturating_sub(1);
                }
                scan_marker_idx += 1;
            }

            let mut enter_count = 0usize;
            let mut nested_non_policy_enter = false;
            let mut next_marker_idx = marker_idx;
            while next_marker_idx < scope_markers.len() {
                let marker = scope_markers[next_marker_idx];
                if marker.offset != idx {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Enter) {
                    if depth_after_exits == 1 && !matches!(marker.scope_kind, ScopeKind::Generic) {
                        nested_non_policy_enter = true;
                    }
                    enter_count += 1;
                }
                next_marker_idx += 1;
            }

            match view.policy_at(idx) {
                Some(policy)
                    if depth_after_exits == 1
                        && !nested_non_policy_enter
                        && policy.dynamic_policy_id().is_some() =>
                {
                    return policy.dynamic_policy_id();
                }
                _ => {}
            }
            active_scope_depth = depth_after_exits.saturating_add(enter_count);
            marker_idx = next_marker_idx;
            idx += 1;
        }
        None
    }

    const fn collect_local_sigs(
        view: crate::global::compiled::LoweringView<'_>,
        start: usize,
        end: usize,
        out: &mut [LocalSig; eff::meta::MAX_EFF_NODES],
    ) -> usize {
        let nodes = view.as_slice();
        let mut len = 0usize;
        let mut idx = start;
        while idx < end && idx < nodes.len() {
            let node = nodes[idx];
            if matches!(node.kind, EffKind::Atom) {
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

pub(crate) const fn validate_all_roles(summary: &LoweringSummary) {
    ProjectionSeal::<0>::validate_compiled_layout(summary.view());
    ProjectionSeal::<1>::validate_compiled_layout(summary.view());
    ProjectionSeal::<2>::validate_compiled_layout(summary.view());
    ProjectionSeal::<3>::validate_compiled_layout(summary.view());
    ProjectionSeal::<4>::validate_compiled_layout(summary.view());
    ProjectionSeal::<5>::validate_compiled_layout(summary.view());
    ProjectionSeal::<6>::validate_compiled_layout(summary.view());
    ProjectionSeal::<7>::validate_compiled_layout(summary.view());
    ProjectionSeal::<8>::validate_compiled_layout(summary.view());
    ProjectionSeal::<9>::validate_compiled_layout(summary.view());
    ProjectionSeal::<10>::validate_compiled_layout(summary.view());
    ProjectionSeal::<11>::validate_compiled_layout(summary.view());
    ProjectionSeal::<12>::validate_compiled_layout(summary.view());
    ProjectionSeal::<13>::validate_compiled_layout(summary.view());
    ProjectionSeal::<14>::validate_compiled_layout(summary.view());
    ProjectionSeal::<15>::validate_compiled_layout(summary.view());
}
