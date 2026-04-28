use crate::{
    control::cap::mint::ControlOp,
    control::lease::planner::LeaseGraphBudget,
    eff::{
        EffKind, EffStruct,
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

const MAX_LOWERING_NODES: usize = crate::eff::meta::MAX_EFF_NODES;
const ROUTE_SCOPE_ORDINAL_WORDS: usize = (MAX_LOWERING_NODES + 63) / 64;
const MAX_TRACKED_ROLE_FACTS: usize = u16::BITS as usize;
#[inline(always)]
const fn checked_role_index(role: u8) -> usize {
    let role = role as usize;
    if role >= MAX_TRACKED_ROLE_FACTS {
        panic!("role index exceeds tracked lowering facts");
    }
    role
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProgramStamp {
    lane0: u64,
    lane1: u64,
}

impl ProgramStamp {
    pub(crate) const EMPTY: Self = Self { lane0: 0, lane1: 0 };

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
        state = Self::mix_u64(state, desc.label() as u64);
        state = Self::mix_u64(state, desc.resource_tag() as u64);
        state = Self::mix_u64(state, desc.scope_kind() as u64);
        state = Self::mix_u64(state, desc.tap_id() as u64);
        state = Self::mix_u64(state, desc.shot() as u64);
        state = Self::mix_u64(state, desc.path() as u64);
        Self::mix_u64(state, desc.op() as u64)
    }
}

#[derive(Clone, Copy)]
struct LoweringSegmentData {
    nodes: [EffStruct; MAX_SEGMENT_EFFS],
    policies: [PolicyMode; MAX_SEGMENT_EFFS],
    control_descs: [Option<ControlDesc>; MAX_SEGMENT_EFFS],
    summary: SegmentSummary,
    node_len: u16,
    scope_marker_start: u16,
    scope_marker_len: u16,
    control_marker_start: u16,
    control_marker_len: u16,
}

impl LoweringSegmentData {
    const EMPTY: Self = Self {
        nodes: [EffStruct::pure(); MAX_SEGMENT_EFFS],
        policies: [PolicyMode::Static; MAX_SEGMENT_EFFS],
        control_descs: [None; MAX_SEGMENT_EFFS],
        summary: SegmentSummary::EMPTY,
        node_len: 0,
        scope_marker_start: 0,
        scope_marker_len: 0,
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
pub(crate) struct LoweringSegmentView<'a> {
    start: usize,
    data: &'a LoweringSegmentData,
    scope_markers: &'a [ScopeMarker],
    control_markers: &'a [ControlMarker],
}

impl<'a> LoweringSegmentView<'a> {
    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        self.start
    }

    #[inline(always)]
    pub(crate) const fn len(self) -> usize {
        self.data.node_len as usize
    }

    #[inline(always)]
    pub(crate) const fn summary(self) -> SegmentSummary {
        self.data.summary
    }

    #[inline(always)]
    pub(crate) const fn node_at_local(self, local: usize) -> EffStruct {
        if local >= self.len() {
            panic!("lowering segment node out of bounds");
        }
        self.data.nodes[local]
    }

    #[inline(always)]
    pub(crate) const fn policy_at_local(self, local: usize) -> Option<PolicyMode> {
        if local >= self.len() {
            return None;
        }
        let policy = self.data.policies[local];
        if policy.is_static() {
            None
        } else {
            Some(policy)
        }
    }

    #[inline(always)]
    pub(crate) const fn control_desc_at_local(self, local: usize) -> Option<ControlDesc> {
        if local >= self.len() {
            None
        } else {
            self.data.control_descs[local]
        }
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(self) -> &'a [ScopeMarker] {
        self.scope_markers
    }

    #[inline(always)]
    pub(crate) const fn control_markers(self) -> &'a [ControlMarker] {
        self.control_markers
    }
}

#[derive(Clone)]
struct LoweringValidationData {
    segments: [LoweringSegmentData; MAX_SEGMENTS],
    len: usize,
    scope_markers: [ScopeMarker; MAX_LOWERING_NODES],
    scope_marker_len: usize,
}

#[derive(Clone)]
struct LoweringProgramData {
    control_markers: [ControlMarker; MAX_LOWERING_NODES],
    control_marker_len: usize,
    lease_budget: LeaseGraphBudget,
    compiled_program_counts: CompiledProgramCounts,
    lowering_facts: ProgramLoweringFacts,
    control_scope_mask: u8,
    stamp: ProgramStamp,
}

#[derive(Clone)]
struct LoweringRoleData {
    facts: [RoleLoweringFacts; MAX_TRACKED_ROLE_FACTS],
    count: u8,
}

#[derive(Clone)]
pub(crate) struct LoweringSummary {
    validation: LoweringValidationData,
    program: LoweringProgramData,
    roles: LoweringRoleData,
}

#[derive(Clone, Copy, Default)]
struct ProgramLoweringFacts {
    scope_count: u16,
    max_active_scope_depth: u16,
    eff_count: u16,
    parallel_enter_count: u16,
    route_scope_count: u16,
}

impl ProgramLoweringFacts {
    const EMPTY: Self = Self {
        scope_count: 0,
        max_active_scope_depth: 0,
        eff_count: 0,
        parallel_enter_count: 0,
        route_scope_count: 0,
    };
}

#[derive(Clone, Copy, Default)]
struct RoleLoweringFacts {
    local_step_count: u16,
    phase_count: u16,
    phase_lane_entry_count: u16,
    phase_lane_word_count: u16,
    passive_linger_route_scope_count: u16,
    active_lane_count: u16,
    endpoint_lane_slot_count: u16,
    logical_lane_count: u16,
}

impl RoleLoweringFacts {
    const EMPTY: Self = Self {
        local_step_count: 0,
        phase_count: 0,
        phase_lane_entry_count: 0,
        phase_lane_word_count: 0,
        passive_linger_route_scope_count: 0,
        active_lane_count: 0,
        endpoint_lane_slot_count: 0,
        logical_lane_count: 0,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct RoleLoweringCounts {
    pub(crate) scope_count: usize,
    pub(crate) max_active_scope_depth: usize,
    pub(crate) eff_count: usize,
    pub(crate) local_step_count: usize,
    pub(crate) phase_count: usize,
    pub(crate) phase_lane_entry_count: usize,
    pub(crate) phase_lane_word_count: usize,
    pub(crate) parallel_enter_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) passive_linger_route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct LoweringView<'a> {
    segments: &'a [LoweringSegmentData; MAX_SEGMENTS],
    len: usize,
    scope_markers: &'a [ScopeMarker],
    control_markers: &'a [ControlMarker],
}

impl<'a> LoweringView<'a> {
    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(&self) -> &'a [ScopeMarker] {
        self.scope_markers
    }

    #[inline(always)]
    pub(crate) const fn segment_count(&self) -> usize {
        let mut count = self.len / MAX_SEGMENT_EFFS;
        if !self.len.is_multiple_of(MAX_SEGMENT_EFFS) {
            count += 1;
        }
        if self.len == 0 { 0 } else { count }
    }

    #[inline(always)]
    pub(crate) const fn segment_at(&self, segment: usize) -> LoweringSegmentView<'a> {
        if segment >= MAX_SEGMENTS {
            panic!("lowering segment out of bounds");
        }
        let data = &self.segments[segment];
        let scope_start = data.scope_marker_start as usize;
        let scope_len = data.scope_marker_len as usize;
        let control_start = data.control_marker_start as usize;
        let control_len = data.control_marker_len as usize;
        LoweringSegmentView {
            start: segment * MAX_SEGMENT_EFFS,
            data,
            scope_markers: unsafe {
                core::slice::from_raw_parts(self.scope_markers.as_ptr().add(scope_start), scope_len)
            },
            control_markers: unsafe {
                core::slice::from_raw_parts(
                    self.control_markers.as_ptr().add(control_start),
                    control_len,
                )
            },
        }
    }

    #[inline(always)]
    const fn segment_slot(offset: usize) -> (usize, usize) {
        if offset >= MAX_LOWERING_NODES {
            panic!("lowering offset out of bounds");
        }
        (offset / MAX_SEGMENT_EFFS, offset % MAX_SEGMENT_EFFS)
    }

    #[inline(always)]
    pub(crate) const fn node_at(&self, offset: usize) -> EffStruct {
        if offset >= self.len {
            panic!("lowering node out of bounds");
        }
        let (segment, local) = Self::segment_slot(offset);
        self.segments[segment].nodes[local]
    }

    #[inline(always)]
    pub(crate) const fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        if offset < self.len {
            let (segment, local) = Self::segment_slot(offset);
            let policy = self.segments[segment].policies[local];
            if policy.is_static() {
                None
            } else {
                Some(policy)
            }
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn control_desc_at(&self, offset: usize) -> Option<ControlDesc> {
        if offset < self.len {
            let (segment, local) = Self::segment_slot(offset);
            self.segments[segment].control_descs[local]
        } else {
            None
        }
    }

    pub(crate) fn first_route_head_dynamic_policy_in_range(
        &self,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8, ControlOp)> {
        if route_enter_marker_idx >= self.scope_markers.len() {
            return None;
        }
        let route_marker = self.scope_markers[route_enter_marker_idx];
        if !matches!(route_marker.event, ScopeEvent::Enter)
            || !matches!(
                route_marker.scope_kind,
                crate::global::const_dsl::ScopeKind::Route
            )
            || route_marker.scope_id.canonical().raw() != route_scope.canonical().raw()
        {
            return None;
        }
        let scope_start = route_marker.offset;
        if scope_start >= MAX_LOWERING_NODES || scope_start >= scope_end {
            return None;
        }

        let mut marker_idx = route_enter_marker_idx + 1;
        let mut active_scope_depth = 1usize;
        let mut idx = scope_start;
        while idx < scope_end && idx < self.len {
            let mut scan_marker_idx = marker_idx;
            let mut depth_after_exits = active_scope_depth;
            while scan_marker_idx < self.scope_markers.len() {
                let marker = self.scope_markers[scan_marker_idx];
                if marker.offset != idx {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Exit) {
                    depth_after_exits = depth_after_exits.saturating_sub(1);
                }
                scan_marker_idx += 1;
            }

            let mut enter_count = 0usize;
            let mut nested_non_policy_enter = false;
            let mut next_marker_idx = marker_idx;
            while next_marker_idx < self.scope_markers.len() {
                let marker = self.scope_markers[next_marker_idx];
                if marker.offset != idx {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Enter) {
                    if depth_after_exits == 1
                        && !matches!(
                            marker.scope_kind,
                            crate::global::const_dsl::ScopeKind::Generic
                        )
                    {
                        nested_non_policy_enter = true;
                    }
                    enter_count += 1;
                }
                next_marker_idx += 1;
            }

            if depth_after_exits == 1 && !nested_non_policy_enter {
                if let Some(policy) = self.policy_at(idx) {
                    if policy.dynamic_policy_id().is_some() {
                        let control = match self.control_desc_at(idx) {
                            Some(control) => control,
                            None => panic!("dynamic route policy requires controller control op"),
                        };
                        if !control.supports_dynamic_policy() {
                            panic!("dynamic policy attached to unsupported control op");
                        }
                        return Some((policy, idx, control.resource_tag(), control.op()));
                    }
                }
            }
            active_scope_depth = depth_after_exits.saturating_add(enter_count);
            marker_idx = next_marker_idx;
            idx += 1;
        }
        None
    }
}

impl LoweringValidationData {
    #[inline(always)]
    const fn view<'a>(&'a self, control_markers: &'a [ControlMarker]) -> LoweringView<'a> {
        LoweringView {
            segments: &self.segments,
            len: self.len,
            scope_markers: unsafe {
                core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len)
            },
            control_markers,
        }
    }

    #[inline(always)]
    const fn control_desc_at(&self, offset: usize) -> Option<ControlDesc> {
        if offset < self.len {
            let segment = offset / MAX_SEGMENT_EFFS;
            let local = offset % MAX_SEGMENT_EFFS;
            self.segments[segment].control_descs[local]
        } else {
            None
        }
    }
}

