use super::{
    EffList, ScopeKind, ScopeMarkerView, eff,
    scope_ranges::{
        parallel_arm_ranges_from_enter, parallel_enter_at, roll_body_range_from_enter,
        roll_continuation_end, route_arm_ranges_from_first_enter, route_enter_at,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct EndpointSelector(u64);

impl EndpointSelector {
    const OUTBOUND: u64 = 0;
    const INBOUND_EVIDENCE: u64 = 1;
    const KIND_SHIFT: u32 = 56;

    const fn outbound(atom: eff::EffAtom) -> Self {
        Self(
            (Self::OUTBOUND << Self::KIND_SHIFT)
                | ((atom.from as u64) << 48)
                | ((atom.label as u64) << 40)
                | atom.payload_schema as u64,
        )
    }

    const fn inbound_evidence(atom_idx: usize) -> Option<Self> {
        if atom_idx >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY {
            None
        } else {
            // Projection validation uses the same compact event identity later
            // carried by the descriptor; frame-label reuse remains independent.
            Some(Self(
                (Self::INBOUND_EVIDENCE << Self::KIND_SHIFT) | atom_idx as u64,
            ))
        }
    }

    const fn is_inbound_evidence(self) -> bool {
        (self.0 >> Self::KIND_SHIFT) == Self::INBOUND_EVIDENCE
    }

    const fn is_outbound(self) -> bool {
        (self.0 >> Self::KIND_SHIFT) == Self::OUTBOUND
    }

    const fn same(self, other: Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObserverPathDecision {
    Continue,
    Accept,
    Reject,
}

const fn observer_path_decision(
    left: Option<EndpointSelector>,
    right: Option<EndpointSelector>,
) -> ObserverPathDecision {
    match (left, right) {
        (Some(selector), Some(other)) => {
            if selector.is_inbound_evidence() && other.is_inbound_evidence() {
                if selector.same(other) {
                    ObserverPathDecision::Continue
                } else {
                    ObserverPathDecision::Accept
                }
            } else {
                ObserverPathDecision::Reject
            }
        }
        (None, None) => ObserverPathDecision::Accept,
        (Some(_), None) | (None, Some(_)) => ObserverPathDecision::Reject,
    }
}

pub(crate) const fn validate_parallel_endpoint_selectors<const E: usize>(
    eff_list: &EffList<E>,
) -> bool {
    let markers = eff_list.scope_markers();
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let Some((left_start, left_end, right_start, right_end)) =
                parallel_arm_ranges_from_enter(markers, idx)
            else {
                return false;
            };
            if parallel_endpoint_selector_conflicts(
                eff_list,
                left_start,
                left_end,
                right_start,
                right_end,
            ) {
                return false;
            }
        }
        idx += 1;
    }
    true
}

pub(crate) const fn validate_roll_reentry_endpoint_selectors<const E: usize>(
    eff_list: &EffList<E>,
) -> bool {
    let markers = eff_list.scope_markers();
    let mut idx = 0usize;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
        {
            let Some((body_start, body_end)) = roll_body_range_from_enter(markers, idx) else {
                return false;
            };
            let continuation_end = roll_continuation_end(markers, idx, body_end, eff_list.len());
            if body_end < continuation_end
                && first_visible_endpoint_selector_conflicts_from_markers(
                    eff_list,
                    body_start,
                    body_end,
                    body_end,
                    continuation_end,
                    idx + 1,
                    0,
                )
            {
                return false;
            }
        }
        idx += 1;
    }
    true
}

const fn parallel_endpoint_selector_conflicts<const E: usize>(
    eff_list: &EffList<E>,
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    let mut idx = left_start;
    while idx < left_end && idx < eff_list.len() {
        let atom = eff_list.atom_at(idx);
        if range_contains_endpoint_selector(
            eff_list,
            right_start,
            right_end,
            EndpointSelector::outbound(atom),
        ) {
            return true;
        }
        if let Some(selector) = inbound_selector_at(idx)
            && range_contains_endpoint_selector(eff_list, right_start, right_end, selector)
        {
            return true;
        }
        idx += 1;
    }
    false
}

const fn range_contains_endpoint_selector<const E: usize>(
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    target: EndpointSelector,
) -> bool {
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        if atom_matches_selector(idx, eff_list.atom_at(idx), target) {
            return true;
        }
        idx += 1;
    }
    false
}

