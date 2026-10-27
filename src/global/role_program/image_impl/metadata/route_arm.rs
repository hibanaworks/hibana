use super::{ColumnRange, ROLE_IMAGE_ROUTE_ARM_STRIDE, column_row_offset, read_u16, read_u32};

#[derive(Clone, Copy)]
pub(super) struct RouteArmLaneMetadata {
    pub(super) event_start: usize,
    pub(super) event_len: usize,
    pub(super) lane_step_len: usize,
}

pub(super) const fn decode_route_arm_lane_metadata<const N: usize>(
    bytes: &[u8; N],
    route_arms: ColumnRange,
    row: usize,
) -> Option<RouteArmLaneMetadata> {
    let offset = match column_row_offset(route_arms, row, ROLE_IMAGE_ROUTE_ARM_STRIDE, 0) {
        Some(offset) => offset,
        None => return None,
    };
    let event_range = match read_u32(bytes, offset) {
        Some(raw) => raw,
        None => return None,
    };
    let metadata_offset = match offset.checked_add(4) {
        Some(offset) => offset,
        None => return None,
    };
    let metadata = match read_u32(bytes, metadata_offset) {
        Some(raw) => raw,
        None => return None,
    };
    let event_start = (event_range >> 16) as usize;
    let event_len = (event_range & u16::MAX as u32) as usize;
    let end = (metadata >> 16) as usize;
    let start = if row == 0 {
        0
    } else {
        let prior = match column_row_offset(route_arms, row - 1, ROLE_IMAGE_ROUTE_ARM_STRIDE, 6) {
            Some(prior) => prior,
            None => return None,
        };
        match read_u16(bytes, prior) {
            Some(start) => start as usize,
            None => return None,
        }
    };
    if event_range == u32::MAX || (event_len == 0 && event_start != 0) || end < start {
        return None;
    }
    let lane_step_len = end - start;
    if lane_step_len > super::LANE_DOMAIN_SIZE || ((event_len == 0) != (lane_step_len == 0)) {
        return None;
    }
    Some(RouteArmLaneMetadata {
        event_start,
        event_len,
        lane_step_len,
    })
}