impl LoweringProgramData {
    #[inline(always)]
    const fn control_markers(&self) -> &[ControlMarker] {
        unsafe {
            core::slice::from_raw_parts(self.control_markers.as_ptr(), self.control_marker_len)
        }
    }

    #[inline(always)]
    const fn validate_projection_program(&self, scope_marker_len: usize) {
        if self.compiled_program_counts.resources > MAX_COMPILED_PROGRAM_RESOURCES {
            panic!("CompiledProgram: MAX_RESOURCES exceeded");
        }
        if self.compiled_program_counts.tap_events > MAX_COMPILED_PROGRAM_TAP_EVENTS {
            panic!("CompiledProgram: MAX_TAP_EVENTS exceeded");
        }
        if self.compiled_program_counts.dynamic_policy_sites > MAX_LOWERING_NODES {
            panic!("CompiledProgram: MAX_DYNAMIC_POLICY_SITES exceeded");
        }
        if self.compiled_program_counts.route_controls > MAX_LOWERING_NODES {
            panic!("CompiledProgram: MAX_ROUTE_CONTROLS exceeded");
        }
        if self.control_markers().len() > MAX_COMPILED_PROGRAM_CONTROLS {
            panic!("CompiledProgram: MAX_CONTROLS exceeded");
        }
        if scope_marker_len > MAX_COMPILED_PROGRAM_SCOPES {
            panic!("CompiledProgram: MAX_SCOPES exceeded");
        }
        self.lease_budget.validate();
    }
}

