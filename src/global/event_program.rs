//! Compiled-row local affine event program view.
//!
//! This is the production event image used by the endpoint cursor. It is a
//! zero-allocation authority over compiled role-local rows; projection checks
//! compare this image against an independent oracle in tests.

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
use crate::global::typestate::LocalNode;
use crate::global::{
    compiled::images::CompiledProgramRef,
    const_dsl::{ReentryMark, ScopeId, ScopeKind},
    role_program::{
        LaneSetView, PackedLaneRange, PackedLocalEventRow, RoleImageRef, RoleLocalDirection,
    },
    typestate::{
        EventCommitMeta, LocalDependency, PackedEventConflict, PassiveArmChildFact, RouteScopeRows,
        StateIndex,
    },
};

pub(crate) struct LocalEventProgram {
    rows: &'static RoleImageRef,
}

impl core::fmt::Debug for LocalEventProgram {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalEventProgram").finish_non_exhaustive()
    }
}

impl LocalEventProgram {
    #[inline(always)]
    pub(crate) const fn from_rows(rows: &'static RoleImageRef) -> Self {
        Self { rows }
    }

    #[inline(always)]
    const fn rows(&self) -> &'static RoleImageRef {
        self.rows
    }

    #[inline(always)]
    pub(crate) const fn program_ref(&self) -> &'static CompiledProgramRef {
        self.rows().program()
    }
}

impl LocalEventProgram {
    /// Read operation identity without decoding its dependency, conflict, or
    /// message metadata. The image constructor validates their consistency.
    #[inline(always)]
    pub(crate) fn operation_at(
        &self,
        idx: usize,
    ) -> Option<(crate::eff::EffIndex, RoleLocalDirection)> {
        let count = self.local_len();
        let row = if idx < count {
            self.rows().lanes().local_step_event(idx)
        } else {
            None
        };
        match row {
            Some(row) => Some((
                crate::eff::EffIndex::from_dense_ordinal(row.eff_index as usize),
                row.direction(),
            )),
            None if idx == count => None,
            None => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) fn receive_candidates(
        &self,
        role: u8,
        label: u8,
    ) -> impl Iterator<Item = usize> + '_ {
        self.rows().receive_candidates(role, label)
    }

