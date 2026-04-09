use crate::{
    eff::{self, EffKind},
    global::{
        compiled::LoweringSummary,
        const_dsl::{ScopeEvent, ScopeId, ScopeKind},
        role_program::{MAX_PHASES, MAX_STEPS},
    },
};

pub(crate) struct ProjectionSeal<const ROLE: u8>;

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
                if seen_route_like_len >= MAX_STEPS {
                    panic!("controller arm table capacity exceeded");
                }
                seen_route_like[seen_route_like_len] = marker.scope_id;
                seen_route_like_len += 1;
            }
            idx += 1;
        }
    }

    const fn validate_phase_capacity(view: crate::global::compiled::LoweringView<'_>) {
        let nodes = view.as_slice();
        let mut present = [false; eff::meta::MAX_EFF_NODES];
        let mut local_len = 0usize;
        let mut idx = 0usize;
        while idx < nodes.len() {
            let node = nodes[idx];
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == ROLE || atom.to == ROLE {
                    if !present[idx] {
                        present[idx] = true;
                        local_len += 1;
                    }
                }
            }
            idx += 1;
        }
        if local_len == 0 {
            return;
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

        if range_len == 0 {
            return;
        }

        let mut phase_count = 0usize;
        let mut current_eff = 0usize;
        let mut range_idx = 0usize;
        while range_idx < range_len {
            let (enter_eff, exit_eff) = ranges[range_idx];
            if Self::has_local_step_in_range(&present, current_eff, enter_eff) {
                phase_count += 1;
                if phase_count > MAX_PHASES {
                    panic!("compiled role phase capacity exceeded");
                }
            }
            if Self::has_local_step_in_range(&present, enter_eff, exit_eff) {
                phase_count += 1;
                if phase_count > MAX_PHASES {
                    panic!("compiled role phase capacity exceeded");
                }
            }
            current_eff = if exit_eff > current_eff {
                exit_eff
            } else {
                current_eff
            };
            range_idx += 1;
        }
        if Self::has_local_step_in_range(&present, current_eff, eff::meta::MAX_EFF_NODES) {
            phase_count += 1;
        }
        if phase_count == 0 {
            phase_count = 1;
        }
        if phase_count > MAX_PHASES {
            panic!("compiled role phase capacity exceeded");
        }
    }

    const fn has_local_step_in_range(
        present: &[bool; eff::meta::MAX_EFF_NODES],
        start: usize,
        end: usize,
    ) -> bool {
        if start >= end {
            return false;
        }
        let mut idx = start;
        while idx < end && idx < eff::meta::MAX_EFF_NODES {
            if present[idx] {
                return true;
            }
            idx += 1;
        }
        false
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
        let (arm0_start, arm0_end, arm1_start, arm1_end) =
            Self::route_arm_ranges(scope_markers, scope_id);
        Self::validate_route_policy_consistency(
            view, scope_id, arm0_start, arm0_end, arm1_start, arm1_end,
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
            view, scope_id, arm0_start, arm0_end, arm1_start, arm1_end,
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
    ) -> (usize, usize, usize, usize) {
        let mut enter_offsets = [usize::MAX; 2];
        let mut exit_offsets = [usize::MAX; 2];
        let mut enter_len = 0usize;
        let mut exit_len = 0usize;
        let mut idx = 0usize;
        while idx < scope_markers.len() {
            let marker = scope_markers[idx];
            if marker.scope_id.raw() == scope_id.raw()
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                match marker.event {
                    ScopeEvent::Enter => {
                        if enter_len < 2 {
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
            enter_offsets[0],
            exit_offsets[0],
            enter_offsets[1],
            exit_offsets[1],
        )
    }

    const fn scope_has_dynamic_policy(
        view: crate::global::compiled::LoweringView<'_>,
        scope_id: ScopeId,
        arm0_start: usize,
        arm0_end: usize,
        arm1_start: usize,
        arm1_end: usize,
    ) -> bool {
        Self::first_dynamic_policy_id_in_range(view, scope_id, arm0_start, arm0_end).is_some()
            || Self::first_dynamic_policy_id_in_range(view, scope_id, arm1_start, arm1_end)
                .is_some()
    }

    const fn validate_route_policy_consistency(
        view: crate::global::compiled::LoweringView<'_>,
        scope_id: ScopeId,
        arm0_start: usize,
        arm0_end: usize,
        arm1_start: usize,
        arm1_end: usize,
    ) {
        let left = Self::first_dynamic_policy_id_in_range(view, scope_id, arm0_start, arm0_end);
        let right = Self::first_dynamic_policy_id_in_range(view, scope_id, arm1_start, arm1_end);
        match (left, right) {
            (Some(left_id), Some(right_id)) => {
                if left_id != right_id {
                    panic!("route scope recorded conflicting controller policy annotations");
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                panic!("route scope recorded conflicting controller policy annotations");
            }
            (None, None) => {}
        }
    }

    const fn first_dynamic_policy_id_in_range(
        view: crate::global::compiled::LoweringView<'_>,
        scope_id: ScopeId,
        start: usize,
        end: usize,
    ) -> Option<u16> {
        let nodes = view.as_slice();
        let mut idx = start;
        while idx < end && idx < nodes.len() {
            match view.policy_at(idx) {
                Some(policy)
                    if policy.is_dynamic()
                        && Self::policy_belongs_to_route_scope(scope_id, policy.scope()) =>
                {
                    return policy.dynamic_policy_id();
                }
                _ => {}
            }
            idx += 1;
        }
        None
    }

    const fn policy_belongs_to_route_scope(route_scope: ScopeId, policy_scope: ScopeId) -> bool {
        if policy_scope.is_none() {
            return true;
        }
        if matches!(policy_scope.kind(), ScopeKind::Route) {
            policy_scope.raw() == route_scope.raw()
        } else {
            true
        }
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