impl LoweringRoleData {
    #[inline(always)]
    const fn lowering_counts<const ROLE: u8>(
        &self,
        program: ProgramLoweringFacts,
    ) -> RoleLoweringCounts {
        let role = self.facts[ROLE as usize];
        RoleLoweringCounts {
            scope_count: program.scope_count as usize,
            max_active_scope_depth: program.max_active_scope_depth as usize,
            eff_count: program.eff_count as usize,
            local_step_count: role.local_step_count as usize,
            phase_count: role.phase_count as usize,
            phase_lane_entry_count: role.phase_lane_entry_count as usize,
            phase_lane_word_count: role.phase_lane_word_count as usize,
            parallel_enter_count: program.parallel_enter_count as usize,
            route_scope_count: program.route_scope_count as usize,
            passive_linger_route_scope_count: role.passive_linger_route_scope_count as usize,
            active_lane_count: role.active_lane_count as usize,
            endpoint_lane_slot_count: role.endpoint_lane_slot_count as usize,
            logical_lane_count: role.logical_lane_count as usize,
        }
    }

    #[inline(always)]
    fn lowering_counts_for_role(
        &self,
        role: u8,
        program: ProgramLoweringFacts,
    ) -> Option<RoleLoweringCounts> {
        let role_idx = role as usize;
        if role_idx >= MAX_TRACKED_ROLE_FACTS {
            return None;
        }
        let role = self.facts[role_idx];
        Some(RoleLoweringCounts {
            scope_count: program.scope_count as usize,
            max_active_scope_depth: program.max_active_scope_depth as usize,
            eff_count: program.eff_count as usize,
            local_step_count: role.local_step_count as usize,
            phase_count: role.phase_count as usize,
            phase_lane_entry_count: role.phase_lane_entry_count as usize,
            phase_lane_word_count: role.phase_lane_word_count as usize,
            parallel_enter_count: program.parallel_enter_count as usize,
            route_scope_count: program.route_scope_count as usize,
            passive_linger_route_scope_count: role.passive_linger_route_scope_count as usize,
            active_lane_count: role.active_lane_count as usize,
            endpoint_lane_slot_count: role.endpoint_lane_slot_count as usize,
            logical_lane_count: role.logical_lane_count as usize,
        })
    }
}

