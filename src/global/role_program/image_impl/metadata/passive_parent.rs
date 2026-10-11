use super::super::super::{
    ROLE_IMAGE_CONFLICT_STRIDE, ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
    RoleImageColumns,
};
use super::super::lane_image::{decode_resident_route_scope, passive_child_parent_matches};
use super::{column_row_offset, decode_route_arm_lane_metadata, read_u16};
use crate::global::typestate::PackedEventConflict;

/// Certify every read skipped by reverse-parent lookup without changing errors.
/// The sorted-scope certificate must additionally establish unique row keys.
/// A false result keeps the original ordered scan, including its early returns.
pub(in crate::global::role_program::image_impl) const fn passive_parent_rows_are_coherent<
    const N: usize,
>(
    bytes: &[u8; N],
    columns: RoleImageColumns,
) -> bool {
    let count = columns.route_scopes.len as usize;
    let Some(arm_count) = count.checked_mul(2) else {
        return false;
    };
    if columns.route_scope_conflicts.len as usize != count
        || columns.route_arms.len as usize != arm_count
    {
        return false;
    }
    let mut slot = 0usize;
    while slot < count {
        let Some(raw) = read_column_u16(
            bytes,
            columns.route_scopes,
            slot,
            ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
            0,
        ) else {
            return false;
        };
        if decode_resident_route_scope(raw).is_none() {
            return false;
        }
        let Some(raw) = read_column_u16(
            bytes,
            columns.route_scope_conflicts,
            slot,
            ROLE_IMAGE_CONFLICT_STRIDE,
            0,
        ) else {
            return false;
        };
        // Direct lookup newly reads even owners not reached by any passive edge.
        if PackedEventConflict::decode_raw(raw).is_none() {
            return false;
        }
        slot += 1;
    }
    let mut lane_step_end = 0usize;
    let mut row = 0usize;
    while row < arm_count {
        let Some(metadata) = decode_route_arm_lane_metadata(bytes, columns.route_arms, row) else {
            return false;
        };
        let Some(event_end) = metadata.event_start.checked_add(metadata.event_len) else {
            return false;
        };
        let Some(end) = lane_step_end.checked_add(metadata.lane_step_len) else {
            return false;
        };
        lane_step_end = end;
        if event_end > columns.events.len as usize
            || ((metadata.event_len == 0) != (metadata.lane_step_len == 0))
            || lane_step_end > columns.route_arm_lane_step_rows.len as usize
        {
            return false;
        }
        let Some(child) = read_column_u16(
            bytes,
            columns.route_arms,
            row,
            ROLE_IMAGE_ROUTE_ARM_STRIDE,
            4,
        ) else {
            return false;
        };
        if child != u16::MAX {
            let parent = row / 2;
            let child = child as usize;
            if child <= parent || child >= count {
                return false;
            }
            let Some(parent_raw) = read_column_u16(
                bytes,
                columns.route_scopes,
                parent,
                ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
                0,
            ) else {
                return false;
            };
            let Some(parent_scope) = decode_resident_route_scope(parent_raw) else {
                return false;
            };
            let Some(child_raw) = read_column_u16(
                bytes,
                columns.route_scopes,
                child,
                ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
                0,
            ) else {
                return false;
            };
            let Some(child_scope) = decode_resident_route_scope(child_raw) else {
                return false;
            };
            let Some(conflict_raw) = read_column_u16(
                bytes,
                columns.route_scope_conflicts,
                child,
                ROLE_IMAGE_CONFLICT_STRIDE,
                0,
            ) else {
                return false;
            };
            let Some(conflict) = PackedEventConflict::decode_raw(conflict_raw) else {
                return false;
            };
            if !passive_child_parent_matches(parent_scope, (row % 2) as u8, child_scope, conflict) {
                return false;
            }
        }
        row += 1;
    }
    true
}

const fn read_column_u16<const N: usize>(
    bytes: &[u8; N],
    column: super::super::super::ColumnRange,
    row: usize,
    stride: usize,
    field: usize,
) -> Option<u16> {
    match column_row_offset(column, row, stride, field) {
        Some(offset) => read_u16(bytes, offset),
        None => None,
    }
}
