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
    DENSE_LANE_NONE, DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord,
};
use crate::global::typestate::{LocalConflict, PackedEventConflict};
const NO_SELECTED_ARM: u8 = u8::MAX;

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitRow {
    conflict: PackedEventConflict,
}

impl SelectedRouteCommitRow {
    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        conflict: PackedEventConflict::none(),
    };

    const fn new(scope: ScopeId, selected_arm: u8) -> Self {
        Self {
            conflict: PackedEventConflict::route_arm(scope, selected_arm),
        }
    }

    pub(in crate::endpoint::kernel) const fn scope(self) -> ScopeId {
        match self.conflict.to_conflict() {
            Some(LocalConflict::RouteArm { scope, .. }) => scope,
            _ => ScopeId::none(),
        }
    }

    pub(in crate::endpoint::kernel) const fn selected_arm(self) -> u8 {
        match self.conflict.to_conflict() {
            Some(LocalConflict::RouteArm { arm, .. }) => arm,
            _ => u8::MAX,
        }
    }

    pub(in crate::endpoint::kernel) const fn is_empty(self) -> bool {
        matches!(self.conflict.to_conflict(), None)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitRowsRef {
    ptr: *const SelectedRouteCommitRow,
    len_and_lane: usize,
}

impl SelectedRouteCommitRowsRef {
    const ROUTE_LANE_SHIFT: usize = usize::BITS as usize - 8;
    const LEN_MASK: usize = (1usize << Self::ROUTE_LANE_SHIFT) - 1;

    pub(in crate::endpoint::kernel) const EMPTY: Self = Self {
        ptr: core::ptr::null(),
        len_and_lane: 0,
    };

    #[inline(always)]
    fn from_slice_for_lane(rows: &[SelectedRouteCommitRow], route_lane: u8) -> Self {
        if rows.is_empty() {
            Self::EMPTY
        } else {
            debug_assert!(rows.len() <= Self::LEN_MASK);
            Self {
                ptr: rows.as_ptr(),
                len_and_lane: ((route_lane as usize) << Self::ROUTE_LANE_SHIFT) | rows.len(),
            }
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn is_empty(self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn len(self) -> usize {
        self.len_and_lane & Self::LEN_MASK
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn packed_selected_lane(self) -> Option<u8> {
        if self.len() == 0 {
            None
        } else {
            Some((self.len_and_lane >> Self::ROUTE_LANE_SHIFT) as u8)
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn get(self, idx: usize) -> Option<SelectedRouteCommitRow> {
        if idx >= self.len() || self.ptr.is_null() {
            return None;
        }
        Some(
            /* SAFETY: `idx < len` and `ptr` was produced from a live bounded route-row workspace for the prepared commit duration. */
            unsafe { *self.ptr.add(idx) },
        )
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
enum RouteCommitRowWorkspaceState {
    Idle,
    Sealed,
}

pub(super) struct RouteCommitRowWorkspace {
    ptr: *mut SelectedRouteCommitRow,
    cap: u16,
    state: RouteCommitRowWorkspaceState,
}

pub(crate) struct PreparedRouteRowsLease {
    workspace: *mut RouteCommitRowWorkspace,
    len_and_lane: u16,
}

impl PreparedRouteRowsLease {
    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn empty() -> Self {
        Self {
            workspace: core::ptr::null_mut(),
            len_and_lane: 0,
        }
    }

    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        (self.len_and_lane & 0xff) as usize
    }

    #[inline(always)]
    pub(crate) const fn selected_lane(&self) -> Option<u8> {
        if self.len() == 0 {
            None
        } else {
            Some((self.len_and_lane >> 8) as u8)
        }
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: usize) -> Option<SelectedRouteCommitRow> {
        if idx >= self.len() || self.workspace.is_null() {
            return None;
        }
        Some(
            /* SAFETY: the sealed lease keeps the route-row workspace reserved until this lease is dropped. */
            unsafe { (*self.workspace).row(idx) },
        )
    }

    #[inline(always)]
    pub(crate) fn take(&mut self) -> Self {
        core::mem::replace(self, Self::empty())
    }
}

impl Drop for PreparedRouteRowsLease {
    #[inline]
    fn drop(&mut self) {
        if self.workspace.is_null() {
            return;
        }
        /* SAFETY: non-empty leases are constructed only by `RouteCommitRowWorkspace::seal`, which stores this workspace pointer. */
        unsafe {
            (*self.workspace).release_sealed();
        }
        self.workspace = core::ptr::null_mut();
        self.len_and_lane = 0;
    }
}

impl RouteCommitRowWorkspace {
    pub(super) unsafe fn init(dst: *mut Self, ptr: *mut SelectedRouteCommitRow, cap: usize) {
        if cap > u16::MAX as usize {
            panic!("route commit row workspace overflow");
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).cap).write(cap as u16);
            core::ptr::addr_of_mut!((*dst).state).write(RouteCommitRowWorkspaceState::Idle);
        }
        let mut idx = 0usize;
        while idx < cap {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                ptr.add(idx).write(SelectedRouteCommitRow::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    pub(super) fn begin(&mut self) -> RecvResult<SelectedRouteCommitRows<'_>> {
        self.resume(0)
    }

    #[inline]
    pub(super) fn resume(&mut self, len: usize) -> RecvResult<SelectedRouteCommitRows<'_>> {
        let cap = self.cap as usize;
        if self.state != RouteCommitRowWorkspaceState::Idle || cap == 0 || len > cap {
            return Err(RecvError::PhaseInvariant);
        }
        let rows = /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */ unsafe { core::slice::from_raw_parts_mut(self.ptr, cap) };
        Ok(SelectedRouteCommitRows { rows, len })
    }

    pub(in crate::endpoint::kernel) fn seal(
        &mut self,
        routes: SelectedRouteCommitRowsRef,
    ) -> RecvResult<PreparedRouteRowsLease> {
        let len = routes.len();
        if len == 0 {
            return Ok(PreparedRouteRowsLease::empty());
        }
        if self.state != RouteCommitRowWorkspaceState::Idle
            || self.ptr.is_null()
            || routes.ptr != self.ptr.cast_const()
            || len > self.cap as usize
            || len > u8::MAX as usize
        {
            return Err(RecvError::PhaseInvariant);
        }
        let lane = routes
            .packed_selected_lane()
            .ok_or(RecvError::PhaseInvariant)?;
        self.state = RouteCommitRowWorkspaceState::Sealed;
        Ok(PreparedRouteRowsLease {
            workspace: self as *mut Self,
            len_and_lane: ((lane as u16) << 8) | len as u16,
        })
    }

    #[inline(always)]
    fn row(&self, idx: usize) -> SelectedRouteCommitRow {
        debug_assert_eq!(self.state, RouteCommitRowWorkspaceState::Sealed);
        debug_assert!(idx < self.cap as usize);
        /* SAFETY: the sealed lease checked `idx < len <= cap`; callers bound `idx` by the lease length. */
        unsafe { *self.ptr.add(idx) }
    }

    #[inline(always)]
    fn release_sealed(&mut self) {
        debug_assert_eq!(self.state, RouteCommitRowWorkspaceState::Sealed);
        self.state = RouteCommitRowWorkspaceState::Idle;
    }
}

pub(super) struct SelectedRouteCommitRows<'a> {
    rows: &'a mut [SelectedRouteCommitRow],
    len: usize,
}

impl SelectedRouteCommitRows<'_> {
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        let mut idx = 0usize;
        while idx < self.len {
            let row = self.rows[idx];
            if row.scope() == scope {
                return Some(row.selected_arm());
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn as_commit_rows(&self, route_lane: u8) -> SelectedRouteCommitRowsRef {
        SelectedRouteCommitRowsRef::from_slice_for_lane(&self.rows[..self.len], route_lane)
    }

    pub(super) fn as_route_only_commit_rows(&self, route_lane: u8) -> RouteOnlyCommitRowsRef {
        RouteOnlyCommitRowsRef {
            selected_routes: self.as_commit_rows(route_lane),
        }
    }

    pub(super) fn push_unique(&mut self, row: SelectedRouteCommitRow) -> RecvResult<()> {
        if let Some(arm) = self.arm_for_scope(row.scope()) {
            return if arm == row.selected_arm() {
                Ok(())
            } else {
                Err(RecvError::PhaseInvariant)
            };
        }
        if self.len >= self.rows.len() {
            return Err(RecvError::PhaseInvariant);
        }
        self.rows[self.len] = row;
        self.len += 1;
        Ok(())
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
        let total = active_lane_count.saturating_mul(depth);
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
            panic!("lane offer state capacity overflow");
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
            panic!("prepared route apply invariant");
        }
        let scope = row.scope();
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            panic!("prepared route apply invariant");
        };
        if scope_slot >= self.scope_selected_arm_count {
            panic!("prepared route apply invariant");
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
            slot.refs = slot.refs.saturating_add(1);
        }
        if len >= self.lane_route_arms.depth() {
            if is_linger {
                panic!("prepared route apply invariant");
            }
            return;
        }
        self.lane_route_arms
            .set(lane_idx, len, RouteArmState { scope, arm });
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.lane_route_arm_lens
                .add(dense)
                .write((len as u8).saturating_add(1));
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
            return;
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            let count = &mut *self.lane_linger_counts.add(dense);
            debug_assert!(*count < u8::MAX);
            *count = count.saturating_add(1);
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
            debug_assert!(false, "lane offer state must exist for active lanes");
            return;
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