pub(crate) const fn first_visible_endpoint_selector_conflicts_from_markers<const E: usize>(
    eff_list: &EffList<E>,
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
    left_marker_floor: usize,
    right_marker_floor: usize,
) -> bool {
    let markers = eff_list.scope_markers();
    if left_start >= left_end || left_start >= eff_list.len() {
        return false;
    }
    if let Some(route_enter) = route_enter_at(markers, left_start, left_end, left_marker_floor) {
        let [(arm0_start, arm0_end), (arm1_start, arm1_end)] =
            route_arm_ranges_from_first_enter(markers, route_enter);
        return first_visible_endpoint_selector_conflicts_from_markers(
            eff_list,
            arm0_start,
            arm0_end,
            right_start,
            right_end,
            route_enter + 1,
            right_marker_floor,
        ) || first_visible_endpoint_selector_conflicts_from_markers(
            eff_list,
            arm1_start,
            arm1_end,
            right_start,
            right_end,
            route_enter + 1,
            right_marker_floor,
        );
    }
    if let Some(par_enter) = parallel_enter_at(markers, left_start, left_end, left_marker_floor) {
        let Some((arm0_start, arm0_end, arm1_start, arm1_end)) =
            parallel_arm_ranges_from_enter(markers, par_enter)
        else {
            return true;
        };
        return first_visible_endpoint_selector_conflicts_from_markers(
            eff_list,
            arm0_start,
            arm0_end,
            right_start,
            right_end,
            par_enter + 1,
            right_marker_floor,
        ) || first_visible_endpoint_selector_conflicts_from_markers(
            eff_list,
            arm1_start,
            arm1_end,
            right_start,
            right_end,
            par_enter + 1,
            right_marker_floor,
        );
    }

    first_visible_endpoint_matches_atom(
        markers,
        eff_list,
        right_start,
        right_end,
        left_start,
        eff_list.atom_at(left_start),
        right_marker_floor,
    )
}

const fn first_visible_endpoint_matches_atom<const E: usize>(
    markers: ScopeMarkerView<'_>,
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    atom_idx: usize,
    atom: eff::EffAtom,
    marker_floor: usize,
) -> bool {
    if first_visible_endpoint_matches(
        markers,
        eff_list,
        start,
        end,
        EndpointSelector::outbound(atom),
        marker_floor,
    ) {
        return true;
    }
    match inbound_selector_at(atom_idx) {
        Some(selector) => {
            first_visible_endpoint_matches(markers, eff_list, start, end, selector, marker_floor)
        }
        None => false,
    }
}

const fn first_visible_endpoint_matches<const E: usize>(
    markers: ScopeMarkerView<'_>,
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    target: EndpointSelector,
    marker_floor: usize,
) -> bool {
    if start >= end || start >= eff_list.len() {
        return false;
    }
    if let Some(route_enter) = route_enter_at(markers, start, end, marker_floor) {
        let [(arm0_start, arm0_end), (arm1_start, arm1_end)] =
            route_arm_ranges_from_first_enter(markers, route_enter);
        return first_visible_endpoint_matches(
            markers,
            eff_list,
            arm0_start,
            arm0_end,
            target,
            route_enter + 1,
        ) || first_visible_endpoint_matches(
            markers,
            eff_list,
            arm1_start,
            arm1_end,
            target,
            route_enter + 1,
        );
    }
    if let Some(par_enter) = parallel_enter_at(markers, start, end, marker_floor) {
        let Some((arm0_start, arm0_end, arm1_start, arm1_end)) =
            parallel_arm_ranges_from_enter(markers, par_enter)
        else {
            return true;
        };
        return first_visible_endpoint_matches(
            markers,
            eff_list,
            arm0_start,
            arm0_end,
            target,
            par_enter + 1,
        ) || first_visible_endpoint_matches(
            markers,
            eff_list,
            arm1_start,
            arm1_end,
            target,
            par_enter + 1,
        );
    }

    atom_matches_selector(start, eff_list.atom_at(start), target)
}

pub(crate) const fn local_route_observer_paths_mergeable<const E: usize>(
    eff_list: &EffList<E>,
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
    role: u8,
) -> bool {
    let mut left_idx = left_start;
    let mut right_idx = right_start;
    loop {
        let left = next_local_endpoint_selector(eff_list, &mut left_idx, left_end, role);
        let right = next_local_endpoint_selector(eff_list, &mut right_idx, right_end, role);
        match observer_path_decision(left, right) {
            ObserverPathDecision::Continue => {}
            ObserverPathDecision::Accept => return true,
            ObserverPathDecision::Reject => return false,
        }
    }
}

const fn next_local_endpoint_selector<const E: usize>(
    eff_list: &EffList<E>,
    idx: &mut usize,
    end: usize,
    role: u8,
) -> Option<EndpointSelector> {
    while *idx < end && *idx < eff_list.len() {
        let atom = eff_list.atom_at(*idx);
        let selector = if atom.from == role {
            Some(EndpointSelector::outbound(atom))
        } else if atom.to == role {
            inbound_selector_at(*idx)
        } else {
            None
        };
        if let Some(selector) = selector {
            *idx += 1;
            return Some(selector);
        }
        *idx += 1;
    }
    None
}

const fn inbound_selector_at(atom_idx: usize) -> Option<EndpointSelector> {
    EndpointSelector::inbound_evidence(atom_idx)
}

const fn atom_matches_selector(
    atom_idx: usize,
    atom: eff::EffAtom,
    target: EndpointSelector,
) -> bool {
    if target.is_outbound() {
        return EndpointSelector::outbound(atom).same(target);
    }
    matches!(inbound_selector_at(atom_idx), Some(selector) if selector.same(target))
}

#[cfg(kani)]
mod kani;
