//! Compiled-row local affine event program view.
//!
//! This is the production event image used by the endpoint cursor. It is a
//! zero-allocation authority over compiled role-local rows; projection checks
//! compare this image against an independent oracle in tests.

use crate::eff::EffIndex;
use crate::global::{
    compiled::images::CompiledProgramRef,
    const_dsl::{ScopeId, ScopeKind},
    role_program::{LaneSetView, LaneSteps, PackedLaneRange, RoleImageRef},
    typestate::{
        LocalAction, LocalDependency, LocalNode, PackedEventConflict, PassiveArmChildFact,
        RouteScopeRows, StateIndex,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct LocalEventProgram {
    rows: RoleImageRef,
}

impl core::fmt::Debug for LocalEventProgram {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalEventProgram").finish_non_exhaustive()
    }
}

impl LocalEventProgram {
    #[inline(always)]
    pub(crate) const fn from_rows(rows: RoleImageRef) -> Self {
        Self { rows }
    }

    #[inline(always)]
    const fn rows(self) -> RoleImageRef {
        self.rows
    }

    #[inline(always)]
    pub(crate) const fn program_ref(self) -> &'static CompiledProgramRef {
        self.rows().program
    }
}

impl LocalEventProgram {
    #[inline(always)]
    pub(crate) const fn footprint(self) -> crate::global::role_program::RuntimeRoleFootprint {
        self.rows().footprint()
    }

    #[inline(always)]
    fn logical_lane_count(self) -> usize {
        self.rows().footprint().logical_lane_count
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
    pub(crate) fn route_arm_lane_first_step_by_slot(
        self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.rows()
            .route_arm_lane_first_step_by_slot(slot, arm, lane)
    }

    #[inline(always)]
    pub(crate) fn route_arm_lane_last_step_by_slot(
        self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.rows()
            .route_arm_lane_last_step_by_slot(slot, arm, lane)
    }

    #[inline(always)]
    pub(crate) fn local_len(self) -> usize {
        self.rows().local_step_count()
    }

    #[inline(always)]
    pub(crate) fn node_len(self) -> usize {
        self.local_len() + 1
    }

    #[inline(always)]
    pub(crate) fn checked_node(self, idx: usize) -> Option<LocalNode> {
        if idx >= self.node_len() {
            None
        } else {
            Some(self.node(idx))
        }
    }

    #[inline(always)]
    pub(crate) fn node(self, idx: usize) -> LocalNode {
        match self.rows().local_step_node(idx) {
            Some(node) => node,
            None if idx == self.local_len() => {
                LocalNode::terminal(StateIndex::from_usize(self.local_len()))
            }
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) fn state_for_step_index(self, step_idx: usize) -> Option<StateIndex> {
        (step_idx < self.local_len()).then(|| StateIndex::from_usize(step_idx))
    }

    #[inline(always)]
    pub(crate) fn route_scope_slot(self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() || !matches!(scope_id.kind(), ScopeKind::Route) {
            return None;
        }
        let target = scope_id.local_ordinal();
        let mut slot = 0usize;
        let limit = self.footprint().route_scope_count;
        while slot < limit {
            if self.rows().route_scope_ordinal_by_slot(slot) == Some(target) {
                return Some(slot);
            }
            slot += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn route_scope_linger(self, scope_id: ScopeId) -> bool {
        self.route_scope_slot(scope_id)
            .map(|slot| self.rows().route_scope_linger_by_slot(slot))
            .unwrap_or(false)
    }

    #[inline(always)]
    pub(crate) fn route_scope_rows(self, scope_id: ScopeId) -> Option<RouteScopeRows> {
        let slot = self.route_scope_slot(scope_id)?;
        self.route_scope_rows_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_scope_rows_by_slot(self, slot: usize) -> Option<RouteScopeRows> {
        let ordinal = self.rows().route_scope_ordinal_by_slot(slot)?;
        let scope_id = ScopeId::route(ordinal);
        let mut start = usize::MAX;
        let mut end = 0usize;
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self.route_arm_event_row_by_slot(slot, arm) {
                if row.start() < start {
                    start = row.start();
                }
                if row.end() > end {
                    end = row.end();
                }
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        RouteScopeRows::new(
            scope_id,
            start,
            end,
            self.rows().route_scope_linger_by_slot(slot),
        )
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
        if idx == self.local_len() {
            return PackedEventConflict::none();
        }
        match self.event_row_at(idx) {
            Some(row) => row.conflict,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) fn route_scope_conflict_by_slot(self, slot: usize) -> PackedEventConflict {
        self.rows().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_commit_range_by_slot(self, slot: usize, arm: u8) -> PackedLaneRange {
        self.rows().route_commit_range_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) fn route_commit_row_at(self, idx: usize) -> PackedEventConflict {
        self.rows().route_commit_row_at(idx)
    }

    #[inline(always)]
    pub(crate) fn passive_arm_child_fact_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<PassiveArmChildFact> {
        let route_scope = ScopeId::route(self.rows().route_scope_ordinal_by_slot(slot)?);
        let child_route_scope = self
            .rows()
            .passive_arm_child_ordinal_by_slot(slot, arm)
            .map(ScopeId::route);
        PassiveArmChildFact::new(route_scope, arm, child_route_scope)
    }

    #[inline(always)]
    pub(crate) fn dependency_row_set(self, dependency: LocalDependency) -> LocalEventRowSet {
        LocalEventRowSet::new(dependency.start(), dependency.end())
    }

    #[inline(always)]
    pub(crate) fn route_arm_event_row_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<LocalEventRowSet> {
        LocalEventRowSet::from_packed(self.rows().route_arm_event_row_by_slot(slot, arm))
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
pub(crate) struct LocalEventRowSet {
    start: u16,
    end: u16,
}

impl LocalEventRowSet {
    #[inline(always)]
    pub(crate) const fn new(start: usize, end: usize) -> Self {
        if start > u16::MAX as usize || end > u16::MAX as usize || start > end {
            panic!("event row set range overflow");
        }
        Self {
            start: start as u16,
            end: end as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed(row: PackedLaneRange) -> Option<Self> {
        if row.is_empty() {
            None
        } else {
            Some(Self::new(row.start(), row.end()))
        }
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        self.start as usize
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> usize {
        self.end as usize
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
