//! # Unsafe Owner Contract
//! Route-state owner: unsafe blocks may index raw table storage only after
//! route scope and dense-lane metadata prove the slot belongs to this endpoint
//! generation.

use super::core::CommitDeltaApplyPermit;
use super::evidence::RouteArmState;
use super::evidence_store::{ScopeEvidenceSlot, ScopeEvidenceTable};
use super::frontier::LaneOfferState;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::{ReentryMark, ScopeId};
use crate::global::role_program::{
    DENSE_LANE_ABSENT, DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord, PackedLaneRange,
};
use crate::global::typestate::{EventCursor, LocalConflict, PackedEventConflict};

mod reentry_clear;
pub(super) use reentry_clear::ReentryScopeLiveness;
mod route_arm_history;
use route_arm_history::RouteArmHistoryView;
#[cfg(kani)]
mod kani;

const SELECTED_ARM_NONE: u8 = u8::MAX;

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitRow {
    conflict: PackedEventConflict,
}

impl SelectedRouteCommitRow {
    const fn new(scope: ScopeId, selected_arm: u8) -> Self {
        Self {
            conflict: PackedEventConflict::route_arm(scope, selected_arm),
        }
    }

    pub(in crate::endpoint::kernel) const fn scope(self) -> ScopeId {
        match self.conflict.to_conflict() {
            Some(LocalConflict::RouteArm { scope, .. }) => scope,
            Some(LocalConflict::Unconditional | LocalConflict::SharedRoute) | None => {
                crate::invariant()
            }
        }
    }

    pub(in crate::endpoint::kernel) const fn selected_arm(self) -> u8 {
        match self.conflict.to_conflict() {
            Some(LocalConflict::RouteArm { arm, .. }) => arm,
            Some(LocalConflict::Unconditional | LocalConflict::SharedRoute) | None => {
                crate::invariant()
            }
        }
    }

