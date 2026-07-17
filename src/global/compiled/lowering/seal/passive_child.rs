use crate::{
    g::ProgramSourceError,
    global::const_dsl::{
        ScopeId, ScopeKind, ScopeMarkerView, passive_route_child_scope,
        route_arm_event_ranges_for_scope, route_parent_arm_for_scope, route_scope_slot_for_scope,
    },
};

pub(super) const fn validate_passive_child_projection_guarantees(
    scope_markers: ScopeMarkerView<'_>,
) -> Option<ProgramSourceError> {
    let mut route_slot = 0usize;
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers.at(marker_idx);
        if scope_markers.is_first_enter(marker_idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if route_arm_event_ranges_for_scope(scope_markers, marker.scope_id).is_none() {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
            if let Some(error) =
                validate_passive_arm_child_projection(scope_markers, marker.scope_id, route_slot, 0)
            {
                return Some(error);
            }
            if let Some(error) =
                validate_passive_arm_child_projection(scope_markers, marker.scope_id, route_slot, 1)
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
    route: ScopeId,
    route_slot: usize,
    arm: u8,
) -> Option<ProgramSourceError> {
    let Some(child) = passive_route_child_scope(scope_markers, route, arm) else {
        return None;
    };
    if child.same(route) {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let Some((parent, parent_arm)) = route_parent_arm_for_scope(scope_markers, child) else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if !parent.same(route) || parent_arm != arm {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let Some(child_slot) = route_scope_slot_for_scope(scope_markers, child) else {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    };
    if child_slot <= route_slot || child_slot >= u16::MAX as usize {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    None
}
