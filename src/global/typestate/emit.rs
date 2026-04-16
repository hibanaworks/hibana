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
        const_dsl::{ScopeId, ScopeKind},
    },
};

use super::registry::{MAX_FIRST_RECV_DISPATCH, ScopeRegion};

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
    if current.is_none() || current == scope_id {
        return None;
    }
    if current.kind() != ScopeKind::Route {
        current = scope_registry.route_parent_of(current)?;
    }
    while !current.is_none() && current != scope_id {
        let Some(parent) = scope_registry.route_parent_of(current) else {
            break;
        };
        if parent == scope_id {
            return Some(current);
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
    route_offer_lane_words: *mut crate::global::role_program::LaneWord,
    route_arm1_lane_words: *mut crate::global::role_program::LaneWord,
    route_lane_word_len: usize,
    lane_slot_count: usize,
    scope_lane_first_eff: *mut EffIndex,
    scope_lane_last_eff: *mut EffIndex,
    route_arm0_lane_last_eff_by_slot: *mut EffIndex,
    route_scope_cap: usize,
    summary: &LoweringSummary,
    scratch: &mut RoleTypestateBuildScratch,
) {
    unsafe {
        core::ptr::addr_of_mut!((*dst).nodes).write(nodes_ptr.cast_const());
        super::emit_walk::init_role_typestate_value(
            nodes_ptr,
            nodes_cap,
            core::ptr::addr_of_mut!((*dst).len),
            core::ptr::addr_of_mut!((*dst).scope_registry),
            role,
            scratch,
            scope_records,
            scope_slots_by_scope,
            route_dense_by_slot,
            route_records,
            route_offer_lane_words,
            route_arm1_lane_words,
            route_lane_word_len,
            lane_slot_count,
            scope_lane_first_eff,
            scope_lane_last_eff,
            route_arm0_lane_last_eff_by_slot,
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
fn count_offer_lane_entries(lanes: crate::global::role_program::LaneSetView) -> usize {
    let mut count = 0usize;
    let mut lane = 0usize;
    while lane < (u8::MAX as usize + 1) {
        if lanes.contains(lane) {
            count += 1;
        }
        lane += 1;
    }
    count
}

#[cfg(test)]
fn count_lane_entries(entries: &[crate::eff::EffIndex]) -> usize {
    let mut count = 0usize;
    let mut idx = 0usize;
    while idx < entries.len() {
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
fn count_arm_lane_last_entries(
    arm0_lane_last_eff: &[crate::eff::EffIndex],
    arm1_lanes: crate::global::role_program::LaneSetView,
) -> usize {
    count_lane_entries(arm0_lane_last_eff) + count_offer_lane_entries(arm1_lanes)
}

#[cfg(test)]
fn count_arm_lane_last_override_entries(
    scope_lane_last_eff: &[crate::eff::EffIndex],
    _route: &super::registry::RouteScopeRecord,
    arm0_lane_last_eff: &[crate::eff::EffIndex],
) -> usize {
    let mut count = 0usize;
    let mut lane = 0usize;
    while lane < arm0_lane_last_eff.len() {
        let eff = arm0_lane_last_eff[lane];
        if eff != crate::eff::EffIndex::MAX && scope_lane_last_eff[lane] != eff {
            count += 1;
        }
        lane += 1;
    }
    count
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

    pub(in crate::global::typestate) fn control_parent(
        &self,
        scope_id: ScopeId,
    ) -> Option<ScopeId> {
        self.scope_registry.control_parent_of(scope_id)
    }

    pub(in crate::global::typestate) fn route_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_registry.route_parent_of(scope_id)
    }

    pub(in crate::global::typestate) fn route_parent_arm(&self, scope_id: ScopeId) -> Option<u8> {
        self.scope_registry.route_parent_arm_of(scope_id)
    }

    pub(in crate::global::typestate) fn parallel_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_registry.parallel_root_of(scope_id)
    }

    pub(in crate::global::typestate) fn enclosing_loop(
        &self,
        scope_id: ScopeId,
    ) -> Option<ScopeId> {
        self.scope_registry.enclosing_loop_of(scope_id)
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
    pub(in crate::global::typestate) fn route_offer_lane_set(
        &self,
        scope_id: ScopeId,
    ) -> Option<crate::global::role_program::LaneSetView> {
        self.scope_registry.route_offer_lane_set(scope_id)
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

    #[cfg(test)]
    #[inline]
    pub(in crate::global) fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.scope_registry.first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH], u8)> {
        self.scope_registry.first_recv_dispatch_table(scope_id)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_label_mask(&self, scope_id: ScopeId) -> u128 {
        self.scope_registry
            .first_recv_dispatch_label_mask(scope_id)
            .unwrap_or(0)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_arm_mask(&self, scope_id: ScopeId) -> u8 {
        self.scope_registry
            .first_recv_dispatch_arm_mask(scope_id)
            .unwrap_or(0)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_lane_mask(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> u8 {
        self.scope_registry
            .first_recv_dispatch_lane_mask(scope_id, arm)
            .unwrap_or(0)
    }

    #[inline]
    pub(in crate::global) fn first_recv_dispatch_arm_label_mask(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> u128 {
        self.scope_registry
            .first_recv_dispatch_arm_label_mask(scope_id, arm)
            .unwrap_or(0)
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
    pub(crate) fn frontier_entry_capacity(&self) -> usize {
        self.scope_registry.frontier_entry_capacity()
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

    #[inline(always)]
    pub(in crate::global) fn has_parallel_phase_scope(&self) -> bool {
        let mut idx = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Parallel)
                && super::emit_route::parallel_phase_eff_range(&self.scope_registry, idx, record)
                    .is_some()
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
                && let Some(range) =
                    super::emit_route::parallel_phase_eff_range(&self.scope_registry, idx, record)
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
                let scope_lane_last_eff = self.scope_registry.scope_lane_last_row(idx);
                let arm0_lane_last_eff = self.scope_registry.route_arm0_lane_last_row(idx);
                let first_recv_entries = route.first_recv_len as usize;
                let arm_lane_last_entries = count_arm_lane_last_entries(
                    arm0_lane_last_eff,
                    self.scope_registry
                        .route_arm1_lane_set(record.scope_id.to_scope_id())
                        .unwrap_or(crate::global::role_program::LaneSetView::EMPTY),
                );
                let arm_lane_last_override_entries = count_arm_lane_last_override_entries(
                    scope_lane_last_eff,
                    route,
                    arm0_lane_last_eff,
                );
                let offer_lane_entries = count_offer_lane_entries(
                    self.scope_registry
                        .route_offer_lane_set(record.scope_id.to_scope_id())
                        .unwrap_or(crate::global::role_program::LaneSetView::EMPTY),
                );

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
            let lane_first_entries =
                count_lane_entries(self.scope_registry.scope_lane_first_row(idx));
            let lane_last_entries =
                count_lane_entries(self.scope_registry.scope_lane_last_row(idx));
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

    #[cfg(test)]
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

    #[inline(always)]
    pub(in crate::global) fn has_parallel_phase_scope(&self) -> bool {
        let mut idx = 0usize;
        while idx < self.scope_registry.record_count() {
            let record = self.scope_registry.record_at(idx);
            if matches!(record.kind, ScopeKind::Parallel)
                && super::emit_route::parallel_phase_eff_range(&self.scope_registry, idx, record)
                    .is_some()
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
                && let Some(range) =
                    super::emit_route::parallel_phase_eff_range(&self.scope_registry, idx, record)
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
        route_offer_lane_words: *mut crate::global::role_program::LaneWord,
        route_arm1_lane_words: *mut crate::global::role_program::LaneWord,
        route_lane_word_len: usize,
        lane_slot_count: usize,
        scope_lane_first_eff: *mut EffIndex,
        scope_lane_last_eff: *mut EffIndex,
        route_arm0_lane_last_eff_by_slot: *mut EffIndex,
        route_scope_cap: usize,
        summary: &LoweringSummary,
        scratch: &mut RoleTypestateBuildScratch,
    ) {
        unsafe {
            super::emit_walk::init_role_typestate_value(
                core::ptr::addr_of_mut!((*dst).nodes).cast::<LocalNode>(),
                super::facts::MAX_STATES,
                core::ptr::addr_of_mut!((*dst).len),
                core::ptr::addr_of_mut!((*dst).scope_registry),
                ROLE,
                scratch,
                scope_records,
                scope_slots_by_scope,
                route_dense_by_slot,
                route_records,
                route_offer_lane_words,
                route_arm1_lane_words,
                route_lane_word_len,
                lane_slot_count,
                scope_lane_first_eff,
                scope_lane_last_eff,
                route_arm0_lane_last_eff_by_slot,
                route_scope_cap,
                summary.view(),
            );
        }
    }
}
