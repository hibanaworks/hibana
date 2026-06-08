use super::CompiledProgramImage;
use crate::eff::EffIndex;
use crate::global::const_dsl::{ControlScopeKind, ScopeEvent, ScopeId, ScopeKind, ScopeMarker};

use super::super::images::program::RouteControlRecord;

#[inline(always)]
fn compiled_program_route_scope_end(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
    scope: ScopeId,
    default_end: usize,
) -> usize {
    let mut scope_end = default_end;
    let mut scan_idx = enter_idx + 1;
    let mut nest_depth = 1usize;
    while scan_idx < scope_markers.len() {
        let scan_marker = scope_markers[scan_idx];
        if scan_marker.scope_id.local_ordinal() == scope.local_ordinal() {
            match scan_marker.event {
                ScopeEvent::Enter => nest_depth += 1,
                ScopeEvent::Exit => {
                    nest_depth -= 1;
                    if nest_depth == 0 {
                        scope_end = scan_marker.offset;
                        break;
                    }
                }
            }
        }
        scan_idx += 1;
    }
    scope_end
}

#[inline(always)]
pub(in crate::global::compiled) fn compiled_program_route_control_for_scope(
    summary: &CompiledProgramImage,
    scope_id: ScopeId,
) -> Option<RouteControlRecord> {
    if scope_id.is_none() {
        return None;
    }
    let view = summary.view();
    let scope_markers = view.scope_markers();
    let default_end = view.len();
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_kind, ScopeKind::Route)
            && marker.scope_id.canonical().raw() == scope_id.canonical().raw()
        {
            let scope_end = compiled_program_route_scope_end(
                scope_markers,
                marker_idx,
                marker.scope_id,
                default_end,
            );
            let (
                decision_policy_id,
                decision_policy_eff,
                decision_policy_tag,
                decision_policy_subject,
            ) = match view.first_route_head_decision_policy_in_range(
                marker.scope_id,
                marker_idx,
                scope_end,
            ) {
                Some((policy, eff_offset, tag, subject)) => (
                    match policy.dynamic_policy_id() {
                        Some(policy_id) => policy_id,
                        None => crate::global::ControlDesc::STATIC_POLICY_SITE,
                    },
                    EffIndex::from_dense_ordinal(eff_offset),
                    tag,
                    Some(subject),
                ),
                None => (
                    crate::global::ControlDesc::STATIC_POLICY_SITE,
                    EffIndex::MAX,
                    0,
                    None,
                ),
            };
            return Some(RouteControlRecord::new(
                marker.scope_id,
                marker.controller_role,
                decision_policy_id,
                decision_policy_eff,
                decision_policy_tag,
                decision_policy_subject,
            ));
        }
        marker_idx += 1;
    }
    None
}

#[inline(always)]
pub(in crate::global::compiled) const fn control_scope_mask_bit(
    scope_kind: ControlScopeKind,
) -> u8 {
    match scope_kind {
        ControlScopeKind::None => 0,
        ControlScopeKind::Loop => 1 << 0,
        ControlScopeKind::State => 1 << 1,
        ControlScopeKind::Abort => 0,
        ControlScopeKind::Topology => 1 << 3,
        ControlScopeKind::Policy => 0,
        ControlScopeKind::Route => 0,
    }
}
