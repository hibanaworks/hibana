//! Compiled-row local affine event program view.
//!
//! This is the production event image used by the endpoint cursor. It is a
//! zero-allocation authority over compiled role-local rows; projection checks
//! compare this image against an independent oracle in tests.

#[cfg(kani)]
mod kani;

#[cfg(hibana_profile)]
pub(crate) mod work;

#[cfg(all(test, hibana_repo_tests))]
use crate::global::typestate::LocalNode;
use crate::global::{
    compiled::images::CompiledProgramRef,
    const_dsl::{ScopeId, ScopeKind},
    role_program::{LaneSetView, PackedLaneRange, RoleImageRef, RoleLocalDirection},
    typestate::{
        EventCommitMeta, LocalDependency, PackedEventConflict, PassiveArmChildFact, RouteScopeRows,
        StateIndex,
    },
};

pub(crate) struct LocalEventProgram {
    rows: &'static RoleImageRef,
    #[cfg(hibana_profile)]
    pub(crate) work: work::Counters,
}

impl core::fmt::Debug for LocalEventProgram {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalEventProgram").finish_non_exhaustive()
    }
}

impl LocalEventProgram {
    #[inline(always)]
    pub(crate) const fn from_rows(rows: &'static RoleImageRef) -> Self {
        Self {
            rows,
            #[cfg(hibana_profile)]
            work: work::Counters::new(),
        }
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
    /// Rows are constructor-checked in source order. A framed message names
    /// its source effect directly; only its co-resident facets can match.
    pub(crate) fn source_effect_candidates(&self, effect: usize) -> core::ops::Range<usize> {
        let mut low = 0;
        let mut high = self.local_len();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.rows().local_step_effect(middle).dense_ordinal() < effect {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let start = low;
        while low < self.local_len() && self.rows().local_step_effect(low).dense_ordinal() == effect
        {
            low += 1;
        }
        start..low
    }

    /// Read operation identity without decoding its dependency, conflict, or
    /// message metadata. The image constructor validates their consistency.
    #[inline(always)]
    pub(crate) fn operation_at(
        &self,
        idx: usize,
    ) -> Option<(crate::eff::EffIndex, RoleLocalDirection)> {
        #[cfg(hibana_profile)]
        self.work.hit(work::Metric::OperationIdentity);
        let count = self.local_len();
        if idx < count {
            Some((
                self.rows().local_step_effect(idx),
                self.rows().local_step_direction(idx),
            ))
        } else if idx == count {
            None
        } else {
            crate::invariant()
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

    pub(crate) fn offer_candidate_lanes(&self, borrow_mask: Option<u16>) -> LaneSetView<'static> {
        self.rows().offer_candidate_lanes(borrow_mask)
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
            if self.rows().route_arm_first_operation(slot, arm, false) == Some(row.start()) {
                let eff = self.rows().local_step_effect(row.start());
                if self.program_ref().event_atom_at(eff.dense_ordinal()).from == controller {
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

    #[inline(always)]
    pub(crate) fn has_reentry_scopes(&self) -> bool {
        self.rows().has_reentry_scopes()
    }

    pub(crate) fn route_arm_first_operation(
        &self,
        slot: usize,
        arm: u8,
        receive: bool,
    ) -> Option<usize> {
        self.rows().route_arm_first_operation(slot, arm, receive)
    }

    #[inline(always)]
    pub(crate) fn next_lane_step(&self, step: usize) -> usize {
        self.rows().next_lane_step(step)
    }

    pub(crate) fn next_offer_lane_alternative(
        &self,
        step: usize,
        preview: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        self.rows().next_offer_lane_alternative(step, preview)
    }

    pub(crate) fn route_arm_view_lane_entry(
        &self,
        slot: usize,
        arm: u8,
        view: u8,
        lane: u8,
    ) -> Option<usize> {
        self.rows().route_arm_view_lane_entry(slot, arm, view, lane)
    }

    pub(crate) fn view_lane_step_at_or_after(
        &self,
        view: u8,
        lane: u8,
        start: usize,
    ) -> Option<usize> {
        self.rows().view_lane_step_at_or_after(view, lane, start)
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
        #[cfg(hibana_profile)]
        self.work.hit(work::Metric::RouteRows);
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
        RouteScopeRows::new(scope_id, start, end)
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
        #[cfg(hibana_profile)]
        self.work.hit(work::Metric::Conflict);
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

    pub(crate) fn route_commit_bytes(&self, range: PackedLaneRange) -> &'static [u8] {
        self.rows().route_commit_bytes(range)
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
        Some(LocalEventRow {
            image: self.rows(),
            index: idx as u16,
        })
    }

    #[inline]
    pub(crate) fn contains_row(&self, row: LocalEventRow) -> bool {
        core::ptr::eq(self.rows(), row.image)
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

/// A canonical row in a sealed image, with no copied descriptor or progress.
/// Only `LocalEventProgram::event_row_at` constructs a row after checking its
/// ordinal. Each query reads the immutable fields it actually needs.
#[derive(Clone, Copy)]
pub(crate) struct LocalEventRow {
    image: &'static RoleImageRef,
    index: u16,
}

impl LocalEventRow {
    #[inline]
    pub(crate) const fn index(self) -> usize {
        self.index as usize
    }

    #[inline(always)]
    pub(crate) fn logical_role(self) -> u8 {
        self.image.local_step_logical_role(self.index as usize)
    }

    #[inline(always)]
    pub(crate) fn commit_meta(self) -> EventCommitMeta {
        let atom = self.atom();
        EventCommitMeta::new(
            self.effect(),
            atom.label,
            atom.origin,
            self.scope(),
            self.route_arm(),
            atom.lane,
        )
    }

    #[inline(always)]
    pub(crate) const fn next(self) -> StateIndex {
        StateIndex::from_usize(self.index as usize + 1)
    }

    #[inline(always)]
    pub(crate) const fn direction(self) -> RoleLocalDirection {
        self.image.local_step_direction(self.index as usize)
    }

    #[inline(always)]
    pub(crate) const fn effect(self) -> crate::eff::EffIndex {
        self.image.local_step_effect(self.index as usize)
    }

    #[inline(always)]
    pub(crate) const fn atom(self) -> crate::eff::EffAtom {
        self.image
            .program()
            .event_atom_at(self.effect().dense_ordinal())
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.image.local_step_scope(self.index as usize)
    }

    #[inline(always)]
    pub(crate) const fn route_arm(self) -> Option<u8> {
        match self.conflict().to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { arm, .. }) => Some(arm),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope(self) -> ScopeId {
        match self.conflict().to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { scope, .. }) => scope,
            _ => ScopeId::none(),
        }
    }

    #[inline(always)]
    pub(crate) const fn choice(self) -> crate::global::typestate::RouteChoiceMark {
        self.image.local_step_choice(self.index as usize)
    }

    #[inline(always)]
    pub(crate) const fn dependency(self) -> Option<LocalDependency> {
        self.image.dependency_for_index(self.index as usize)
    }

    #[inline(always)]
    pub(crate) const fn conflict(self) -> PackedEventConflict {
        self.image.event_conflict_for_index(self.index as usize)
    }

    pub(crate) fn matches_commit(self, event: EventCommitMeta) -> bool {
        let atom = self.atom();
        self.scope() == event.scope
            && self.route_arm() == event.route_arm
            && self.effect() == event.eff_index
            && atom.label == event.label
            && atom.origin == event.origin
            && atom.lane == event.lane
    }
}
