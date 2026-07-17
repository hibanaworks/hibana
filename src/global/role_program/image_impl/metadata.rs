use super::super::{
    ColumnRange, LANE_DOMAIN_SIZE, PackedLaneRange, ROLE_IMAGE_LANE_RANGE_STRIDE,
    ROLE_IMAGE_LANE_STRIDE, ROLE_IMAGE_ROLL_SCOPE_STRIDE, ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    ROLE_IMAGE_ROUTE_ARM_STRIDE, RoleImageColumns, RuntimeRoleFootprint, lane_byte_count,
};
use crate::global::const_dsl::ScopeId;

mod scope_ranges;
pub(super) use scope_ranges::{roll_scope_columns_are_coherent, route_commit_capacity_is_exact};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActiveLaneMetadata {
    pub(super) active_lane_count: usize,
    pub(super) logical_lane_count: usize,
    pub(super) first_active_lane: u16,
}

/// Reconstruct the only legal active-lane metadata from its resident bitmap.
/// The active row is the first lane-bit row, so empty and trailing-zero
/// encodings have one canonical representation.
pub(super) const fn derive_active_lane_metadata<const N: usize>(
    bytes: &[u8; N],
    lane_bits: ColumnRange,
    active_lane_row: PackedLaneRange,
) -> Option<ActiveLaneMetadata> {
    if !active_lane_row.is_canonical_optional_range()
        || active_lane_row.start() != 0
        || active_lane_row.end() > lane_bits.len as usize
        || active_lane_row.len() > lane_byte_count(LANE_DOMAIN_SIZE)
    {
        return None;
    }

    let mut active_lane_count = 0usize;
    let mut first_active_lane = u16::MAX;
    let mut last_active_lane = None;
    let mut byte_index = 0usize;
    while byte_index < active_lane_row.len() {
        let row = active_lane_row.start() + byte_index;
        let offset = lane_bits.offset as usize + row * ROLE_IMAGE_LANE_STRIDE;
        if offset >= N {
            return None;
        }
        let byte = bytes[offset];
        let mut bit = 0usize;
        while bit < u8::BITS as usize {
            if byte & (1u8 << bit) != 0 {
                let lane = byte_index * u8::BITS as usize + bit;
                if lane >= LANE_DOMAIN_SIZE {
                    return None;
                }
                if first_active_lane == u16::MAX {
                    first_active_lane = lane as u16;
                }
                last_active_lane = Some(lane);
                active_lane_count += 1;
            }
            bit += 1;
        }
        byte_index += 1;
    }

    let logical_lane_count = match last_active_lane {
        Some(last) => last + 1,
        None => 1,
    };
    let byte_len = if active_lane_count == 0 {
        0
    } else {
        lane_byte_count(logical_lane_count)
    };
    if active_lane_row.len() != byte_len {
        return None;
    }
    Some(ActiveLaneMetadata {
        active_lane_count,
        logical_lane_count,
        first_active_lane,
    })
}

const fn read_u32<const N: usize>(bytes: &[u8; N], offset: usize) -> Option<u32> {
    let end = match offset.checked_add(4) {
        Some(end) => end,
        None => return None,
    };
    if end > N {
        return None;
    }
    Some(
        bytes[offset] as u32
            | (bytes[offset + 1] as u32) << 8
            | (bytes[offset + 2] as u32) << 16
            | (bytes[offset + 3] as u32) << 24,
    )
}

const fn read_u16<const N: usize>(bytes: &[u8; N], offset: usize) -> Option<u16> {
    let end = match offset.checked_add(2) {
        Some(end) => end,
        None => return None,
    };
    if end > N {
        return None;
    }
    Some(bytes[offset] as u16 | (bytes[offset + 1] as u16) << 8)
}

const fn column_row_offset(
    column: ColumnRange,
    row: usize,
    stride: usize,
    field: usize,
) -> Option<usize> {
    if row >= column.len as usize || field >= stride {
        return None;
    }
    let relative = match row.checked_mul(stride) {
        Some(relative) => relative,
        None => return None,
    };
    let base = match (column.offset as usize).checked_add(relative) {
        Some(base) => base,
        None => return None,
    };
    base.checked_add(field)
}

