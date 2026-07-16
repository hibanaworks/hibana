use crate::{
    eff::EffKind,
    global::const_dsl::{EffList, ScopeEvent},
};

use super::super::images::program::CompiledProgramCounts;

mod impls;

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
pub(crate) struct CompiledProgramImage {
    program: ProgramImageData,
    max_role: u8,
}

#[derive(Clone, Copy)]
struct ProgramLoweringFacts {
    scope_count: u16,
    max_active_scope_depth: u16,
    max_route_commit_count: u16,
    eff_count: u16,
    parallel_enter_count: u16,
    route_scope_count: u16,
}

impl ProgramLoweringFacts {
    const EMPTY: Self = Self {
        scope_count: 0,
        max_active_scope_depth: 0,
        max_route_commit_count: 0,
        eff_count: 0,
        parallel_enter_count: 0,
        route_scope_count: 0,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct RoleCompiledCounts {
    pub(crate) max_route_commit_count: usize,
    pub(crate) local_step_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
}
