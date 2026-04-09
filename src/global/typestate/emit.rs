//! Lowering walk facade for typestate synthesis.

#[cfg(test)]
use super::builder::RoleTypestate;
use super::builder::RoleTypestateValue;
use super::{
    emit_walk::RoleTypestateBuildScratch,
    facts::{LocalAction, LocalNode, StateIndex, state_index_to_usize},
};
use crate::{
    eff::EffIndex,
    global::{
        compiled::LoweringSummary,
        const_dsl::{PolicyMode, ScopeId, ScopeKind},
        role_program::{LocalStep, MAX_LANES, MAX_PHASES, MAX_STEPS, Phase, PhaseRouteGuard},
    },
};

use super::registry::ScopeRegion;

#[inline(always)]
fn phase_route_guard_for_scope_registry(
    scope_registry: &super::registry::ScopeRegistry,
    _role: u8,
    state: StateIndex,
) -> Option<(ScopeId, u8)> {
    if state.is_max() {
        return None;
    }
    let state_idx = state_index_to_usize(state);
    let mut best_scope = ScopeId::none();
    let mut best_arm = 0u8;
    let mut best_nest = u16::MAX;
    let mut idx = 0usize;
    while idx < scope_registry.record_count() {
        let record = scope_registry.record_at(idx);
        let record_start = state_index_to_usize(record.start);
        let record_end = state_index_to_usize(record.end);
        if matches!(record.kind, ScopeKind::Route)
            && record_start <= state_idx
            && state_idx < record_end
            && record.nest < best_nest
            && let Some(arm) =
                super::emit_route::phase_route_arm_for_record(record, _role, state_idx)
        {
            best_scope = record.scope_id.to_scope_id();
            best_arm = arm;
            best_nest = record.nest;
        }
        idx += 1;
    }
    if best_scope.is_none() {
        None
    } else {
        Some((best_scope, best_arm))
    }
}

#[inline(always)]
fn controller_arm_entry_label(node: LocalNode) -> Option<u8> {
    match node.action() {
        LocalAction::Local { label, .. } => Some(label),
        _ => None,
    }
}

#[inline(always)]
fn controller_arm_entry_by_arm_for_scope_registry(
    scope_registry: &super::registry::ScopeRegistry,
    scope_id: ScopeId,
    arm: u8,
    mut node_at: impl FnMut(usize) -> LocalNode,
) -> Option<(StateIndex, u8)> {
    let entry = scope_registry.controller_arm_entry(scope_id, arm)?;
    let label = controller_arm_entry_label(node_at(state_index_to_usize(entry)))?;
    Some((entry, label))
}

#[inline(always)]
fn passive_arm_scope_by_arm_for_scope_registry(
    scope_registry: &super::registry::ScopeRegistry,
    scope_id: ScopeId,
    arm: u8,
    mut node_at: impl FnMut(usize) -> LocalNode,
) -> Option<ScopeId> {
    let entry = scope_registry.passive_arm_entry(scope_id, arm)?;
    let mut current = node_at(state_index_to_usize(entry)).scope();
    let mut candidate = ScopeId::none();
    while !current.is_none() && current.raw() != scope_id.raw() {
        if matches!(current.kind(), ScopeKind::Route) {
            candidate = current;
        }
        let Some(parent) = scope_registry.parent_of(current) else {
            break;
        };
        if parent == scope_id {
            return (!candidate.is_none()).then_some(candidate);
        }
        current = parent;
    }
    None
}

#[inline(always)]
fn controller_arm_entry_for_label_for_scope_registry(
    scope_registry: &super::registry::ScopeRegistry,
    scope_id: ScopeId,
    label: u8,
    mut node_at: impl FnMut(usize) -> LocalNode,
) -> Option<StateIndex> {
    let mut arm = 0u8;
    while arm < 2 {
        if let Some((entry, entry_label)) = controller_arm_entry_by_arm_for_scope_registry(
            scope_registry,
            scope_id,
            arm,
            &mut node_at,
        ) && entry_label == label
        {
            return Some(entry);
        }
        arm += 1;
    }
    None
}

#[inline(always)]
pub(crate) fn phase_route_guard_for_state_for_role(
    typestate: &RoleTypestateValue,
    role: u8,
    state: StateIndex,
) -> Option<(ScopeId, u8)> {
    phase_route_guard_for_scope_registry(&typestate.scope_registry, role, state)
}

