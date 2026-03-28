use core::{mem::MaybeUninit, ptr};

use crate::{
    control::{
        cluster::effects::{CpEffect, EffectEnvelope},
        lease::planner::LeaseGraphBudget,
    },
    eff::{EffKind, EffStruct},
    global::{
        ControlLabelSpec,
        const_dsl::{ControlMarker, EffList, PolicyMode, ScopeMarker},
    },
};

const MAX_LOWERING_NODES: usize = crate::eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProgramStamp {
    lane0: u64,
    lane1: u64,
    len: u16,
    scope_budget: u16,
    scope_markers_len: u16,
    control_markers_len: u16,
    policy_markers_len: u16,
    control_specs_len: u16,
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
        match policy {
            PolicyMode::Static => Self::mix_u64(state, 0),
            PolicyMode::Dynamic { policy_id, scope } => {
                state = Self::mix_u64(state, 1);
                state = Self::mix_u64(state, policy_id as u64);
                Self::mix_u64(state, scope.raw())
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
}

#[derive(Clone)]
pub(crate) struct LoweringSummary {
    nodes: [EffStruct; MAX_LOWERING_NODES],
    len: usize,
    scope_markers: [ScopeMarker; MAX_LOWERING_NODES],
    scope_marker_len: usize,
    control_markers: [ControlMarker; MAX_LOWERING_NODES],
    control_marker_len: usize,
    policies: [Option<PolicyMode>; MAX_LOWERING_NODES],
    control_specs: [Option<ControlLabelSpec>; MAX_LOWERING_NODES],
    stamp: ProgramStamp,
}

#[derive(Clone, Copy)]
pub(crate) struct LoweringView<'a> {
    nodes: &'a [EffStruct],
    scope_markers: &'a [ScopeMarker],
    control_markers: &'a [ControlMarker],
    policies: &'a [Option<PolicyMode>; MAX_LOWERING_NODES],
    control_specs: &'a [Option<ControlLabelSpec>; MAX_LOWERING_NODES],
}

impl<'a> LoweringView<'a> {
    #[inline(always)]
    pub(crate) const fn as_slice(&self) -> &'a [EffStruct] {
        self.nodes
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(&self) -> &'a [ScopeMarker] {
        self.scope_markers
    }

    #[inline(always)]
    pub(crate) const fn control_markers(&self) -> &'a [ControlMarker] {
        self.control_markers
    }

    #[inline(always)]
    pub(crate) const fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        if offset < MAX_LOWERING_NODES {
            self.policies[offset]
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        if offset < MAX_LOWERING_NODES {
            self.control_specs[offset]
        } else {
            None
        }
    }

    pub(crate) const fn first_dynamic_policy_in_range(
        &self,
        scope_start: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8)> {
        if scope_start >= MAX_LOWERING_NODES || scope_start >= scope_end {
            return None;
        }
        let mut best_offset = MAX_LOWERING_NODES;
        let mut best_policy = None;
        let mut idx = scope_start;
        while idx < scope_end && idx < self.nodes.len() {
            if let Some(policy) = self.policy_at(idx)
                && policy.is_dynamic()
                && idx < best_offset
            {
                best_offset = idx;
                best_policy = Some(policy);
            }
            idx += 1;
        }
        match best_policy {
            Some(policy) => {
                let eff_struct = self.nodes[best_offset];
                let tag = if matches!(eff_struct.kind, EffKind::Atom) {
                    match eff_struct.atom_data().resource {
                        Some(tag) => tag,
                        None => 0,
                    }
                } else {
                    0
                };
                Some((policy, best_offset, tag))
            }
            None => None,
        }
    }

    pub(crate) const fn lease_budget(&self) -> LeaseGraphBudget {
        let mut lease_budget = LeaseGraphBudget::new();
        let nodes = self.as_slice();
        let mut offset = 0usize;
        while offset < nodes.len() {
            let node = nodes[offset];
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                let policy = match self.policy_at(offset) {
                    Some(policy) => policy,
                    None => PolicyMode::Static,
                };
                lease_budget = lease_budget.include_atom(atom.label, atom.resource, policy);
            }
            offset += 1;
        }
        lease_budget
    }

    pub(crate) const fn validate_projection_program(&self) {
        let mut resource_count = 0usize;
        let mut dynamic_policy_sites = 0usize;
        let mut cp_effect_count = 0usize;
        let mut tap_event_count = 0usize;
        let nodes = self.as_slice();
        let mut offset = 0usize;
        while offset < nodes.len() {
            let node = nodes[offset];
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                let policy = match self.policy_at(offset) {
                    Some(policy) => policy,
                    None => PolicyMode::Static,
                };
                if atom.is_control {
                    if let Some(tag) = atom.resource {
                        resource_count += 1;
                        if resource_count > EffectEnvelope::MAX_RESOURCES {
                            panic!("EffectEnvelope: MAX_RESOURCES exceeded");
                        }
                        if CpEffect::from_resource_tag(tag).is_some() {
                            cp_effect_count += 1;
                            if cp_effect_count > EffectEnvelope::MAX_CP_EFFECTS {
                                panic!("EffectEnvelope: MAX_CP_EFFECTS exceeded");
                            }
                        }
                    }
                } else if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
                    panic!("static policy attached to non-control atom");
                }

                tap_event_count += 1;
                if tap_event_count > EffectEnvelope::MAX_TAP_EVENTS {
                    panic!("EffectEnvelope: MAX_TAP_EVENTS exceeded");
                }

                if policy.is_dynamic() {
                    dynamic_policy_sites += 1;
                    if dynamic_policy_sites > MAX_LOWERING_NODES {
                        panic!("CompiledProgram: MAX_DYNAMIC_POLICY_SITES exceeded");
                    }
                }
            }
            offset += 1;
        }

        let mut control_idx = 0usize;
        while control_idx < self.control_markers().len() {
            let marker = self.control_markers()[control_idx];
            if marker.tap_id != 0 {
                tap_event_count += 1;
                if tap_event_count > EffectEnvelope::MAX_TAP_EVENTS {
                    panic!("EffectEnvelope: MAX_TAP_EVENTS exceeded");
                }
            }
            control_idx += 1;
        }

        if self.scope_markers().len() > EffectEnvelope::MAX_SCOPES {
            panic!("EffectEnvelope: MAX_SCOPES exceeded");
        }
        if self.control_markers().len() > EffectEnvelope::MAX_CONTROLS {
            panic!("EffectEnvelope: MAX_CONTROLS exceeded");
        }

        self.lease_budget().validate();
    }
}

