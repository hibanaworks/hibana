use super::super::{
    authority::{Arm, RouteArmToken},
    decision_state::RouteState,
    lane_slots::LaneSlotArray,
};
use crate::{
    endpoint::kernel::decision_state::SelectedRouteCommitRows,
    endpoint::{RecvError, RecvResult},
    global::{
        const_dsl::ScopeId,
        role_program::LaneSetView,
        typestate::{EventCursor, PackedEventConflict},
    },
    rendezvous::port::Port,
    session::types::Lane,
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

fn prepare_selected_route_commit_row_from_parts(
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

pub(in crate::endpoint::kernel) fn prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    event_idx: usize,
    arm: u8,
    rows: &mut SelectedRouteCommitRows,
) -> RecvResult<()> {
    prepare_selected_route_commit_rows_from_resident_route_commit_range(
        decision_state,
        cursor,
        lane,
        cursor.event_conflict_for_index(event_idx),
        Some(arm),
        rows,
    )
}

pub(in crate::endpoint::kernel::core) fn prepare_route_site_materialization_rows_from_resident_route_commit_range(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
    rows: &mut SelectedRouteCommitRows,
) -> RecvResult<()> {
    validate_scope_arm(scope, arm)?;
    prepare_selected_route_commit_rows_from_resident_route_commit_range(
        decision_state,
        cursor,
        lane,
        PackedEventConflict::route_arm(scope, arm),
        Some(arm),
        rows,
    )
}

pub(in crate::endpoint::kernel) fn prepare_descriptor_checked_recv_linger_rows_from_resident_route_commit_range(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    scope: ScopeId,
    arm: u8,
    rows: &mut SelectedRouteCommitRows,
) -> RecvResult<()> {
    validate_scope_arm(scope, arm)?;
    prepare_selected_route_commit_rows_from_resident_route_commit_range(
        decision_state,
        cursor,
        lane,
        PackedEventConflict::route_arm(scope, arm),
        Some(arm),
        rows,
    )
}

#[inline]
fn validate_scope_arm(scope: ScopeId, arm: u8) -> RecvResult<()> {
    if scope.is_none() || arm > 1 {
        return Err(RecvError::PhaseInvariant);
    }
    Ok(())
}

fn prepare_selected_route_commit_rows_from_resident_route_commit_range(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    conflict: PackedEventConflict,
    first_arm: Option<u8>,
    rows: &mut SelectedRouteCommitRows,
) -> RecvResult<()> {
    let range = cursor
        .route_commit_range_for_conflict(conflict, first_arm)
        .ok_or(RecvError::PhaseInvariant)?;
    let mut idx = 0usize;
    while idx < range.len() {
        let route_row = cursor
            .route_commit_row_at(range, idx)
            .and_then(super::SelectedRouteCommitRow::from_resident_conflict)
            .ok_or(RecvError::PhaseInvariant)?;
        let scope = route_row.scope();
        let arm = route_row.selected_arm();
        if let Some(existing) = rows.arm_for_scope(cursor, scope)
            && existing != arm
        {
            return Err(RecvError::PhaseInvariant);
        }
        let row =
            prepare_selected_route_commit_row_from_parts(decision_state, cursor, lane, scope, arm)
                .ok_or(RecvError::PhaseInvariant)?;
        if row.scope() != scope || row.selected_arm() != arm {
            return Err(RecvError::PhaseInvariant);
        }
        idx += 1;
    }
    rows.merge_chain(cursor, lane, conflict, first_arm)
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

fn preview_scope_ack_token_non_consuming_from_parts<'r, const ROLE: u8, T: Transport + 'r>(
    ports: &LaneSlotArray<Port<'r, T>>,
    decision_state: &RouteState,
    cursor: &EventCursor,
    scope_id: ScopeId,
    offer_lanes: LaneSetView,
) -> Option<RouteArmToken> {
    if let Some(slot) = scope_slot_for_route_from_cursor(cursor, scope_id)
        && let Some(token) = decision_state.scope_evidence.peek_ack(slot)
    {
        return Some(token);
    }
    let lane_limit = cursor.logical_lane_count();
    let mut next = offer_lanes.first_set(lane_limit);
    while let Some(lane_idx) = next {
        let pending = ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .is_some_and(|port| {
                port.has_pending_route_arm_selection_for_lane(
                    scope_id,
                    ROLE,
                    Lane::new(lane_idx as u32),
                )
            });
        if !pending {
            next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
            continue;
        }
        let Some(port) = ports.get(lane_idx).and_then(|port| port.as_ref()) else {
            next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
            continue;
        };
        let Some(arm) = port.peek_route_arm_selection(scope_id, ROLE) else {
            next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
            continue;
        };
        if let Some(arm) = Arm::new(arm) {
            return Some(RouteArmToken::from_ack(arm));
        }
        next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
    }
    None
}

pub(in crate::endpoint::kernel::core) fn preview_selected_arm_for_scope_from_parts<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
>(
    ports: &LaneSlotArray<Port<'r, T>>,
    decision_state: &RouteState,
    cursor: &EventCursor,
    scope_id: ScopeId,
) -> Option<u8> {
    if let Some(arm) = selected_arm_for_scope_from_parts(decision_state, cursor, scope_id) {
        return Some(arm);
    }
    let offer_lanes = match cursor.route_scope_offer_lane_set(scope_id) {
        Some(lanes) => lanes,
        None => LaneSetView::EMPTY,
    };
    preview_scope_ack_token_non_consuming_from_parts::<ROLE, T>(
        ports,
        decision_state,
        cursor,
        scope_id,
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
