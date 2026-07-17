use super::{ScopeId, ScopeKind, ScopeMarkerView};

mod nesting;
pub(crate) use nesting::{StructuredScopeRange, innermost_scope_range, outermost_scope_range};

mod route;
pub(crate) use route::{
    closed_route_arm_ranges_from_first_enter, passive_route_child_scope,
    route_arm_event_ranges_for_scope, route_arm_ranges_from_first_enter,
    route_parent_arm_for_scope, route_scope_slot_for_scope, structured_scope_event_range,
};

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub(crate) const fn route_enter_at(
    markers: ScopeMarkerView<'_>,
    start: usize,
    end: usize,
    marker_floor: usize,
) -> Option<usize> {
    let mut idx = marker_floor;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.offset() > start {
            break;
        }
        if marker.offset() == start
            && marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && markers.is_first_enter(idx)
            && route_exits_within(markers, idx, end)
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

const fn route_exits_within(markers: ScopeMarkerView<'_>, enter_idx: usize, end: usize) -> bool {
    match closed_route_arm_ranges_from_first_enter(markers, enter_idx) {
        Some([_, (_, arm1_end)]) => arm1_end <= end,
        None => false,
    }
}

pub(crate) const fn parallel_enter_at(
    markers: ScopeMarkerView<'_>,
    start: usize,
    end: usize,
    marker_floor: usize,
) -> Option<usize> {
    let mut idx = marker_floor;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.offset() > start {
            break;
        }
        if marker.offset() == start
            && marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            && parallel_exits_within(markers, idx, end)
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

const fn parallel_exits_within(markers: ScopeMarkerView<'_>, enter_idx: usize, end: usize) -> bool {
    let Some((_, _, _, right_end)) = parallel_arm_ranges_from_enter(markers, enter_idx) else {
        return false;
    };
    right_end <= end
}

pub(crate) const fn roll_body_range_from_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
) -> Option<(usize, usize)> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let marker = scope_markers.at(enter_idx);
    if !marker.event.is_enter() || !matches!(marker.scope_id.kind(), Some(ScopeKind::Roll)) {
        return None;
    }
    let end = marker.segment_end();
    if end <= marker.offset() {
        None
    } else {
        Some((marker.offset(), end))
    }
}

pub(crate) const fn scope_segment_end_from_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
    segment_limit: Option<usize>,
) -> usize {
    if enter_idx >= scope_markers.len() {
        panic!("scope enter marker index out of bounds");
    }
    let marker = scope_markers.at(enter_idx);
    if !marker.event.is_enter() {
        panic!("scope segment boundary requires an enter marker");
    }
    let end = marker.segment_end();
    if end <= marker.offset() || matches!(segment_limit, Some(limit) if end > limit) {
        crate::invariant();
    }
    end
}

pub(crate) const fn roll_continuation_end(
    scope_markers: ScopeMarkerView<'_>,
    roll_enter_idx: usize,
    roll_end: usize,
    eff_len: usize,
) -> usize {
    let roll_start = scope_markers.at(roll_enter_idx).offset();
    let mut boundary = eff_len;
    let mut idx = 0usize;
    while idx < roll_enter_idx {
        let marker = scope_markers.at(idx);
        if marker.event.is_enter()
            && marker.offset() <= roll_start
            && let Some(candidate) = scope_boundary_for_enter(scope_markers, idx, roll_start)
            && candidate > roll_end
            && candidate < boundary
        {
            boundary = candidate;
        }
        idx += 1;
    }
    boundary
}

const fn scope_boundary_for_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
    contained_offset: usize,
) -> Option<usize> {
    let marker = scope_markers.at(enter_idx);
    match marker.scope_id.kind() {
        Some(ScopeKind::Parallel) => {
            let Some((_, left_end, _, right_end)) =
                parallel_arm_ranges_from_enter(scope_markers, enter_idx)
            else {
                return None;
            };
            if contained_offset < left_end {
                Some(left_end)
            } else {
                Some(right_end)
            }
        }
        Some(ScopeKind::Route) => route_arm_exit_from_enter(scope_markers, enter_idx),
        Some(ScopeKind::Roll) => match roll_body_range_from_enter(scope_markers, enter_idx) {
            Some((_, end)) => Some(end),
            None => None,
        },
        None => None,
    }
}

const fn route_arm_exit_from_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
) -> Option<usize> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let marker = scope_markers.at(enter_idx);
    if !marker.event.is_enter() || !matches!(marker.scope_id.kind(), Some(ScopeKind::Route)) {
        return None;
    }
    let end = marker.segment_end();
    if end <= marker.offset() {
        None
    } else {
        Some(end)
    }
}

pub(crate) const fn parallel_arm_ranges_from_enter(
    scope_markers: ScopeMarkerView<'_>,
    enter_idx: usize,
) -> Option<(usize, usize, usize, usize)> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let marker = scope_markers.at(enter_idx);
    if !marker.event.is_primary_enter()
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
    {
        return None;
    }
    let Some(split) = marker.event.parallel_split() else {
        return None;
    };
    let exit = marker.segment_end();
    if marker.offset() >= split || split >= exit {
        None
    } else {
        Some((marker.offset(), split, split, exit))
    }
}
