use super::super::super::{LANE_DOMAIN_SIZE, PackedLaneRange, RouteArmLaneStepRow};
use crate::global::{
    const_dsl::{ScopeId, ScopeKind},
    typestate::{LocalConflict, PackedEventConflict},
};

pub(in crate::global::role_program::image_impl) const fn decode_resident_route_scope(
    raw: u16,
) -> Option<ScopeId> {
    let scope = match ScopeId::decode_raw(raw) {
        Some(scope) => scope,
        None => return None,
    };
    if !matches!(scope.kind(), Some(ScopeKind::Route)) {
        return None;
    }
    Some(scope)
}

pub(in crate::global::role_program::image_impl) const fn decode_resident_roll_scope(
    raw_ordinal: u16,
) -> Option<ScopeId> {
    if raw_ordinal >= ScopeId::LOCAL_CAPACITY {
        return None;
    }
    Some(ScopeId::roll_scope(raw_ordinal))
}

pub(in crate::global::role_program::image_impl) const fn decode_resident_local_step_lane(
    raw: u8,
    logical_lane_count: usize,
) -> Option<u8> {
    if logical_lane_count == 0
        || logical_lane_count > LANE_DOMAIN_SIZE
        || raw as usize >= logical_lane_count
    {
        return None;
    }
    Some(raw)
}

pub(in crate::global::role_program::image_impl) const fn route_commit_decisions_match(
    left: PackedEventConflict,
    right: PackedEventConflict,
) -> bool {
    match (left.to_conflict(), right.to_conflict()) {
        (
            Some(LocalConflict::RouteArm {
                scope: left_scope,
                arm: left_arm,
            }),
            Some(LocalConflict::RouteArm {
                scope: right_scope,
                arm: right_arm,
            }),
        ) => left_scope.same(right_scope) && left_arm == right_arm,
        _ => false,
    }
}

pub(in crate::global::role_program::image_impl) const fn passive_child_parent_matches(
    parent_scope: ScopeId,
    arm: u8,
    child_scope: ScopeId,
    child_conflict: PackedEventConflict,
) -> bool {
    if child_scope.same(parent_scope) {
        return false;
    }
    matches!(
        child_conflict.to_conflict(),
        Some(LocalConflict::RouteArm {
            scope: recorded_parent,
            arm: recorded_arm,
        }) if recorded_parent.same(parent_scope) && recorded_arm == arm
    )
}

pub(in crate::global::role_program::image_impl) const fn decode_resident_route_arm_lane_step(
    row: RouteArmLaneStepRow,
    logical_lane_count: usize,
    event_row: PackedLaneRange,
) -> Option<RouteArmLaneStepRow> {
    if logical_lane_count == 0
        || logical_lane_count > LANE_DOMAIN_SIZE
        || row.lane() as usize >= logical_lane_count
        || event_row.is_absent_or_zero_len()
        || (row.first_step() as usize) < event_row.start()
        || row.first_step() > row.last_step()
        || row.last_step() as usize >= event_row.end()
    {
        return None;
    }
    Some(row)
}
