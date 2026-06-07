use crate::{
    control::cap::mint::ControlOp,
    control::lease::planner::LeaseGraphBudget,
    eff::{
        EffAtom, EffKind, EffStruct,
        meta::{MAX_SEGMENT_EFFS, MAX_SEGMENTS},
    },
    global::{
        ControlDesc,
        const_dsl::{
            ControlMarker, EffList, PolicyMode, ScopeEvent, ScopeId, ScopeMarker, SegmentSummary,
        },
    },
};

use super::super::images::program::{
    CompiledProgramCounts, MAX_COMPILED_PROGRAM_CONTROLS, MAX_COMPILED_PROGRAM_RESOURCES,
    MAX_COMPILED_PROGRAM_SCOPES, MAX_COMPILED_PROGRAM_TAP_EVENTS,
};
use super::program_lowering::control_scope_mask_bit;

const MAX_COMPILED_IMAGE_NODES: usize = crate::eff::meta::MAX_EFF_NODES;
const ROUTE_SCOPE_ORDINAL_WORDS: usize = (MAX_COMPILED_IMAGE_NODES + 63) / 64;
const MAX_TRACKED_ROLE_FACTS: usize = u16::BITS as usize;
const MAX_COMPILED_SCOPE_MARKERS: usize = MAX_COMPILED_PROGRAM_SCOPES;
const MAX_COMPILED_ATOM_ROWS: usize = crate::eff::meta::MAX_EFF_NODES;
const MAX_COMPILED_POLICY_ROWS: usize = MAX_SEGMENTS * 2;
const MAX_COMPILED_CONTROL_DESC_ROWS: usize = MAX_SEGMENTS * 2;
const MAX_COMPILED_CONTROL_MARKERS: usize = MAX_SEGMENTS * 2;

mod impls;
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
pub(crate) struct ProgramSourceLookup {
    policy_at: Option<fn(usize) -> Option<PolicyMode>>,
    control_desc_at: Option<fn(usize) -> Option<ControlDesc>>,
}

impl ProgramSourceLookup {
    #[inline(always)]
    pub(crate) const fn empty() -> Self {
        Self {
            policy_at: None,
            control_desc_at: None,
        }
    }

    #[inline(always)]
    pub(crate) const fn new(
        policy_at: fn(usize) -> Option<PolicyMode>,
        control_desc_at: fn(usize) -> Option<ControlDesc>,
    ) -> Self {
        Self {
            policy_at: Some(policy_at),
            control_desc_at: Some(control_desc_at),
        }
    }

    #[inline(always)]
    fn policy_at(self, offset: usize) -> Option<PolicyMode> {
        self.policy_at.and_then(|lookup| lookup(offset))
    }

    #[inline(always)]
    fn control_desc_at(self, offset: usize) -> Option<ControlDesc> {
        self.control_desc_at.and_then(|lookup| lookup(offset))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProgramStamp {
    lane0: u64,
    lane1: u64,
}

impl ProgramStamp {
    const SEED0: u64 = 0xcbf2_9ce4_8422_2325;
    const SEED1: u64 = 0x8422_2325_cbf2_9ce4;
    const PRIME0: u64 = 0x0000_0100_0000_01b3;
    const PRIME1: u64 = 0x9e37_79b1_85eb_ca87;

    #[inline(always)]
    const fn mix_u64(state: u64, value: u64) -> u64 {
        state.wrapping_mul(Self::PRIME0) ^ value.wrapping_mul(Self::PRIME1)
    }

    #[inline(always)]
    const fn mix_eff_struct(mut state: u64, node: EffStruct) -> u64 {
        state = Self::mix_u64(state, node.kind as u64);
        match node.kind {
            EffKind::Pure => state,
            EffKind::Atom => {
                let atom = node.atom_data();
                state = Self::mix_u64(state, atom.from as u64);
                state = Self::mix_u64(state, atom.to as u64);
                state = Self::mix_u64(state, atom.label as u64);
                state = Self::mix_u64(state, atom.is_control as u64);
                state = Self::mix_u64(
                    state,
                    match atom.resource {
                        Some(resource) => resource as u64,
                        None => u8::MAX as u64,
                    },
                );
                Self::mix_u64(state, atom.lane as u64)
            }
        }
    }

    #[inline(always)]
    const fn mix_policy(mut state: u64, policy: PolicyMode) -> u64 {
        match policy.dynamic_policy_id() {
            None => Self::mix_u64(state, 0),
            Some(policy_id) => {
                state = Self::mix_u64(state, 1);
                state = Self::mix_u64(state, policy_id as u64);
                Self::mix_u64(state, policy.scope().raw())
            }
        }
    }

    #[inline(always)]
    const fn mix_control_desc(mut state: u64, desc: ControlDesc) -> u64 {
        state = Self::mix_u64(state, desc.resource_tag() as u64);
        state = Self::mix_u64(state, desc.scope_kind() as u64);
        state = Self::mix_u64(state, desc.tap_id() as u64);
        state = Self::mix_u64(state, desc.shot() as u64);
        state = Self::mix_u64(state, desc.path() as u64);
        Self::mix_u64(state, desc.op() as u64)
    }

    #[inline(always)]
    pub(crate) const fn words(self) -> [u64; 2] {
        [self.lane0, self.lane1]
    }
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
    policy: PolicyMode,
}

impl ProgramPolicyRow {
    const EMPTY: Self = Self {
        offset: u16::MAX,
        policy: PolicyMode::Static,
    };

    #[inline(always)]
    const fn new(offset: usize, policy: PolicyMode) -> Self {
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
    policy_rows_complete: bool,
    control_desc_rows: [ProgramControlDescRow; MAX_COMPILED_CONTROL_DESC_ROWS],
    control_desc_row_len: usize,
    control_desc_rows_complete: bool,
}

#[derive(Clone)]
struct ProgramImageData {
    control_markers: [ControlMarker; MAX_COMPILED_CONTROL_MARKERS],
    control_marker_len: usize,
    control_markers_complete: bool,
    lease_budget: LeaseGraphBudget,
    compiled_program_counts: CompiledProgramCounts,
    lowering_facts: ProgramLoweringFacts,
    control_scope_mask: u8,
    stamp: ProgramStamp,
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
    source_lookup: ProgramSourceLookup,
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
    pub(crate) scope_count: usize,
    pub(crate) max_active_scope_depth: usize,
    pub(crate) max_route_stack_depth: usize,
    pub(crate) eff_count: usize,
    pub(crate) local_step_count: usize,
    pub(crate) resident_row_count: usize,
    pub(crate) resident_row_lane_entry_count: usize,
    pub(crate) resident_row_lane_word_count: usize,
    pub(crate) parallel_enter_count: usize,
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
    policy_rows_complete: bool,
    control_desc_rows: &'a [ProgramControlDescRow],
    control_desc_rows_complete: bool,
    source_lookup: ProgramSourceLookup,
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
