use super::{
    ColumnRange, ROLE_IMAGE_LANE_RANGE_STRIDE, ROLE_IMAGE_ROLL_SCOPE_STRIDE, ScopeId,
    column_row_offset, read_u16, read_u32,
};

#[derive(Clone, Copy)]
struct RollScopeMetadata {
    scope: u16,
    start: usize,
    end: usize,
}

const fn decode_roll_scope_metadata<const N: usize>(
    bytes: &[u8; N],
    roll_scopes: ColumnRange,
    row: usize,
    event_count: usize,
) -> Option<RollScopeMetadata> {
    let offset = match column_row_offset(roll_scopes, row, ROLE_IMAGE_ROLL_SCOPE_STRIDE, 0) {
        Some(offset) => offset,
        None => return None,
    };
    let scope = match read_u16(bytes, offset) {
        Some(scope) if scope < ScopeId::LOCAL_CAPACITY => scope,
        Some(_) | None => return None,
    };
    let raw = match read_u32(bytes, offset + 2) {
        Some(raw) if raw != u32::MAX => raw,
        Some(_) | None => return None,
    };
    let start = (raw >> 16) as usize;
    let len = (raw & u16::MAX as u32) as usize;
    let end = match start.checked_add(len) {
        Some(end) if len != 0 && end <= event_count => end,
        Some(_) | None => return None,
    };
    Some(RollScopeMetadata { scope, start, end })
}

const fn roll_scope_ranges_are_laminar(left: RollScopeMetadata, right: RollScopeMetadata) -> bool {
    if left.scope == right.scope {
        return false;
    }
    left.end <= right.start
        || right.end <= left.start
        || (left.start <= right.start && right.end <= left.end)
        || (right.start <= left.start && left.end <= right.end)
}

/// Bind iteration rows to the structured source invariant consumed by cursor nesting.
pub(in crate::global::role_program::image_impl) const fn roll_scope_columns_are_coherent<
    const N: usize,
>(
    bytes: &[u8; N],
    roll_scopes: ColumnRange,
    event_count: usize,
) -> bool {
    let mut row = 0usize;
    while row < roll_scopes.len as usize {
        let current = match decode_roll_scope_metadata(bytes, roll_scopes, row, event_count) {
            Some(current) => current,
            None => return false,
        };
        let mut prior_row = 0usize;
        while prior_row < row {
            let prior = match decode_roll_scope_metadata(bytes, roll_scopes, prior_row, event_count)
            {
                Some(prior) => prior,
                None => return false,
            };
            if !roll_scope_ranges_are_laminar(prior, current) {
                return false;
            }
            prior_row += 1;
        }
        row += 1;
    }
    true
}

/// Validate the exact route-commit partition and its derived builder maximum.
pub(in crate::global::role_program::image_impl) const fn route_commit_capacity_is_exact<
    const N: usize,
>(
    bytes: &[u8; N],
    ranges: ColumnRange,
    route_commit_row_count: usize,
    max_route_commit_count: usize,
) -> bool {
    if ranges.len == 0 {
        return route_commit_row_count == 0 && max_route_commit_count == 0;
    }
    let mut expected_start = 0usize;
    let mut observed_max = 0usize;
    let mut row = 0usize;
    while row < ranges.len as usize {
        let offset = ranges.offset as usize + row * ROLE_IMAGE_LANE_RANGE_STRIDE;
        let Some(raw) = read_u32(bytes, offset) else {
            return false;
        };
        if raw == u32::MAX {
            return false;
        }
        let start = (raw >> 16) as usize;
        let len = (raw & u16::MAX as u32) as usize;
        let Some(end) = start.checked_add(len) else {
            return false;
        };
        if start != expected_start || len == 0 || end > route_commit_row_count {
            return false;
        }
        if len > observed_max {
            observed_max = len;
        }
        expected_start = end;
        row += 1;
    }
    expected_start == route_commit_row_count && observed_max == max_route_commit_count
}
