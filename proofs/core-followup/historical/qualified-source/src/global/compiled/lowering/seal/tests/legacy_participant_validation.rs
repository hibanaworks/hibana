// Frozen from a6339772 before production edit; test-only differential oracle.
use super::*;

// Global route authority and selector facts do not depend on a projected role.
// Validate them once per scope, then check every role's observer obligation.
pub(super) const fn legacy_route_projection_guarantees<const E: usize>(
    summary: &CompiledProgramImage,
    eff_list: &EffList<E>,
) -> Option<ProgramSourceError> {
    let scope_markers = eff_list.scope_markers();
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers.at(marker_idx);
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && marker.event.is_primary_enter()
            && let Some(error) = legacy_route_scope(
                summary.compiled_program_role_count(),
                eff_list,
                scope_markers,
                marker_idx,
            )
        {
            return Some(error);
        }
        marker_idx += 1;
    }
    None
}

const fn legacy_route_scope<const E: usize>(
    role_count: usize,
    eff_list: &EffList<E>,
    scope_markers: crate::global::const_dsl::ScopeMarkerView<'_>,
    route_enter_marker_idx: usize,
) -> Option<ProgramSourceError> {
    let [(arm0_start, arm0_end), (arm1_start, arm1_end)] =
        route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
    let route_scope = scope_markers.at(route_enter_marker_idx).scope_id;
    let has_dynamic_resolver = scope_has_dynamic_resolver(eff_list, route_scope);
    let controller = match first_visible_controller(eff_list, arm0_start, arm0_end)
        .merge(first_visible_controller(eff_list, arm1_start, arm1_end))
        .unique()
    {
        Some(controller) => controller,
        None => return Some(ProgramSourceError::RouteControllerMismatch),
    };
    if !has_dynamic_resolver
        && first_visible_endpoint_selector_conflicts_from_markers(
            eff_list,
            arm0_start,
            arm0_end,
            arm1_start,
            arm1_end,
            route_enter_marker_idx + 1,
            route_enter_marker_idx + 1,
        )
    {
        return Some(ProgramSourceError::ProjectionRouteUnprojectable);
    }
    let mut role = 0usize;
    while role < role_count {
        let observer_paths_mergeable = local_route_observer_paths_mergeable(
            eff_list, arm0_start, arm0_end, arm1_start, arm1_end, role as u8,
        );
        if !route_role_has_branch_knowledge(role as u8, controller, observer_paths_mergeable) {
            return Some(ProgramSourceError::ProjectionRouteUnprojectable);
        }
        role += 1;
    }
    None
}

#[inline(always)]
const fn route_role_has_branch_knowledge(
    role: u8,
    controller: u8,
    observer_paths_mergeable: bool,
) -> bool {
    role == controller || observer_paths_mergeable
}

#[inline(always)]
const fn scope_has_dynamic_resolver<const E: usize>(
    eff_list: &EffList<E>,
    route_scope: crate::global::const_dsl::ScopeId,
) -> bool {
    eff_list.resolver_for_scope(route_scope).is_some()
}

pub(crate) const fn legacy_projection_error_all_roles<const E: usize>(
    summary: &CompiledProgramImage,
    eff_list: &EffList<E>,
) -> Option<ProgramSourceError> {
    if !validate_receive_lane_causality(eff_list) {
        return Some(ProgramSourceError::ReceiveLaneCausalityConflict);
    }
    if !validate_parallel_endpoint_selectors(eff_list) {
        return Some(ProgramSourceError::ParallelAmbiguousEndpointSelector);
    }
    if !validate_roll_reentry_endpoint_selectors(eff_list) {
        return Some(ProgramSourceError::ReentryAmbiguousEndpointSelector);
    }
    if let Some(error) =
        passive_child::validate_passive_child_projection_guarantees(eff_list.scope_markers())
    {
        return Some(error);
    }
    legacy_route_projection_guarantees(summary, eff_list)
}