impl LoweringSummary {
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

    #[inline(always)]
    fn control_marker_eq(
        lhs: crate::global::const_dsl::ControlMarker,
        rhs: crate::global::const_dsl::ControlMarker,
    ) -> bool {
        lhs.offset == rhs.offset
            && lhs.scope_kind as u8 == rhs.scope_kind as u8
            && lhs.tap_id == rhs.tap_id
    }

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

    const fn scan_impl(eff_list: &EffList) -> Self {
        let mut nodes = [EffStruct::pure(); MAX_LOWERING_NODES];
        let mut scope_markers = [ScopeMarker::empty(); MAX_LOWERING_NODES];
        let mut control_markers = [ControlMarker::empty(); MAX_LOWERING_NODES];
        let mut policies = [None; MAX_LOWERING_NODES];
        let mut control_specs = [None; MAX_LOWERING_NODES];

        let mut lane0 = ProgramStamp::SEED0;
        let mut lane1 = ProgramStamp::SEED1;
        let mut policy_markers_len = 0u16;
        let mut control_specs_len = 0u16;

        lane0 = ProgramStamp::mix_u64(lane0, eff_list.len() as u64);
        lane1 = ProgramStamp::mix_u64(lane1, eff_list.scope_budget() as u64);

        let src_nodes = eff_list.as_slice();
        let mut idx = 0usize;
        while idx < src_nodes.len() {
            let node = src_nodes[idx];
            nodes[idx] = node;
            lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
            lane1 = ProgramStamp::mix_eff_struct(lane1, node);
            if let Some((policy, _scope)) = eff_list.policy_with_scope(idx) {
                policies[idx] = Some(policy);
                policy_markers_len = policy_markers_len.saturating_add(1);
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_policy(lane1, policy);
            }
            if let Some(spec) = eff_list.control_spec_at(idx) {
                control_specs[idx] = Some(spec);
                control_specs_len = control_specs_len.saturating_add(1);
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_control_spec(lane1, spec);
            }
            idx += 1;
        }

        let src_scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        while scope_idx < src_scope_markers.len() {
            let marker = src_scope_markers[scope_idx];
            scope_markers[scope_idx] = marker;
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
            scope_idx += 1;
        }

        let src_control_markers = eff_list.control_markers();
        let mut control_idx = 0usize;
        while control_idx < src_control_markers.len() {
            let marker = src_control_markers[control_idx];
            control_markers[control_idx] = marker;
            lane0 = ProgramStamp::mix_u64(lane0, control_idx as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.offset as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.scope_kind as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.tap_id as u64);
            control_idx += 1;
        }

        Self {
            nodes,
            len: src_nodes.len(),
            scope_markers,
            scope_marker_len: src_scope_markers.len(),
            control_markers,
            control_marker_len: src_control_markers.len(),
            policies,
            control_specs,
            stamp: ProgramStamp {
                lane0,
                lane1,
                len: eff_list.len() as u16,
                scope_budget: eff_list.scope_budget(),
                scope_markers_len: src_scope_markers.len() as u16,
                control_markers_len: src_control_markers.len() as u16,
                policy_markers_len,
                control_specs_len,
            },
        }
    }

