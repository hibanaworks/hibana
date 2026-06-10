use crate::{
    control::lease::planner::LeaseCapacityBudget,
    eff::{
        EffAtom, EffKind,
        meta::{MAX_SEGMENT_EFFS, MAX_SEGMENTS},
    },
    global::{
        ControlDesc,
        const_dsl::{
            ControlMarker, EffList, ResolverMode, ScopeEvent, ScopeMarker, SegmentSummary,
        },
    },
};

use super::super::images::program::{
    CompiledProgramCounts, MAX_COMPILED_PROGRAM_CONTROLS, MAX_COMPILED_PROGRAM_RESOURCES,
    MAX_COMPILED_PROGRAM_SCOPES, MAX_COMPILED_PROGRAM_TAP_EVENTS,
};
const MAX_COMPILED_IMAGE_NODES: usize = crate::eff::meta::MAX_EFF_NODES;
const ROUTE_SCOPE_ORDINAL_WORDS: usize = (MAX_COMPILED_IMAGE_NODES + 63) / 64;
const MAX_TRACKED_ROLE_FACTS: usize = u16::BITS as usize;
const MAX_COMPILED_SCOPE_MARKERS: usize = MAX_COMPILED_PROGRAM_SCOPES;
const MAX_COMPILED_ATOM_ROWS: usize = crate::eff::meta::MAX_EFF_NODES;
const MAX_COMPILED_POLICY_ROWS: usize = crate::eff::meta::MAX_EFF_NODES;
const MAX_COMPILED_CONTROL_DESC_ROWS: usize = crate::eff::meta::MAX_EFF_NODES;
const MAX_COMPILED_CONTROL_MARKERS: usize = MAX_SEGMENTS * 2;

mod impls;

#[inline(always)]
const fn control_scope_mask_bit(scope_kind: crate::global::const_dsl::ControlScopeKind) -> u8 {
    match scope_kind {
        crate::global::const_dsl::ControlScopeKind::None => 0,
        crate::global::const_dsl::ControlScopeKind::Loop => 1 << 0,
        crate::global::const_dsl::ControlScopeKind::State => 1 << 1,
        crate::global::const_dsl::ControlScopeKind::Abort => 0,
        crate::global::const_dsl::ControlScopeKind::Topology => 1 << 3,
        crate::global::const_dsl::ControlScopeKind::Policy => 0,
        crate::global::const_dsl::ControlScopeKind::Route => 0,
    }
}

#[inline(always)]
const fn reject_dynamic_policy_unsupported() -> ! {
    panic!("policy op");
}

#[inline(always)]
const fn checked_role_index(role: u8) -> usize {
    let role = role as usize;
    if role >= MAX_TRACKED_ROLE_FACTS {
        panic!("role index exceeds tracked lowering facts");
    }
    role
}

#[derive(Clone, Copy)]
struct ProgramImageSegmentData {
    atom_mask: u128,
    summary: SegmentSummary,
    node_len: u16,
    atom_row_start: u16,
    atom_row_len: u16,
    scope_marker_start: u16,
    scope_marker_len: u16,
    policy_row_start: u16,
    policy_row_len: u16,
    control_desc_row_start: u16,
    control_desc_row_len: u16,
    control_marker_start: u16,
    control_marker_len: u16,
}

impl ProgramImageSegmentData {
    const EMPTY: Self = Self {
        atom_mask: 0,
        summary: SegmentSummary::EMPTY,
        node_len: 0,
        atom_row_start: 0,
        atom_row_len: 0,
        scope_marker_start: 0,
        scope_marker_len: 0,
        policy_row_start: 0,
        policy_row_len: 0,
        control_desc_row_start: 0,
        control_desc_row_len: 0,
        control_marker_start: 0,
        control_marker_len: 0,
    };

    #[inline(always)]
    const fn compact_count(value: usize) -> u16 {
        if value > u16::MAX as usize {
            panic!("lowering segment row count overflow");
        }
        value as u16
    }
}

