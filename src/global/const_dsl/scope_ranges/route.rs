use super::{
    ScopeId, ScopeKind, ScopeMarkerView, StructuredScopeRange, innermost_scope_range,
    outermost_scope_range, scope_segment_end_from_enter,
};

pub(crate) type RouteArmRanges = [(usize, usize); 2];

pub(crate) const fn closed_route_arm_ranges_from_first_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
) -> Option<RouteArmRanges> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let first = scope_markers.at(enter_idx);
    if !first.event.is_primary_enter()
        || !matches!(first.scope_id.kind(), Some(ScopeKind::Route))
        || first.segment_end() <= first.offset()
    {
        return None;
    }
    let Some(right_end) = first.event.route_end() else {
        return None;
    };
    let split = first.segment_end();
    if split >= right_end {
        return None;
    }
    Some([(first.offset(), split), (split, right_end)])
}

pub(crate) const fn route_arm_event_ranges_for_scope(
    scope_markers: ScopeMarkerView<'_>,
    scope: ScopeId,
) -> Option<[(usize, usize); 2]> {
    let enter_idx = match scope_markers.first_enter_index(scope) {
        Some(index) => index,
        None => return None,
    };
    closed_route_arm_ranges_from_first_enter(scope_markers, enter_idx)
}

pub(crate) const fn structured_scope_event_range(
    scope_markers: ScopeMarkerView<'_>,
    scope: ScopeId,
) -> Option<(usize, usize)> {
    let enter_idx = match scope_markers.first_enter_index(scope) {
        Some(index) => index,
        None => return None,
    };
    let marker = scope_markers.at(enter_idx);
    match marker.scope_id.kind() {
        Some(ScopeKind::Route) => {
            let [left, right] = match route_arm_event_ranges_for_scope(scope_markers, scope) {
                Some(ranges) => ranges,
                None => return None,
            };
            Some((left.0, right.1))
        }
        Some(ScopeKind::Roll | ScopeKind::Parallel) => Some((
            marker.offset(),
            scope_segment_end_from_enter(scope_markers, enter_idx, None),
        )),
        None => None,
    }
}

pub(crate) const fn route_scope_slot_for_scope(
    scope_markers: ScopeMarkerView<'_>,
    route: ScopeId,
) -> Option<usize> {
    if !matches!(route.kind(), Some(ScopeKind::Route)) {
        return None;
    }
    let mut slot = 0usize;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers.at(idx);
        if scope_markers.is_first_enter(idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if marker.scope_id.same(route) {
                return Some(slot);
            }
            slot += 1;
        }
        idx += 1;
    }
    None
}

pub(crate) const fn route_parent_arm_for_scope(
    scope_markers: ScopeMarkerView<'_>,
    scope: ScopeId,
) -> Option<(ScopeId, u8)> {
    let (target_start, target_end) = match structured_scope_event_range(scope_markers, scope) {
        Some(range) => range,
        None => return None,
    };
    let mut parent: Option<StructuredScopeRange> = None;
    let mut parent_arm = 0u8;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers.at(idx);
        if scope_markers.is_first_enter(idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && !marker.scope_id.same(scope)
        {
            let ranges = match route_arm_event_ranges_for_scope(scope_markers, marker.scope_id) {
                Some(ranges) => ranges,
                None => crate::invariant(),
            };
            let mut arm = 0usize;
            while arm < ranges.len() {
                let (start, end) = ranges[arm];
                if start <= target_start && target_end <= end {
                    let candidate = StructuredScopeRange::new(marker.scope_id, start, end);
                    let selected = match parent {
                        Some(current) => innermost_scope_range(current, candidate),
                        None => candidate,
                    };
                    if selected.scope().same(candidate.scope()) {
                        parent_arm = arm as u8;
                    }
                    parent = Some(selected);
                }
                arm += 1;
            }
        }
        idx += 1;
    }
    match parent {
        Some(parent) => Some((parent.scope(), parent_arm)),
        None => None,
    }
}

pub(crate) const fn passive_route_child_scope(
    scope_markers: ScopeMarkerView<'_>,
    route: ScopeId,
    arm: u8,
) -> Option<ScopeId> {
    if arm > 1 || !matches!(route.kind(), Some(ScopeKind::Route)) {
        return None;
    }
    let ranges = match route_arm_event_ranges_for_scope(scope_markers, route) {
        Some(ranges) => ranges,
        None => return None,
    };
    let (arm_start, arm_end) = ranges[arm as usize];
    let mut child: Option<StructuredScopeRange> = None;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers.at(idx);
        if scope_markers.is_first_enter(idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && !marker.scope_id.same(route)
            && marker.offset() == arm_start
        {
            let (_, child_end) = match structured_scope_event_range(scope_markers, marker.scope_id)
            {
                Some(range) => range,
                None => crate::invariant(),
            };
            if child_end <= arm_end {
                let candidate = StructuredScopeRange::new(marker.scope_id, arm_start, child_end);
                child = Some(match child {
                    Some(current) => outermost_scope_range(current, candidate),
                    None => candidate,
                });
            }
        }
        idx += 1;
    }
    match child {
        Some(child) => Some(child.scope()),
        None => None,
    }
}

pub(crate) const fn route_arm_ranges_from_first_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
) -> RouteArmRanges {
    let Some(ranges) = closed_route_arm_ranges_from_first_enter(scope_markers, enter_idx) else {
        panic!("route requires exactly 2 contiguous non-empty closed arms");
    };
    ranges
}