    #[inline(always)]
    pub(crate) const fn scan_const(eff_list: &EffList) -> Self {
        Self::scan_impl(eff_list)
    }

    #[inline(always)]
    pub(crate) const fn view(&self) -> LoweringView<'_> {
        LoweringView {
            nodes: unsafe { core::slice::from_raw_parts(self.nodes.as_ptr(), self.len) },
            scope_markers: unsafe {
                core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len)
            },
            control_markers: unsafe {
                core::slice::from_raw_parts(self.control_markers.as_ptr(), self.control_marker_len)
            },
            policies: &self.policies,
            control_specs: &self.control_specs,
        }
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.stamp
    }

    #[inline(always)]
    pub(crate) const fn lease_budget(&self) -> LeaseGraphBudget {
        self.view().lease_budget()
    }

    #[inline(always)]
    pub(crate) const fn validate_projection_program(&self) {
        self.view().validate_projection_program();
    }

    pub(crate) fn equivalent_eff_list(&self, eff_list: &EffList) -> bool {
        if self.len != eff_list.len()
            || self.scope_marker_len != eff_list.scope_markers().len()
            || self.control_marker_len != eff_list.control_markers().len()
        {
            return false;
        }

        let nodes = eff_list.as_slice();
        let mut idx = 0usize;
        while idx < nodes.len() {
            if !Self::eff_struct_eq(self.nodes[idx], nodes[idx]) {
                return false;
            }
            if self.policies[idx] != eff_list.policy_with_scope(idx).map(|(policy, _)| policy) {
                return false;
            }
            if self.control_specs[idx] != eff_list.control_spec_at(idx) {
                return false;
            }
            idx += 1;
        }

        let scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        while scope_idx < scope_markers.len() {
            if !Self::scope_marker_eq(self.scope_markers[scope_idx], scope_markers[scope_idx]) {
                return false;
            }
            scope_idx += 1;
        }

        let control_markers = eff_list.control_markers();
        let mut control_idx = 0usize;
        while control_idx < control_markers.len() {
            if !Self::control_marker_eq(
                self.control_markers[control_idx],
                control_markers[control_idx],
            ) {
                return false;
            }
            control_idx += 1;
        }

        true
    }
}

#[cfg(test)]
mod runtime_scan_counter {
    use std::cell::Cell;