impl LoweringSummary {
    #[cfg(test)]
    #[inline(always)]
    fn scope_marker_eq(
        lhs: crate::global::const_dsl::ScopeMarker,
        rhs: crate::global::const_dsl::ScopeMarker,
    ) -> bool {
        lhs.offset == rhs.offset
            && lhs.scope_id.raw() == rhs.scope_id.raw()
            && lhs.scope_kind as u8 == rhs.scope_kind as u8
            && lhs.event as u8 == rhs.event as u8
            && lhs.linger == rhs.linger
            && lhs.controller_role == rhs.controller_role
    }

    #[cfg(test)]
    #[inline(always)]
    fn control_marker_eq(
        lhs: crate::global::const_dsl::ControlMarker,
        rhs: crate::global::const_dsl::ControlMarker,
    ) -> bool {
        lhs.offset == rhs.offset
            && lhs.scope_kind as u8 == rhs.scope_kind as u8
            && lhs.tap_id == rhs.tap_id
    }

    #[cfg(test)]
    #[inline(always)]
    fn eff_struct_eq(lhs: EffStruct, rhs: EffStruct) -> bool {
        if lhs.kind != rhs.kind {
            return false;
        }
        match lhs.kind {
            EffKind::Pure => true,
            EffKind::Atom => lhs.atom_data() == rhs.atom_data(),
        }
    }

    #[inline(always)]
    const fn segment_for_scope_marker_offset(
        offset: usize,
        current_len: usize,
        event: ScopeEvent,
    ) -> usize {
        if offset > current_len || current_len > MAX_LOWERING_NODES {
            panic!("lowering marker offset out of bounds");
        }
        if matches!(event, ScopeEvent::Enter) {
            if offset >= MAX_LOWERING_NODES {
                panic!("lowering marker offset out of bounds");
            }
            return offset / MAX_SEGMENT_EFFS;
        }
        if current_len == 0 {
            0
        } else if offset == current_len && offset % MAX_SEGMENT_EFFS == 0 {
            (offset / MAX_SEGMENT_EFFS) - 1
        } else {
            offset / MAX_SEGMENT_EFFS
        }
    }

    const fn segment_for_effect_indexed_marker_offset(offset: usize) -> usize {
        if offset >= MAX_LOWERING_NODES {
            panic!("lowering effect marker offset out of bounds");
        }
        offset / MAX_SEGMENT_EFFS
    }