#[derive(Clone, Copy)]
struct LaneBitmapRow {
    start: usize,
    len: usize,
}

impl LaneBitmapRow {
    const fn end(self) -> Option<usize> {
        self.start.checked_add(self.len)
    }
}

const fn decode_lane_bitmap_row<const N: usize>(
    bytes: &[u8; N],
    lane_bits: ColumnRange,
    ranges: ColumnRange,
    row: usize,
    active_lane_row: PackedLaneRange,
    expected_start: usize,
) -> Option<LaneBitmapRow> {
    let range_offset = match column_row_offset(ranges, row, ROLE_IMAGE_LANE_RANGE_STRIDE, 0) {
        Some(offset) => offset,
        None => return None,
    };
    let raw = match read_u32(bytes, range_offset) {
        Some(raw) => raw,
        None => return None,
    };
    if raw == u32::MAX {
        return None;
    }
    let start = (raw >> 16) as usize;
    let len = (raw & u16::MAX as u32) as usize;
    if len == 0 {
        return if start == 0 {
            Some(LaneBitmapRow { start, len })
        } else {
            None
        };
    }
    if start != expected_start || len > active_lane_row.len() {
        return None;
    }
    let end = match start.checked_add(len) {
        Some(end) => end,
        None => return None,
    };
    if end > lane_bits.len as usize {
        return None;
    }
    Some(LaneBitmapRow { start, len })
}

const fn lane_bitmap_byte<const N: usize>(
    bytes: &[u8; N],
    lane_bits: ColumnRange,
    row: LaneBitmapRow,
    byte_index: usize,
) -> Option<u8> {
    if byte_index >= row.len {
        return Some(0);
    }
    let relative = match row.start.checked_add(byte_index) {
        Some(relative) => relative,
        None => return None,
    };
    let offset = match column_row_offset(lane_bits, relative, ROLE_IMAGE_LANE_STRIDE, 0) {
        Some(offset) => offset,
        None => return None,
    };
    if offset >= N {
        return None;
    }
    Some(bytes[offset])
}

const fn lane_bitmap_row_is_minimal_active_subset<const N: usize>(
    bytes: &[u8; N],
    lane_bits: ColumnRange,
    row: LaneBitmapRow,
    active_lane_row: PackedLaneRange,
) -> bool {
    if row.len == 0 {
        return true;
    }
    let mut byte_index = 0usize;
    while byte_index < row.len {
        let byte = match lane_bitmap_byte(bytes, lane_bits, row, byte_index) {
            Some(byte) => byte,
            None => return false,
        };
        let active = match lane_bitmap_byte(
            bytes,
            lane_bits,
            LaneBitmapRow {
                start: active_lane_row.start(),
                len: active_lane_row.len(),
            },
            byte_index,
        ) {
            Some(byte) => byte,
            None => return false,
        };
        if byte & !active != 0 || (byte_index + 1 == row.len && byte == 0) {
            return false;
        }
        byte_index += 1;
    }
    true
}

#[derive(Clone, Copy)]
struct RouteArmLaneMetadata {
    event_start: usize,
    event_len: usize,
    lane_step_len: usize,
}

