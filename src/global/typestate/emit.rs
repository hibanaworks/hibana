//! Lowering walk facade for typestate synthesis.

use super::{
    builder::{MAX_FIRST_RECV_DISPATCH, RoleTypestate, RoleTypestateValue},
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
/// Reusable runtime scratch owner for role-local lowering.
///
/// This keeps the canonical `no_std`/`no_alloc` path off the call stack by
/// moving builder workspaces into a stable owner held by the control plane.
pub(crate) struct RoleCompileScratch {
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
    #[cfg(any(test, not(feature = "std")))]
    pub(crate) const fn new() -> Self {
        Self {
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

    #[cfg(feature = "std")]
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).by_eff_index).write([LocalStep::EMPTY; MAX_STEPS]);
            core::ptr::addr_of_mut!((*dst).present).write([false; MAX_STEPS]);
            core::ptr::addr_of_mut!((*dst).steps).write([LocalStep::EMPTY; MAX_STEPS]);
            core::ptr::addr_of_mut!((*dst).eff_index_to_step).write([u16::MAX; MAX_STEPS]);
            core::ptr::addr_of_mut!((*dst).step_index_to_state).write([StateIndex::MAX; MAX_STEPS]);
            core::ptr::addr_of_mut!((*dst).route_guards).write([PhaseRouteGuard::EMPTY; MAX_STEPS]);
            core::ptr::addr_of_mut!((*dst).phases).write([Phase::EMPTY; MAX_PHASES]);
            core::ptr::addr_of_mut!((*dst).parallel_ranges).write([(0usize, 0usize); MAX_PHASES]);
        }
    }
}
impl<const ROLE: u8> RoleTypestate<ROLE> {
    /// Number of nodes present in the typestate (including the terminal node).
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
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

    pub(in crate::global::typestate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_registry.parent_of(scope_id)
    }

    /// Get the PassiveObserverBranch Jump target for the specified arm in a scope.
    ///
    /// Returns the StateIndex of the Jump's target node for the given arm (0 or 1),
    /// or `None` if no PassiveObserverBranch Jump is registered for that arm.
    pub(in crate::global::typestate) fn passive_arm_jump(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.passive_arm_jump(scope_id, arm)
    }

