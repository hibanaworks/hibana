use super::{ScopeEvent, ScopeKind, ScopeMarker};

pub(crate) const fn route_enter_at(
    markers: &[ScopeMarker],
    start: usize,
    end: usize,
    marker_floor: usize,
) -> Option<usize> {
    let mut idx = marker_floor;
    while idx < markers.len() {
        let marker = markers[idx];
        if marker.offset() > start {
            break;
        }
        if marker.offset() == start
            && matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && first_enter_for_scope(markers, idx)
            && route_exits_within(markers, idx, end)
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

const fn first_enter_for_scope(markers: &[ScopeMarker], marker_idx: usize) -> bool {
    let marker = markers[marker_idx];
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = markers[idx];
        if matches!(candidate.event, ScopeEvent::Enter) && candidate.scope_id.same(marker.scope_id)
        {
            return false;
        }
        idx += 1;
    }
    true
}

const fn route_exits_within(markers: &[ScopeMarker], enter_idx: usize, end: usize) -> bool {
    let (_, _, _, _, _, arm1_end) = route_arm_ranges_from_first_enter(markers, enter_idx);
    arm1_end <= end
}

pub(crate) const fn parallel_enter_at(
    markers: &[ScopeMarker],
    start: usize,
    end: usize,
    marker_floor: usize,
) -> Option<usize> {
    let mut idx = marker_floor;
    while idx < markers.len() {
        let marker = markers[idx];
        if marker.offset() > start {
            break;
        }
        if marker.offset() == start
            && matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            && parallel_exits_within(markers, idx, end)
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

const fn parallel_exits_within(markers: &[ScopeMarker], enter_idx: usize, end: usize) -> bool {
    let Some((_, _, _, right_end)) = parallel_arm_ranges_from_enter(markers, enter_idx) else {
        return false;
    };
    right_end <= end
}

pub(crate) const fn roll_body_range_from_enter(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
) -> Option<(usize, usize)> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let marker = scope_markers[enter_idx];
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
    {
        return None;
    }
    let mut idx = enter_idx + 1;
    while idx < scope_markers.len() {
        let candidate = scope_markers[idx];
        if candidate.scope_id.same(marker.scope_id) && matches!(candidate.event, ScopeEvent::Exit) {
            return Some((marker.offset(), candidate.offset()));
        }
        idx += 1;
    }
    None
}

pub(crate) const fn roll_continuation_end(
    scope_markers: &[ScopeMarker],
    roll_enter_idx: usize,
    roll_end: usize,
    eff_len: usize,
) -> usize {
    let roll_start = scope_markers[roll_enter_idx].offset();
    let mut boundary = eff_len;
    let mut idx = 0usize;
    while idx < roll_enter_idx {
        let marker = scope_markers[idx];
        if matches!(marker.event, ScopeEvent::Enter) && marker.offset() <= roll_start {
            let candidate = scope_boundary_for_enter(scope_markers, idx, roll_start);
            if candidate > roll_end && candidate < boundary {
                boundary = candidate;
            }
        }
        idx += 1;
    }
    boundary
}

const fn scope_boundary_for_enter(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
    contained_offset: usize,
) -> usize {
    let marker = scope_markers[enter_idx];
    match marker.scope_id.kind() {
        Some(ScopeKind::Parallel) => {
            let Some((_, left_end, _, right_end)) =
                parallel_arm_ranges_from_enter(scope_markers, enter_idx)
            else {
                return usize::MAX;
            };
            if contained_offset < left_end {
                left_end
            } else {
                right_end
            }
        }
        Some(ScopeKind::Route) => match route_arm_exit_from_enter(scope_markers, enter_idx) {
            Some(end) => end,
            None => usize::MAX,
        },
        Some(ScopeKind::Roll) => {
            let Some((_, end)) = roll_body_range_from_enter(scope_markers, enter_idx) else {
                return usize::MAX;
            };
            end
        }
        None => usize::MAX,
    }
}

const fn route_arm_exit_from_enter(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
) -> Option<usize> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let marker = scope_markers[enter_idx];
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
    {
        return None;
    }
    let mut idx = enter_idx + 1;
    while idx < scope_markers.len() {
        let candidate = scope_markers[idx];
        if candidate.scope_id.same(marker.scope_id)
            && matches!(candidate.scope_id.kind(), Some(ScopeKind::Route))
            && matches!(candidate.event, ScopeEvent::Exit)
        {
            return Some(candidate.offset());
        }
        idx += 1;
    }
    None
}

pub(crate) const fn parallel_arm_ranges_from_enter(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
) -> Option<(usize, usize, usize, usize)> {
    if enter_idx >= scope_markers.len() {
        return None;
    }
    let marker = scope_markers[enter_idx];
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
    {
        return None;
    }
    let mut split = usize::MAX;
    let mut exit = usize::MAX;
    let mut idx = enter_idx + 1;
    while idx < scope_markers.len() {
        let candidate = scope_markers[idx];
        if candidate.scope_id.same(marker.scope_id) {
            match candidate.event {
                ScopeEvent::Split => {
                    if split == usize::MAX {
                        split = candidate.offset();
                    }
                }
                ScopeEvent::Exit => {
                    exit = candidate.offset();
                    break;
                }
                ScopeEvent::Enter => {}
            }
        }
        idx += 1;
    }
    if split == usize::MAX || exit == usize::MAX || marker.offset() > split || split > exit {
        None
    } else {
        Some((marker.offset(), split, split, exit))
    }
}

pub(crate) const fn route_arm_ranges_from_first_enter(
    scope_markers: &[ScopeMarker],
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
    enter_offsets[0] = scope_markers[enter_idx].offset();
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
                        enter_offsets[enter_len] = marker.offset();
                    }
                    enter_len += 1;
                }
                ScopeEvent::Exit => {
                    if exit_len < 2 {
                        exit_offsets[exit_len] = marker.offset();
                    }
                    exit_len += 1;
                }
                ScopeEvent::Split => {}
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
