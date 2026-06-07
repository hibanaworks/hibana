//! Compiled-row local affine event program view.
//!
//! This is the production event image used by the endpoint cursor. It is a
//! zero-allocation authority over compiled role-local rows; projection checks
//! compare this image against an independent oracle in tests.

use crate::eff::EffIndex;
use crate::global::{
    compiled::images::{CompiledProgramRef, ControlSemanticsTable, RoleDescriptorRef},
    const_dsl::ScopeId,
    role_program::{LaneSetView, LaneSteps, RoleImageRef},
    typestate::{
        FirstRecvDispatchSpec, LocalAction, LocalDependency, LocalNode, PackedEventConflict,
        ScopeRegion, StateIndex,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct LocalEventProgram {
    role_descriptor: RoleDescriptorRef,
    rows: RoleImageRef,
}

impl core::fmt::Debug for LocalEventProgram {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalEventProgram")
            .field("role_descriptor", &self.role_descriptor)
            .finish()
    }
}

impl PartialEq for LocalEventProgram {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.role_descriptor == other.role_descriptor
    }
}

impl Eq for LocalEventProgram {}

impl LocalEventProgram {
    #[inline(always)]
    pub(crate) const fn from_descriptor(role_descriptor: RoleDescriptorRef) -> Self {
        Self {
            rows: role_descriptor.local_event_rows(),
            role_descriptor,
        }
    }

    #[inline(always)]
    const fn rows(self) -> RoleImageRef {
        self.rows
    }
}

impl LocalEventProgram {
    #[inline(always)]
    const fn descriptor(self) -> RoleDescriptorRef {
        self.role_descriptor
    }

    #[inline(always)]
    pub(crate) fn role(self) -> u8 {
        self.descriptor().role()
    }

    #[inline(always)]
    pub(crate) fn program(self) -> CompiledProgramRef {
        self.descriptor().program()
    }

