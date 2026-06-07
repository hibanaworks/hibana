//! Compiled-row local affine event program view.
//!
//! This is the production event image used by the endpoint cursor. It is a
//! zero-allocation authority over compiled role-local rows; projection checks
//! compare this image against an independent oracle in tests.

use crate::eff::EffIndex;
use crate::global::{
    compiled::images::RoleDescriptorRef,
    const_dsl::ScopeId,
    role_program::{LaneSetView, LaneSteps, PackedLaneRange, RoleImageRef},
    typestate::{LocalAction, LocalDependency, LocalNode, PackedEventConflict},
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

impl PartialEq for LocalEventProgram {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.rows.image, other.rows.image)
    }
}

impl Eq for LocalEventProgram {}

impl LocalEventProgram {
    #[inline(always)]
    pub(crate) const fn from_descriptor(role_descriptor: RoleDescriptorRef) -> Self {
        Self {
            rows: role_descriptor.local_event_rows(),
        }
    }

    #[inline(always)]
    const fn rows(self) -> RoleImageRef {
        self.rows
    }
}

impl LocalEventProgram {
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
    fn local_len(self) -> usize {
        self.rows().local_step_count()
    }

    #[inline(always)]
    fn checked_node(self, idx: usize) -> Option<LocalNode> {
        self.rows().local_step_node(idx)
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
