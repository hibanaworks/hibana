use super::LANE_DOMAIN_SIZE;
use super::lane_set::lane_word_count;
use crate::global::{
    compiled::images::CompiledProgramRef,
    typestate::{PackedEventConflict, PackedLocalDependency},
};
pub(crate) const MAX_RESIDENT_ROW_LANE_ROWS: usize = u8::MAX as usize + 1;
pub(crate) const MAX_RESIDENT_ROW_BOUNDARY_ROWS: usize = MAX_RESIDENT_ROW_LANE_ROWS + 1;
pub(crate) const MAX_LOCAL_STEP_LANES: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_ROUTE_SCOPE_LANE_ROWS: usize = crate::eff::meta::MAX_EFF_NODES / 2;
pub(crate) const MAX_ROUTE_ARM_LANE_ROWS: usize = MAX_ROUTE_SCOPE_LANE_ROWS * 2;
pub(crate) const MAX_RESIDENT_LANE_BIT_BYTES: usize = LANE_DOMAIN_SIZE * 4;
pub(crate) const PACKED_LANE_RANGE_EMPTY: u32 = u32::MAX;
pub(crate) const PACKED_ROUTE_ARM_ROW_EMPTY: u64 = u64::MAX;
pub(crate) const ROLE_IMAGE_EVENT_STRIDE: usize = 10;
pub(crate) const ROLE_IMAGE_LANE_STRIDE: usize = 1;
pub(crate) const ROLE_IMAGE_DEPENDENCY_STRIDE: usize = 8;
pub(crate) const ROLE_IMAGE_CONFLICT_STRIDE: usize = 2;
pub(crate) const ROLE_IMAGE_U16_STRIDE: usize = 2;
pub(crate) const ROLE_IMAGE_ROUTE_ARM_STRIDE: usize = 8;
pub(crate) const ROLE_IMAGE_LANE_RANGE_STRIDE: usize = 4;
pub(crate) const ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE: usize = 5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedLaneRange(u32);

impl PackedLaneRange {
    pub(crate) const EMPTY: Self = Self(PACKED_LANE_RANGE_EMPTY);

    #[inline(always)]
    pub(crate) const fn new(start: usize, len: usize) -> Self {
        if start > u16::MAX as usize || len > u16::MAX as usize {
            panic!("lane range descriptor overflow");
        }
        Self(((start as u32) << 16) | len as u32)
    }

