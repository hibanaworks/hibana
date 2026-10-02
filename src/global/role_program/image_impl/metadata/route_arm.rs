use super::{ColumnRange, ROLE_IMAGE_ROUTE_ARM_STRIDE, column_row_offset, read_u32};

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
    let encoded_step_len = ((metadata >> 16) & u8::MAX as u32) as usize;
    if event_range == u32::MAX
        || (event_len == 0 && event_start != 0)
        || metadata & 0xff00_0000 != 0
    {
        return None;
    }
    let lane_step_len = if event_len == 0 {
        if encoded_step_len == 0 {
            0
        } else {
            return None;
        }
    } else {
        match encoded_step_len.checked_add(1) {
            Some(len) => len,
            None => return None,
        }
    };
    Some(RouteArmLaneMetadata {
        event_start,
        event_len,
        lane_step_len,
    })
}