    pub(in crate::endpoint::kernel) const fn is_empty(self) -> bool {
        self.conflict.to_conflict().is_none()
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn from_resident_conflict(
        conflict: PackedEventConflict,
    ) -> Option<Self> {
        match conflict.to_conflict() {
            Some(LocalConflict::RouteArm { .. }) => Some(Self { conflict }),
            Some(LocalConflict::Unconditional | LocalConflict::SharedRoute) | None => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitRowsRef {
    range: PackedLaneRange,
    lane: u8,
}

impl SelectedRouteCommitRowsRef {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        range: PackedLaneRange::EMPTY,
        lane: 0,
    };

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn from_resident_range_for_lane(
        range: PackedLaneRange,
        route_lane: u8,
    ) -> Self {
        if range.is_absent_or_zero_len() {
            Self::EMPTY
        } else {
            Self {
                range,
                lane: route_lane,
            }
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn is_empty(self) -> bool {
        self.range.is_absent_or_zero_len()
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn len(self) -> usize {
        if self.is_empty() { 0 } else { self.range.len() }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn selected_lane(self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            Some(self.lane)
        }
    }

    #[inline(always)]
    const fn range(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else {
            self.range
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn get(
        self,
        cursor: &EventCursor,
        idx: usize,
    ) -> Option<SelectedRouteCommitRow> {
        let len = self.len();
        if idx >= len {
            return None;
        }
        SelectedRouteCommitRow::from_resident_conflict(
            cursor.route_commit_row_at(self.range(), idx)?,
        )
    }

    #[inline(always)]
    fn contains(self, cursor: &EventCursor, row: SelectedRouteCommitRow) -> bool {
        let mut idx = 0usize;
        while idx < self.len() {
            if self.get(cursor, idx).is_some_and(|candidate| {
                candidate.scope() == row.scope() && candidate.selected_arm() == row.selected_arm()
            }) {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline(always)]
    fn contains_all(self, cursor: &EventCursor, other: SelectedRouteCommitRowsRef) -> bool {
        let mut idx = 0usize;
        while idx < other.len() {
            let Some(row) = other.get(cursor, idx) else {
                return false;
            };
            if !self.contains(cursor, row) {
                return false;
            }
            idx += 1;
        }
        true
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RouteOnlyCommitRowsRef {
    selected_routes: SelectedRouteCommitRowsRef,
}

impl RouteOnlyCommitRowsRef {
    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn selected_routes(self) -> SelectedRouteCommitRowsRef {
        self.selected_routes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RouteCommitRowSetBuilder {
    max_len: u16,
}

pub(crate) struct PreparedRouteCommitRows {
    selected_routes: SelectedRouteCommitRowsRef,
}

impl PreparedRouteCommitRows {
    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn empty() -> Self {
        Self {
            selected_routes: SelectedRouteCommitRowsRef::EMPTY,
        }
    }

    #[inline(always)]
    const fn from_routes(selected_routes: SelectedRouteCommitRowsRef) -> Self {
        Self { selected_routes }
    }

    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        self.selected_routes.len()
    }

    #[inline(always)]
    pub(crate) const fn selected_lane(&self) -> Option<u8> {
        if self.len() == 0 {
            None
        } else {
            self.selected_routes.selected_lane()
        }
    }

    #[inline(always)]
    pub(crate) fn get(&self, cursor: &EventCursor, idx: usize) -> Option<SelectedRouteCommitRow> {
        self.selected_routes.get(cursor, idx)
    }

    #[inline(always)]
    pub(crate) fn take(&mut self) -> Self {
        core::mem::replace(self, Self::empty())
    }
}

impl RouteCommitRowSetBuilder {
    pub(super) unsafe fn init(dst: *mut Self, cap: usize) {
        if cap > u16::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: endpoint route initialization passes an unpublished `RouteCommitRowSetBuilder`; checked capacity is written before any builder borrow. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).max_len).write(cap as u16);
        }
    }

    #[inline]
    pub(super) fn begin(&mut self) -> SelectedRouteCommitRows {
        SelectedRouteCommitRows {
            routes: SelectedRouteCommitRowsRef::EMPTY,
            max_len: self.max_len,
        }
    }

    pub(in crate::endpoint::kernel) fn seal(
        &mut self,
        routes: SelectedRouteCommitRowsRef,
    ) -> RecvResult<PreparedRouteCommitRows> {
        let len = routes.len();
        if len == 0 {
            return Ok(PreparedRouteCommitRows::empty());
        }
        if len > self.max_len as usize {
            return Err(RecvError::PhaseInvariant);
        }
        routes.selected_lane().ok_or(RecvError::PhaseInvariant)?;
        Ok(PreparedRouteCommitRows::from_routes(routes))
    }
}

pub(super) struct SelectedRouteCommitRows {
    routes: SelectedRouteCommitRowsRef,
    max_len: u16,
}

impl SelectedRouteCommitRows {
    #[inline]
    pub(in crate::endpoint::kernel) fn from_seed(
        routes: SelectedRouteCommitRowsRef,
    ) -> RecvResult<Self> {
        if routes.is_empty() {
            return Err(RecvError::PhaseInvariant);
        }
        Ok(Self {
            routes,
            max_len: u16::MAX,
        })
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.routes.len()
    }

    pub(super) fn arm_for_scope(&self, cursor: &EventCursor, scope: ScopeId) -> Option<u8> {
        let mut idx = 0usize;
        while idx < self.len() {
            let row = self.routes.get(cursor, idx)?;
            if row.scope() == scope {
                return Some(row.selected_arm());
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn finish_for_lane(self, route_lane: u8) -> RecvResult<SelectedRouteCommitRowsRef> {
        if !self.routes.is_empty() && self.routes.selected_lane() != Some(route_lane) {
            return Err(RecvError::PhaseInvariant);
        }
        Ok(self.routes)
    }

    pub(super) fn finish_route_only_for_lane(
        self,
        route_lane: u8,
    ) -> RecvResult<RouteOnlyCommitRowsRef> {
        Ok(RouteOnlyCommitRowsRef {
            selected_routes: self.finish_for_lane(route_lane)?,
        })
    }

    pub(super) fn merge_chain(
        &mut self,
        cursor: &EventCursor,
        lane: u8,
        conflict: PackedEventConflict,
    ) -> RecvResult<()> {
        let range = cursor
            .route_commit_range_for_conflict(conflict)
            .ok_or(RecvError::PhaseInvariant)?;
        if range.len() > self.max_len as usize {
            return Err(RecvError::PhaseInvariant);
        }
        let incoming = SelectedRouteCommitRowsRef::from_resident_range_for_lane(range, lane);
        if self.routes.is_empty() {
            self.routes = incoming;
            return Ok(());
        }
        if self.routes.selected_lane() != Some(lane) {
            return Err(RecvError::PhaseInvariant);
        }
        if self.routes.contains_all(cursor, incoming) {
            return Ok(());
        }
        if incoming.contains_all(cursor, self.routes) {
            self.routes = incoming;
            return Ok(());
        }
        let mut idx = 0usize;
        while idx < incoming.len() {
            let Some(row) = incoming.get(cursor, idx) else {
                return Err(RecvError::PhaseInvariant);
            };
            if let Some(existing) = self.arm_for_scope(cursor, row.scope())
                && existing != row.selected_arm()
            {
                return Err(RecvError::PhaseInvariant);
            }
            idx += 1;
        }
        Err(RecvError::PhaseInvariant)
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct RouteScopeSelectedArmSlot {
    arm: u8,
    refs: u16,
}

impl RouteScopeSelectedArmSlot {
    const EMPTY: Self = Self {
        arm: SELECTED_ARM_NONE,
        refs: 0,
    };

    #[inline]
    fn commit_existing_lane_reselection(&mut self, current_arm: u8, selected_arm: u8) {
        if current_arm == selected_arm {
            return;
        }
        if self.refs == 0 {
            crate::invariant();
        }
        if self.arm == current_arm {
            if self.refs != 1 {
                crate::invariant();
            }
            self.arm = selected_arm;
        } else if self.arm != selected_arm {
            crate::invariant();
        }
    }

    #[inline]
    fn prepared_release(mut self) -> Self {
        if self.refs == 0 {
            crate::invariant();
        }
        self.refs -= 1;
        if self.refs == 0 { Self::EMPTY } else { self }
    }
}

struct LaneOfferStateView {
    ptr: *mut LaneOfferState,
    lane_dense_by_lane: *mut DenseLaneOrdinal,
    lane_slot_count: usize,
    len: usize,
}

impl LaneOfferStateView {
    unsafe fn init(
        dst: *mut Self,
        ptr: *mut LaneOfferState,
        lane_dense_by_lane: *mut DenseLaneOrdinal,
        lane_slot_count: usize,
        len: usize,
    ) {
        if len >= DENSE_LANE_ABSENT.get() {
            crate::invariant();
        }
        /* SAFETY: `RouteState::init_empty` passes an unpublished `LaneOfferStateView`; offer-state column and dense map are installed before lookup. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).len).write(len);
        }
        let mut idx = 0usize;
        while idx < len {
            /* SAFETY: `idx < len` selects one unpublished lane-offer slot; every active dense lane starts EMPTY before publication. */
            unsafe {
                ptr.add(idx).write(LaneOfferState::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn lane_dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.lane_slot_count {
            return None;
        }
        let dense = /* SAFETY: `lane_idx < lane_slot_count` bounds this lane-offer dense map; this read only translates to a dense offer-state slot. */ unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == DENSE_LANE_ABSENT || dense.get() >= self.len {
            None
        } else {
            Some(dense.get())
        }
    }

    #[inline]
    fn get(&self, lane_idx: usize) -> LaneOfferState {
        let Some(dense) = self.lane_dense_ordinal(lane_idx) else {
            return LaneOfferState::EMPTY;
        };
        /* SAFETY: `dense < len` bounds the initialized lane-offer column; shared access copies the resident offer state. */
        unsafe { *self.ptr.add(dense) }
    }

    #[inline]
    fn get_mut(&mut self, lane_idx: usize) -> Option<&mut LaneOfferState> {
        let dense = self.lane_dense_ordinal(lane_idx)?;
        Some(
            /* SAFETY: `dense < len` bounds the initialized lane-offer column, and `&mut self` owns this lane mutation. */
            unsafe { &mut *self.ptr.add(dense) },
        )
    }
}

pub(super) struct RouteState {
    lane_route_arms: RouteArmHistoryView,
    lane_offer_states: LaneOfferStateView,
    pub(super) scope_evidence: ScopeEvidenceTable,
    scope_selected_arms: *mut RouteScopeSelectedArmSlot,
    scope_selected_arm_count: usize,
    lane_reentry_lanes: LaneSet,
    lane_offer_reentry_lanes: LaneSet,
    active_offer_lanes: LaneSet,
}

pub(super) struct RouteStateStorage {
    pub(super) route_arm_storage: *mut RouteArmState,
    pub(super) lane_route_arm_lengths: *mut u16,
    pub(super) lane_offer_state_storage: *mut LaneOfferState,
    pub(super) scope_evidence_slots: *mut ScopeEvidenceSlot,
    pub(super) scope_selected_arms: *mut RouteScopeSelectedArmSlot,
    pub(super) lane_dense_by_lane: *mut DenseLaneOrdinal,
    pub(super) lane_reentry_words: *mut LaneWord,
    pub(super) lane_offer_reentry_words: *mut LaneWord,
    pub(super) active_offer_lane_words: *mut LaneWord,
}

pub(super) struct RouteStateCapacity {
    pub(super) lane_slot_count: usize,
    pub(super) lane_word_count: usize,
    pub(super) lane_offer_state_count: usize,
    pub(super) route_arm_state_capacity: usize,
    pub(super) scope_evidence_count: usize,
    pub(super) scope_selected_arm_count: usize,
}

struct SelectedRoutePreflight {
    lane_idx: usize,
    scope: ScopeId,
    scope_slot: usize,
    arm: u8,
    reentry: ReentryMark,
    effective_arm: u8,
    effective_refs: u16,
}

impl RouteState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        storage: RouteStateStorage,
        capacity: RouteStateCapacity,
    ) {
        /* SAFETY: endpoint route initialization passes an unpublished `RouteState`; sub-owners receive disjoint arena columns before exposure. */
        unsafe {
            RouteArmHistoryView::init(
                core::ptr::addr_of_mut!((*dst).lane_route_arms),
                storage.route_arm_storage,
                storage.lane_route_arm_lengths,
                storage.lane_dense_by_lane,
                capacity.lane_slot_count,
                capacity.lane_offer_state_count,
                capacity.route_arm_state_capacity,
            );
            LaneOfferStateView::init(
                core::ptr::addr_of_mut!((*dst).lane_offer_states),
                storage.lane_offer_state_storage,
                storage.lane_dense_by_lane,
                capacity.lane_slot_count,
                capacity.lane_offer_state_count,
            );
            ScopeEvidenceTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).scope_evidence),
                storage.scope_evidence_slots,
                capacity.scope_evidence_count,
            );
            core::ptr::addr_of_mut!((*dst).scope_selected_arms).write(storage.scope_selected_arms);
            core::ptr::addr_of_mut!((*dst).scope_selected_arm_count)
                .write(capacity.scope_selected_arm_count);
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lane_reentry_lanes),
                storage.lane_reentry_words,
                capacity.lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lane_offer_reentry_lanes),
                storage.lane_offer_reentry_words,
                capacity.lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).active_offer_lanes),
                storage.active_offer_lane_words,
                capacity.lane_word_count,
            );
            let mut scope_idx = 0usize;
            while scope_idx < capacity.scope_selected_arm_count {
                storage
                    .scope_selected_arms
                    .add(scope_idx)
                    .write(RouteScopeSelectedArmSlot::EMPTY);
                scope_idx += 1;
            }
        }
    }

    #[inline]
    pub(super) fn lane_route_arm_len(&self, lane_idx: usize) -> usize {
        self.lane_route_arms.lane_len(lane_idx)
    }

    fn preflight_selected_route_with_effective_slot(
        &self,
        input: SelectedRoutePreflight,
    ) -> Option<SelectedRouteCommitRow> {
        self.lane_offer_states.lane_dense_ordinal(input.lane_idx)?;
        if input.scope_slot > u16::MAX as usize {
            return None;
        }
        let len = self.lane_route_arms.lane_len(input.lane_idx);
        let mut idx = 0usize;
        while idx < len {
            let current = self.lane_route_arms.get(input.lane_idx, idx);
            if current.scope == input.scope {
                if current.arm == input.arm
                    || (input.effective_refs == 1 && input.effective_arm == current.arm)
                    || (input.reentry.is_reentrant() && input.effective_arm == input.arm)
                {
                    return Some(SelectedRouteCommitRow::new(input.scope, input.arm));
                }
                return None;
            }
            idx += 1;
        }

        if !self.lane_route_arms.has_capacity() {
            return None;
        }
        if input.effective_refs == 0
            || (input.effective_arm == input.arm && input.effective_refs != u16::MAX)
        {
            Some(SelectedRouteCommitRow::new(input.scope, input.arm))
        } else {
            None
        }
    }

    pub(super) fn preflight_selected_route_commit(
        &self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        arm: u8,
        reentry: ReentryMark,
    ) -> Option<SelectedRouteCommitRow> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = /* SAFETY: selected-route preflight bounds `scope_slot` inside selected-arm column; it only reads resident arm/ref. */ unsafe { &*self.scope_selected_arms.add(scope_slot) };
        self.preflight_selected_route_with_effective_slot(SelectedRoutePreflight {
            lane_idx,
            scope,
            scope_slot,
            arm,
            reentry,
            effective_arm: slot.arm,
            effective_refs: slot.refs,
        })
    }

    pub(super) fn apply_prepared_route_selection(
        &mut self,
        lane_idx: usize,
        scope_slot: usize,
        reentry: ReentryMark,
        row: SelectedRouteCommitRow,
        _permit: CommitDeltaApplyPermit,
    ) {
        if row.is_empty() {
            crate::invariant();
        }
        let scope = row.scope();
        if self
            .lane_offer_states
            .lane_dense_ordinal(lane_idx)
            .is_none()
        {
            crate::invariant();
        }
        if scope_slot >= self.scope_selected_arm_count {
            crate::invariant();
        }
        let arm = row.selected_arm();
        let mut next_slot = /* SAFETY: prepared route selection bounds
        `scope_slot` inside the initialized selected-arm column. The copied next
        state remains unpublished until the sparse lane history commits. */ unsafe {
            *self.scope_selected_arms.add(scope_slot)
        };
        let len = self.lane_route_arms.lane_len(lane_idx);
        let mut pos = 0usize;
        while pos < len {
            let current = self.lane_route_arms.get(lane_idx, pos);
            if current.scope == scope {
                next_slot.commit_existing_lane_reselection(current.arm, arm);
                if !self.lane_route_arms.set(lane_idx, pos, scope, arm) {
                    crate::invariant();
                }
                /* SAFETY: `scope_slot` was bounded above. The history mutation
                completed, so this single write publishes the matching ref state. */
                unsafe {
                    self.scope_selected_arms.add(scope_slot).write(next_slot);
                }
                return;
            }
            pos += 1;
        }

        if next_slot.refs == 0 {
            next_slot.arm = arm;
            next_slot.refs = 1;
        } else {
            if next_slot.refs == u16::MAX {
                crate::invariant();
            }
            next_slot.refs += 1;
        }
        if !self.lane_route_arms.has_capacity() {
            crate::invariant();
        }
        if !self.lane_route_arms.push(lane_idx, scope, arm) {
            crate::invariant();
        }
        /* SAFETY: `scope_slot` was bounded above. Every fallible history check
        and mutation completed before this single selected-arm publication. */
        unsafe {
            self.scope_selected_arms.add(scope_slot).write(next_slot);
        }
        if reentry.is_reentrant() {
            self.lane_reentry_lanes.insert(lane_idx);
        }
    }

    #[inline]
    pub(super) fn selected_arm_for_scope_slot(&self, scope_slot: usize) -> Option<u8> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = /* SAFETY: selected-arm lookup bounds `scope_slot` inside selected-arm column; this read copies arm/ref state. */ unsafe { *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 || slot.arm == SELECTED_ARM_NONE {
            None
        } else {
            Some(slot.arm)
        }
    }

    #[inline]
    pub(super) fn lane_offer_state(&self, lane_idx: usize) -> LaneOfferState {
        self.lane_offer_states.get(lane_idx)
    }

    #[inline]
    pub(super) fn lane_offer_state_mut(&mut self, lane_idx: usize) -> Option<&mut LaneOfferState> {
        self.lane_offer_states.get_mut(lane_idx)
    }

    #[inline]
    pub(super) fn clear_lane_offer_state(&mut self, lane_idx: usize) -> LaneOfferState {
        let detached = self.lane_offer_state(lane_idx);
        if let Some(state) = self.lane_offer_state_mut(lane_idx) {
            *state = LaneOfferState::EMPTY;
        }
        self.active_offer_lanes.remove(lane_idx);
        self.lane_offer_reentry_lanes.remove(lane_idx);
        detached
    }

    #[inline]
    pub(super) fn set_lane_offer_state(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
        reentry: ReentryMark,
    ) {
        let Some(state) = self.lane_offer_state_mut(lane_idx) else {
            crate::invariant();
        };
        *state = info;
        self.active_offer_lanes.insert(lane_idx);
        if reentry.is_reentrant() {
            self.lane_offer_reentry_lanes.insert(lane_idx);
        } else {
            self.lane_offer_reentry_lanes.remove(lane_idx);
        }
    }

    #[inline]
    pub(super) fn active_offer_lanes(&self) -> LaneSetView<'_> {
        self.active_offer_lanes.view()
    }

    #[inline]
    pub(super) fn lane_reentry_lanes(&self) -> LaneSetView<'_> {
        self.lane_reentry_lanes.view()
    }

    #[inline]
    pub(super) fn lane_offer_reentry_lanes(&self) -> LaneSetView<'_> {
        self.lane_offer_reentry_lanes.view()
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
