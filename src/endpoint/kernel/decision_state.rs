//! # Unsafe Owner Contract
//! Route-state owner: unsafe blocks may index raw table storage only after
//! route scope and dense-lane metadata prove the slot belongs to this endpoint
//! generation.

use super::core::CommitDeltaApplyPermit;
use super::evidence::RouteArmState;
use super::evidence_store::{ScopeEvidenceSlot, ScopeEvidenceTable};
use super::frontier::LaneOfferState;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{
    DENSE_LANE_NONE, DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord, PackedLaneRange,
};
use crate::global::typestate::{EventCursor, LocalConflict, PackedEventConflict};
const NO_SELECTED_ARM: u8 = u8::MAX;

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
        matches!(self.conflict.to_conflict(), None)
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
    range_lane_len: u32,
}

impl SelectedRouteCommitRowsRef {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self { range_lane_len: 0 };

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn from_resident_range_for_lane(
        range: PackedLaneRange,
        route_lane: u8,
    ) -> Self {
        if range.is_absent_or_zero_len() {
            Self::EMPTY
        } else {
            if range.start() > u16::MAX as usize || range.len() > u8::MAX as usize {
                crate::invariant();
            }
            Self {
                range_lane_len: ((range.start() as u32) << 16)
                    | ((route_lane as u32) << 8)
                    | range.len() as u32,
            }
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn is_empty(self) -> bool {
        (self.range_lane_len & 0xff) == 0
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn len(self) -> usize {
        (self.range_lane_len & 0xff) as usize
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn packed_selected_lane(self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            Some(((self.range_lane_len >> 8) & 0xff) as u8)
        }
    }

    #[inline(always)]
    const fn range(self) -> PackedLaneRange {
        if self.is_empty() {
            PackedLaneRange::EMPTY
        } else {
            PackedLaneRange::new((self.range_lane_len >> 16) as usize, self.len())
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
            self.selected_routes.packed_selected_lane()
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
    pub(super) unsafe fn init(dst: *mut Self, _ptr: *mut SelectedRouteCommitRow, cap: usize) {
        if cap > u16::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).max_len).write(cap as u16);
        }
    }

    #[inline]
    pub(super) fn begin(&mut self) -> RecvResult<SelectedRouteCommitRows> {
        Ok(SelectedRouteCommitRows {
            routes: SelectedRouteCommitRowsRef::EMPTY,
            max_len: self.max_len,
        })
    }

    pub(in crate::endpoint::kernel) fn seal(
        &mut self,
        routes: SelectedRouteCommitRowsRef,
    ) -> RecvResult<PreparedRouteCommitRows> {
        let len = routes.len();
        if len == 0 {
            return Ok(PreparedRouteCommitRows::empty());
        }
        if len > self.max_len as usize || len > u8::MAX as usize {
            return Err(RecvError::PhaseInvariant);
        }
        routes
            .packed_selected_lane()
            .ok_or(RecvError::PhaseInvariant)?;
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
            max_len: u8::MAX as u16,
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
    pub(super) fn as_commit_rows(&self, route_lane: u8) -> SelectedRouteCommitRowsRef {
        if self.routes.is_empty() {
            return SelectedRouteCommitRowsRef::EMPTY;
        }
        if self.routes.packed_selected_lane() != Some(route_lane) {
            return SelectedRouteCommitRowsRef::EMPTY;
        }
        self.routes
    }

    pub(super) fn as_route_only_commit_rows(&self, route_lane: u8) -> RouteOnlyCommitRowsRef {
        RouteOnlyCommitRowsRef {
            selected_routes: self.as_commit_rows(route_lane),
        }
    }

    pub(super) fn merge_chain(
        &mut self,
        cursor: &EventCursor,
        lane: u8,
        conflict: PackedEventConflict,
        first_arm: Option<u8>,
    ) -> RecvResult<()> {
        let range = cursor
            .route_commit_range_for_conflict(conflict, first_arm)
            .ok_or(RecvError::PhaseInvariant)?;
        if range.len() > self.max_len as usize {
            return Err(RecvError::PhaseInvariant);
        }
        let incoming = SelectedRouteCommitRowsRef::from_resident_range_for_lane(range, lane);
        if self.routes.is_empty() {
            self.routes = incoming;
            return Ok(());
        }
        if self.routes.packed_selected_lane() != Some(lane) {
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
        arm: NO_SELECTED_ARM,
        refs: 0,
    };
}

#[derive(Clone, Copy)]
struct RouteArmStackView {
    ptr: *mut RouteArmState,
    lane_dense_by_lane: *mut DenseLaneOrdinal,
    lane_slot_count: usize,
    active_lane_count: usize,
    depth: u8,
}

impl RouteArmStackView {
    unsafe fn init(
        dst: *mut Self,
        ptr: *mut RouteArmState,
        lane_dense_by_lane: *mut DenseLaneOrdinal,
        lane_slot_count: usize,
        active_lane_count: usize,
        depth: usize,
    ) {
        if depth > u8::MAX as usize {
            panic!("route arm stack depth overflow");
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).active_lane_count).write(active_lane_count);
            core::ptr::addr_of_mut!((*dst).depth).write(depth as u8);
        }
        let total = active_lane_count.checked_mul(depth).expect("invariant");
        let mut idx = 0usize;
        while idx < total {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                ptr.add(idx).write(RouteArmState::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn lane_dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.lane_slot_count {
            return None;
        }
        let dense = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == DENSE_LANE_NONE || dense.get() >= self.active_lane_count {
            None
        } else {
            Some(dense.get())
        }
    }

    #[inline]
    fn depth(&self) -> usize {
        self.depth as usize
    }

    #[inline]
    fn get(&self, lane_idx: usize, arm_idx: usize) -> RouteArmState {
        let Some(dense) = self.lane_dense_ordinal(lane_idx) else {
            return RouteArmState::EMPTY;
        };
        let depth = self.depth();
        if arm_idx >= depth {
            return RouteArmState::EMPTY;
        }
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { *self.ptr.add(dense * depth + arm_idx) }
    }

    #[inline]
    fn set(&mut self, lane_idx: usize, arm_idx: usize, state: RouteArmState) -> bool {
        let Some(dense) = self.lane_dense_ordinal(lane_idx) else {
            return false;
        };
        let depth = self.depth();
        if arm_idx >= depth {
            return false;
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.ptr.add(dense * depth + arm_idx).write(state);
        }
        true
    }
}

#[derive(Clone, Copy)]
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
        if len >= DENSE_LANE_NONE.get() {
            crate::invariant();
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).len).write(len);
        }
        let mut idx = 0usize;
        while idx < len {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
        let dense = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == DENSE_LANE_NONE || dense.get() >= self.len {
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
        if dense >= self.len as usize {
            return LaneOfferState::EMPTY;
        }
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { *self.ptr.add(dense) }
    }

    #[inline]
    fn get_mut(&mut self, lane_idx: usize) -> Option<&mut LaneOfferState> {
        let dense = self.lane_dense_ordinal(lane_idx)?;
        if dense >= self.len as usize {
            return None;
        }
        Some(
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { &mut *self.ptr.add(dense) },
        )
    }
}

pub(super) struct RouteState {
    lane_route_arms: RouteArmStackView,
    lane_offer_states: LaneOfferStateView,
    pub(super) scope_evidence: ScopeEvidenceTable,
    scope_selected_arms: *mut RouteScopeSelectedArmSlot,
    scope_selected_arm_count: usize,
    lane_route_arm_lens: *mut u8,
    lane_linger_counts: *mut u8,
    lane_linger_lanes: LaneSet,
    lane_offer_linger_lanes: LaneSet,
    active_offer_lanes: LaneSet,
}

impl RouteState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        route_arm_storage: *mut RouteArmState,
        lane_offer_state_storage: *mut LaneOfferState,
        scope_evidence_slots: *mut ScopeEvidenceSlot,
        scope_selected_arms: *mut RouteScopeSelectedArmSlot,
        lane_dense_by_lane: *mut DenseLaneOrdinal,
        lane_slot_count: usize,
        lane_route_arm_lens: *mut u8,
        lane_linger_counts: *mut u8,
        lane_linger_words: *mut LaneWord,
        lane_offer_linger_words: *mut LaneWord,
        active_offer_lane_words: *mut LaneWord,
        active_lane_count: usize,
        lane_word_count: usize,
        lane_offer_state_count: usize,
        route_frame_depth: usize,
        scope_evidence_count: usize,
        scope_selected_arm_count: usize,
    ) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            RouteArmStackView::init(
                core::ptr::addr_of_mut!((*dst).lane_route_arms),
                route_arm_storage,
                lane_dense_by_lane,
                lane_slot_count,
                active_lane_count,
                route_frame_depth,
            );
            LaneOfferStateView::init(
                core::ptr::addr_of_mut!((*dst).lane_offer_states),
                lane_offer_state_storage,
                lane_dense_by_lane,
                lane_slot_count,
                lane_offer_state_count,
            );
            ScopeEvidenceTable::init_from_parts(
                core::ptr::addr_of_mut!((*dst).scope_evidence),
                scope_evidence_slots,
                scope_evidence_count,
            );
            core::ptr::addr_of_mut!((*dst).scope_selected_arms).write(scope_selected_arms);
            core::ptr::addr_of_mut!((*dst).scope_selected_arm_count)
                .write(scope_selected_arm_count);
            core::ptr::addr_of_mut!((*dst).lane_route_arm_lens).write(lane_route_arm_lens);
            core::ptr::addr_of_mut!((*dst).lane_linger_counts).write(lane_linger_counts);
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lane_linger_lanes),
                lane_linger_words,
                lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lane_offer_linger_lanes),
                lane_offer_linger_words,
                lane_word_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).active_offer_lanes),
                active_offer_lane_words,
                lane_word_count,
            );
            let mut lane_idx = 0usize;
            while lane_idx < active_lane_count {
                lane_route_arm_lens.add(lane_idx).write(0);
                lane_linger_counts.add(lane_idx).write(0);
                lane_idx += 1;
            }
            let mut scope_idx = 0usize;
            while scope_idx < scope_selected_arm_count {
                scope_selected_arms
                    .add(scope_idx)
                    .write(RouteScopeSelectedArmSlot::EMPTY);
                scope_idx += 1;
            }
        }
    }

    #[inline]
    pub(super) fn lane_route_arm_len(&self, lane_idx: usize) -> usize {
        self.lane_offer_states
            .lane_dense_ordinal(lane_idx)
            .map(|dense| /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_route_arm_lens.add(dense) as usize })
            .unwrap_or(0)
    }

    fn preflight_selected_route_with_effective_slot(
        &self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        arm: u8,
        is_linger: bool,
        effective_arm: u8,
        effective_refs: u16,
    ) -> Option<SelectedRouteCommitRow> {
        let dense = self.lane_offer_states.lane_dense_ordinal(lane_idx)?;
        if scope_slot > u16::MAX as usize {
            return None;
        }
        let len = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_route_arm_lens.add(dense) as usize };
        let mut idx = 0usize;
        while idx < len {
            let current = self.lane_route_arms.get(lane_idx, idx);
            if current.scope == scope {
                if current.arm == arm || (effective_refs == 1 && effective_arm == current.arm) {
                    return Some(SelectedRouteCommitRow::new(scope, arm));
                }
                return None;
            }
            idx += 1;
        }

        if len >= self.lane_route_arms.depth() && is_linger {
            return None;
        }
        if effective_refs == 0 || (effective_arm == arm && effective_refs != u16::MAX) {
            Some(SelectedRouteCommitRow::new(scope, arm))
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
        is_linger: bool,
    ) -> Option<SelectedRouteCommitRow> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.scope_selected_arms.add(scope_slot) };
        self.preflight_selected_route_with_effective_slot(
            lane_idx, scope, scope_slot, arm, is_linger, slot.arm, slot.refs,
        )
    }

    pub(super) fn apply_prepared_route_selection(
        &mut self,
        lane_idx: usize,
        scope_slot: usize,
        is_linger: bool,
        row: SelectedRouteCommitRow,
        _permit: CommitDeltaApplyPermit,
    ) {
        if row.is_empty() {
            crate::invariant();
        }
        let scope = row.scope();
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            crate::invariant();
        };
        if scope_slot >= self.scope_selected_arm_count {
            crate::invariant();
        }
        let arm = row.selected_arm();
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *self.scope_selected_arms.add(scope_slot) };
        let len = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_route_arm_lens.add(dense) as usize };
        let mut pos = 0usize;
        while pos < len {
            let current = self.lane_route_arms.get(lane_idx, pos);
            if current.scope == scope {
                if current.arm != arm {
                    slot.arm = arm;
                    slot.refs = 1;
                }
                self.lane_route_arms
                    .set(lane_idx, pos, RouteArmState { scope, arm });
                return;
            }
            pos += 1;
        }

        if slot.refs == 0 {
            slot.arm = arm;
            slot.refs = 1;
        } else {
            if slot.refs == u16::MAX {
                crate::invariant();
            }
            slot.refs += 1;
        }
        if len >= self.lane_route_arms.depth() {
            if is_linger {
                crate::invariant();
            }
            return;
        }
        self.lane_route_arms
            .set(lane_idx, len, RouteArmState { scope, arm });
        if len >= u8::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.lane_route_arm_lens.add(dense).write((len as u8) + 1);
        }
        if is_linger {
            self.increment_linger_count(lane_idx);
        }
    }

    #[inline]
    pub(super) fn selected_arm_for_scope_slot(&self, scope_slot: usize) -> Option<u8> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 || slot.arm == NO_SELECTED_ARM {
            None
        } else {
            Some(slot.arm)
        }
    }

    pub(super) fn active_linger_scope_for_lane<F>(
        &self,
        lane_idx: usize,
        mut is_linger_route: F,
    ) -> Option<ScopeId>
    where
        F: FnMut(ScopeId) -> bool,
    {
        let len = self.lane_route_arm_len(lane_idx);
        let mut idx = len;
        while idx > 0 {
            idx -= 1;
            let slot = self.lane_route_arms.get(lane_idx, idx);
            let scope = slot.scope;
            if scope.is_none() || slot.arm != 0 {
                continue;
            }
            if is_linger_route(scope) {
                return Some(scope);
            }
        }
        None
    }

    #[inline]
    pub(super) fn increment_linger_count(&mut self, lane_idx: usize) {
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            crate::invariant();
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let count = &mut *self.lane_linger_counts.add(dense);
            if *count == u8::MAX {
                crate::invariant();
            }
            *count += 1;
            if *count == 1 {
                self.lane_linger_lanes.insert(lane_idx);
            }
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
        let old = self.lane_offer_state(lane_idx);
        if let Some(state) = self.lane_offer_state_mut(lane_idx) {
            *state = LaneOfferState::EMPTY;
        }
        self.active_offer_lanes.remove(lane_idx);
        self.lane_offer_linger_lanes.remove(lane_idx);
        old
    }

    #[inline]
    pub(super) fn set_lane_offer_state(
        &mut self,
        lane_idx: usize,
        info: LaneOfferState,
        is_linger: bool,
    ) {
        let Some(state) = self.lane_offer_state_mut(lane_idx) else {
            crate::invariant();
        };
        *state = info;
        self.active_offer_lanes.insert(lane_idx);
        if is_linger {
            self.lane_offer_linger_lanes.insert(lane_idx);
        } else {
            self.lane_offer_linger_lanes.remove(lane_idx);
        }
    }

    #[inline]
    pub(super) fn active_offer_lanes(&self) -> LaneSetView<'_> {
        self.active_offer_lanes.view()
    }

    #[inline]
    pub(super) fn lane_linger_lanes(&self) -> LaneSetView<'_> {
        self.lane_linger_lanes.view()
    }

    #[inline]
    pub(super) fn lane_offer_linger_lanes(&self) -> LaneSetView<'_> {
        self.lane_offer_linger_lanes.view()
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