    #[inline(always)]
    pub(crate) fn control_semantics(self) -> &'static ControlSemanticsTable {
        self.program().control_semantics()
    }

    #[inline(always)]
    pub(crate) fn resident_row_min_start(self, idx: usize) -> Option<u16> {
        self.rows().resident_row_min_start(idx)
    }

    #[inline(always)]
    pub(crate) fn resident_row_lane_steps(self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        self.rows().resident_row_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    pub(crate) fn resident_row_lane_step_at(
        self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        self.rows()
            .resident_row_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    pub(crate) fn resident_row_lane_step_ordinal(
        self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        self.rows()
            .resident_row_lane_step_ordinal(idx, lane_idx, step_idx)
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(self) -> crate::endpoint::kernel::FrontierScratchLayout {
        self.descriptor().frontier_scratch_layout()
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(self) -> usize {
        self.descriptor().max_frontier_entries()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(self) -> usize {
        self.descriptor().logical_lane_count()
    }

    #[inline(always)]
    pub(crate) fn scope_region_by_id(self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.descriptor().scope_region_by_id(scope_id)
    }

    #[inline(always)]
    pub(crate) fn route_scope_for_selected_child_arm(
        self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<ScopeId> {
        self.descriptor()
            .route_scope_for_selected_child_arm(scope_id, arm)
    }

    #[inline(always)]
    pub(crate) fn parallel_root(self, scope_id: ScopeId) -> Option<ScopeId> {
        self.descriptor().parallel_root(scope_id)
    }

    #[inline(always)]
    pub(crate) fn enclosing_loop(self, scope_id: ScopeId) -> Option<ScopeId> {
        self.descriptor().enclosing_loop(scope_id)
    }

    #[inline(always)]
    pub(crate) fn route_scope_linger(self, scope_id: ScopeId) -> bool {
        self.descriptor().route_scope_linger(scope_id)
    }

    #[inline(always)]
    pub(crate) fn passive_arm_entry(self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.descriptor().passive_arm_entry(scope_id, arm)
    }

    #[inline(always)]
    pub(crate) fn route_recv_state(self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.descriptor().route_recv_state(scope_id, arm)
    }

    #[inline(always)]
    pub(crate) fn route_scope_offer_lane_set_by_slot(
        self,
        slot: usize,
    ) -> Option<LaneSetView<'static>> {
        self.rows().route_scope_offer_lane_set_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_scope_arm_lane_set_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.rows().route_scope_arm_lane_set_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) fn route_scope_offer_entry_by_slot(self, slot: usize) -> Option<StateIndex> {
        self.descriptor().route_scope_offer_entry_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_scope_dense_ordinal(self, scope_id: ScopeId) -> Option<usize> {
        self.descriptor().route_scope_dense_ordinal(scope_id)
    }

    #[inline(always)]
    pub(crate) fn first_recv_dispatch_target_for_lane_frame_label(
        self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.descriptor()
            .first_recv_dispatch_target_for_lane_frame_label(scope_id, lane, frame_label)
    }

    #[inline(always)]
    pub(crate) fn first_recv_dispatch_table(
        self,
        scope_id: ScopeId,
    ) -> Option<(
        [FirstRecvDispatchSpec; crate::global::typestate::MAX_FIRST_RECV_DISPATCH],
        u8,
    )> {
        self.descriptor().first_recv_dispatch_table(scope_id)
    }

    #[inline(always)]
    pub(crate) fn controller_arm_entry_for_label(
        self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        self.descriptor()
            .controller_arm_entry_for_label(scope_id, label)
    }

    #[inline(always)]
    pub(crate) fn controller_arm_entry_by_arm(
        self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.descriptor().controller_arm_entry_by_arm(scope_id, arm)
    }

    #[inline(always)]
    pub(crate) fn node_len(self) -> usize {
        self.role_descriptor.node_len()
    }

    #[inline(always)]
    pub(crate) fn local_len(self) -> usize {
        self.role_descriptor.local_len()
    }

    #[inline(always)]
    pub(crate) fn node(self, idx: usize) -> LocalNode {
        self.role_descriptor.node(idx)
    }

    #[inline(always)]
    pub(crate) fn checked_node(self, idx: usize) -> Option<LocalNode> {
        self.role_descriptor.checked_node(idx)
    }

    #[inline(always)]
    pub(crate) fn state_for_step_index(self, step_idx: usize) -> Option<StateIndex> {
        self.role_descriptor.state_for_step_index(step_idx)
    }

    #[inline(always)]
    pub(crate) fn local_step_lane(self, step_idx: usize) -> Option<u8> {
        if step_idx >= self.local_len() {
            None
        } else {
            self.rows().local_step_lane(step_idx)
        }
    }

    #[inline(always)]
    pub(crate) fn dependency_for_index(self, idx: usize) -> Option<LocalDependency> {
        self.event_row_at(idx).and_then(LocalEventRow::dependency)
    }

    #[inline(always)]
    pub(crate) fn event_conflict_for_index(self, idx: usize) -> PackedEventConflict {
        self.event_row_at(idx)
            .map(LocalEventRow::conflict)
            .unwrap_or_else(PackedEventConflict::none)
    }

    #[inline(always)]
    pub(crate) fn route_scope_conflict_by_slot(self, slot: usize) -> PackedEventConflict {
        self.rows().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn event_row_at(self, idx: usize) -> Option<LocalEventRow> {
        if idx >= self.local_len() {
            return None;
        }
        let node = self.checked_node(idx)?;
        let lane = match node.action() {
            LocalAction::Send { lane, .. }
            | LocalAction::Recv { lane, .. }
            | LocalAction::Local { lane, .. } => lane,
            LocalAction::Terminate => return None,
        };
        let dependency = self.rows().dependency_for_index(idx);
        let conflict = self.rows().event_conflict_for_index(idx);
        Some(LocalEventRow {
            node,
            lane,
            dependency,
            conflict,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalEventRow {
    node: LocalNode,
    lane: u8,
    dependency: Option<LocalDependency>,
    conflict: PackedEventConflict,
}

impl LocalEventRow {
    #[inline(always)]
    pub(crate) const fn dependency(self) -> Option<LocalDependency> {
        self.dependency
    }

    #[inline(always)]
    pub(crate) const fn conflict(self) -> PackedEventConflict {
        self.conflict
    }

    #[inline(always)]
    pub(crate) fn matches_commit(
        self,
        eff_index: EffIndex,
        label: u8,
        is_control: bool,
        scope: ScopeId,
        route_arm: Option<u8>,
        lane: u8,
    ) -> bool {
        if self.lane != lane || self.node.scope() != scope || self.node.route_arm() != route_arm {
            return false;
        }
        match self.node.action() {
            LocalAction::Send {
                eff_index: row_eff,
                label: row_label,
                is_control: row_control,
                ..
            }
            | LocalAction::Recv {
                eff_index: row_eff,
                label: row_label,
                is_control: row_control,
                ..
            }
            | LocalAction::Local {
                eff_index: row_eff,
                label: row_label,
                is_control: row_control,
                ..
            } => row_eff == eff_index && row_label == label && row_control == is_control,
            LocalAction::Terminate => false,
        }
    }
}
