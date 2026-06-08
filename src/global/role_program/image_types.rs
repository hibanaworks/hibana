#[cfg(test)]
use super::lane_set::{lane_word_count, logical_lane_count_for_role};
use super::{CompiledProgramImage, LANE_DOMAIN_SIZE};
use crate::global::typestate::{LocalNode, PackedEventConflict, PackedLocalDependency};
pub(crate) const MAX_RESIDENT_ROW_LANE_ROWS: usize = u8::MAX as usize + 1;
pub(crate) const MAX_RESIDENT_ROW_BOUNDARY_ROWS: usize = MAX_RESIDENT_ROW_LANE_ROWS + 1;
pub(crate) const MAX_LOCAL_STEP_LANES: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_ROUTE_SCOPE_LANE_ROWS: usize = crate::eff::meta::MAX_EFF_NODES / 2;
pub(crate) const MAX_ROUTE_ARM_LANE_ROWS: usize = MAX_ROUTE_SCOPE_LANE_ROWS * 2;
pub(crate) const MAX_RESIDENT_LANE_BIT_BYTES: usize = LANE_DOMAIN_SIZE * 4;
pub(crate) const PACKED_LANE_RANGE_EMPTY: u32 = u32::MAX;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LabelUniverseViolation {
    pub(crate) max: u8,
    pub(crate) actual: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImage {
    pub(crate) facts: RoleFacts,
    #[cfg(test)]
    pub(crate) source: RoleImageSource,
    pub(crate) lanes: RoleLaneImage,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleLaneImage {
    pub(crate) local_step_nodes: [LocalNode; MAX_LOCAL_STEP_LANES],
    pub(crate) local_step_lanes: [u8; MAX_LOCAL_STEP_LANES],
    pub(crate) local_step_dependencies: [PackedLocalDependency; MAX_LOCAL_STEP_LANES],
    pub(crate) local_step_conflicts: [PackedEventConflict; MAX_LOCAL_STEP_LANES],
    pub(crate) route_scope_rows: [u16; MAX_ROUTE_SCOPE_LANE_ROWS],
    pub(crate) route_scope_conflicts: [PackedEventConflict; MAX_ROUTE_SCOPE_LANE_ROWS],
    pub(crate) route_arm_event_rows: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS],
    pub(crate) resident_row_boundaries: [u16; MAX_RESIDENT_ROW_BOUNDARY_ROWS],
    pub(crate) lane_bit_rows: [u8; MAX_RESIDENT_LANE_BIT_BYTES],
    pub(crate) route_arm_lane_rows: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS],
    pub(crate) route_offer_lane_rows: [PackedLaneRange; MAX_ROUTE_SCOPE_LANE_ROWS],
    pub(crate) active_lane_row: PackedLaneRange,
    pub(crate) resident_row_len: u16,
    pub(crate) lane_bit_row_len: u16,
    pub(crate) first_active_lane: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFacts {
    pub(crate) words: [u16; 14],
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageRef {
    pub(crate) image: &'static RoleImage,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageSource {
    #[cfg(test)]
    program_image: fn() -> &'static CompiledProgramImage,
}

impl RoleImageSource {
    #[inline(always)]
    pub(crate) const fn new(program_image: fn() -> &'static CompiledProgramImage) -> Self {
        let _ = program_image;
        Self {
            #[cfg(test)]
            program_image,
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn program_image(self) -> &'static CompiledProgramImage {
        (self.program_image)()
    }
}

pub(crate) mod private {
    pub trait RoleProgramViewSeal {}
}

pub(crate) trait RoleProgramView<const ROLE: u8>: private::RoleProgramViewSeal {
    fn compiled_role_image(&self) -> &'static crate::global::compiled::images::CompiledRoleImage;
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFootprint {
    #[cfg(test)]
    pub(crate) scope_count: usize,
    #[cfg(test)]
    pub(crate) max_active_scope_depth: usize,
    #[cfg(test)]
    pub(crate) eff_count: usize,
    #[cfg(test)]
    pub(crate) resident_row_count: usize,
    #[cfg(test)]
    pub(crate) resident_row_lane_entry_count: usize,
    #[cfg(test)]
    pub(crate) resident_row_lane_word_count: usize,
    #[cfg(test)]
    pub(crate) parallel_enter_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) local_step_count: usize,
    pub(crate) passive_linger_route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
    pub(crate) logical_lane_word_count: usize,
    pub(crate) max_route_stack_depth: usize,
    pub(crate) scope_evidence_count: usize,
    pub(crate) frontier_entry_count: usize,
}

impl RoleFootprint {
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

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn for_endpoint_layout(
        active_lane_count: usize,
        endpoint_lane_slot_count: usize,
        logical_lane_count: usize,
        max_route_stack_depth: usize,
        scope_evidence_count: usize,
        frontier_entry_count: usize,
    ) -> Self {
        let endpoint_lane_slot_count = if endpoint_lane_slot_count == 0 {
            1
        } else {
            endpoint_lane_slot_count
        };
        let logical_lane_seed = if logical_lane_count > endpoint_lane_slot_count {
            logical_lane_count
        } else {
            endpoint_lane_slot_count
        };
        let logical_lane_count = logical_lane_count_for_role(active_lane_count, logical_lane_seed);
        Self {
            #[cfg(test)]
            scope_count: 0,
            #[cfg(test)]
            max_active_scope_depth: 0,
            #[cfg(test)]
            eff_count: 0,
            #[cfg(test)]
            resident_row_count: 0,
            #[cfg(test)]
            resident_row_lane_entry_count: 0,
            #[cfg(test)]
            resident_row_lane_word_count: 0,
            #[cfg(test)]
            parallel_enter_count: 0,
            route_scope_count: 0,
            local_step_count: 0,
            passive_linger_route_scope_count: 0,
            active_lane_count,
            endpoint_lane_slot_count,
            logical_lane_count,
            logical_lane_word_count: lane_word_count(logical_lane_count),
            max_route_stack_depth,
            scope_evidence_count,
            frontier_entry_count,
        }
    }
}