    const fn scan_into(summary: &mut Self, eff_list: &EffList) {
        let mut lane0 = ProgramStamp::mix_u64(ProgramStamp::SEED0, eff_list.len() as u64);
        let mut lane1 = ProgramStamp::mix_u64(ProgramStamp::SEED1, eff_list.scope_budget() as u64);
        let mut scope_count = 0u16;
        let mut policy_markers_len = 0u16;
        let mut role_count = 0usize;
        let mut route_scope_ordinals = [0u64; ROUTE_SCOPE_ORDINAL_WORDS];
        let mut lease_budget = LeaseGraphBudget::new();
        summary.program.lowering_facts.eff_count = eff_list.len() as u16;
        let mut segment = 0usize;
        while segment < eff_list.segment_count() {
            let segment_start = EffList::segment_start(segment);
            let segment_len = eff_list.segment_len(segment);
            summary.validation.segments[segment].summary = eff_list.segment_summary(segment);
            summary.validation.segments[segment].node_len =
                LoweringSegmentData::compact_count(segment_len);
            let mut local = 0usize;
            while local < segment_len {
                let idx = segment_start + local;
                let node = eff_list.node_at(idx);
                summary.validation.segments[segment].nodes[local] = node;
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_eff_struct(lane1, node);
                let policy = if let Some((policy, _scope)) = eff_list.policy_with_scope(idx) {
                    summary.validation.segments[segment].policies[local] = policy;
                    policy_markers_len = policy_markers_len.saturating_add(1);
                    lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                    lane1 = ProgramStamp::mix_policy(lane1, policy);
                    policy
                } else {
                    PolicyMode::Static
                };
                if let Some(spec) = eff_list.control_spec_at(idx) {
                    let desc = ControlDesc::from_static(spec).with_sites(
                        crate::eff::EffIndex::from_dense_ordinal(idx),
                        ControlDesc::STATIC_POLICY_SITE,
                    );
                    summary.validation.segments[segment].control_descs[local] = Some(desc);
                    lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                    lane1 = ProgramStamp::mix_control_desc(lane1, desc);
                }
                if matches!(node.kind, EffKind::Atom) {
                    let atom = node.atom_data();
                    let from = checked_role_index(atom.from);
                    let to = checked_role_index(atom.to);
                    summary.roles.facts[from].local_step_count =
                        summary.roles.facts[from].local_step_count.saturating_add(1);
                    if to != from {
                        summary.roles.facts[to].local_step_count =
                            summary.roles.facts[to].local_step_count.saturating_add(1);
                    }
                    if from + 1 > role_count {
                        role_count = from + 1;
                    }
                    if to + 1 > role_count {
                        role_count = to + 1;
                    }
                    lease_budget =
                        lease_budget.include_atom(summary.validation.control_desc_at(idx), policy);
                    summary.program.compiled_program_counts.tap_events += 1;
                    if atom.is_control {
                        if policy.is_dynamic()
                            && let Some(control_spec) = summary.validation.control_desc_at(idx)
                            && !control_spec.supports_dynamic_policy()
                        {
                            panic!("dynamic policy attached to unsupported control op");
                        }
                        if atom.resource.is_some() {
                            summary.program.compiled_program_counts.resources += 1;
                        }
                    } else if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
                        panic!("static policy attached to non-control atom");
                    }
                    if policy.is_dynamic() {
                        summary.program.compiled_program_counts.dynamic_policy_sites += 1;
                    }
                }
                local += 1;
            }
            segment += 1;
        }