    #[inline]
    pub(crate) fn outbound_candidates(
        &self,
        role: u8,
        label: u8,
    ) -> impl Iterator<Item = usize> + Clone + '_ {
        self.rows().outbound_candidates(role, label)
    }

    pub(crate) fn parallel_borrow_mask(&self, view: u8, start: usize, stop: usize) -> Option<u16> {
        self.rows().parallel_borrow_mask(view, start, stop)
    }

    #[inline]
    pub(crate) fn parallel_borrow_owns_lane(&self, mask: u16, lane: usize) -> bool {
        self.rows().parallel_borrow_owns_lane(mask, lane)
    }

    #[inline(always)]
    pub(crate) const fn footprint(&self) -> crate::global::role_program::RuntimeRoleFootprint {
        self.rows().footprint()
    }

    #[inline(always)]
    pub(crate) fn resident_row_min_start(&self, idx: usize) -> Option<u16> {
        self.rows().resident_row_min_start(idx)
    }

    #[inline(always)]
    pub(crate) fn route_scope_offer_lane_set_by_slot(&self, slot: usize) -> LaneSetView<'static> {
        self.rows().route_scope_offer_lane_set_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> LaneSetView<'static> {
        self.rows().route_scope_arm_lane_set_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) fn route_arm_lane_first_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.rows()
            .route_arm_lane_first_step_by_slot(slot, arm, lane)
    }

    #[inline(always)]
    pub(crate) fn route_arm_lane_last_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.rows()
            .route_arm_lane_last_step_by_slot(slot, arm, lane)
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.rows().local_step_count()
    }

    #[inline(always)]
    pub(crate) fn node_len(&self) -> usize {
        self.local_len() + 1
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn node(&self, idx: usize) -> LocalNode {
        match self.rows().local_step_node(idx) {
            Some(node) => node,
            None if idx == self.local_len() => {
                LocalNode::terminal(StateIndex::from_usize(self.local_len()))
            }
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) fn state_for_step_index(&self, step_idx: usize) -> Option<StateIndex> {
        (step_idx < self.local_len()).then(|| StateIndex::from_usize(step_idx))
    }

    #[inline(always)]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() || !matches!(scope_id.kind(), Some(ScopeKind::Route)) {
            return None;
        }
        self.rows().route_scope_slot(scope_id)
    }

    /// Immutable controller membership. Live issuance and the caller's active
    /// logical view remain separate kernel checks.
    pub(crate) fn contains_route_controller(&self, scope: ScopeId) -> bool {
        let Some(slot) = self.route_scope_slot(scope) else {
            return false;
        };
        let controller = self.program_ref().route_controller_role(scope);
        for arm in 0..=1 {
            let Some(row) = self.route_arm_event_row_by_slot(slot, arm) else {
                continue;
            };
            if row.start() < row.end() {
                let Some((eff, direction)) = self.operation_at(row.start()) else {
                    crate::invariant();
                };
                if matches!(
                    direction,
                    RoleLocalDirection::Send | RoleLocalDirection::Local
                ) && self.program_ref().event_atom_at(eff.dense_ordinal()).from == controller
                {
                    return true;
                }
            }
        }
        false
    }

    #[inline(always)]
    pub(crate) fn route_scope_reentry(&self, scope_id: ScopeId) -> bool {
        self.route_scope_slot(scope_id)
            .is_some_and(|slot| self.rows().route_scope_reentry_by_slot(slot))
    }

    #[inline(never)]
    pub(crate) fn has_reentry_scopes(&self) -> bool {
        if self.rows().roll_scope_row(0).is_some() {
            return true;
        }
        let mut slot = 0usize;
        let limit = self.footprint().route_scope_count;
        while slot < limit {
            if self.rows().route_scope_reentry_by_slot(slot) {
                return true;
            }
            slot += 1;
        }
        false
    }

    #[inline(always)]
    pub(crate) fn roll_scope_row_by_slot(
        &self,
        slot: usize,
    ) -> Option<(ScopeId, LocalEventRowSet)> {
        let row = self.rows().roll_scope_row(slot)?;
        let scope = row.scope();
        let events = LocalEventRowSet::from_packed(row.event_row())?;
        Some((scope, events))
    }

    #[inline(always)]
    pub(crate) fn roll_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() || !matches!(scope_id.kind(), Some(ScopeKind::Roll)) {
            return None;
        }
        let target = scope_id.local_ordinal();
        let mut low = 0;
        let mut high = self.footprint().roll_scope_count;
        while low < high {
            let middle = low + (high - low) / 2;
            let row = crate::invariant_some(self.rows().roll_scope_row(middle));
            match row.scope().local_ordinal().cmp(&target) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => return Some(middle),
            }
        }
        None
    }

    #[inline(always)]
    pub(crate) fn roll_scope_row(&self, scope_id: ScopeId) -> Option<LocalEventRowSet> {
        self.roll_scope_row_by_slot(self.roll_scope_slot(scope_id)?)
            .map(|(_, row)| row)
    }

    pub(crate) fn roll_ancestors(&self, index: usize) -> impl Iterator<Item = ScopeId> + '_ {
        let source = match self.operation_at(index) {
            Some((event, _)) => event.dense_ordinal(),
            None => self.program_ref().columns.atom_count(),
        };
        self.program_ref().source_roll_ancestors(source)
    }

    pub(crate) fn roll_entry_range(&self, scope: ScopeId) -> Option<PackedLaneRange> {
        Some(self.rows().roll_entry_range(self.roll_scope_slot(scope)?))
    }

    pub(crate) fn roll_entry_view(&self, index: usize) -> (u8, u16) {
        self.rows().roll_entry_view(index)
    }

    pub(crate) fn roll_entry_receipts(
        &self,
        index: usize,
    ) -> crate::global::role_program::PackedLaneRange {
        self.rows().roll_entry_receipts(index)
    }

    pub(crate) fn owned_receipt(&self, index: usize) -> u16 {
        self.rows().owned_receipt(index)
    }

    #[inline(always)]
    pub(crate) fn route_scope_rows(&self, scope_id: ScopeId) -> Option<RouteScopeRows> {
        let slot = self.route_scope_slot(scope_id)?;
        self.route_scope_rows_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_scope_slot_count(&self) -> usize {
        self.footprint().route_scope_count
    }

    pub(crate) fn route_scope_rows_by_slot(&self, slot: usize) -> Option<RouteScopeRows> {
        let scope_id = self.rows().route_scope_by_slot(slot)?;
        let mut bounds: Option<(usize, usize)> = None;
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self.route_arm_event_row_by_slot(slot, arm)
                && row.start() < row.end()
            {
                bounds = Some(match bounds {
                    Some((start, end)) => (start.min(row.start()), end.max(row.end())),
                    None => (row.start(), row.end()),
                });
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        let (start, end) = bounds?;
        let reentry = if self.rows().route_scope_reentry_by_slot(slot) {
            ReentryMark::Reentrant
        } else {
            ReentryMark::SinglePass
        };
        RouteScopeRows::new(scope_id, start, end, reentry)
    }

    #[inline(always)]
    pub(crate) fn local_step_lane(&self, step_idx: usize) -> Option<u8> {
        if step_idx >= self.local_len() {
            None
        } else {
            self.rows().local_step_lane(step_idx)
        }
    }

    #[inline(always)]
    pub(crate) fn event_conflict_for_index(&self, idx: usize) -> PackedEventConflict {
        if idx == self.local_len() {
            return PackedEventConflict::none();
        }
        if idx > self.local_len() {
            crate::invariant();
        }
        // The opaque image proves every resident node and referenced dependency
        // before publication. This query needs only the immutable conflict.
        self.rows().event_conflict_for_index(idx)
    }

    #[inline(always)]
    pub(crate) fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        self.rows().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) fn route_commit_range_by_slot(&self, slot: usize, arm: u8) -> PackedLaneRange {
        self.rows().route_commit_range_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) fn route_commit_row_at(&self, idx: usize) -> PackedEventConflict {
        self.rows().route_commit_row_at(idx)
    }

    #[inline(always)]
    pub(crate) fn has_passive_parent_index(&self) -> bool {
        self.rows().has_passive_parent_index()
    }

    #[inline(always)]
    pub(crate) fn passive_arm_child_fact_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<PassiveArmChildFact> {
        let route_scope = self.rows().route_scope_by_slot(slot)?;
        let child_route_scope = self
            .rows()
            .passive_arm_child_ordinal_by_slot(slot, arm)
            .map(ScopeId::route);
        match PassiveArmChildFact::new(route_scope, arm, child_route_scope) {
            Some(fact) => Some(fact),
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) fn dependency_for_index(&self, idx: usize) -> Option<LocalDependency> {
        self.rows().dependency_for_index(idx)
    }

    #[inline(always)]
    pub(crate) fn dependency_row_set(&self, dependency: LocalDependency) -> LocalEventRowSet {
        LocalEventRowSet::new(dependency.start(), dependency.end())
    }

    #[inline(always)]
    pub(crate) fn route_arm_event_row_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<LocalEventRowSet> {
        LocalEventRowSet::from_packed(self.rows().route_arm_event_row_by_slot(slot, arm))
    }

    #[inline(always)]
    pub(crate) fn event_row_at(&self, idx: usize) -> Option<LocalEventRow> {
        if idx >= self.local_len() {
            return None;
        }
        let event = crate::invariant_some(self.rows().lanes().local_step_event(idx));
        let atom = self.program_ref().event_atom_at(event.eff_index as usize);
        let conflict = self.rows().event_conflict_for_index(idx);
        Some(LocalEventRow {
            image: self.rows(),
            event,
            atom,
            next: StateIndex::from_usize(idx + 1),
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

#[derive(Clone, Copy)]
pub(crate) struct LocalEventRow {
    image: &'static RoleImageRef,
    event: PackedLocalEventRow,
    atom: crate::eff::EffAtom,
    next: StateIndex,
    conflict: PackedEventConflict,
}

impl LocalEventRow {
    #[inline(always)]
    pub(crate) fn logical_role(self) -> u8 {
        if self.direction() == RoleLocalDirection::Recv {
            self.atom.to
        } else {
            self.atom.from
        }
    }

    #[inline(always)]
    pub(crate) fn commit_meta(self) -> EventCommitMeta {
        EventCommitMeta::new(
            self.effect(),
            self.atom.label,
            self.atom.origin,
            self.scope(),
            self.route_arm(),
            self.atom.lane,
        )
    }

    #[inline(always)]
    pub(crate) const fn next(self) -> StateIndex {
        self.next
    }

    #[inline(always)]
    pub(crate) const fn direction(self) -> RoleLocalDirection {
        self.event.direction()
    }

    #[inline(always)]
    pub(crate) const fn effect(self) -> crate::eff::EffIndex {
        crate::eff::EffIndex::from_dense_ordinal(self.event.eff_index as usize)
    }

    #[inline(always)]
    pub(crate) const fn atom(self) -> crate::eff::EffAtom {
        self.atom
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.event.scope()
    }

    #[inline(always)]
    pub(crate) const fn route_arm(self) -> Option<u8> {
        match self.conflict.to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { arm, .. }) => Some(arm),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope(self) -> ScopeId {
        match self.conflict.to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { scope, .. }) => scope,
            _ => ScopeId::none(),
        }
    }

    #[inline(always)]
    pub(crate) const fn choice(self) -> crate::global::typestate::RouteChoiceMark {
        self.event.choice_mark()
    }

    #[inline(always)]
    pub(crate) const fn dependency(self) -> Option<LocalDependency> {
        self.image.lanes().dependency_row(self.event.dependency_row)
    }

    #[inline(always)]
    pub(crate) const fn conflict(self) -> PackedEventConflict {
        self.conflict
    }

    pub(crate) fn matches_commit(self, event: EventCommitMeta) -> bool {
        self.scope() == event.scope
            && self.route_arm() == event.route_arm
            && self.effect() == event.eff_index
            && self.atom.label == event.label
            && self.atom.origin == event.origin
            && self.atom.lane == event.lane
    }
}
