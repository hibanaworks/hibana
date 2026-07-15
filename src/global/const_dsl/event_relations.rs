use super::{
    ScopeEvent, ScopeKind, ScopeMarkerView, parallel_arm_ranges_from_enter,
    route_arm_ranges_from_first_enter,
};

const fn first_route_enter(markers: ScopeMarkerView<'_>, marker_idx: usize) -> bool {
    let marker = markers.at(marker_idx);
    if !matches!(marker.event, ScopeEvent::Enter)
        || !matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
    {
        return false;
    }
    let mut idx = 0usize;
    while idx < marker_idx {
        let candidate = markers.at(idx);
        if matches!(candidate.event, ScopeEvent::Enter) && candidate.scope_id.same(marker.scope_id)
        {
            return false;
        }
        idx += 1;
    }
    true
}

const fn route_has_two_arms(markers: ScopeMarkerView<'_>, marker_idx: usize) -> bool {
    let scope = markers.at(marker_idx).scope_id;
    let mut enters = 0usize;
    let mut idx = marker_idx;
    while idx < markers.len() {
        let candidate = markers.at(idx);
        if matches!(candidate.event, ScopeEvent::Enter) && candidate.scope_id.same(scope) {
            enters += 1;
        }
        idx += 1;
    }
    enters == 2
}

const fn route_arm_at(
    markers: ScopeMarkerView<'_>,
    route_enter_idx: usize,
    eff_idx: usize,
) -> Option<u8> {
    let (_, left_start, left_end, _, right_start, right_end) =
        route_arm_ranges_from_first_enter(markers, route_enter_idx);
    if left_start <= eff_idx && eff_idx < left_end {
        Some(0)
    } else if right_start <= eff_idx && eff_idx < right_end {
        Some(1)
    } else {
        None
    }
}

pub(super) const fn events_share_route_path(
    markers: ScopeMarkerView<'_>,
    left_eff_idx: usize,
    right_eff_idx: usize,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        if first_route_enter(markers, marker_idx) && route_has_two_arms(markers, marker_idx) {
            match (
                route_arm_at(markers, marker_idx, left_eff_idx),
                route_arm_at(markers, marker_idx, right_eff_idx),
            ) {
                (None, None) | (Some(0), Some(0)) | (Some(1), Some(1)) => {}
                _ => return false,
            }
        }
        marker_idx += 1;
    }
    true
}

const fn parallel_arm_at(
    markers: ScopeMarkerView<'_>,
    parallel_enter_idx: usize,
    eff_idx: usize,
) -> Option<u8> {
    let Some((left_start, left_end, right_start, right_end)) =
        parallel_arm_ranges_from_enter(markers, parallel_enter_idx)
    else {
        panic!("parallel scope arm range missing");
    };
    if left_start <= eff_idx && eff_idx < left_end {
        Some(0)
    } else if right_start <= eff_idx && eff_idx < right_end {
        Some(1)
    } else {
        None
    }
}

pub(super) const fn events_are_route_exclusive(
    markers: ScopeMarkerView<'_>,
    left_eff_idx: usize,
    right_eff_idx: usize,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        if first_route_enter(markers, marker_idx) {
            let left_arm = route_arm_at(markers, marker_idx, left_eff_idx);
            let right_arm = route_arm_at(markers, marker_idx, right_eff_idx);
            if matches!(
                (left_arm, right_arm),
                (Some(0), Some(1)) | (Some(1), Some(0))
            ) {
                return true;
            }
        }
        marker_idx += 1;
    }
    false
}

pub(super) const fn events_are_parallel(
    markers: ScopeMarkerView<'_>,
    left_eff_idx: usize,
    right_eff_idx: usize,
) -> bool {
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let left_arm = parallel_arm_at(markers, marker_idx, left_eff_idx);
            let right_arm = parallel_arm_at(markers, marker_idx, right_eff_idx);
            if matches!(
                (left_arm, right_arm),
                (Some(0), Some(1)) | (Some(1), Some(0))
            ) {
                return true;
            }
        }
        marker_idx += 1;
    }
    false
}

pub(super) const fn events_are_locally_ordered(
    markers: ScopeMarkerView<'_>,
    earlier_eff_idx: usize,
    later_eff_idx: usize,
) -> bool {
    earlier_eff_idx < later_eff_idx
        && !events_are_parallel(markers, earlier_eff_idx, later_eff_idx)
        && !events_are_route_exclusive(markers, earlier_eff_idx, later_eff_idx)
}