        let src_scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        let mut active_scope_depth = 0u16;
        let mut max_active_scope_depth = 0u16;
        while scope_idx < src_scope_markers.len() {
            let marker = src_scope_markers[scope_idx];
            summary.validation.scope_markers[scope_idx] = marker;
            let marker_segment =
                Self::segment_for_scope_marker_offset(marker.offset, eff_list.len(), marker.event);
            if summary.validation.segments[marker_segment].scope_marker_len == 0 {
                summary.validation.segments[marker_segment].scope_marker_start =
                    LoweringSegmentData::compact_count(scope_idx);
            }
            summary.validation.segments[marker_segment].scope_marker_len =
                LoweringSegmentData::compact_count(
                    summary.validation.segments[marker_segment]
                        .scope_marker_len
                        .saturating_add(1) as usize,
                );
            if matches!(marker.event, ScopeEvent::Enter) {
                scope_count = scope_count.saturating_add(1);
                active_scope_depth = active_scope_depth.saturating_add(1);
                if active_scope_depth > max_active_scope_depth {
                    max_active_scope_depth = active_scope_depth;
                }
                if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Parallel
                ) {
                    summary.program.lowering_facts.parallel_enter_count = summary
                        .program
                        .lowering_facts
                        .parallel_enter_count
                        .saturating_add(1);
                } else if matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Route
                ) {
                    let ordinal = marker.scope_id.local_ordinal() as usize;
                    let word = ordinal / 64;
                    let bit = ordinal % 64;
                    if word >= route_scope_ordinals.len() {
                        panic!("route scope ordinal overflow");
                    }
                    let mask = 1u64 << bit;
                    if (route_scope_ordinals[word] & mask) == 0 {
                        route_scope_ordinals[word] |= mask;
                        summary.program.lowering_facts.route_scope_count = summary
                            .program
                            .lowering_facts
                            .route_scope_count
                            .saturating_add(1);
                        summary.program.compiled_program_counts.route_controls =
                            summary.program.lowering_facts.route_scope_count as usize;
                        if marker.linger
                            && let Some(controller_role) = marker.controller_role
                        {
                            let mut role_idx = 0usize;
                            while role_idx < summary.roles.facts.len() {
                                if role_idx != controller_role as usize {
                                    summary.roles.facts[role_idx]
                                        .passive_linger_route_scope_count = summary.roles.facts
                                        [role_idx]
                                        .passive_linger_route_scope_count
                                        .saturating_add(1);
                                }
                                role_idx += 1;
                            }
                        }
                    }
                }
            } else {
                active_scope_depth = active_scope_depth.saturating_sub(1);
            }
            lane0 = ProgramStamp::mix_u64(lane0, scope_idx as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.offset as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.scope_id.raw());
            lane0 = ProgramStamp::mix_u64(lane0, marker.scope_kind as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.event as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.linger as u64);
            lane1 = ProgramStamp::mix_u64(
                lane1,
                match marker.controller_role {
                    Some(role) => role as u64,
                    None => u8::MAX as u64,
                },
            );
            if let Some(controller_role) = marker.controller_role {
                let controller_role = checked_role_index(controller_role);
                if controller_role + 1 > role_count {
                    role_count = controller_role + 1;
                }
            }
            scope_idx += 1;
        }

        let mut role_idx = 0usize;
        while role_idx < role_count {
            let exact_facts = {
                let view = summary.validation.view(summary.program.control_markers());
                super::seal::exact_role_phase_facts(view, role_idx as u8)
            };
            summary.roles.facts[role_idx].phase_count = exact_facts.phase_count;
            summary.roles.facts[role_idx].phase_lane_entry_count =
                exact_facts.phase_lane_entry_count;
            summary.roles.facts[role_idx].phase_lane_word_count = exact_facts.phase_lane_word_count;
            summary.roles.facts[role_idx].active_lane_count = exact_facts.active_lane_count;
            summary.roles.facts[role_idx].endpoint_lane_slot_count =
                exact_facts.endpoint_lane_slot_count;
            summary.roles.facts[role_idx].logical_lane_count = exact_facts.logical_lane_count;
            role_idx += 1;
        }

        let src_control_markers = eff_list.control_markers();
        summary.program.compiled_program_counts.controls = src_control_markers.len();
        let mut control_idx = 0usize;
        while control_idx < src_control_markers.len() {
            let marker = src_control_markers[control_idx];
            summary.program.control_markers[control_idx] = marker;
            let marker_segment =
                Self::segment_for_effect_indexed_marker_offset(marker.offset as usize);
            if summary.validation.segments[marker_segment].control_marker_len == 0 {
                summary.validation.segments[marker_segment].control_marker_start =
                    LoweringSegmentData::compact_count(control_idx);
            }
            summary.validation.segments[marker_segment].control_marker_len =
                LoweringSegmentData::compact_count(
                    summary.validation.segments[marker_segment]
                        .control_marker_len
                        .saturating_add(1) as usize,
                );
            summary.program.control_scope_mask |= control_scope_mask_bit(marker.scope_kind);
            lane0 = ProgramStamp::mix_u64(lane0, control_idx as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.offset as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.scope_kind as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.tap_id as u64);
            if marker.tap_id != 0 {
                summary.program.compiled_program_counts.tap_events += 1;
            }
            control_idx += 1;
        }
        lease_budget.validate();

        summary.program.lowering_facts.scope_count = scope_count;
        summary.program.lowering_facts.max_active_scope_depth = max_active_scope_depth;
        summary.program.lease_budget = lease_budget;
        summary.roles.count = if role_count > u8::MAX as usize {
            u8::MAX
        } else {
            role_count as u8
        };
        summary.program.stamp = ProgramStamp { lane0, lane1 };
    }

    const fn scan_impl(eff_list: &EffList) -> Self {
        let src_scope_markers = eff_list.scope_markers();
        let src_control_markers = eff_list.control_markers();
        let mut summary = Self {
            validation: LoweringValidationData {
                segments: [LoweringSegmentData::EMPTY; MAX_SEGMENTS],
                len: eff_list.len(),
                scope_markers: [ScopeMarker::empty(); MAX_LOWERING_NODES],
                scope_marker_len: src_scope_markers.len(),
            },
            program: LoweringProgramData {
                control_markers: [ControlMarker::empty(); MAX_LOWERING_NODES],
                control_marker_len: src_control_markers.len(),
                lease_budget: LeaseGraphBudget::new(),
                compiled_program_counts: CompiledProgramCounts {
                    tap_events: 0,
                    resources: 0,
                    controls: 0,
                    dynamic_policy_sites: 0,
                    route_controls: 0,
                },
                lowering_facts: ProgramLoweringFacts::EMPTY,
                control_scope_mask: 0,
                stamp: ProgramStamp {
                    lane0: ProgramStamp::SEED0,
                    lane1: ProgramStamp::SEED1,
                },
            },
            roles: LoweringRoleData {
                facts: [RoleLoweringFacts::EMPTY; MAX_TRACKED_ROLE_FACTS],
                count: 0,
            },
        };
        Self::scan_into(&mut summary, eff_list);
        summary
    }

    #[inline(always)]
    pub(crate) const fn scan_const(eff_list: &EffList) -> Self {
        Self::scan_impl(eff_list)
    }

    #[inline(always)]
    pub(crate) const fn view(&self) -> LoweringView<'_> {
        self.validation.view(self.program.control_markers())
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn segment_summary(&self, segment: usize) -> SegmentSummary {
        if segment >= crate::eff::meta::MAX_SEGMENTS {
            panic!("lowering segment summary out of bounds");
        }
        self.validation.segments[segment].summary
    }

    #[cfg(test)]
    #[inline(always)]
    const fn control_desc_at(&self, offset: usize) -> Option<ControlDesc> {
        self.validation.control_desc_at(offset)
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.program.stamp
    }

    #[inline(always)]
    pub(crate) const fn compiled_program_counts(&self) -> CompiledProgramCounts {
        self.program.compiled_program_counts
    }

    #[inline(always)]
    pub(crate) const fn compiled_program_role_count(&self) -> usize {
        self.roles.count as usize
    }

    #[inline(always)]
    pub(crate) const fn role_lowering_counts<const ROLE: u8>(&self) -> RoleLoweringCounts {
        self.roles
            .lowering_counts::<ROLE>(self.program.lowering_facts)
    }

    #[inline(always)]
    pub(crate) fn role_lowering_counts_for_role(&self, role: u8) -> Option<RoleLoweringCounts> {
        self.roles
            .lowering_counts_for_role(role, self.program.lowering_facts)
    }

    #[inline(always)]
    pub(crate) const fn compiled_program_control_scope_mask(&self) -> u8 {
        self.program.control_scope_mask
    }

    #[inline(always)]
    pub(crate) const fn validate_projection_program(&self) {
        self.program
            .validate_projection_program(self.validation.scope_marker_len);
    }

    #[cfg(test)]
    pub(crate) fn equivalent_summary(&self, other: &Self) -> bool {
        if self.validation.len != other.validation.len
            || self.validation.scope_marker_len != other.validation.scope_marker_len
            || self.program.control_marker_len != other.program.control_marker_len
        {
            return false;
        }

        let mut segment = 0usize;
        while segment < MAX_SEGMENTS {
            let lhs = self.validation.segments[segment].clone();
            let rhs = other.validation.segments[segment].clone();
            if lhs.summary != rhs.summary
                || lhs.node_len != rhs.node_len
                || lhs.scope_marker_start != rhs.scope_marker_start
                || lhs.scope_marker_len != rhs.scope_marker_len
                || lhs.control_marker_start != rhs.control_marker_start
                || lhs.control_marker_len != rhs.control_marker_len
            {
                return false;
            }
            segment += 1;
        }

        let mut idx = 0usize;
        while idx < self.validation.len {
            let self_view = self.view();
            let other_view = other.view();
            if !Self::eff_struct_eq(self_view.node_at(idx), other_view.node_at(idx)) {
                return false;
            }
            if self_view.policy_at(idx) != other_view.policy_at(idx) {
                return false;
            }
            if self.control_desc_at(idx) != other.control_desc_at(idx) {
                return false;
            }
            idx += 1;
        }

        let mut scope_idx = 0usize;
        while scope_idx < self.validation.scope_marker_len {
            if !Self::scope_marker_eq(
                self.validation.scope_markers[scope_idx],
                other.validation.scope_markers[scope_idx],
            ) {
                return false;
            }
            scope_idx += 1;
        }

        let mut control_idx = 0usize;
        while control_idx < self.program.control_marker_len {
            if !Self::control_marker_eq(
                self.program.control_markers[control_idx],
                other.program.control_markers[control_idx],
            ) {
                return false;
            }
            control_idx += 1;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::LoweringSummary;
    use crate::eff::{EffAtom, EffStruct};
    use crate::global::StaticControlDesc;
    use crate::global::const_dsl::{ControlScopeKind, EffList, PolicyMode, ScopeId, ScopeKind};
    use crate::substrate::cap::advanced::LoopContinueKind;

    const fn atom(label: u8) -> EffStruct {
        EffStruct::atom(EffAtom {
            from: 0,
            to: 1,
            label,
            is_control: false,
            resource: None,
            lane: 0,
        })
    }

    const fn prefix_at_segment_boundary() -> EffList {
        let mut list = EffList::new();
        let mut idx = 0usize;
        while idx < crate::eff::meta::MAX_SEGMENT_EFFS {
            list = list.push(atom(idx as u8));
            idx += 1;
        }
        list
    }

    const fn scoped_suffix() -> EffList {
        EffList::new()
            .push(atom(0xaa))
            .with_scope(ScopeId::new(ScopeKind::Route, 9))
    }

    const fn scope_enter_at_boundary_program() -> EffList {
        prefix_at_segment_boundary().extend_list(scoped_suffix())
    }

    const fn scope_exit_at_boundary_program() -> EffList {
        prefix_at_segment_boundary().with_scope(ScopeId::new(ScopeKind::Route, 10))
    }

    const fn control_spec_at_boundary_program() -> EffList {
        prefix_at_segment_boundary()
            .push(atom(0xbb))
            .push_control_spec(
                crate::eff::meta::MAX_SEGMENT_EFFS,
                StaticControlDesc::of::<LoopContinueKind>(),
            )
            .push_control_marker(
                crate::eff::meta::MAX_SEGMENT_EFFS,
                ControlScopeKind::Route,
                77,
            )
            .push_policy(crate::eff::meta::MAX_SEGMENT_EFFS, PolicyMode::dynamic(77))
    }

    static SCOPE_ENTER_AT_BOUNDARY: EffList = scope_enter_at_boundary_program();
    static SCOPE_EXIT_AT_BOUNDARY: EffList = scope_exit_at_boundary_program();
    static CONTROL_SPEC_AT_BOUNDARY: EffList = control_spec_at_boundary_program();
    static SCOPE_ENTER_AT_BOUNDARY_SUMMARY: LoweringSummary =
        LoweringSummary::scan_const(&SCOPE_ENTER_AT_BOUNDARY);
    static SCOPE_EXIT_AT_BOUNDARY_SUMMARY: LoweringSummary =
        LoweringSummary::scan_const(&SCOPE_EXIT_AT_BOUNDARY);
    static CONTROL_SPEC_AT_BOUNDARY_SUMMARY: LoweringSummary =
        LoweringSummary::scan_const(&CONTROL_SPEC_AT_BOUNDARY);

    #[test]
    fn lowering_scope_enter_at_exact_segment_boundary_belongs_to_next_segment() {
        let summary = &SCOPE_ENTER_AT_BOUNDARY_SUMMARY;

        assert_eq!(summary.segment_summary(0).scope_marker_len(), 0);
        assert_eq!(summary.segment_summary(1).scope_marker_len(), 2);
        assert_eq!(summary.segment_summary(1).route_scope_enter_len(), 1);
        assert_eq!(summary.validation.segments[1].scope_marker_start, 0);
        assert_eq!(summary.validation.segments[1].scope_marker_len, 2);
    }

    #[test]
    fn lowering_scope_exit_at_exact_segment_boundary_belongs_to_previous_segment() {
        let summary = &SCOPE_EXIT_AT_BOUNDARY_SUMMARY;

        assert_eq!(summary.segment_summary(0).scope_marker_len(), 2);
        assert_eq!(summary.segment_summary(0).route_scope_enter_len(), 1);
        assert_eq!(summary.segment_summary(1).scope_marker_len(), 0);
        assert_eq!(summary.validation.segments[0].scope_marker_start, 0);
        assert_eq!(summary.validation.segments[0].scope_marker_len, 2);
    }

    #[test]
    fn lowering_control_spec_at_segment_boundary_belongs_to_effect_segment() {
        let summary = &CONTROL_SPEC_AT_BOUNDARY_SUMMARY;

        assert_eq!(summary.segment_summary(0).control_marker_len(), 0);
        assert_eq!(summary.segment_summary(0).policy_marker_len(), 0);
        assert_eq!(summary.segment_summary(0).control_spec_len(), 0);
        assert_eq!(summary.segment_summary(1).control_marker_len(), 1);
        assert_eq!(summary.segment_summary(1).policy_marker_len(), 1);
        assert_eq!(summary.segment_summary(1).control_spec_len(), 1);
        assert_eq!(summary.validation.segments[1].control_marker_start, 0);
        assert_eq!(summary.validation.segments[1].control_marker_len, 1);
    }
}
