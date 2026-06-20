use crate::{
    eff::EffKind,
    global::const_dsl::{EffList, RouteResolver, ScopeEvent},
};

use super::super::images::program::{CompiledProgramCounts, MAX_COMPILED_PROGRAM_SCOPES};
const MAX_COMPILED_IMAGE_NODES: usize = crate::eff::meta::MAX_EFF_NODES;
const ROUTE_SCOPE_ORDINAL_BYTES: usize = MAX_COMPILED_IMAGE_NODES.div_ceil(8);
const MAX_TRACKED_ROLE_FACTS: usize = u16::BITS as usize;

mod impls;

#[inline(always)]
const fn checked_role_index(role: u8) -> usize {
    let role = role as usize;
    if role >= MAX_TRACKED_ROLE_FACTS {
        panic!("role index exceeds tracked lowering facts");
    }
    role
}

#[inline(always)]
const fn increment_compact_count(value: u16) -> u16 {
    if value == u16::MAX {
        panic!("lowering count overflow");
    }
    value + 1
}

#[derive(Clone)]
struct ProgramImageData {
    compiled_program_counts: CompiledProgramCounts,
    lowering_facts: ProgramLoweringFacts,
}

#[derive(Clone)]
struct ProgramRoleImageData {
    facts: [RoleCompiledFacts; MAX_TRACKED_ROLE_FACTS],
    count: u8,
}

#[derive(Clone)]
pub(crate) struct CompiledProgramImage {
    program: ProgramImageData,
    roles: ProgramRoleImageData,
}

#[derive(Clone, Copy)]
struct ProgramLoweringFacts {
    scope_count: u16,
    max_active_scope_depth: u16,
    max_route_stack_depth: u16,
    eff_count: u16,
    parallel_enter_count: u16,
    route_scope_count: u16,
}

impl ProgramLoweringFacts {
    const EMPTY: Self = Self {
        scope_count: 0,
        max_active_scope_depth: 0,
        max_route_stack_depth: 0,
        eff_count: 0,
        parallel_enter_count: 0,
        route_scope_count: 0,
    };
}

#[derive(Clone, Copy)]
struct RoleCompiledFacts {
    local_step_count: u16,
    resident_row_count: u16,
    resident_row_lane_entry_count: u16,
    resident_row_lane_word_count: u16,
    active_lane_count: u16,
    endpoint_lane_slot_count: u16,
    logical_lane_count: u16,
}

impl RoleCompiledFacts {
    const EMPTY: Self = Self {
        local_step_count: 0,
        resident_row_count: 0,
        resident_row_lane_entry_count: 0,
        resident_row_lane_word_count: 0,
        active_lane_count: 0,
        endpoint_lane_slot_count: 0,
        logical_lane_count: 0,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct RoleCompiledCounts {
    pub(crate) max_route_stack_depth: usize,
    pub(crate) local_step_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
}
