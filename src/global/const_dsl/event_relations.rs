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

/// Exact route-path equivalence for one contiguous event interval. The IDs
/// are temporary lowering scratch, not wire labels or persisted metadata.
/// Each closed route refines the current partition by left/right/outside
/// membership, so equality is identical to `events_share_route_path`.
pub(super) struct RoutePathClasses<const E: usize> {
    ids: [u16; E],
    start: usize,
    cached: bool,
}

impl<const E: usize> RoutePathClasses<E> {
    pub(super) const fn new(markers: ScopeMarkerView<'_>, start: usize, end: usize) -> Self {
        let mut result = Self {
            ids: [0; E],
            start,
            cached: end - start <= u16::MAX as usize,
        };
        // Published scope offsets already fit u16. Keep the original relation
        // for larger private inputs as well, without introducing a new limit.
        if !result.cached || start == end {
            return result;
        }
        let mut remap = [u16::MAX; E];
        let mut class_count = 1usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                && markers.is_first_enter(marker_idx)
                && let Some([(left_start, split), (_, right_end)]) =
                    closed_route_arm_ranges_from_first_enter(markers, marker_idx)
                // Uniform membership cannot split any current class. Testing
                // both endpoints alone would miss a route inside the body.
                && !(end <= left_start
                    || right_end <= start
                    || (left_start <= start && end <= split)
                    || (split <= start && end <= right_end))
            {
                let mut next_id = 0u16;
                let mut category = 0u8;
                while category < 3 {
                    let mut reset_idx = 0usize;
                    while reset_idx < class_count {
                        remap[reset_idx] = u16::MAX;
                        reset_idx += 1;
                    }
                    let mut event_idx = start;
                    while event_idx < end {
                        let membership = if left_start <= event_idx && event_idx < split {
                            0
                        } else if split <= event_idx && event_idx < right_end {
                            1
                        } else {
                            2
                        };
                        if membership == category {
                            // An event is visited in exactly one category, so
                            // this slot still contains its previous class ID.
                            let key = result.ids[event_idx - start] as usize;
                            if remap[key] == u16::MAX {
                                remap[key] = next_id;
                                next_id += 1;
                            }
                            result.ids[event_idx - start] = remap[key];
                        }
                        event_idx += 1;
                    }
                    category += 1;
                }
                class_count = next_id as usize;
            }
            marker_idx += 1;
        }
        result
    }

    pub(super) const fn share_path(
        &self,
        markers: ScopeMarkerView<'_>,
        left: usize,
        right: usize,
    ) -> bool {
        if self.cached {
            self.ids[left - self.start] == self.ids[right - self.start]
        } else {
            events_share_route_path(markers, left, right)
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
