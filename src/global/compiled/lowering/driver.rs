use crate::{
    control::{cluster::effects::CpEffect, lease::planner::LeaseGraphBudget},
    eff::{EffKind, EffStruct},
    global::{
        ControlLabelSpec,
        const_dsl::{ControlMarker, EffList, PolicyMode, ScopeEvent, ScopeId, ScopeMarker},
    },
};

use super::program::{
    CompiledProgramCounts, MAX_COMPILED_PROGRAM_CONTROLS, MAX_COMPILED_PROGRAM_CP_EFFECTS,
    MAX_COMPILED_PROGRAM_RESOURCES, MAX_COMPILED_PROGRAM_SCOPES, MAX_COMPILED_PROGRAM_TAP_EVENTS,
    control_scope_mask_bit,
};

const MAX_LOWERING_NODES: usize = crate::eff::meta::MAX_EFF_NODES;
const CONTROL_SPEC_MASK_BYTES: usize = (MAX_LOWERING_NODES + 7) / 8;
const ROUTE_SCOPE_ORDINAL_WORDS: usize = (MAX_LOWERING_NODES + 63) / 64;
const MAX_TRACKED_ROLE_FACTS: usize = u8::MAX as usize + 1;
#[inline(always)]
const fn checked_role_index(role: u8) -> usize {
    let role = role as usize;
    if role >= MAX_TRACKED_ROLE_FACTS {
        panic!("role index exceeds tracked lowering facts");
    }
    role
}
const EMPTY_CONTROL_SPEC: ControlLabelSpec = ControlLabelSpec {
    label: 0,
    resource_tag: 0,
    scope_kind: crate::global::const_dsl::ControlScopeKind::None,
    tap_id: 0,
    shot: crate::control::cap::mint::CapShot::One,
    handling: crate::global::ControlHandling::None,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProgramStamp {
    lane0: u64,
    lane1: u64,
    len: u16,
    scope_budget: u16,
    scope_markers_len: u16,
    scope_count: u16,
    control_markers_len: u16,
    policy_markers_len: u16,
    control_specs_len: u16,
}

impl ProgramStamp {
    pub(crate) const EMPTY: Self = Self {
        lane0: 0,
        lane1: 0,
        len: 0,
        scope_budget: 0,
        scope_markers_len: 0,
        scope_count: 0,
        control_markers_len: 0,
        policy_markers_len: 0,
        control_specs_len: 0,
    };

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
    const fn mix_control_spec(mut state: u64, spec: ControlLabelSpec) -> u64 {
        state = Self::mix_u64(state, spec.label as u64);
        state = Self::mix_u64(state, spec.resource_tag as u64);
        state = Self::mix_u64(state, spec.scope_kind as u64);
        state = Self::mix_u64(state, spec.tap_id as u64);
        state = Self::mix_u64(state, spec.shot as u64);
        Self::mix_u64(state, spec.handling as u64)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn scope_count(&self) -> usize {
        self.scope_count as usize
    }
}

#[derive(Clone)]
struct LoweringValidationData {
    nodes: [EffStruct; MAX_LOWERING_NODES],
    len: usize,
    scope_markers: [ScopeMarker; MAX_LOWERING_NODES],
    scope_marker_len: usize,
    policies: [PolicyMode; MAX_LOWERING_NODES],
    control_specs: [ControlLabelSpec; MAX_LOWERING_NODES],
    control_spec_present: [u8; CONTROL_SPEC_MASK_BYTES],
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
    eff_count: u16,
    parallel_enter_count: u16,
    route_scope_count: u16,
}

impl ProgramLoweringFacts {
    const EMPTY: Self = Self {
        scope_count: 0,
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
    logical_lane_word_count: u16,
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
        logical_lane_word_count: 0,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct RoleLoweringCounts {
    pub(crate) scope_count: usize,
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
    pub(crate) logical_lane_word_count: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct LoweringView<'a> {
    nodes: &'a [EffStruct],
    scope_markers: &'a [ScopeMarker],
    policies: &'a [PolicyMode; MAX_LOWERING_NODES],
    control_specs: &'a [ControlLabelSpec; MAX_LOWERING_NODES],
    control_spec_present: &'a [u8; CONTROL_SPEC_MASK_BYTES],
}

impl<'a> LoweringView<'a> {
    #[inline(always)]
    const fn control_spec_present_at(&self, offset: usize) -> bool {
        if offset >= MAX_LOWERING_NODES {
            return false;
        }
        let byte = offset / 8;
        let bit = offset & 7;
        (self.control_spec_present[byte] & (1u8 << bit)) != 0
    }

    #[inline(always)]
    pub(crate) const fn as_slice(&self) -> &'a [EffStruct] {
        self.nodes
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(&self) -> &'a [ScopeMarker] {
        self.scope_markers
    }

    #[inline(always)]
    pub(crate) const fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        if offset < MAX_LOWERING_NODES {
            let policy = self.policies[offset];
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
    pub(crate) const fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        if offset < self.nodes.len() && self.control_spec_present_at(offset) {
            Some(self.control_specs[offset])
        } else {
            None
        }
    }

    pub(crate) fn first_route_head_dynamic_policy_in_range(
        &self,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8)> {
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
        while idx < scope_end && idx < self.nodes.len() {
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
                        let eff_struct = self.nodes[idx];
                        let tag = if matches!(eff_struct.kind, EffKind::Atom) {
                            match eff_struct.atom_data().resource {
                                Some(tag) => tag,
                                None => 0,
                            }
                        } else {
                            0
                        };
                        return Some((policy, idx, tag));
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
    const fn view(&self) -> LoweringView<'_> {
        LoweringView {
            nodes: unsafe { core::slice::from_raw_parts(self.nodes.as_ptr(), self.len) },
            scope_markers: unsafe {
                core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len)
            },
            policies: &self.policies,
            control_specs: &self.control_specs,
            control_spec_present: &self.control_spec_present,
        }
    }

    #[cfg(test)]
    #[inline(always)]
    const fn control_spec_present_at(&self, offset: usize) -> bool {
        if offset >= MAX_LOWERING_NODES {
            return false;
        }
        let byte = offset / 8;
        let bit = offset & 7;
        (self.control_spec_present[byte] & (1u8 << bit)) != 0
    }

    #[cfg(test)]
    #[inline(always)]
    const fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        if offset < self.len && self.control_spec_present_at(offset) {
            Some(self.control_specs[offset])
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
        if self.compiled_program_counts.cp_effects > MAX_COMPILED_PROGRAM_CP_EFFECTS {
            panic!("CompiledProgram: MAX_CP_EFFECTS exceeded");
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
            logical_lane_word_count: role.logical_lane_word_count as usize,
        }
    }
}

impl LoweringSummary {
    #[inline(always)]
    pub(crate) unsafe fn write_clone_to(&self, dst: *mut Self) {
        // Keep the only transient summary copy on the lowering lease slab so
        // small-stack enter paths do not materialize a full stack temporary.
        debug_assert!(!core::mem::needs_drop::<Self>());
        unsafe {
            core::ptr::copy_nonoverlapping(self as *const Self, dst, 1);
        }
    }

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

    const fn scan_into(summary: &mut Self, eff_list: &EffList) {
        let mut lane0 = ProgramStamp::mix_u64(ProgramStamp::SEED0, eff_list.len() as u64);
        let mut lane1 = ProgramStamp::mix_u64(ProgramStamp::SEED1, eff_list.scope_budget() as u64);
        let mut scope_count = 0u16;
        let mut policy_markers_len = 0u16;
        let mut control_specs_len = 0u16;
        let mut role_count = 0usize;
        let mut route_scope_ordinals = [0u64; ROUTE_SCOPE_ORDINAL_WORDS];
        let mut lease_budget = LeaseGraphBudget::new();
        let src_nodes = eff_list.as_slice();
        summary.program.lowering_facts.eff_count = src_nodes.len() as u16;
        let mut idx = 0usize;
        while idx < src_nodes.len() {
            let node = src_nodes[idx];
            summary.validation.nodes[idx] = node;
            lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
            lane1 = ProgramStamp::mix_eff_struct(lane1, node);
            if let Some((policy, _scope)) = eff_list.policy_with_scope(idx) {
                summary.validation.policies[idx] = policy;
                policy_markers_len = policy_markers_len.saturating_add(1);
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_policy(lane1, policy);
            }
            if let Some(spec) = eff_list.control_spec_at(idx) {
                summary.validation.control_specs[idx] = spec;
                summary.validation.control_spec_present[idx / 8] |= 1u8 << (idx & 7);
                control_specs_len = control_specs_len.saturating_add(1);
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_control_spec(lane1, spec);
            }
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                let policy = summary.validation.policies[idx];
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
                lease_budget = lease_budget.include_atom(atom.label, atom.resource, policy);
                summary.program.compiled_program_counts.tap_events += 1;
                if atom.is_control {
                    if let Some(tag) = atom.resource {
                        summary.program.compiled_program_counts.resources += 1;
                        if CpEffect::from_resource_tag(tag).is_some() {
                            summary.program.compiled_program_counts.cp_effects += 1;
                        }
                    }
                } else if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
                    panic!("static policy attached to non-control atom");
                }
                if policy.is_dynamic() {
                    summary.program.compiled_program_counts.dynamic_policy_sites += 1;
                }
            }
            idx += 1;
        }

        let src_scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        while scope_idx < src_scope_markers.len() {
            let marker = src_scope_markers[scope_idx];
            summary.validation.scope_markers[scope_idx] = marker;
            if matches!(marker.event, ScopeEvent::Enter) {
                scope_count = scope_count.saturating_add(1);
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
                let view = summary.validation.view();
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
            summary.roles.facts[role_idx].logical_lane_word_count =
                exact_facts.logical_lane_word_count;
            role_idx += 1;
        }

        let src_control_markers = eff_list.control_markers();
        summary.program.compiled_program_counts.controls = src_control_markers.len();
        let mut control_idx = 0usize;
        while control_idx < src_control_markers.len() {
            let marker = src_control_markers[control_idx];
            summary.program.control_markers[control_idx] = marker;
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
        summary.program.lease_budget = lease_budget;
        summary.roles.count = if role_count > u8::MAX as usize {
            u8::MAX
        } else {
            role_count as u8
        };
        summary.program.stamp = ProgramStamp {
            lane0,
            lane1,
            len: eff_list.len() as u16,
            scope_budget: eff_list.scope_budget(),
            scope_markers_len: src_scope_markers.len() as u16,
            scope_count,
            control_markers_len: src_control_markers.len() as u16,
            policy_markers_len,
            control_specs_len,
        };
    }

    const fn scan_impl(eff_list: &EffList) -> Self {
        let src_nodes = eff_list.as_slice();
        let src_scope_markers = eff_list.scope_markers();
        let src_control_markers = eff_list.control_markers();
        let mut summary = Self {
            validation: LoweringValidationData {
                nodes: [EffStruct::pure(); MAX_LOWERING_NODES],
                len: src_nodes.len(),
                scope_markers: [ScopeMarker::empty(); MAX_LOWERING_NODES],
                scope_marker_len: src_scope_markers.len(),
                policies: [PolicyMode::Static; MAX_LOWERING_NODES],
                control_specs: [EMPTY_CONTROL_SPEC; MAX_LOWERING_NODES],
                control_spec_present: [0u8; CONTROL_SPEC_MASK_BYTES],
            },
            program: LoweringProgramData {
                control_markers: [ControlMarker::empty(); MAX_LOWERING_NODES],
                control_marker_len: src_control_markers.len(),
                lease_budget: LeaseGraphBudget::new(),
                compiled_program_counts: CompiledProgramCounts {
                    cp_effects: 0,
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
                    len: eff_list.len() as u16,
                    scope_budget: eff_list.scope_budget(),
                    scope_markers_len: src_scope_markers.len() as u16,
                    scope_count: 0,
                    control_markers_len: src_control_markers.len() as u16,
                    policy_markers_len: 0,
                    control_specs_len: 0,
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
        self.validation.view()
    }

    #[cfg(test)]
    #[inline(always)]
    const fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        self.validation.control_spec_at(offset)
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

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn control_markers(&self) -> &[ControlMarker] {
        self.program.control_markers()
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

        let mut idx = 0usize;
        while idx < self.validation.len {
            if !Self::eff_struct_eq(self.validation.nodes[idx], other.validation.nodes[idx]) {
                return false;
            }
            if self.validation.policies[idx] != other.validation.policies[idx] {
                return false;
            }
            if self.control_spec_at(idx) != other.control_spec_at(idx) {
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
