use crate::{
    g::ProgramSourceError,
    global::{
        compiled::lowering::CompiledProgramView,
        const_dsl::{ScopeEvent, ScopeId, ScopeKind, ScopeMarker},
    },
};

#[inline(always)]
pub(super) const fn validate_passive_child_projection_guarantees(
    view: &CompiledProgramView<'_>,
) -> Option<ProgramSourceError> {
    let scope_markers = view.scope_markers();
    let mut route_slot = 0usize;
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if first_enter_for_scope(scope_markers, marker_idx)
            && matches!(marker.scope_kind, ScopeKind::Route)
        {
            let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
                route_arm_ranges(scope_markers, marker.scope_id);
            if let Some(error) = validate_passive_arm_child_projection(
                scope_markers,
                marker.scope_id,
                route_slot,
                0,
                arm0_start,
                arm0_end,
            ) {
                return Some(error);
            }
            if let Some(error) = validate_passive_arm_child_projection(
                scope_markers,
                marker.scope_id,
                route_slot,
                1,
                arm1_start,
                arm1_end,
            ) {
                return Some(error);
            }
            route_slot += 1;
        }
        marker_idx += 1;
    }
    None
}

const fn validate_passive_arm_child_projection(
    scope_markers: &[ScopeMarker],
    route: ScopeId,
    route_slot: usize,
    arm: u8,
    arm_start: usize,
    arm_end: usize,
) -> Option<ProgramSourceError> {
    let Some(child) = passive_child_route_scope(scope_markers, route, arm, arm_start, arm_end)
    else {
        return None;
    };
    if child.canonical().raw() == route.canonical().raw() {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let Some((parent, parent_arm)) = nearest_route_parent_for_scope(scope_markers, child) else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if parent.canonical().raw() != route.canonical().raw() || parent_arm != arm {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let Some(child_slot) = route_scope_slot_for_scope(scope_markers, child) else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if child_slot <= route_slot || child_slot - route_slot > u8::MAX as usize {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    None
}

const fn passive_child_route_scope(
    scope_markers: &[ScopeMarker],
    route: ScopeId,
    arm: u8,
    arm_start: usize,
    arm_end: usize,
) -> Option<ScopeId> {
    let Some(idx) = passive_child_route_enter_index(scope_markers, route, arm, arm_start, arm_end)
    else {
        return None;
    };
    Some(scope_markers[idx].scope_id)
}

const fn passive_child_route_enter_index(
    scope_markers: &[ScopeMarker],
    route: ScopeId,
    arm: u8,
    arm_start: usize,
    arm_end: usize,
) -> Option<usize> {
    if route.is_none() || arm > 1 || arm_start >= arm_end {
        return None;
    }
    let mut child_idx = usize::MAX;
    let mut child_span = usize::MAX;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers[idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_kind, ScopeKind::Route)
            && marker.scope_id.canonical().raw() != route.canonical().raw()
            && marker.offset == arm_start
        {
            let (_, _, left_end, _, _, right_end) =
                route_arm_ranges(scope_markers, marker.scope_id);
            let end = if left_end > right_end {
                left_end
            } else {
                right_end
            };
            if end <= arm_end {
                let span = end - arm_start;
                if child_idx == usize::MAX || span > child_span {
                    child_idx = idx;
                    child_span = span;
                }
            }
        }
        idx += 1;
    }
    if child_idx == usize::MAX {
        None
    } else {
        Some(child_idx)
    }
}

const fn nearest_route_parent_for_scope(
    scope_markers: &[ScopeMarker],
    scope: ScopeId,
) -> Option<(ScopeId, u8)> {
    let Some(start) = scope_first_enter_offset(scope_markers, scope) else {
        return None;
    };
    let Some(end) = scope_full_end(scope_markers, scope) else {
        return None;
    };
    let mut best_scope = ScopeId::none();
    let mut best_arm = 0u8;
    let mut best_span = usize::MAX;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers[idx];
        if first_enter_for_scope(scope_markers, idx)
            && matches!(marker.scope_kind, ScopeKind::Route)
            && marker.scope_id.canonical().raw() != scope.canonical().raw()
        {
            let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
                route_arm_ranges(scope_markers, marker.scope_id);
            if start >= arm0_start && end <= arm0_end {
                let span = arm0_end - arm0_start;
                if best_scope.is_none() || span < best_span {
                    best_scope = marker.scope_id;
                    best_arm = 0;
                    best_span = span;
                }
            }
            if start >= arm1_start && end <= arm1_end {
                let span = arm1_end - arm1_start;
                if best_scope.is_none() || span < best_span {
                    best_scope = marker.scope_id;
                    best_arm = 1;
                    best_span = span;
                }
            }
        }
        idx += 1;
    }
    if best_scope.is_none() {
        None
    } else {
        Some((best_scope, best_arm))
    }
}

const fn scope_full_end(scope_markers: &[ScopeMarker], scope: ScopeId) -> Option<usize> {
    let Some(enter_idx) = scope_first_enter_index(scope_markers, scope) else {
        return None;
    };
    let marker = scope_markers[enter_idx];
    if matches!(marker.scope_kind, ScopeKind::Route) {
        let (_, _, left_end, _, _, right_end) = route_arm_ranges(scope_markers, scope);
        Some(if left_end > right_end {
            left_end
        } else {
            right_end
        })
    } else {
        Some(scope_segment_end(scope_markers, enter_idx, usize::MAX))
    }
}

const fn scope_first_enter_offset(scope_markers: &[ScopeMarker], scope: ScopeId) -> Option<usize> {
    let Some(idx) = scope_first_enter_index(scope_markers, scope) else {
        return None;
    };
    Some(scope_markers[idx].offset)
}

const fn scope_first_enter_index(scope_markers: &[ScopeMarker], scope: ScopeId) -> Option<usize> {
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers[idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && marker.scope_id.canonical().raw() == scope.canonical().raw()
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

const fn scope_segment_end(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
    segment_limit: usize,
) -> usize {
    let marker = scope_markers[enter_idx];
    let mut scan = enter_idx + 1;
    while scan < scope_markers.len() {
        let candidate = scope_markers[scan];
        if matches!(candidate.event, ScopeEvent::Exit)
            && candidate.scope_id.canonical().raw() == marker.scope_id.canonical().raw()
        {
            return candidate.offset;
        }
        scan += 1;
    }
    segment_limit
}

const fn route_scope_slot_for_scope(
    scope_markers: &[ScopeMarker],
    route: ScopeId,
) -> Option<usize> {
    let mut slot = 0usize;
    let mut idx = 0usize;
    while idx < scope_markers.len() {
        let marker = scope_markers[idx];
        if first_enter_for_scope(scope_markers, idx)
            && matches!(marker.scope_kind, ScopeKind::Route)
        {
            if marker.scope_id.canonical().raw() == route.canonical().raw() {
                return Some(slot);
            }
            slot += 1;
        }
        idx += 1;
    }
    None
}

const fn first_enter_for_scope(scope_markers: &[ScopeMarker], marker_idx: usize) -> bool {
    let marker = scope_markers[marker_idx];
    if !matches!(marker.event, ScopeEvent::Enter) {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = scope_markers[idx];
        if matches!(candidate.event, ScopeEvent::Enter)
            && candidate.scope_id.canonical().raw() == marker.scope_id.canonical().raw()
        {
            return false;
        }
        idx += 1;
    }
    true
}

const fn route_arm_ranges(
    scope_markers: &[ScopeMarker],
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
