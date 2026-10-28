use crate::{
    g::ProgramSourceError,
    global::const_dsl::{
        ScopeKind, ScopeMarkerView, StructuredScopeRange, closed_route_arm_ranges_from_first_enter,
        innermost_scope_range, passive_route_child_enter,
    },
};

pub(crate) const fn validate_passive_child_projection_guarantees(
    scope_markers: ScopeMarkerView<'_>,
) -> Option<ProgramSourceError> {
    let mut route_slot = 0usize;
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers.at(marker_idx);
        if scope_markers.is_first_enter(marker_idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if closed_route_arm_ranges_from_first_enter(scope_markers, marker_idx).is_none() {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            if let Some(error) =
                validate_passive_arm_child_projection(scope_markers, marker_idx, route_slot, 0)
            {
                return Some(error);
            }
            if let Some(error) =
                validate_passive_arm_child_projection(scope_markers, marker_idx, route_slot, 1)
            {
                return Some(error);
            }
            route_slot += 1;
        }
        marker_idx += 1;
    }
    None
}

const fn validate_passive_arm_child_projection(
    scope_markers: ScopeMarkerView<'_>,
    route_enter: usize,
    route_slot: usize,
    arm: u8,
) -> Option<ProgramSourceError> {
    let Some(child_enter) = passive_route_child_enter(scope_markers, route_enter, arm) else {
        return None;
    };
    let route = scope_markers.at(route_enter).scope_id;
    let child = scope_markers.at(child_enter).scope_id;
    if child.same(route) {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let Some([(start, _), (_, end)]) =
        closed_route_arm_ranges_from_first_enter(scope_markers, child_enter)
    else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    // Parent range and route slot come from the same ordered traversal. Neither
    // requires finding the already-known child or parent enter a second time.
    let mut parent: Option<StructuredScopeRange> = None;
    let mut parent_arm = 0u8;
    let mut child_slot = None;
    let mut slot = 0usize;
    let mut index = 0usize;
    while index < scope_markers.len() {
        let marker = scope_markers.at(index);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if index == child_enter {
                child_slot = Some(slot);
            }
            if !marker.scope_id.same(child) {
                let Some(ranges) = closed_route_arm_ranges_from_first_enter(scope_markers, index)
                else {
                    crate::invariant();
                };
                let mut candidate_arm = 0usize;
                while candidate_arm < 2 {
                    let (first, stop) = ranges[candidate_arm];
                    if first <= start && end <= stop {
                        let candidate = StructuredScopeRange::new(marker.scope_id, first, stop);
                        let selected = match parent {
                            Some(current) => innermost_scope_range(current, candidate),
                            None => candidate,
                        };
                        if selected.scope().same(marker.scope_id) {
                            parent_arm = candidate_arm as u8;
                        }
                        parent = Some(selected);
                    }
                    candidate_arm += 1;
                }
            }
            slot += 1;
        }
        index += 1;
    }
    let Some(parent) = parent else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if !parent.scope().same(route) || parent_arm != arm {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let Some(child_slot) = child_slot else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if child_slot <= route_slot || child_slot >= u16::MAX as usize {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    None
}
