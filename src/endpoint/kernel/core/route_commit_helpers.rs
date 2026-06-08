use super::super::{
    authority::{Arm, RouteArmToken},
    decision_state::RouteState,
    lane_slots::LaneSlotArray,
};
use crate::{
    control::{cap::mint::EpochTable, types::Lane},
    endpoint::{RecvError, RecvResult},
    global::{const_dsl::ScopeId, role_program::LaneSetView, typestate::EventCursor},
    rendezvous::port::Port,
    transport::Transport,
};

#[inline]
pub(in crate::endpoint::kernel) fn scope_slot_for_route_from_cursor(
    cursor: &EventCursor,
    scope: ScopeId,
) -> Option<usize> {
    cursor.route_scope_slot(scope)
}

#[inline]
pub(in crate::endpoint::kernel) fn is_linger_route_from_cursor(
    cursor: &EventCursor,
    scope: ScopeId,
) -> bool {
    cursor.route_scope_linger(scope)
}

pub(in crate::endpoint::kernel) fn prepare_selected_route_commit_row_from_parts(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> Option<super::SelectedRouteCommitRow> {
    let lane_idx = lane as usize;
    if lane_idx >= cursor.logical_lane_count() {
        return None;
    }
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    if scope_slot > u16::MAX as usize {
        return None;
    }
    decision_state.preflight_selected_route_commit(
        lane_idx,
        scope,
        scope_slot,
        arm,
        is_linger_route_from_cursor(cursor, scope),
    )
}

pub(in crate::endpoint::kernel) fn prepare_event_selected_route_commit_row_from_event_rows(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    event_idx: usize,
    arm: u8,
) -> Option<super::SelectedRouteCommitRow> {
    let route_scope = cursor.route_scope_for_event_arm(event_idx, arm)?;
    prepare_selected_route_commit_row_from_parts(decision_state, cursor, lane, route_scope, arm)
}

pub(in crate::endpoint::kernel) fn event_selected_route_scope_from_event_rows(
    cursor: &EventCursor,
    event_idx: usize,
    arm: u8,
) -> Option<ScopeId> {
    cursor.route_scope_for_event_arm(event_idx, arm)
}

#[inline]
pub(in crate::endpoint::kernel) fn require_selected_route_commit_row_from_parts(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
) -> RecvResult<super::SelectedRouteCommitRow> {
    prepare_selected_route_commit_row_from_parts(decision_state, cursor, lane, scope, arm)
        .ok_or(RecvError::PhaseInvariant)
}

#[inline]
fn selected_arm_for_scope_from_parts(
    decision_state: &RouteState,
    cursor: &EventCursor,
    scope: ScopeId,
) -> Option<u8> {
    let scope_slot = scope_slot_for_route_from_cursor(cursor, scope)?;
    decision_state.selected_arm_for_scope_slot(scope_slot)
}

fn preview_scope_ack_token_non_consuming_from_parts<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    E: EpochTable + 'r,
>(
    ports: &LaneSlotArray<Port<'r, T, E>>,
    decision_state: &RouteState,
    cursor: &EventCursor,
    scope_id: ScopeId,
    summary_lane_idx: usize,
    offer_lanes: LaneSetView,
) -> Option<RouteArmToken> {
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
                port.has_pending_route_arm_selection_for_lane(
                    scope_id,
                    ROLE,
                    Lane::new(lane_idx as u32),
                )
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
        let Some(arm) = port.peek_route_arm_selection(scope_id, ROLE) else {
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
            continue;
        };
        if let Some(arm) = Arm::new(arm) {
            return Some(RouteArmToken::from_ack(arm));
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
    cursor: &EventCursor,
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
