use crate::{
    g::ProgramSourceError,
    global::{
        compiled::lowering::CompiledProgramImage,
        const_dsl::{
            EffList, ScopeKind, first_visible_controller,
            first_visible_endpoint_selector_conflicts_from_markers,
            local_route_observer_paths_mergeable, route_arm_ranges_from_first_enter,
            validate_parallel_endpoint_selectors, validate_receive_lane_causality,
            validate_roll_reentry_endpoint_selectors,
        },
        role_program::{
            LaneWord, compact_local_step_count, lane_word_count, logical_lane_count_for_role,
        },
    },
};

#[cfg(kani)]
mod kani;
mod passive_child;
pub(crate) use passive_child::validate_passive_child_projection_guarantees;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

const PARTICIPATING_ROLE_BYTES: usize = (u8::MAX as usize + 1) / 8;

const LANE_FACT_WORDS: usize = lane_word_count(u8::MAX as usize + 1);

#[derive(Clone, Copy)]
pub(super) struct ExactRoleFacts {
    pub(super) local_step_count: u16,
    pub(super) active_lane_count: u16,
    pub(super) endpoint_lane_slot_count: u16,
    pub(super) logical_lane_count: u16,
}

#[inline(always)]
const fn lane_word_parts(lane: usize) -> (usize, LaneWord) {
    let bits = LaneWord::BITS as usize;
    (lane / bits, 1u32 << (lane % bits))
}

#[inline(always)]
const fn encode_u16_count(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role count overflow");
    }
    value as u16
}

#[inline(always)]
const fn insert_lane(words: &mut [LaneWord; LANE_FACT_WORDS], lane: usize) -> bool {
    let (word_idx, bit) = lane_word_parts(lane);
    let seen = (words[word_idx] & bit) != 0;
    if !seen {
        words[word_idx] |= bit;
    }
    !seen
}

pub(super) const fn exact_role_facts<const E: usize>(
    eff_list: &EffList<E>,
    role: u8,
) -> ExactRoleFacts {
    let mut active_lanes = [0u32; LANE_FACT_WORDS];
    let mut local_len = 0usize;
    let mut active_lane_count = 0usize;
    let mut endpoint_lane_slot_count = 0usize;
    let mut idx = 0usize;
    while idx < eff_list.len() {
        let atom = eff_list.atom_at(idx);
        if atom.from == role || atom.to == role {
            local_len += 1;
            let lane = atom.lane as usize;
            let lane_slot_count = lane + 1;
            if lane_slot_count > endpoint_lane_slot_count {
                endpoint_lane_slot_count = lane_slot_count;
            }
            if insert_lane(&mut active_lanes, lane) {
                active_lane_count += 1;
            }
        }
        idx += 1;
    }

    if endpoint_lane_slot_count == 0 {
        endpoint_lane_slot_count = 1;
    }
    let logical_lane_count =
        logical_lane_count_for_role(active_lane_count, endpoint_lane_slot_count);

    ExactRoleFacts {
        local_step_count: compact_local_step_count(local_len),
        active_lane_count: encode_u16_count(active_lane_count),
        endpoint_lane_slot_count: encode_u16_count(endpoint_lane_slot_count),
        logical_lane_count: encode_u16_count(logical_lane_count),
    }
}

// A globally absent role has no selector in either route arm, so its observer
// check always accepts. Collect both endpoints; receiver-only roles still matter.
const fn participating_roles<const E: usize>(
    eff_list: &EffList<E>,
) -> [u8; PARTICIPATING_ROLE_BYTES] {
    let mut roles = [0u8; PARTICIPATING_ROLE_BYTES];
    let mut event_idx = 0usize;
    while event_idx < eff_list.len() {
        let atom = eff_list.atom_at(event_idx);
        let sender = atom.from as usize;
        let receiver = atom.to as usize;
        roles[sender / 8] |= 1u8 << (sender % 8);
        roles[receiver / 8] |= 1u8 << (receiver % 8);
        event_idx += 1;
    }
    roles
}

// Global route authority and selector facts do not depend on a projected role.
// Validate them once per scope, then check every role's observer obligation.
pub(crate) const fn validate_route_projection_guarantees<const E: usize>(
    summary: &CompiledProgramImage,
    eff_list: &EffList<E>,
) -> Option<ProgramSourceError> {
    let participants = participating_roles(eff_list);
    let scope_markers = eff_list.scope_markers();
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers.at(marker_idx);
        if matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && marker.event.is_primary_enter()
            && let Some(error) = validate_route_scope(
                summary.compiled_program_role_count(),
                &participants,
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

const fn validate_route_scope<const E: usize>(
    role_count: usize,
    participants: &[u8; PARTICIPATING_ROLE_BYTES],
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
        if (participants[role / 8] & (1u8 << (role % 8))) != 0 {
            let observer_paths_mergeable = local_route_observer_paths_mergeable(
                eff_list, arm0_start, arm0_end, arm1_start, arm1_end, role as u8,
            );
            if !route_role_has_branch_knowledge(role as u8, controller, observer_paths_mergeable) {
                return Some(ProgramSourceError::ProjectionRouteUnprojectable);
            }
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

pub(crate) const fn projection_error_all_roles<const E: usize>(
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
    validate_route_projection_guarantees(summary, eff_list)
}

mod diagnostic;
pub(crate) use diagnostic::projection_diagnostic;
