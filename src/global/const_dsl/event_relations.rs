use super::{
    ScopeKind, ScopeMarkerView, closed_route_arm_ranges_from_first_enter,
    route_arm_ranges_from_first_enter,
};

const fn route_arm_at(
    markers: ScopeMarkerView<'_>,
    route_enter_idx: usize,
    eff_idx: usize,
) -> Option<u8> {
    let [(left_start, left_end), (right_start, right_end)] =
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
        let marker = markers.at(marker_idx);
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && markers.is_first_enter(marker_idx)
            && closed_route_arm_ranges_from_first_enter(markers, marker_idx).is_some()
        {
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