#[inline(always)]
#[cfg(test)]
pub(crate) fn phase_route_guard_for_built_state_for_role<const ROLE: u8>(
    typestate: &RoleTypestate<ROLE>,
    role: u8,
    state: StateIndex,
) -> Option<(ScopeId, u8)> {
    phase_route_guard_for_scope_registry(&typestate.scope_registry, role, state)
}

#[inline(never)]
pub(crate) unsafe fn init_value_from_summary_for_role(
    dst: *mut RoleTypestateValue,
    nodes_ptr: *mut LocalNode,
    nodes_cap: usize,
    role: u8,
    scope_records: &mut [super::registry::ScopeRecord],
    scope_slots_by_scope: *mut u16,
    route_dense_by_slot: *mut u16,
    route_records: *mut super::registry::RouteScopeRecord,
    route_scope_cap: usize,
    summary: &LoweringSummary,
    scratch: &mut RoleCompileScratch,
) {
    unsafe {
        core::ptr::addr_of_mut!((*dst).nodes).write(nodes_ptr.cast_const());
        super::emit_walk::init_role_typestate_value(
            nodes_ptr,
            nodes_cap,
            core::ptr::addr_of_mut!((*dst).len),
            core::ptr::addr_of_mut!((*dst).scope_registry),
            role,
            &mut scratch.typestate_build,
            scope_records,
            scope_slots_by_scope,
            route_dense_by_slot,
            route_records,
            route_scope_cap,
            summary.view(),
        );
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RouteScopePayloadStats {
    pub route_scope_count: usize,
    pub total_first_recv_entries: usize,
    pub max_first_recv_entries: usize,
    pub total_arm_lane_last_entries: usize,
    pub max_arm_lane_last_entries: usize,
    pub total_arm_lane_last_override_entries: usize,
    pub max_arm_lane_last_override_entries: usize,
    pub total_offer_lane_entries: usize,
    pub max_offer_lane_entries: usize,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopePayloadStats {
    pub scope_count: usize,
    pub total_lane_first_entries: usize,
    pub max_lane_first_entries: usize,
    pub total_lane_last_entries: usize,
    pub max_lane_last_entries: usize,
    pub total_arm_entries: usize,
    pub max_arm_entries: usize,
    pub total_passive_arm_scopes: usize,
    pub max_passive_arm_scopes: usize,
}

#[cfg(test)]
const fn count_offer_lane_entries(mask: u8) -> usize {
    mask.count_ones() as usize
}

#[cfg(test)]
fn count_lane_entries(entries: &[crate::eff::EffIndex; MAX_LANES]) -> usize {
    let mut count = 0usize;
    let mut idx = 0usize;
    while idx < MAX_LANES {
        if entries[idx] != crate::eff::EffIndex::MAX {
            count += 1;
        }
        idx += 1;
    }
    count
}

#[cfg(test)]
fn count_state_entries(entries: &[StateIndex; 2]) -> usize {
    let mut count = 0usize;
    let mut idx = 0usize;
    while idx < 2 {
        if entries[idx] != StateIndex::MAX {
            count += 1;
        }
        idx += 1;
    }
    count
}

#[cfg(test)]
fn count_arm_lane_last_entries(record: &super::registry::RouteScopeRecord) -> usize {
    count_lane_entries(&record.arm0_lane_last_eff) + count_offer_lane_entries(record.arm1_lane_mask)
}

#[cfg(test)]
fn count_arm_lane_last_override_entries(
    scope: &super::registry::ScopeRecord,
    route: &super::registry::RouteScopeRecord,
) -> usize {
    let mut count = 0usize;
    let mut lane = 0usize;
    while lane < MAX_LANES {
        let eff = route.arm0_lane_last_eff[lane];
        if eff != crate::eff::EffIndex::MAX && scope.lane_last_eff[lane] != eff {
            count += 1;
        }
        lane += 1;
    }
    count
}

/// Reusable runtime scratch owner for role-local lowering.
///
/// This keeps the canonical `no_std`/`no_alloc` path off the call stack by
/// moving builder workspaces into a stable owner held by the control plane.
pub(crate) struct RoleCompileScratch {
    pub(crate) typestate_build: RoleTypestateBuildScratch,
    pub(crate) by_eff_index: [LocalStep; MAX_STEPS],
    pub(crate) present: [bool; MAX_STEPS],
    pub(crate) steps: [LocalStep; MAX_STEPS],
    pub(crate) eff_index_to_step: [u16; MAX_STEPS],
    pub(crate) step_index_to_state: [StateIndex; MAX_STEPS],
    pub(crate) route_guards: [PhaseRouteGuard; MAX_STEPS],
    pub(crate) phases: [Phase; MAX_PHASES],
    pub(crate) parallel_ranges: [(usize, usize); MAX_PHASES],
}

impl RoleCompileScratch {
    #[cfg(test)]
    pub(crate) const fn new() -> Self {
        Self {
            typestate_build: RoleTypestateBuildScratch::new(),
            by_eff_index: [LocalStep::EMPTY; MAX_STEPS],
            present: [false; MAX_STEPS],
            steps: [LocalStep::EMPTY; MAX_STEPS],
            eff_index_to_step: [u16::MAX; MAX_STEPS],
            step_index_to_state: [StateIndex::MAX; MAX_STEPS],
            route_guards: [PhaseRouteGuard::EMPTY; MAX_STEPS],
            phases: [Phase::EMPTY; MAX_PHASES],
            parallel_ranges: [(0usize, 0usize); MAX_PHASES],
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            RoleTypestateBuildScratch::init_empty(core::ptr::addr_of_mut!((*dst).typestate_build));
            let by_eff_index = core::ptr::addr_of_mut!((*dst).by_eff_index).cast::<LocalStep>();
            let present = core::ptr::addr_of_mut!((*dst).present).cast::<bool>();
            let steps = core::ptr::addr_of_mut!((*dst).steps).cast::<LocalStep>();
            let eff_index_to_step = core::ptr::addr_of_mut!((*dst).eff_index_to_step).cast::<u16>();
            let step_index_to_state =
                core::ptr::addr_of_mut!((*dst).step_index_to_state).cast::<StateIndex>();
            let route_guards =
                core::ptr::addr_of_mut!((*dst).route_guards).cast::<PhaseRouteGuard>();
            let mut i = 0;
            while i < MAX_STEPS {
                by_eff_index.add(i).write(LocalStep::EMPTY);
                present.add(i).write(false);
                steps.add(i).write(LocalStep::EMPTY);
                eff_index_to_step.add(i).write(u16::MAX);
                step_index_to_state.add(i).write(StateIndex::MAX);
                route_guards.add(i).write(PhaseRouteGuard::EMPTY);
                i += 1;
            }

            let phases = core::ptr::addr_of_mut!((*dst).phases).cast::<Phase>();
            let parallel_ranges =
                core::ptr::addr_of_mut!((*dst).parallel_ranges).cast::<(usize, usize)>();
            let mut j = 0;
            while j < MAX_PHASES {
                phases.add(j).write(Phase::EMPTY);
                parallel_ranges.add(j).write((0usize, 0usize));
                j += 1;
            }
        }
    }
}

impl RoleTypestateValue {
    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub(crate) fn node(&self, index: usize) -> LocalNode {
        unsafe { *self.nodes.add(index) }
    }

    pub(in crate::global::typestate) fn scope_region_for(
        &self,
        scope_id: ScopeId,
    ) -> Option<ScopeRegion> {
        self.scope_registry.lookup_region(scope_id)
    }

    pub(in crate::global::typestate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_registry.parent_of(scope_id)
    }

    pub(in crate::global::typestate) fn passive_arm_jump(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.passive_arm_jump(scope_id, arm)
    }

    pub(in crate::global::typestate) fn passive_arm_entry(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.passive_arm_entry(scope_id, arm)
    }

    pub(in crate::global::typestate) fn passive_arm_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<ScopeId> {
        passive_arm_scope_by_arm_for_scope_registry(&self.scope_registry, scope_id, arm, |idx| {
            self.node(idx)
        })
    }

    #[inline]
    pub(in crate::global::typestate) fn route_recv_state(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.route_recv_state(scope_id, arm)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_arm_count(&self, scope_id: ScopeId) -> Option<u16> {
        self.scope_registry.route_arm_count(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_offer_lane_list(
        &self,
        scope_id: ScopeId,
    ) -> Option<([u8; MAX_LANES], usize)> {
        self.scope_registry.route_offer_lane_list(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_offer_entry(
        &self,
        scope_id: ScopeId,
    ) -> Option<StateIndex> {
        self.scope_registry.route_offer_entry(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_scope_slot(
        &self,
        scope_id: ScopeId,
    ) -> Option<usize> {
        self.scope_registry.route_scope_slot(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_scope_dense_ordinal(
        &self,
        slot: usize,
    ) -> Option<usize> {
        self.scope_registry.route_scope_dense_ordinal(slot)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.scope_registry.first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_target_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.scope_registry
            .first_recv_dispatch_target_for_label(scope_id, label)
    }

    #[inline]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.scope_registry.route_scope_count()
    }

    #[inline]
    pub(crate) fn max_offer_entries(&self) -> usize {
        self.scope_registry.max_offer_entries()
    }

    #[inline]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.scope_registry.max_route_stack_depth()
    }

    #[inline]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.scope_registry.max_loop_stack_depth()
    }

    #[inline]
    pub(in crate::global::typestate) fn scope_lane_first_eff(
        &self,
        scope_id: ScopeId,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_registry.scope_lane_first_eff(scope_id, lane)
    }

    #[inline]
    pub(in crate::global::typestate) fn scope_lane_last_eff(
        &self,
        scope_id: ScopeId,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_registry.scope_lane_last_eff(scope_id, lane)
    }

    #[inline]
    pub(in crate::global::typestate) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_registry
            .scope_lane_last_eff_for_arm(scope_id, arm, lane)
    }

    #[inline]
    pub(in crate::global) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        controller_arm_entry_by_arm_for_scope_registry(&self.scope_registry, scope_id, arm, |idx| {
            self.node(idx)
        })
    }

    #[inline]
    pub(in crate::global) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        controller_arm_entry_for_label_for_scope_registry(
            &self.scope_registry,
            scope_id,
            label,
            |idx| self.node(idx),
        )
    }

    #[inline]
    pub(in crate::global::typestate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
        self.scope_registry.route_controller(scope_id)
    }

    #[inline(always)]
    pub(in crate::global) fn has_parallel_phase_scope(&self) -> bool {
        let mut idx = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Parallel)
                && super::emit_route::parallel_phase_eff_range(record).is_some()
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(in crate::global) fn parallel_phase_range_at(
        &self,
        ordinal: usize,
    ) -> Option<(usize, usize)> {
        let mut idx = 0usize;
        let mut seen = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Parallel)
                && let Some(range) = super::emit_route::parallel_phase_eff_range(record)
            {
                if seen == ordinal {
                    return Some(range);
                }
                seen += 1;
            }
            idx += 1;
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn route_scope_payload_stats(&self) -> RouteScopePayloadStats {
        let mut stats = RouteScopePayloadStats::default();
        let mut idx = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Route) {
                let Some(route) = self.scope_registry.route_payload_at_slot(idx) else {
                    idx += 1;
                    continue;
                };
                let first_recv_entries = route.first_recv_len as usize;
                let arm_lane_last_entries = count_arm_lane_last_entries(route);
                let arm_lane_last_override_entries =
                    count_arm_lane_last_override_entries(record, route);
                let offer_lane_entries = count_offer_lane_entries(route.offer_lanes);

                stats.route_scope_count += 1;
                stats.total_first_recv_entries += first_recv_entries;
                stats.total_arm_lane_last_entries += arm_lane_last_entries;
                stats.total_arm_lane_last_override_entries += arm_lane_last_override_entries;
                stats.total_offer_lane_entries += offer_lane_entries;

                if first_recv_entries > stats.max_first_recv_entries {
                    stats.max_first_recv_entries = first_recv_entries;
                }
                if arm_lane_last_entries > stats.max_arm_lane_last_entries {
                    stats.max_arm_lane_last_entries = arm_lane_last_entries;
                }
                if arm_lane_last_override_entries > stats.max_arm_lane_last_override_entries {
                    stats.max_arm_lane_last_override_entries = arm_lane_last_override_entries;
                }
                if offer_lane_entries > stats.max_offer_lane_entries {
                    stats.max_offer_lane_entries = offer_lane_entries;
                }
            }
            idx += 1;
        }
        stats
    }

    #[cfg(test)]
    pub(crate) fn scope_payload_stats(&self) -> ScopePayloadStats {
        let mut stats = ScopePayloadStats::default();
        let mut idx = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            let lane_first_entries = count_lane_entries(&record.lane_first_eff);
            let lane_last_entries = count_lane_entries(&record.lane_last_eff);
            let arm_entries = count_state_entries(&record.arm_entry);
            let mut passive_arm_scopes = 0usize;
            let mut arm = 0u8;
            while arm < 2 {
                if passive_arm_scope_by_arm_for_scope_registry(
                    &self.scope_registry,
                    record.scope_id.to_scope_id(),
                    arm,
                    |node_idx| self.node(node_idx),
                )
                .is_some()
                {
                    passive_arm_scopes += 1;
                }
                arm += 1;
            }

            stats.scope_count += 1;
            stats.total_lane_first_entries += lane_first_entries;
            stats.total_lane_last_entries += lane_last_entries;
            stats.total_arm_entries += arm_entries;
            stats.total_passive_arm_scopes += passive_arm_scopes;

            if lane_first_entries > stats.max_lane_first_entries {
                stats.max_lane_first_entries = lane_first_entries;
            }
            if lane_last_entries > stats.max_lane_last_entries {
                stats.max_lane_last_entries = lane_last_entries;
            }
            if arm_entries > stats.max_arm_entries {
                stats.max_arm_entries = arm_entries;
            }
            if passive_arm_scopes > stats.max_passive_arm_scopes {
                stats.max_passive_arm_scopes = passive_arm_scopes;
            }
            idx += 1;
        }
        stats
    }
}

#[cfg(test)]
impl<const ROLE: u8> RoleTypestate<ROLE> {
    /// Number of nodes present in the typestate (including the terminal node).
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Access a node by index.
    #[inline(always)]
    pub(crate) const fn node(&self, index: usize) -> LocalNode {
        self.nodes[index]
    }

    pub(in crate::global::typestate) fn scope_region_for(
        &self,
        scope_id: ScopeId,
    ) -> Option<ScopeRegion> {
        self.scope_registry.lookup_region(scope_id)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.scope_registry.first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        controller_arm_entry_by_arm_for_scope_registry(&self.scope_registry, scope_id, arm, |idx| {
            self.node(idx)
        })
    }

    #[inline]
    pub(in crate::global::typestate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
        self.scope_registry.route_controller(scope_id)
    }

    #[inline(always)]
    pub(in crate::global) fn has_parallel_phase_scope(&self) -> bool {
        let mut idx = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Parallel)
                && super::emit_route::parallel_phase_eff_range(record).is_some()
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(in crate::global) fn parallel_phase_range_at(
        &self,
        ordinal: usize,
    ) -> Option<(usize, usize)> {
        let mut idx = 0usize;
        let mut seen = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Parallel)
                && let Some(range) = super::emit_route::parallel_phase_eff_range(record)
            {
                if seen == ordinal {
                    return Some(range);
                }
                seen += 1;
            }
            idx += 1;
        }
        None
    }

    #[inline(never)]
    pub(crate) unsafe fn init_value_from_summary(
        dst: *mut Self,
        scope_records: &mut [super::registry::ScopeRecord],
        scope_slots_by_scope: *mut u16,
        route_dense_by_slot: *mut u16,
        route_records: *mut super::registry::RouteScopeRecord,
        route_scope_cap: usize,
        summary: &LoweringSummary,
        scratch: &mut RoleCompileScratch,
    ) {
        unsafe {
            super::emit_walk::init_role_typestate_value(
                core::ptr::addr_of_mut!((*dst).nodes).cast::<LocalNode>(),
                super::facts::MAX_STATES,
                core::ptr::addr_of_mut!((*dst).len),
                core::ptr::addr_of_mut!((*dst).scope_registry),
                ROLE,
                &mut scratch.typestate_build,
                scope_records,
                scope_slots_by_scope,
                route_dense_by_slot,
                route_records,
                route_scope_cap,
                summary.view(),
            );
        }
    }
}
