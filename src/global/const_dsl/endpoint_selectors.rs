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
    // Decode each event once, rather than decoding both sides of every pair.
    let mut outbound = [0u64; E];
    let mut event = 0;
    while event < eff_list.len() {
        outbound[event] = EndpointSelector::outbound(eff_list.atom_at(event)).0;
        event += 1;
    }
    let sorted = sorted_selector_events(&outbound, eff_list.len());
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
                &outbound,
                &sorted,
                eff_list.len(),
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

// A bounded heap sort groups equal public send contracts once for all scopes.
// Event indices remain exact, so no hashing or collision assumption is used.
const fn sorted_selector_events<const E: usize>(keys: &[u64; E], len: usize) -> [usize; E] {
    let mut order = [0usize; E];
    let mut i = 0;
    while i < len {
        order[i] = i;
        i += 1;
    }
    let mut root = len / 2;
    while root > 0 {
        root -= 1;
        sift_selector_heap(keys, &mut order, root, len);
    }
    let mut end = len;
    while end > 1 {
        end -= 1;
        let last = order[end];
        order[end] = order[0];
        order[0] = last;
        sift_selector_heap(keys, &mut order, 0, end);
    }
    order
}

const fn sift_selector_heap<const E: usize>(
    keys: &[u64; E],
    order: &mut [usize; E],
    mut root: usize,
    end: usize,
) {
    loop {
        let mut child = root * 2 + 1;
        if child >= end {
            return;
        }
        if child + 1 < end && keys[order[child]] < keys[order[child + 1]] {
            child += 1;
        }
        if keys[order[root]] >= keys[order[child]] {
            return;
        }
        let old = order[root];
        order[root] = order[child];
        order[child] = old;
        root = child;
    }
}

const fn parallel_endpoint_selector_conflicts<const E: usize>(
    outbound: &[u64; E],
    sorted: &[usize; E],
    len: usize,
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    // Identical inbound evidence can only occur in the ranges' intersection.
    let first = if left_start > right_start {
        left_start
    } else {
        right_start
    };
    if first < left_end
        && first < right_end
        && first < len
        && first < crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY
    {
        return true;
    }
    let mut left = left_start;
    while left < left_end && left < len {
        let key = outbound[left];
        let mut low = 0;
        let mut high = len;
        while low < high {
            let mid = low + (high - low) / 2;
            if outbound[sorted[mid]] < key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        while low < len && outbound[sorted[low]] == key {
            let event = sorted[low];
            if event >= right_start && event < right_end {
                return true;
            }
            low += 1;
        }
        left += 1;
    }
    false
}

#[cfg(test)]
const fn range_contains_endpoint_selector<const E: usize>(
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    target: EndpointSelector,
) -> bool {
    // Inbound evidence is the unique event index, so membership is exact
    // without scanning unrelated events or comparing their payloads.
    if target.is_inbound_evidence() {
        let index = (target.0 & ((1u64 << EndpointSelector::KIND_SHIFT) - 1)) as usize;
        return index >= start && index < end && index < eff_list.len();
    }
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
    // Every recursive branch remains inside this event range. A distinct
    // inbound event cannot match any endpoint in it.
    if target.is_inbound_evidence() {
        let index = (target.0 & ((1u64 << EndpointSelector::KIND_SHIFT) - 1)) as usize;
        if index < start || index >= end {
            return false;
        }
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

#[cfg(test)]
mod pruning_tests {
    use super::*;

    #[test]
    fn cached_parallel_selectors_match_pairwise_reference() {
        let mut events = EffList::<16>::new_partitioned(8, 0, 0);
        let mut outbound = [0u64; 16];
        for (i, key) in outbound.iter_mut().enumerate().take(8) {
            let atom = eff::EffAtom {
                from: (i % 2) as u8,
                to: ((i + 1) % 2) as u8,
                label: (i % 3) as u8,
                payload_schema: 0,
                origin: eff::EventOrigin::User,
                lane: 0,
            };
            events.push_event_mut(atom);
            *key = EndpointSelector::outbound(atom).0;
        }
        for left_start in 0..10 {
            for left_end in 0..10 {
                for right_start in 0..10 {
                    for right_end in 0..10 {
                        let expected = (left_start..left_end.min(events.len())).any(|left| {
                            (right_start..right_end.min(events.len())).any(|right| {
                                EndpointSelector::outbound(events.atom_at(left))
                                    .same(EndpointSelector::outbound(events.atom_at(right)))
                                    || inbound_selector_at(left)
                                        .unwrap()
                                        .same(inbound_selector_at(right).unwrap())
                            })
                        });
                        assert_eq!(
                            parallel_endpoint_selector_conflicts(
                                &outbound,
                                &sorted_selector_events(&outbound, events.len()),
                                events.len(),
                                left_start,
                                left_end,
                                right_start,
                                right_end
                            ),
                            expected
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn inbound_range_membership_matches_event_scan() {
        let mut events = EffList::<16>::new_partitioned(8, 0, 0);
        for label in 0..8 {
            events.push_event_mut(eff::EffAtom {
                from: 0,
                to: 1,
                label,
                payload_schema: 0,
                origin: eff::EventOrigin::User,
                lane: 0,
            });
        }
        for index in 0..12 {
            let target = EndpointSelector::inbound_evidence(index).unwrap();
            for start in 0..12 {
                for end in 0..12 {
                    let expected = (start..end.min(events.len()))
                        .any(|i| atom_matches_selector(i, events.atom_at(i), target));
                    assert_eq!(
                        range_contains_endpoint_selector(&events, start, end, target),
                        expected
                    );
                    let first = start < end && start < events.len() && start == index;
                    assert_eq!(
                        first_visible_endpoint_matches(
                            events.scope_markers(),
                            &events,
                            start,
                            end,
                            target,
                            0
                        ),
                        first
                    );
                }
            }
        }
    }
}
