use super::super::{authority::Arm, decision_state::RouteState};
use crate::{
    endpoint::kernel::decision_state::SelectedRouteCommitRows,
    endpoint::{RecvError, RecvResult},
    global::{
        const_dsl::{ReentryMark, ScopeId},
        typestate::{EventCursor, PackedEventConflict},
    },
};

#[inline]
pub(in crate::endpoint::kernel) fn scope_slot_for_route_from_cursor(
    cursor: &EventCursor,
    scope: ScopeId,
) -> Option<usize> {
    cursor.route_scope_slot(scope)
}

#[inline]
fn route_reentry_from_cursor(cursor: &EventCursor, scope: ScopeId) -> ReentryMark {
    if cursor.route_scope_reentry(scope) {
        ReentryMark::Reentrant
    } else {
        ReentryMark::SinglePass
    }
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
    let reentry = route_reentry_from_cursor(cursor, scope);
    if reentry.is_reentrant()
        && let Some(existing) = selected_arm_for_scope_from_parts(decision_state, cursor, scope)
    {
        let mut selected_arm_for_scope =
            |candidate| selected_arm_for_scope_from_parts(decision_state, cursor, candidate);
        if cursor.reentrant_route_arm_event_row_done(scope, existing, &mut selected_arm_for_scope) {
            return super::SelectedRouteCommitRow::from_resident_conflict(
                PackedEventConflict::route_arm(scope, arm),
            );
        }
        if existing != arm {
            return None;
        }
    }
    decision_state.preflight_selected_route_commit(lane_idx, scope, scope_slot, arm, reentry)
}

pub(in crate::endpoint::kernel) fn prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
    decision_state: &RouteState,
    cursor: &EventCursor,
    lane: u8,
    event_idx: usize,
    rows: &mut SelectedRouteCommitRows,
) -> RecvResult<()> {
    prepare_selected_route_commit_rows_from_resident_route_commit_range(
        decision_state,
        cursor,
        lane,
        cursor.event_conflict_for_index(event_idx),
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
        rows,
    )
}

pub(in crate::endpoint::kernel) fn prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range(
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
    rows: &mut SelectedRouteCommitRows,
) -> RecvResult<()> {
    let range = cursor
        .route_commit_range_for_conflict(conflict)
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
    rows.merge_chain(cursor, lane, conflict)
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

pub(in crate::endpoint::kernel::core) fn preview_selected_arm_for_scope_from_parts(
    decision_state: &RouteState,
    cursor: &EventCursor,
    scope_id: ScopeId,
) -> Option<u8> {
    if let Some(arm) = selected_arm_for_scope_from_parts(decision_state, cursor, scope_id) {
        return Some(arm);
    }
    scope_slot_for_route_from_cursor(cursor, scope_id)
        .and_then(|slot| decision_state.scope_evidence.peek_ack(slot))
        .map(|token| token.arm().as_u8())
        .or_else(|| {
            let slot = scope_slot_for_route_from_cursor(cursor, scope_id)?;
            let mask = decision_state.scope_evidence.poll_ready_arm_mask(slot);
            Arm::from_single_ready_mask(mask).map(Arm::as_u8)
        })
}
