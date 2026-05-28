use super::super::{
    authority::{Arm, RouteDecisionToken},
    decision_state::{RouteArmCommitProof, RouteState},
    lane_slots::LaneSlotArray,
};
use crate::{
    control::{cap::mint::EpochTable, types::Lane},
    endpoint::{RecvError, RecvResult},
    global::{
        const_dsl::{ScopeId, ScopeKind},
        role_program::LaneSetView,
        typestate::{PhaseCursor, state_index_to_usize},
    },
    rendezvous::port::Port,
    transport::Transport,
};

#[inline]
pub(in crate::endpoint::kernel) fn scope_slot_for_route_from_cursor(
    cursor: &PhaseCursor,
    scope: ScopeId,
) -> Option<usize> {
    if scope.is_none() || scope.kind() != ScopeKind::Route {
        return None;
    }
    cursor.route_scope_slot(scope)
}

#[inline]
pub(in crate::endpoint::kernel) fn is_linger_route_from_cursor(
    cursor: &PhaseCursor,
    scope: ScopeId,
) -> bool {
    cursor
        .scope_region_by_id(scope)
        .map(|region| {
            if region.kind == ScopeKind::Loop {
                return true;
            }
            region.kind == ScopeKind::Route && region.linger
        })
        .unwrap_or(false)
}

pub(in crate::endpoint::kernel) fn preflight_route_arm_commit_from_parts(
    decision_state: &RouteState,
    cursor: &PhaseCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> Option<RouteArmCommitProof> {
    if scope.is_none() || scope.kind() != ScopeKind::Route {
        return None;
    }
    let lane_idx = lane as usize;
    if lane_idx >= cursor.logical_lane_count() {
        return None;
    }
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    decision_state.preflight_route_arm_commit(
        lane_idx,
        scope,
        scope_slot,
        arm,
        is_linger_route_from_cursor(cursor, scope),
    )
}

pub(in crate::endpoint::kernel) fn preflight_route_arm_commit_after_clearing_other_lanes_from_parts(
    decision_state: &RouteState,
    cursor: &PhaseCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> Option<RouteArmCommitProof> {
    if scope.is_none() || scope.kind() != ScopeKind::Route {
        return None;
    }
    let lane_idx = lane as usize;
    if lane_idx >= cursor.logical_lane_count() {
        return None;
    }
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    decision_state.preflight_route_arm_commit_after_clearing_other_lanes(
        lane_idx,
        scope,
        scope_slot,
        arm,
        is_linger_route_from_cursor(cursor, scope),
    )
}

#[inline]
pub(in crate::endpoint::kernel) fn require_route_arm_commit_proof_from_parts(
    decision_state: &RouteState,
    cursor: &PhaseCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> RecvResult<RouteArmCommitProof> {
    preflight_route_arm_commit_from_parts(decision_state, cursor, lane, scope, arm)
        .ok_or(RecvError::PhaseInvariant)
}

#[inline]
fn selected_arm_for_scope_from_parts(
    decision_state: &RouteState,
    cursor: &PhaseCursor,
    scope: ScopeId,
) -> Option<u8> {
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    decision_state.selected_arm_for_scope_slot(scope_slot)
}

#[inline]
pub(in crate::endpoint::kernel::core) fn route_scope_materialization_index_from_cursor(
    cursor: &PhaseCursor,
    scope_id: ScopeId,
) -> Option<usize> {
    if let Some(offer_entry) = cursor.route_scope_offer_entry(scope_id)
        && !offer_entry.is_max()
    {
        return Some(state_index_to_usize(offer_entry));
    }
    cursor
        .scope_region_by_id(scope_id)
        .map(|region| region.start)
}

fn preview_scope_ack_token_non_consuming_from_parts<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    E: EpochTable + 'r,
>(
    ports: &LaneSlotArray<Port<'r, T, E>>,
    decision_state: &RouteState,
    cursor: &PhaseCursor,
    scope_id: ScopeId,
    summary_lane_idx: usize,
    offer_lanes: LaneSetView,
) -> Option<RouteDecisionToken> {
    if let Some(slot) = scope_slot_for_route_from_cursor(cursor, scope_id)
        && let Some(token) = decision_state.scope_evidence.peek_ack(slot)
    {
        return Some(token);
    }
    let lane_limit = cursor.logical_lane_count();
    if summary_lane_idx >= lane_limit {
        return None;
    }
    let mut next = offer_lanes.first_set(lane_limit);
    while let Some(lane_idx) = next {
        let pending = ports
            .get(summary_lane_idx)
            .and_then(|port| port.as_ref())
            .map(|port| {
                port.has_pending_route_decision_for_lane(scope_id, ROLE, Lane::new(lane_idx as u32))
            })
            .unwrap_or(false);
        if !pending {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        }
        let Some(port) = ports.get(lane_idx).and_then(|port| port.as_ref()) else {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        };
        let Some(arm) = port.peek_route_decision(scope_id, ROLE) else {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        };
        if let Some(arm) = Arm::new(arm) {
            return Some(RouteDecisionToken::from_ack(arm));
        }
        next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
    }
    None
}

pub(in crate::endpoint::kernel::core) fn preview_selected_arm_for_scope_from_parts<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    E: EpochTable + 'r,
>(
    ports: &LaneSlotArray<Port<'r, T, E>>,
    decision_state: &RouteState,
    cursor: &PhaseCursor,
    scope_id: ScopeId,
) -> Option<u8> {
    if let Some(arm) = selected_arm_for_scope_from_parts(decision_state, cursor, scope_id) {
        return Some(arm);
    }
    let offer_lanes = cursor
        .route_scope_offer_lane_set(scope_id)
        .unwrap_or(LaneSetView::EMPTY);
    let summary_lane_idx = offer_lanes.first_set(cursor.logical_lane_count())?;
    preview_scope_ack_token_non_consuming_from_parts::<ROLE, T, E>(
        ports,
        decision_state,
        cursor,
        scope_id,
        summary_lane_idx,
        offer_lanes,
    )
    .map(|token| token.arm().as_u8())
    .or_else(|| {
        let slot = scope_slot_for_route_from_cursor(cursor, scope_id)?;
        let mask = decision_state.scope_evidence.poll_ready_arm_mask(slot);
        (mask.count_ones() == 1)
            .then(|| Arm::new(mask.trailing_zeros() as u8))
            .flatten()
            .map(Arm::as_u8)
    })
}