    #[inline(always)]
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.0 == PACKED_LANE_RANGE_EMPTY
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        (self.0 >> 16) as usize
    }

    pub(crate) const fn len(self) -> usize {
        (self.0 & 0xffff) as usize
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> usize {
        self.start().saturating_add(self.len())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RouteArmLaneStepRow {
    lane: u8,
    first_step: u16,
    last_step: u16,
}

impl RouteArmLaneStepRow {
    pub(crate) const EMPTY: Self = Self {
        lane: 0,
        first_step: u16::MAX,
        last_step: u16::MAX,
    };

    #[inline(always)]
    pub(crate) const fn new(lane: u8, first_step: usize, last_step: usize) -> Self {
        if first_step > u16::MAX as usize || last_step > u16::MAX as usize {
            panic!("route arm lane step row overflow");
        }
        if first_step > last_step {
            panic!("route arm lane step row order");
        }
        Self {
            lane,
            first_step: first_step as u16,
            last_step: last_step as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    #[inline(always)]
    pub(crate) const fn first_step(self) -> u16 {
        self.first_step
    }

    #[inline(always)]
    pub(crate) const fn last_step(self) -> u16 {
        self.last_step
    }

    #[inline(always)]
    pub(crate) const fn with_last_step(self, last_step: usize) -> Self {
        Self::new(self.lane, self.first_step as usize, last_step)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedRouteArmRow(u64);

impl PackedRouteArmRow {
    pub(crate) const EMPTY: Self = Self(PACKED_ROUTE_ARM_ROW_EMPTY);
    const CHILD_SHIFT: u32 = 24;
    const LANE_START_SHIFT: u32 = 48;
    const LANE_LEN_SHIFT: u32 = 32;
    const START_SHIFT: u32 = 12;
    const FIELD_MASK: u32 = 0x0fff;
    const LANE_FIELD_MASK: u64 = 0xffff;
    const CHILD_NONE: u32 = 0;

    #[inline(always)]
    pub(crate) const fn new(
        event_row: PackedLaneRange,
        child_slot_delta: Option<usize>,
        lane_step_row: PackedLaneRange,
    ) -> Self {
        if event_row.is_empty()
            || event_row.start() > Self::FIELD_MASK as usize
            || event_row.len() > Self::FIELD_MASK as usize
        {
            panic!("route arm projection row event range overflow");
        }
        if !lane_step_row.is_empty()
            && (lane_step_row.start() > Self::LANE_FIELD_MASK as usize
                || lane_step_row.len() > Self::LANE_FIELD_MASK as usize)
        {
            panic!("route arm lane step row range overflow");
        }
        let child = match child_slot_delta {
            Some(delta) => {
                if delta == 0 || delta > u8::MAX as usize {
                    panic!("passive route child slot delta overflow");
                }
                delta as u32
            }
            None => Self::CHILD_NONE,
        };
        let lane_start = if lane_step_row.is_empty() {
            0u64
        } else {
            lane_step_row.start() as u64
        };
        let lane_len = if lane_step_row.is_empty() {
            0u64
        } else {
            lane_step_row.len() as u64
        };
        Self(
            (lane_start << Self::LANE_START_SHIFT)
                | (lane_len << Self::LANE_LEN_SHIFT)
                | ((child as u64) << Self::CHILD_SHIFT)
                | ((event_row.start() as u64) << Self::START_SHIFT)
                | event_row.len() as u64,
        )
    }

    #[inline(always)]
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.0 == PACKED_ROUTE_ARM_ROW_EMPTY
    }

    #[inline(always)]
    pub(crate) const fn event_row(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else {
            PackedLaneRange::new(
                ((self.0 >> Self::START_SHIFT) & Self::FIELD_MASK as u64) as usize,
                (self.0 & Self::FIELD_MASK as u64) as usize,
            )
        }
    }

    #[inline(always)]
    pub(crate) const fn lane_step_row(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else {
            let start = ((self.0 >> Self::LANE_START_SHIFT) & Self::LANE_FIELD_MASK) as usize;
            let len = ((self.0 >> Self::LANE_LEN_SHIFT) & Self::LANE_FIELD_MASK) as usize;
            PackedLaneRange::new(start, len)
        }
    }

    #[inline(always)]
    pub(crate) const fn child_slot_delta(self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            let delta = (self.0 >> Self::CHILD_SHIFT) as u8;
            if delta == Self::CHILD_NONE as u8 {
                None
            } else {
                Some(delta)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedLocalEventRow {
    pub(crate) eff_index: u16,
    pub(crate) dependency_row: u16,
    pub(crate) conflict_row: u16,
    pub(crate) scope_slot: u16,
    pub(crate) frame_label: u8,
    pub(crate) flags: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedColumn {
    pub(crate) offset: u16,
    pub(crate) len: u16,
    pub(crate) stride: u8,
}

impl PackedColumn {
    pub(crate) const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        stride: 1,
    };

    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > u16::MAX as usize || len > u16::MAX as usize || stride > u8::MAX as usize {
            panic!("role image packed column descriptor overflow");
        }
        if stride == 0 {
            panic!("role image packed column stride must be nonzero");
        }
        let byte_len = len.saturating_mul(stride);
        if offset.saturating_add(byte_len) > u16::MAX as usize {
            panic!("role image packed column byte range overflow");
        }
        Self {
            offset: offset as u16,
            len: len as u16,
            stride: stride as u8,
        }
    }

    #[inline(always)]
    pub(crate) const fn byte_len(self) -> usize {
        self.len as usize * self.stride as usize
    }

    #[inline(always)]
    pub(crate) const fn end_offset(self) -> usize {
        self.offset as usize + self.byte_len()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RoleImageColumns {
    pub(crate) events: PackedColumn,
    pub(crate) lanes: PackedColumn,
    pub(crate) dependencies: PackedColumn,
    pub(crate) conflicts: PackedColumn,
    pub(crate) route_scopes: PackedColumn,
    pub(crate) route_scope_conflicts: PackedColumn,
    pub(crate) route_arms: PackedColumn,
    pub(crate) resident_boundaries: PackedColumn,
    pub(crate) lane_bits: PackedColumn,
    pub(crate) route_arm_lane_rows: PackedColumn,
    pub(crate) route_offer_lane_rows: PackedColumn,
    pub(crate) route_arm_lane_step_rows: PackedColumn,
    pub(crate) route_commit_ranges: PackedColumn,
    pub(crate) route_commit_rows: PackedColumn,
}

impl RoleImageColumns {
    #[inline(always)]
    pub(crate) const fn empty() -> Self {
        Self {
            events: PackedColumn::EMPTY,
            lanes: PackedColumn::EMPTY,
            dependencies: PackedColumn::EMPTY,
            conflicts: PackedColumn::EMPTY,
            route_scopes: PackedColumn::EMPTY,
            route_scope_conflicts: PackedColumn::EMPTY,
            route_arms: PackedColumn::EMPTY,
            resident_boundaries: PackedColumn::EMPTY,
            lane_bits: PackedColumn::EMPTY,
            route_arm_lane_rows: PackedColumn::EMPTY,
            route_offer_lane_rows: PackedColumn::EMPTY,
            route_arm_lane_step_rows: PackedColumn::EMPTY,
            route_commit_ranges: PackedColumn::EMPTY,
            route_commit_rows: PackedColumn::EMPTY,
        }
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        let mut len = self.events.end_offset();
        if self.lanes.end_offset() > len {
            len = self.lanes.end_offset();
        }
        if self.dependencies.end_offset() > len {
            len = self.dependencies.end_offset();
        }
        if self.conflicts.end_offset() > len {
            len = self.conflicts.end_offset();
        }
        if self.route_scopes.end_offset() > len {
            len = self.route_scopes.end_offset();
        }
        if self.route_scope_conflicts.end_offset() > len {
            len = self.route_scope_conflicts.end_offset();
        }
        if self.route_arms.end_offset() > len {
            len = self.route_arms.end_offset();
        }
        if self.resident_boundaries.end_offset() > len {
            len = self.resident_boundaries.end_offset();
        }
        if self.lane_bits.end_offset() > len {
            len = self.lane_bits.end_offset();
        }
        if self.route_arm_lane_rows.end_offset() > len {
            len = self.route_arm_lane_rows.end_offset();
        }
        if self.route_offer_lane_rows.end_offset() > len {
            len = self.route_offer_lane_rows.end_offset();
        }
        if self.route_arm_lane_step_rows.end_offset() > len {
            len = self.route_arm_lane_step_rows.end_offset();
        }
        if self.route_commit_ranges.end_offset() > len {
            len = self.route_commit_ranges.end_offset();
        }
        if self.route_commit_rows.end_offset() > len {
            len = self.route_commit_rows.end_offset();
        }
        len
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageBlobStorage<const N: usize> {
    pub(crate) columns: RoleImageColumns,
    pub(crate) bytes: [u8; N],
    pub(crate) len: u16,
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LabelUniverseViolation {
    pub(crate) max: u8,
    pub(crate) actual: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleLaneScratch {
    pub(crate) local_step_events: [PackedLocalEventRow; MAX_LOCAL_STEP_LANES],
    pub(crate) local_step_lanes: [u8; MAX_LOCAL_STEP_LANES],
    pub(crate) local_step_dependencies: [PackedLocalDependency; MAX_LOCAL_STEP_LANES],
    pub(crate) local_step_conflicts: [PackedEventConflict; MAX_LOCAL_STEP_LANES],
    pub(crate) route_scope_rows: [u16; MAX_ROUTE_SCOPE_LANE_ROWS],
    pub(crate) route_scope_conflicts: [PackedEventConflict; MAX_ROUTE_SCOPE_LANE_ROWS],
    pub(crate) route_arm_rows: [PackedRouteArmRow; MAX_ROUTE_ARM_LANE_ROWS],
    pub(crate) resident_row_boundaries: [u16; MAX_RESIDENT_ROW_BOUNDARY_ROWS],
    pub(crate) lane_bit_rows: [u8; MAX_RESIDENT_LANE_BIT_BYTES],
    pub(crate) route_arm_lane_rows: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS],
    pub(crate) route_offer_lane_rows: [PackedLaneRange; MAX_ROUTE_SCOPE_LANE_ROWS],
    pub(crate) route_commit_ranges: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS],
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) resident_row_len: u16,
    pub(crate) dependency_row_len: u16,
    pub(crate) conflict_row_len: u16,
    pub(crate) lane_bit_row_len: u16,
    pub(crate) route_commit_row_len: u16,
    pub(crate) route_arm_lane_step_row_len: u16,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageRef {
    pub(crate) program: CompiledProgramRef,
    pub(crate) role: u8,
    pub(crate) facts: RuntimeRoleFacts,
    pub(crate) columns: RoleImageColumns,
    pub(crate) blob: &'static [u8],
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleLaneImage {
    pub(crate) columns: RoleImageColumns,
    pub(crate) blob: &'static [u8],
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RuntimeRoleFacts {
    pub(crate) words: [u16; 7],
}

pub(crate) mod private {
    pub trait RoleProgramViewSeal {}
}

pub(crate) trait RoleProgramView<const ROLE: u8>: private::RoleProgramViewSeal {
    fn role_image_ref(&self) -> &'static crate::global::role_program::RoleImageRef;
}

#[derive(Clone, Copy)]
pub(crate) struct RuntimeRoleFootprint {
    pub(crate) max_route_stack_depth: usize,
    pub(crate) local_step_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) passive_linger_route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
}

impl RuntimeRoleFootprint {
    #[inline(always)]
    pub(crate) const fn frontier_entry_count_for_route_depth(route_depth: usize) -> usize {
        if route_depth == 0 {
            1
        } else {
            let doubled = route_depth.saturating_mul(2);
            if doubled > u8::BITS as usize {
                u8::BITS as usize
            } else if doubled == 0 {
                1
            } else {
                doubled
            }
        }
    }

    #[inline(always)]
    pub(crate) const fn lane_word_count(self) -> usize {
        lane_word_count(self.logical_lane_count)
    }

    #[inline(always)]
    pub(crate) const fn scope_evidence_count(self) -> usize {
        self.route_scope_count
    }

    #[inline(always)]
    pub(crate) const fn frontier_entry_count(self) -> usize {
        Self::frontier_entry_count_for_route_depth(self.max_route_stack_depth)
    }
}