    std::thread_local! {
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn bump() {
        COUNT.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn reset() {
        COUNT.with(|count| count.set(0));
    }

    pub(crate) fn read() -> usize {
        COUNT.with(Cell::get)
    }
}

impl LoweringSummary {
    pub(crate) unsafe fn init_scan(dst: *mut Self, eff_list: &EffList) {
        #[cfg(test)]
        runtime_scan_counter::bump();

        unsafe {
            ptr::addr_of_mut!((*dst).nodes).write([EffStruct::pure(); MAX_LOWERING_NODES]);
            ptr::addr_of_mut!((*dst).len).write(eff_list.len());
            ptr::addr_of_mut!((*dst).scope_markers)
                .write([ScopeMarker::empty(); MAX_LOWERING_NODES]);
            ptr::addr_of_mut!((*dst).scope_marker_len).write(eff_list.scope_markers().len());
            ptr::addr_of_mut!((*dst).control_markers)
                .write([ControlMarker::empty(); MAX_LOWERING_NODES]);
            ptr::addr_of_mut!((*dst).control_marker_len).write(eff_list.control_markers().len());
            ptr::addr_of_mut!((*dst).policies).write([None; MAX_LOWERING_NODES]);
            ptr::addr_of_mut!((*dst).control_specs).write([None; MAX_LOWERING_NODES]);
        }

        let nodes = unsafe { &mut *ptr::addr_of_mut!((*dst).nodes) };
        let scope_markers = unsafe { &mut *ptr::addr_of_mut!((*dst).scope_markers) };
        let control_markers = unsafe { &mut *ptr::addr_of_mut!((*dst).control_markers) };
        let policies = unsafe { &mut *ptr::addr_of_mut!((*dst).policies) };
        let control_specs = unsafe { &mut *ptr::addr_of_mut!((*dst).control_specs) };

        let mut lane0 = ProgramStamp::SEED0;
        let mut lane1 = ProgramStamp::SEED1;
        let mut policy_markers_len = 0u16;
        let mut control_specs_len = 0u16;

        lane0 = ProgramStamp::mix_u64(lane0, eff_list.len() as u64);
        lane1 = ProgramStamp::mix_u64(lane1, eff_list.scope_budget() as u64);

        let src_nodes = eff_list.as_slice();
        let mut idx = 0usize;
        while idx < src_nodes.len() {
            let node = src_nodes[idx];
            nodes[idx] = node;
            lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
            lane1 = ProgramStamp::mix_eff_struct(lane1, node);
            if let Some((policy, _scope)) = eff_list.policy_with_scope(idx) {
                policies[idx] = Some(policy);
                policy_markers_len = policy_markers_len.saturating_add(1);
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_policy(lane1, policy);
            }
            if let Some(spec) = eff_list.control_spec_at(idx) {
                control_specs[idx] = Some(spec);
                control_specs_len = control_specs_len.saturating_add(1);
                lane0 = ProgramStamp::mix_u64(lane0, idx as u64);
                lane1 = ProgramStamp::mix_control_spec(lane1, spec);
            }
            idx += 1;
        }

        let src_scope_markers = eff_list.scope_markers();
        let mut scope_idx = 0usize;
        while scope_idx < src_scope_markers.len() {
            let marker = src_scope_markers[scope_idx];
            scope_markers[scope_idx] = marker;
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
            scope_idx += 1;
        }

        let src_control_markers = eff_list.control_markers();
        let mut control_idx = 0usize;
        while control_idx < src_control_markers.len() {
            let marker = src_control_markers[control_idx];
            control_markers[control_idx] = marker;
            lane0 = ProgramStamp::mix_u64(lane0, control_idx as u64);
            lane0 = ProgramStamp::mix_u64(lane0, marker.offset as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.scope_kind as u64);
            lane1 = ProgramStamp::mix_u64(lane1, marker.tap_id as u64);
            control_idx += 1;
        }

        unsafe {
            ptr::addr_of_mut!((*dst).stamp).write(ProgramStamp {
                lane0,
                lane1,
                len: eff_list.len() as u16,
                scope_budget: eff_list.scope_budget(),
                scope_markers_len: src_scope_markers.len() as u16,
                control_markers_len: src_control_markers.len() as u16,
                policy_markers_len,
                control_specs_len,
            });
        }
    }

    #[inline(always)]
    pub(crate) fn scan(eff_list: &EffList) -> Self {
        let mut summary = MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_scan(summary.as_mut_ptr(), eff_list);
            summary.assume_init()
        }
    }

    #[cfg(test)]
    pub(crate) fn reset_runtime_scan_count() {
        runtime_scan_counter::reset();
    }

    #[cfg(test)]
    pub(crate) fn runtime_scan_count() -> usize {
        runtime_scan_counter::read()
    }
}
