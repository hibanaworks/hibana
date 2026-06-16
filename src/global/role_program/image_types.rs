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
pub(crate) const PACKED_ROUTE_ARM_ROW_EMPTY: u32 = u32::MAX;
pub(crate) const ROLE_IMAGE_EVENT_STRIDE: usize = 10;
pub(crate) const ROLE_IMAGE_LANE_STRIDE: usize = 1;
pub(crate) const ROLE_IMAGE_DEPENDENCY_STRIDE: usize = 8;
pub(crate) const ROLE_IMAGE_CONFLICT_STRIDE: usize = 2;
pub(crate) const ROLE_IMAGE_U16_STRIDE: usize = 2;
pub(crate) const ROLE_IMAGE_ROUTE_ARM_STRIDE: usize = 8;
pub(crate) const ROLE_IMAGE_LANE_RANGE_STRIDE: usize = 4;
pub(crate) const ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE: usize = 5;
pub(crate) const ROLE_IMAGE_ROLL_SCOPE_STRIDE: usize = 6;

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
    pub(crate) const fn is_zero_len(self) -> bool {
        (self.0 & 0xffff) == 0
    }

    #[inline(always)]
    pub(crate) const fn is_absent_or_zero_len(self) -> bool {
        self.is_empty() || self.is_zero_len()
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
        self.start() + self.len()
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
pub(crate) struct PackedRouteArmRow {
    event_and_child: u32,
    lane_step_row: PackedLaneRange,
}

impl PackedRouteArmRow {
    pub(crate) const EMPTY: Self = Self {
        event_and_child: PACKED_ROUTE_ARM_ROW_EMPTY,
        lane_step_row: PackedLaneRange::EMPTY,
    };
    const CHILD_SHIFT: u32 = 24;
    const START_SHIFT: u32 = 12;
    const FIELD_MASK: u32 = 0x0fff;
    const CHILD_ABSENT_DELTA: u32 = 0;

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
            && (lane_step_row.start() > u16::MAX as usize
                || lane_step_row.len() > u16::MAX as usize)
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
            None => Self::CHILD_ABSENT_DELTA,
        };
        let lane_step_row = if lane_step_row.is_empty() {
            PackedLaneRange::new(0, 0)
        } else {
            lane_step_row
        };
        Self {
            event_and_child: (child << Self::CHILD_SHIFT)
                | ((event_row.start() as u32) << Self::START_SHIFT)
                | event_row.len() as u32,
            lane_step_row,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_parts(event_and_child: u32, lane_step_raw: u32) -> Self {
        Self {
            event_and_child,
            lane_step_row: PackedLaneRange::from_raw(lane_step_raw),
        }
    }

    #[inline(always)]
    pub(crate) const fn event_and_child_raw(self) -> u32 {
        self.event_and_child
    }

    #[inline(always)]
    pub(crate) const fn lane_step_raw(self) -> u32 {
        self.lane_step_row.raw()
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.event_and_child == PACKED_ROUTE_ARM_ROW_EMPTY
    }

    #[inline(always)]
    pub(crate) const fn event_row(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else {
            PackedLaneRange::new(
                ((self.event_and_child >> Self::START_SHIFT) & Self::FIELD_MASK) as usize,
                (self.event_and_child & Self::FIELD_MASK) as usize,
            )
        }
    }

    #[inline(always)]
    pub(crate) const fn lane_step_row(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else {
            self.lane_step_row
        }
    }

    #[inline(always)]
    pub(crate) const fn child_slot_delta(self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            let delta = (self.event_and_child >> Self::CHILD_SHIFT) as u8;
            if delta == Self::CHILD_ABSENT_DELTA as u8 {
                None
            } else {
                Some(delta)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedRollScopeRow {
    scope: u16,
    event_row: PackedLaneRange,
}

impl PackedRollScopeRow {
    pub(crate) const EMPTY: Self = Self {
        scope: u16::MAX,
        event_row: PackedLaneRange::EMPTY,
    };

    #[inline(always)]
    pub(crate) const fn new(
        scope: crate::global::const_dsl::ScopeId,
        row: PackedLaneRange,
    ) -> Self {
        if scope.is_none()
            || !matches!(scope.kind(), crate::global::const_dsl::ScopeKind::Roll)
            || row.is_empty()
            || row.start() > u16::MAX as usize
            || row.len() > u16::MAX as usize
        {
            panic!("roll scope row overflow");
        }
        Self {
            scope: scope.local_ordinal(),
            event_row: row,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_parts(scope: u16, event_row_raw: u32) -> Self {
        Self {
            scope,
            event_row: PackedLaneRange::from_raw(event_row_raw),
        }
    }

    #[inline(always)]
    pub(crate) const fn scope_raw(self) -> u16 {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn event_row_raw(self) -> u32 {
        self.event_row.raw()
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.scope == u16::MAX
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> Option<crate::global::const_dsl::ScopeId> {
        if self.is_empty() {
            None
        } else {
            Some(crate::global::const_dsl::ScopeId::roll_scope(self.scope))
        }
    }

    #[inline(always)]
    pub(crate) const fn event_row(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else if self.event_row.is_empty() {
            crate::invariant();
        } else {
            self.event_row
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

#[derive(Clone, Copy)]
pub(crate) struct BlobPtr {
    base: *const u8,
}

// SAFETY: BlobPtr is only constructed from immutable static bucket storage and exposes no mutation.
unsafe impl Sync for BlobPtr {}

impl BlobPtr {
    #[inline(always)]
    pub(in crate::global) const fn from_array<const N: usize>(
        bytes: &'static [u8; N],
        len: usize,
    ) -> Self {
        if len > N {
            panic!("resident blob pointer");
        }
        Self {
            base: bytes.as_ptr(),
        }
    }

    #[inline(always)]
    pub(in crate::global) const fn as_ptr(self) -> *const u8 {
        self.base
    }

    #[inline(always)]
    pub(in crate::global) const fn byte_at(self, offset: usize) -> u8 {
        // SAFETY: callers check offset against the column-derived blob length.
        unsafe { *self.base.add(offset) }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ColumnRange {
    pub(crate) offset: u16,
    pub(crate) len: u16,
}

impl ColumnRange {
    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > u16::MAX as usize || len > u16::MAX as usize {
            panic!("role image packed column descriptor overflow");
        }
        if stride == 0 {
            panic!("role image packed column stride must be nonzero");
        }
        let byte_len = len * stride;
        if byte_len > (u16::MAX as usize - offset) {
            panic!("role image packed column byte range overflow");
        }
        Self {
            offset: offset as u16,
            len: len as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn byte_len(self, stride: usize) -> usize {
        self.len as usize * stride
    }

    #[inline(always)]
    pub(crate) const fn end_offset(self, stride: usize) -> usize {
        self.offset as usize + self.byte_len(stride)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RoleImageColumns {
    pub(crate) events: ColumnRange,
    pub(crate) lanes: ColumnRange,
    pub(crate) dependencies: ColumnRange,
    pub(crate) conflicts: ColumnRange,
    pub(crate) route_scopes: ColumnRange,
    pub(crate) route_scope_conflicts: ColumnRange,
    pub(crate) route_arms: ColumnRange,
    pub(crate) resident_boundaries: ColumnRange,
    pub(crate) lane_bits: ColumnRange,
    pub(crate) route_arm_lane_rows: ColumnRange,
    pub(crate) route_offer_lane_rows: ColumnRange,
    pub(crate) route_arm_lane_step_rows: ColumnRange,
    pub(crate) route_commit_ranges: ColumnRange,
    pub(crate) route_commit_rows: ColumnRange,
    pub(crate) roll_scopes: ColumnRange,
}

impl RoleImageColumns {
    #[inline(always)]
    const fn max_end(mut len: usize, column: ColumnRange, stride: usize) -> usize {
        let end = column.end_offset(stride);
        if end > len {
            len = end;
        }
        len
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        let mut len = Self::max_end(0, self.events, ROLE_IMAGE_EVENT_STRIDE);
        len = Self::max_end(len, self.lanes, ROLE_IMAGE_LANE_STRIDE);
        len = Self::max_end(len, self.dependencies, ROLE_IMAGE_DEPENDENCY_STRIDE);
        len = Self::max_end(len, self.conflicts, ROLE_IMAGE_CONFLICT_STRIDE);
        len = Self::max_end(len, self.route_scopes, ROLE_IMAGE_U16_STRIDE);
        len = Self::max_end(len, self.route_scope_conflicts, ROLE_IMAGE_CONFLICT_STRIDE);
        len = Self::max_end(len, self.route_arms, ROLE_IMAGE_ROUTE_ARM_STRIDE);
        len = Self::max_end(len, self.resident_boundaries, ROLE_IMAGE_U16_STRIDE);
        len = Self::max_end(len, self.lane_bits, ROLE_IMAGE_LANE_STRIDE);
        len = Self::max_end(len, self.route_arm_lane_rows, ROLE_IMAGE_LANE_RANGE_STRIDE);
        len = Self::max_end(
            len,
            self.route_offer_lane_rows,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        );
        len = Self::max_end(
            len,
            self.route_arm_lane_step_rows,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        );
        len = Self::max_end(len, self.route_commit_ranges, ROLE_IMAGE_LANE_RANGE_STRIDE);
        len = Self::max_end(len, self.route_commit_rows, ROLE_IMAGE_CONFLICT_STRIDE);
        len = Self::max_end(len, self.roll_scopes, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
        len
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageBytes<const N: usize> {
    pub(super) bytes: [u8; N],
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
    pub(crate) roll_scope_rows: [PackedRollScopeRow; MAX_LOCAL_STEP_LANES],
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) resident_row_len: u16,
    pub(crate) dependency_row_len: u16,
    pub(crate) conflict_row_len: u16,
    pub(crate) lane_bit_row_len: u16,
    pub(crate) route_commit_row_len: u16,
    pub(crate) route_arm_lane_step_row_len: u16,
    pub(crate) roll_scope_row_len: u16,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageRef {
    pub(crate) program: &'static CompiledProgramRef,
    pub(crate) role: u8,
    pub(crate) facts: RuntimeRoleFacts,
    pub(crate) columns: RoleImageColumns,
    pub(crate) blob: BlobPtr,
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleLaneImage {
    pub(crate) columns: RoleImageColumns,
    pub(crate) blob: BlobPtr,
}

#[derive(Clone, Copy)]
pub(crate) struct RuntimeRoleFacts {
    pub(crate) words: [u16; 6],
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
            if route_depth > (u8::BITS as usize / 2) {
                u8::BITS as usize
            } else {
                route_depth * 2
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