const fn decode_route_arm_lane_metadata<const N: usize>(
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

const fn arm_bitmap_matches_lane_steps<const N: usize>(
    bytes: &[u8; N],
    columns: RoleImageColumns,
    bitmap: LaneBitmapRow,
    step_start: usize,
    metadata: RouteArmLaneMetadata,
    logical_lane_count: usize,
) -> bool {
    let step_end = match step_start.checked_add(metadata.lane_step_len) {
        Some(end) => end,
        None => return false,
    };
    let event_end = match metadata.event_start.checked_add(metadata.event_len) {
        Some(end) => end,
        None => return false,
    };
    if step_end > columns.route_arm_lane_step_rows.len as usize
        || event_end > columns.lanes.len as usize
        || logical_lane_count > LANE_DOMAIN_SIZE
    {
        return false;
    }

    let mut event_seen = [0u8; lane_byte_count(LANE_DOMAIN_SIZE)];
    let mut first_steps = [u16::MAX; LANE_DOMAIN_SIZE];
    let mut last_steps = [0u16; LANE_DOMAIN_SIZE];
    let mut event = metadata.event_start;
    while event < event_end {
        let offset = match column_row_offset(columns.lanes, event, ROLE_IMAGE_LANE_STRIDE, 0) {
            Some(offset) => offset,
            None => return false,
        };
        if offset >= N {
            return false;
        }
        let lane = bytes[offset] as usize;
        if lane >= logical_lane_count || event >= u16::MAX as usize {
            return false;
        }
        let byte = lane / u8::BITS as usize;
        let bit = 1u8 << (lane % u8::BITS as usize);
        event_seen[byte] |= bit;
        if first_steps[lane] == u16::MAX {
            first_steps[lane] = event as u16;
        }
        last_steps[lane] = event as u16;
        event += 1;
    }

    let mut relation_seen = [0u8; lane_byte_count(LANE_DOMAIN_SIZE)];
    let mut step = step_start;
    while step < step_end {
        let offset = match column_row_offset(
            columns.route_arm_lane_step_rows,
            step,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
            0,
        ) {
            Some(offset) => offset,
            None => return false,
        };
        if offset >= N {
            return false;
        }
        let lane = bytes[offset] as usize;
        if lane >= logical_lane_count {
            return false;
        }
        let first_step = match read_u16(bytes, offset + 1) {
            Some(step) => step,
            None => return false,
        };
        let last_step = match read_u16(bytes, offset + 3) {
            Some(step) => step,
            None => return false,
        };
        let byte = lane / u8::BITS as usize;
        let bit = 1u8 << (lane % u8::BITS as usize);
        if relation_seen[byte] & bit != 0
            || first_steps[lane] == u16::MAX
            || first_step != first_steps[lane]
            || last_step != last_steps[lane]
        {
            return false;
        }
        relation_seen[byte] |= bit;
        step += 1;
    }

    let mut byte_index = 0usize;
    while byte_index < lane_byte_count(logical_lane_count) {
        let actual = match lane_bitmap_byte(bytes, columns.lane_bits, bitmap, byte_index) {
            Some(byte) => byte,
            None => return false,
        };
        if actual != relation_seen[byte_index]
            || relation_seen[byte_index] != event_seen[byte_index]
        {
            return false;
        }
        byte_index += 1;
    }
    true
}

const fn offer_bitmap_is_arm_union<const N: usize>(
    bytes: &[u8; N],
    lane_bits: ColumnRange,
    left: LaneBitmapRow,
    right: LaneBitmapRow,
    offer: LaneBitmapRow,
    active_byte_len: usize,
) -> bool {
    let mut byte_index = 0usize;
    while byte_index < active_byte_len {
        let left_byte = match lane_bitmap_byte(bytes, lane_bits, left, byte_index) {
            Some(byte) => byte,
            None => return false,
        };
        let right_byte = match lane_bitmap_byte(bytes, lane_bits, right, byte_index) {
            Some(byte) => byte,
            None => return false,
        };
        let offer_byte = match lane_bitmap_byte(bytes, lane_bits, offer, byte_index) {
            Some(byte) => byte,
            None => return false,
        };
        if offer_byte != left_byte | right_byte {
            return false;
        }
        byte_index += 1;
    }
    true
}

const fn resident_event_lanes_match_active<const N: usize>(
    bytes: &[u8; N],
    columns: RoleImageColumns,
    active_lane_row: PackedLaneRange,
    footprint: RuntimeRoleFootprint,
) -> bool {
    if columns.events.len as usize != footprint.local_step_count
        || columns.lanes.len as usize != footprint.local_step_count
        || footprint.logical_lane_count > LANE_DOMAIN_SIZE
    {
        return false;
    }
    let mut seen = [0u8; lane_byte_count(LANE_DOMAIN_SIZE)];
    let mut row = 0usize;
    while row < columns.lanes.len as usize {
        let offset = match column_row_offset(columns.lanes, row, ROLE_IMAGE_LANE_STRIDE, 0) {
            Some(offset) => offset,
            None => return false,
        };
        if offset >= N {
            return false;
        }
        let lane = bytes[offset] as usize;
        if lane >= footprint.logical_lane_count {
            return false;
        }
        seen[lane / u8::BITS as usize] |= 1u8 << (lane % u8::BITS as usize);
        row += 1;
    }

    let mut byte_index = 0usize;
    while byte_index < lane_byte_count(footprint.logical_lane_count) {
        let active = match lane_bitmap_byte(
            bytes,
            columns.lane_bits,
            LaneBitmapRow {
                start: active_lane_row.start(),
                len: active_lane_row.len(),
            },
            byte_index,
        ) {
            Some(byte) => byte,
            None => return false,
        };
        if active != seen[byte_index] {
            return false;
        }
        byte_index += 1;
    }
    true
}

/// Bind every resident lane column to one fact: active lanes first, followed by
/// `(left arm, right arm, offer union)` for each route and one lane-step row per
/// arm bit. No bitmap or lane-step bytes may be orphaned.
pub(super) const fn lane_columns_are_coherent<const N: usize>(
    bytes: &[u8; N],
    columns: RoleImageColumns,
    active_lane_row: PackedLaneRange,
    footprint: RuntimeRoleFootprint,
) -> bool {
    let route_arm_row_count = match footprint.route_scope_count.checked_mul(2) {
        Some(count) => count,
        None => return false,
    };
    if columns.route_arms.len as usize != route_arm_row_count
        || columns.route_arm_lane_rows.len as usize != route_arm_row_count
        || columns.route_offer_lane_rows.len as usize != footprint.route_scope_count
        || active_lane_row.end() > columns.lane_bits.len as usize
    {
        return false;
    }
    if !resident_event_lanes_match_active(bytes, columns, active_lane_row, footprint) {
        return false;
    }
    let mut expected_start = active_lane_row.end();
    let mut lane_step_start = 0usize;
    let mut route_slot = 0usize;
    while route_slot < footprint.route_scope_count {
        let mut arm_rows = [LaneBitmapRow { start: 0, len: 0 }; 2];
        let mut arm = 0;
        while arm < arm_rows.len() {
            let row_index = route_slot * 2 + arm;
            let row = match decode_lane_bitmap_row(
                bytes,
                columns.lane_bits,
                columns.route_arm_lane_rows,
                row_index,
                active_lane_row,
                expected_start,
            ) {
                Some(row) => row,
                None => return false,
            };
            if !lane_bitmap_row_is_minimal_active_subset(
                bytes,
                columns.lane_bits,
                row,
                active_lane_row,
            ) {
                return false;
            }
            let metadata =
                match decode_route_arm_lane_metadata(bytes, columns.route_arms, row_index) {
                    Some(metadata) => metadata,
                    None => return false,
                };
            if !arm_bitmap_matches_lane_steps(
                bytes,
                columns,
                row,
                lane_step_start,
                metadata,
                footprint.logical_lane_count,
            ) {
                return false;
            }
            lane_step_start = match lane_step_start.checked_add(metadata.lane_step_len) {
                Some(next) => next,
                None => return false,
            };
            expected_start = match row.end() {
                Some(end) if row.len != 0 => end,
                Some(_) => expected_start,
                None => return false,
            };
            arm_rows[arm] = row;
            arm += 1;
        }
        let offer = match decode_lane_bitmap_row(
            bytes,
            columns.lane_bits,
            columns.route_offer_lane_rows,
            route_slot,
            active_lane_row,
            expected_start,
        ) {
            Some(row) => row,
            None => return false,
        };
        if !lane_bitmap_row_is_minimal_active_subset(
            bytes,
            columns.lane_bits,
            offer,
            active_lane_row,
        ) || !offer_bitmap_is_arm_union(
            bytes,
            columns.lane_bits,
            arm_rows[0],
            arm_rows[1],
            offer,
            active_lane_row.len(),
        ) {
            return false;
        }
        expected_start = match offer.end() {
            Some(end) if offer.len != 0 => end,
            Some(_) => expected_start,
            None => return false,
        };
        route_slot += 1;
    }
    expected_start == columns.lane_bits.len as usize
        && lane_step_start == columns.route_arm_lane_step_rows.len as usize
}