#[derive(Clone, Copy)]
struct ProgramAtomRow {
    offset: u16,
    atom: EffAtom,
}

impl ProgramAtomRow {
    const EMPTY: Self = Self {
        offset: u16::MAX,
        atom: EffAtom {
            from: 0,
            to: 0,
            label: 0,
            is_control: false,
            resource: None,
            lane: 0,
        },
    };

    #[inline(always)]
    const fn new(offset: usize, atom: EffAtom) -> Self {
        Self {
            offset: ProgramImageSegmentData::compact_count(offset),
            atom,
        }
    }
}

#[derive(Clone, Copy)]
struct ProgramPolicyRow {
    offset: u16,
    policy: ResolverMode,
}

impl ProgramPolicyRow {
    const EMPTY: Self = Self {
        offset: u16::MAX,
        policy: ResolverMode::Static,
    };

    #[inline(always)]
    const fn new(offset: usize, policy: ResolverMode) -> Self {
        Self {
            offset: ProgramImageSegmentData::compact_count(offset),
            policy,
        }
    }
}

#[derive(Clone, Copy)]
struct ProgramControlDescRow {
    offset: u16,
    desc: Option<ControlDesc>,
}

impl ProgramControlDescRow {
    const EMPTY: Self = Self {
        offset: u16::MAX,
        desc: None,
    };

    #[inline(always)]
    const fn new(offset: usize, desc: ControlDesc) -> Self {
        Self {
            offset: ProgramImageSegmentData::compact_count(offset),
            desc: Some(desc),
        }
    }
}

#[derive(Clone)]
struct ProgramImageValidationData {
    segments: [ProgramImageSegmentData; MAX_SEGMENTS],
    len: usize,
    atom_rows: [ProgramAtomRow; MAX_COMPILED_ATOM_ROWS],
    atom_row_len: usize,
    scope_markers: [ScopeMarker; MAX_COMPILED_SCOPE_MARKERS],
    scope_marker_len: usize,
    policy_rows: [ProgramPolicyRow; MAX_COMPILED_POLICY_ROWS],
    policy_row_len: usize,
    control_desc_rows: [ProgramControlDescRow; MAX_COMPILED_CONTROL_DESC_ROWS],
    control_desc_row_len: usize,
}

#[derive(Clone)]
struct ProgramImageData {
    control_markers: [ControlMarker; MAX_COMPILED_CONTROL_MARKERS],
    control_marker_len: usize,
    control_markers_complete: bool,
    lease_budget: LeaseCapacityBudget,
    compiled_program_counts: CompiledProgramCounts,
    lowering_facts: ProgramLoweringFacts,
    control_scope_mask: u8,
}

#[derive(Clone)]
struct ProgramRoleImageData {
    facts: [RoleCompiledFacts; MAX_TRACKED_ROLE_FACTS],
    count: u8,
}

#[derive(Clone)]
pub(crate) struct CompiledProgramImage {
    validation: ProgramImageValidationData,
    program: ProgramImageData,
    roles: ProgramRoleImageData,
}

#[derive(Clone, Copy, Default)]
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

#[derive(Clone, Copy, Default)]
struct RoleCompiledFacts {
    local_step_count: u16,
    resident_row_count: u16,
    resident_row_lane_entry_count: u16,
    resident_row_lane_word_count: u16,
    passive_linger_route_scope_count: u16,
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
        passive_linger_route_scope_count: 0,
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
    pub(crate) passive_linger_route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramView<'a> {
    segments: &'a [ProgramImageSegmentData; MAX_SEGMENTS],
    len: usize,
    atom_rows: &'a [ProgramAtomRow],
    scope_markers: &'a [ScopeMarker],
    policy_rows: &'a [ProgramPolicyRow],
    control_desc_rows: &'a [ProgramControlDescRow],
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