    /// Get the passive arm entry index for the specified arm.
    ///
    /// Returns the StateIndex of the first cross-role node (Send or Recv) in the arm,
    /// or `None` if not set.
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
        self.scope_registry.passive_arm_scope(scope_id, arm)
    }

    /// FIRST-recv dispatch lookup for passive observers.
    ///
    /// Given a recv label, returns the route arm and leaf recv StateIndex.
    /// Returns `(arm, target_idx)` where:
    /// - `arm` is the route arm (0 or 1)
    /// - `target_idx` is the StateIndex of the recv node
    ///
    /// Returns `None` if label not found.
    /// Flattens nested routes for O(1) dispatch.
    pub(crate) fn first_recv_target(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.scope_registry.first_recv_target(scope_id, label)
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
    pub(in crate::global) const fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.scope_registry.first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_scope_slot(
        &self,
        scope_id: ScopeId,
    ) -> Option<usize> {
        self.scope_registry.route_scope_slot(scope_id)
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
    pub(in crate::global::typestate) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        self.scope_registry
            .controller_arm_entry_for_label(scope_id, label)
    }

    #[inline]
    pub(in crate::global::typestate) fn is_at_controller_arm_entry(
        &self,
        scope_id: ScopeId,
        idx: StateIndex,
    ) -> bool {
        self.scope_registry
            .is_at_controller_arm_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global) const fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.scope_registry
            .controller_arm_entry_by_arm(scope_id, arm)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
        self.scope_registry.route_controller(scope_id)
    }

    #[inline(always)]
    pub(in crate::global) const fn has_parallel_phase_scope(&self) -> bool {
        let mut idx = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present
                && matches!(record.kind, ScopeKind::Parallel)
                && super::emit_route::parallel_phase_eff_range(record).is_some()
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(in crate::global) const fn parallel_phase_range_at(
        &self,
        ordinal: usize,
    ) -> Option<(usize, usize)> {
        let mut idx = 0usize;
        let mut seen = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present
                && matches!(record.kind, ScopeKind::Parallel)
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

    #[inline(always)]
    pub(in crate::global) const fn phase_route_guard_for_state(
        &self,
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
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present
                && matches!(record.kind, ScopeKind::Route)
                && record.start <= state_idx
                && state_idx < record.end
                && record.nest < best_nest
                && let Some(arm) =
                    super::emit_route::phase_route_arm_for_record::<ROLE>(record, state_idx)
            {
                best_scope = record.scope_id;
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

    #[inline(never)]
    pub(crate) const fn from_summary(summary: &LoweringSummary) -> Self {
        let view = summary.view();
        super::emit_walk::build_role_typestate::<ROLE>(view, view.as_slice())
    }

    #[inline(never)]
    pub(crate) unsafe fn init_value_from_summary(
        dst: *mut RoleTypestateValue,
        summary: &LoweringSummary,
    ) {
        let built = Self::from_summary(summary);
        unsafe {
            core::ptr::addr_of_mut!((*dst).nodes).write(built.nodes);
            core::ptr::addr_of_mut!((*dst).len).write(built.len);
            core::ptr::addr_of_mut!((*dst).scope_registry).write(built.scope_registry);
        }
    }

    pub(crate) const fn validate_compiled_layout(&self) {
        self.validate_phase_capacity();
        self.validate_controller_arm_table_capacity();
        self.validate_first_recv_dispatch_capacity();
    }

    const fn validate_phase_capacity(&self) {
        if self.compiled_phase_count() > MAX_PHASES {
            panic!("compiled role phase capacity exceeded");
        }
    }

    const fn validate_controller_arm_table_capacity(&self) {
        if self.compiled_controller_arm_entry_count() > ScopeId::ORDINAL_CAPACITY as usize * 2 {
            panic!("controller arm table capacity exceeded");
        }
    }

    const fn compiled_controller_arm_entry_count(&self) -> usize {
        let mut count = 0usize;
        let mut ordinal = 0usize;
        while ordinal < ScopeId::ORDINAL_CAPACITY as usize {
            let route_scope = ScopeId::route(ordinal as u16);
            let mut arm = 0u8;
            while arm <= 1 {
                if self.controller_arm_entry_by_arm(route_scope, arm).is_some() {
                    count += 1;
                }
                if arm == 1 {
                    break;
                }
                arm += 1;
            }

            let loop_scope = ScopeId::loop_scope(ordinal as u16);
            let mut loop_arm = 0u8;
            while loop_arm <= 1 {
                if self
                    .controller_arm_entry_by_arm(loop_scope, loop_arm)
                    .is_some()
                {
                    count += 1;
                }
                if loop_arm == 1 {
                    break;
                }
                loop_arm += 1;
            }

            ordinal += 1;
        }
        count
    }

    const fn validate_first_recv_dispatch_capacity(&self) {
        let mut count = 0usize;
        let mut idx = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present && matches!(record.kind, ScopeKind::Route) {
                count += record.first_recv_len as usize;
                if count > ScopeId::ORDINAL_CAPACITY as usize * MAX_FIRST_RECV_DISPATCH {
                    panic!("first recv dispatch table capacity exceeded");
                }
            }
            idx += 1;
        }
    }

    const fn compiled_phase_count(&self) -> usize {
        let mut present = [false; MAX_STEPS];
        let mut local_len = 0usize;
        let mut node_idx = 0usize;
        while node_idx < self.len() {
            match self.node(node_idx).action() {
                LocalAction::Send { eff_index, .. }
                | LocalAction::Recv { eff_index, .. }
                | LocalAction::Local { eff_index, .. } => {
                    let idx = eff_index.as_usize();
                    if idx >= MAX_STEPS {
                        panic!("local step eff_index exceeds MAX_STEPS");
                    }
                    if !present[idx] {
                        present[idx] = true;
                        local_len += 1;
                    }
                }
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => {}
            }
            node_idx += 1;
        }

        if local_len == 0 {
            return 0;
        }
        if !self.has_parallel_phase_scope() {
            return 1;
        }

        let mut phase_count = 0usize;
        let mut current_eff = 0usize;
        let mut ordinal = 0usize;
        loop {
            let Some((enter_eff, exit_eff)) = self.parallel_phase_range_at(ordinal) else {
                break;
            };
            if Self::has_local_step_in_range(&present, current_eff, enter_eff) {
                phase_count += 1;
            }
            if Self::has_local_step_in_range(&present, enter_eff, exit_eff) {
                phase_count += 1;
            }
            current_eff = exit_eff;
            ordinal += 1;
        }

        if Self::has_local_step_in_range(&present, current_eff, MAX_STEPS) {
            phase_count += 1;
        }

        if phase_count == 0 { 1 } else { phase_count }
    }

    const fn has_local_step_in_range(
        present: &[bool; MAX_STEPS],
        start: usize,
        end: usize,
    ) -> bool {
        let mut idx = start;
        while idx < end && idx < MAX_STEPS {
            if present[idx] {
                return true;
            }
            idx += 1;
        }
        false
    }
}
