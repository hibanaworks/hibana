//! Route-state owner for endpoint kernel runtime bookkeeping.

use super::evidence::RouteArmState;
use super::evidence_store::{ScopeEvidenceSlot, ScopeEvidenceTable};
use super::frontier::LaneOfferState;
use crate::endpoint::{RecvError, RecvResult};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{
    DENSE_LANE_NONE, DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord,
};
const NO_SELECTED_ARM: u8 = u8::MAX;
const ROUTE_ARM_COMMIT_INSERT: u16 = u16::MAX;

#[derive(Clone, Copy)]
pub(super) struct RouteArmCommitProof {
    scope: ScopeId,
    lane_idx: u16,
    dense: u16,
    scope_slot: u16,
    pos: u16,
    arm: u8,
    is_linger: bool,
}

impl RouteArmCommitProof {
    pub(super) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        lane_idx: 0,
        dense: 0,
        scope_slot: 0,
        pos: 0,
        arm: 0,
        is_linger: false,
    };

    #[inline]
    pub(super) const fn lane_idx(self) -> u16 {
        self.lane_idx
    }

    #[inline]
    pub(super) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline]
    pub(super) const fn arm(self) -> u8 {
        self.arm
    }
}

pub(super) struct RouteCommitProofWorkspace {
    ptr: *mut RouteArmCommitProof,
    cap: u16,
}

impl RouteCommitProofWorkspace {
    pub(super) unsafe fn init(dst: *mut Self, ptr: *mut RouteArmCommitProof, cap: usize) {
        if cap > u16::MAX as usize {
            panic!("route commit proof workspace overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).cap).write(cap as u16);
        }
        let mut idx = 0usize;
        while idx < cap {
            unsafe {
                ptr.add(idx).write(RouteArmCommitProof::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    pub(super) fn begin(&mut self, required: usize) -> RecvResult<RouteCommitProofList<'_>> {
        let cap = self.cap as usize;
        if required > cap {
            return Err(RecvError::PhaseInvariant);
        }
        let rows = unsafe { core::slice::from_raw_parts_mut(self.ptr, cap) };
        Ok(RouteCommitProofList { rows, len: 0 })
    }
}

pub(super) struct RouteCommitProofList<'a> {
    rows: &'a mut [RouteArmCommitProof],
    len: usize,
}

impl RouteCommitProofList<'_> {
    #[cfg(test)]
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn contains_lane_scope(&self, lane: u8, scope: ScopeId) -> bool {
        let mut idx = 0usize;
        while idx < self.len {
            let proof = self.rows[idx];
            if proof.lane_idx() == lane as u16 && proof.scope() == scope {
                return true;
            }
            idx += 1;
        }
        false
    }

    pub(super) fn arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        let mut idx = 0usize;
        while idx < self.len {
            let proof = self.rows[idx];
            if proof.scope() == scope {
                return Some(proof.arm());
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn iter(&self) -> impl Iterator<Item = RouteArmCommitProof> + '_ {
        self.rows[..self.len].iter().copied()
    }

    pub(super) fn push_unique(&mut self, proof: RouteArmCommitProof) -> RecvResult<()> {
        if self.contains_lane_scope(proof.lane_idx() as u8, proof.scope()) {
            return Ok(());
        }
        if self.len >= self.rows.len() {
            return Err(RecvError::PhaseInvariant);
        }
        self.rows[self.len] = proof;
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
        let dense = unsafe { *self.lane_dense_by_lane.add(lane_idx) };
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
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).lane_slot_count).write(lane_slot_count);
            core::ptr::addr_of_mut!((*dst).len).write(len);
        }
        let mut idx = 0usize;
        while idx < len {
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
        let dense = unsafe { *self.lane_dense_by_lane.add(lane_idx) };
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
        unsafe { *self.ptr.add(dense) }
    }

    #[inline]
    fn get_mut(&mut self, lane_idx: usize) -> Option<&mut LaneOfferState> {
        let dense = self.lane_dense_ordinal(lane_idx)?;
        if dense >= self.len as usize {
            return None;
        }
        Some(unsafe { &mut *self.ptr.add(dense) })
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
    active_route_lanes: LaneSet,
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
        active_route_lane_words: *mut LaneWord,
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
                core::ptr::addr_of_mut!((*dst).active_route_lanes),
                active_route_lane_words,
                lane_word_count,
            );
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
            .map(|dense| unsafe { *self.lane_route_arm_lens.add(dense) as usize })
            .unwrap_or(0)
    }

    fn preflight_route_arm_with_effective_slot(
        &self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        arm: u8,
        is_linger: bool,
        effective_arm: u8,
        effective_refs: u16,
    ) -> Option<RouteArmCommitProof> {
        let dense = self.lane_offer_states.lane_dense_ordinal(lane_idx)?;
        if lane_idx > u16::MAX as usize
            || dense > u16::MAX as usize
            || scope_slot > u16::MAX as usize
        {
            return None;
        }
        let len = unsafe { *self.lane_route_arm_lens.add(dense) as usize };
        let mut idx = 0usize;
        while idx < len {
            let current = self.lane_route_arms.get(lane_idx, idx);
            if current.scope == scope {
                if current.arm == arm || (effective_refs == 1 && effective_arm == current.arm) {
                    return Some(RouteArmCommitProof {
                        scope,
                        lane_idx: lane_idx as u16,
                        dense: dense as u16,
                        scope_slot: scope_slot as u16,
                        pos: idx as u16,
                        arm,
                        is_linger,
                    });
                }
                return None;
            }
            idx += 1;
        }

        if len >= self.lane_route_arms.depth() || len > u16::MAX as usize {
            return None;
        }
        if effective_refs == 0 || (effective_arm == arm && effective_refs != u16::MAX) {
            Some(RouteArmCommitProof {
                scope,
                lane_idx: lane_idx as u16,
                dense: dense as u16,
                scope_slot: scope_slot as u16,
                pos: ROUTE_ARM_COMMIT_INSERT,
                arm,
                is_linger,
            })
        } else {
            None
        }
    }

    pub(super) fn preflight_route_arm_commit(
        &self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        arm: u8,
        is_linger: bool,
    ) -> Option<RouteArmCommitProof> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = unsafe { &*self.scope_selected_arms.add(scope_slot) };
        self.preflight_route_arm_with_effective_slot(
            lane_idx, scope, scope_slot, arm, is_linger, slot.arm, slot.refs,
        )
    }

    pub(super) fn preflight_route_arm_commit_after_clearing_other_lanes(
        &self,
        keep_lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        arm: u8,
        is_linger: bool,
    ) -> Option<RouteArmCommitProof> {
        if self
            .lane_offer_states
            .lane_dense_ordinal(keep_lane_idx)
            .is_none()
            || scope_slot >= self.scope_selected_arm_count
        {
            return None;
        }

        let slot = unsafe { &*self.scope_selected_arms.add(scope_slot) };
        let mut effective_arm = slot.arm;
        let mut effective_refs = slot.refs;
        let active_route_lanes = self.active_route_lanes.view();
        let mut next = active_route_lanes.first_set(self.lane_offer_states.lane_slot_count);
        while let Some(lane_idx) = next {
            if lane_idx != keep_lane_idx {
                let len = self.lane_route_arm_len(lane_idx);
                let mut idx = 0usize;
                while idx < len {
                    let current = self.lane_route_arms.get(lane_idx, idx);
                    if current.scope == scope && effective_refs != 0 && effective_arm == current.arm
                    {
                        effective_refs = effective_refs.saturating_sub(1);
                        if effective_refs == 0 {
                            effective_arm = NO_SELECTED_ARM;
                        }
                        break;
                    }
                    idx += 1;
                }
            }
            next = active_route_lanes.next_set_from(
                lane_idx.saturating_add(1),
                self.lane_offer_states.lane_slot_count,
            );
        }

        self.preflight_route_arm_with_effective_slot(
            keep_lane_idx,
            scope,
            scope_slot,
            arm,
            is_linger,
            effective_arm,
            effective_refs,
        )
    }

    pub(super) fn commit_route_arm_after_preflight(&mut self, proof: RouteArmCommitProof) {
        let scope = proof.scope;
        let lane_idx = proof.lane_idx as usize;
        let dense = proof.dense as usize;
        let scope_slot = proof.scope_slot as usize;
        let arm = proof.arm;
        let slot = unsafe { &mut *self.scope_selected_arms.add(scope_slot) };
        if proof.pos == ROUTE_ARM_COMMIT_INSERT {
            let len = unsafe { *self.lane_route_arm_lens.add(dense) as usize };
            if slot.refs == 0 {
                slot.arm = arm;
                slot.refs = 1;
            } else {
                slot.refs = slot.refs.saturating_add(1);
            }
            self.lane_route_arms
                .set(lane_idx, len, RouteArmState { scope, arm });
            unsafe {
                self.lane_route_arm_lens
                    .add(dense)
                    .write((len as u8).saturating_add(1));
            }
            if proof.is_linger {
                self.increment_linger_count(lane_idx);
            }
        } else {
            let pos = proof.pos as usize;
            let current = self.lane_route_arms.get(lane_idx, pos);
            if current.arm != arm {
                slot.arm = arm;
                slot.refs = 1;
            }
            self.lane_route_arms
                .set(lane_idx, pos, RouteArmState { scope, arm });
        }
        self.active_route_lanes.insert(lane_idx);
    }

    #[inline]
    pub(super) fn active_route_lanes(&self) -> LaneSetView {
        self.active_route_lanes.view()
    }

    pub(super) fn pop_route_arm(
        &mut self,
        lane_idx: usize,
        scope: ScopeId,
        scope_slot: usize,
        is_linger: bool,
    ) -> bool {
        let len = self.lane_route_arm_len(lane_idx);
        if len == 0 {
            return false;
        }
        let Some(pos) = (0..len)
            .rev()
            .find(|&idx| self.lane_route_arms.get(lane_idx, idx).scope == scope)
        else {
            return false;
        };

        let removed = self.lane_route_arms.get(lane_idx, pos);
        let last = len - 1;
        for idx in pos..last {
            let next = self.lane_route_arms.get(lane_idx, idx + 1);
            self.lane_route_arms.set(lane_idx, idx, next);
        }
        self.lane_route_arms
            .set(lane_idx, last, RouteArmState::EMPTY);
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            return false;
        };
        unsafe {
            let count = self.lane_route_arm_lens.add(dense);
            count.write((*count).saturating_sub(1));
        }
        if !self.decrement_scope_selected_arm(scope_slot, removed.arm) {
            return false;
        }
        if self.lane_route_arm_len(lane_idx) == 0 {
            self.active_route_lanes.remove(lane_idx);
        }
        if is_linger {
            self.decrement_linger_count(lane_idx);
        }
        true
    }

    #[inline]
    pub(super) fn last_lane_scope(&self, lane_idx: usize) -> Option<ScopeId> {
        let len = self.lane_route_arm_len(lane_idx);
        if len == 0 {
            None
        } else {
            Some(self.lane_route_arms.get(lane_idx, len - 1).scope)
        }
    }

    pub(super) fn route_arm_for(&self, lane_idx: usize, scope: ScopeId) -> Option<u8> {
        let len = self.lane_route_arm_len(lane_idx);
        (0..len).rev().find_map(|idx| {
            let slot = self.lane_route_arms.get(lane_idx, idx);
            (slot.scope == scope).then_some(slot.arm)
        })
    }

    #[inline]
    pub(super) fn selected_arm_for_scope_slot(&self, scope_slot: usize) -> Option<u8> {
        if scope_slot >= self.scope_selected_arm_count {
            return None;
        }
        let slot = unsafe { *self.scope_selected_arms.add(scope_slot) };
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
    pub(super) fn decrement_linger_count(&mut self, lane_idx: usize) {
        let Some(dense) = self.lane_offer_states.lane_dense_ordinal(lane_idx) else {
            return;
        };
        unsafe {
            let count = &mut *self.lane_linger_counts.add(dense);
            debug_assert!(*count > 0);
            if *count == 0 {
                return;
            }
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.lane_linger_lanes.remove(lane_idx);
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
    fn decrement_scope_selected_arm(&mut self, scope_slot: usize, arm: u8) -> bool {
        if scope_slot >= self.scope_selected_arm_count {
            return false;
        }
        let slot = unsafe { &mut *self.scope_selected_arms.add(scope_slot) };
        if slot.refs == 0 || slot.arm != arm {
            return false;
        }
        slot.refs -= 1;
        if slot.refs == 0 {
            *slot = RouteScopeSelectedArmSlot::EMPTY;
        }
        true
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
    pub(super) fn active_offer_lanes(&self) -> LaneSetView {
        self.active_offer_lanes.view()
    }

    #[inline]
    pub(super) fn lane_linger_lanes(&self) -> LaneSetView {
        self.lane_linger_lanes.view()
    }

    #[inline]
    pub(super) fn lane_offer_linger_lanes(&self) -> LaneSetView {
        self.lane_offer_linger_lanes.view()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global::role_program::{DenseLaneOrdinal, LaneWord, lane_word_count};
    use core::mem::MaybeUninit;

    struct RouteStateFixture {
        state: RouteState,
        _route_arm_storage: std::vec::Vec<RouteArmState>,
        _lane_offer_state_storage: std::vec::Vec<LaneOfferState>,
        _scope_evidence_slots: std::vec::Vec<MaybeUninit<ScopeEvidenceSlot>>,
        _scope_selected_arms: std::vec::Vec<RouteScopeSelectedArmSlot>,
        _lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal>,
        _lane_route_arm_lens: std::vec::Vec<u8>,
        _lane_linger_counts: std::vec::Vec<u8>,
        _active_route_lane_words: std::vec::Vec<LaneWord>,
        _lane_linger_words: std::vec::Vec<LaneWord>,
        _lane_offer_linger_words: std::vec::Vec<LaneWord>,
        _active_offer_lane_words: std::vec::Vec<LaneWord>,
    }

    fn route_state_fixture(
        lanes: usize,
        route_depth: usize,
        scope_count: usize,
    ) -> RouteStateFixture {
        let lane_words = lane_word_count(lanes);
        let mut lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal> = (0..lanes)
            .map(|lane| DenseLaneOrdinal::new(lane).expect("test lane dense ordinal"))
            .collect();
        let mut route_arm_storage = std::vec::Vec::with_capacity(lanes * route_depth);
        route_arm_storage.resize(lanes * route_depth, RouteArmState::EMPTY);
        let mut lane_offer_state_storage = std::vec::Vec::with_capacity(lanes);
        lane_offer_state_storage.resize(lanes, LaneOfferState::EMPTY);
        let mut scope_evidence_slots = std::vec::Vec::<MaybeUninit<ScopeEvidenceSlot>>::new();
        let mut scope_selected_arms = std::vec::Vec::with_capacity(scope_count);
        scope_selected_arms.resize(scope_count, RouteScopeSelectedArmSlot::EMPTY);
        let mut lane_route_arm_lens = std::vec::Vec::with_capacity(lanes);
        lane_route_arm_lens.resize(lanes, 0u8);
        let mut lane_linger_counts = std::vec::Vec::with_capacity(lanes);
        lane_linger_counts.resize(lanes, 0u8);
        let mut active_route_lane_words = std::vec::Vec::with_capacity(lane_words);
        active_route_lane_words.resize(lane_words, 0usize);
        let mut lane_linger_words = std::vec::Vec::with_capacity(lane_words);
        lane_linger_words.resize(lane_words, 0usize);
        let mut lane_offer_linger_words = std::vec::Vec::with_capacity(lane_words);
        lane_offer_linger_words.resize(lane_words, 0usize);
        let mut active_offer_lane_words = std::vec::Vec::with_capacity(lane_words);
        active_offer_lane_words.resize(lane_words, 0usize);
        let mut state = MaybeUninit::<RouteState>::uninit();
        unsafe {
            RouteState::init_empty(
                state.as_mut_ptr(),
                route_arm_storage.as_mut_ptr(),
                lane_offer_state_storage.as_mut_ptr(),
                scope_evidence_slots
                    .as_mut_ptr()
                    .cast::<ScopeEvidenceSlot>(),
                scope_selected_arms.as_mut_ptr(),
                lane_dense_by_lane.as_mut_ptr(),
                lanes,
                lane_route_arm_lens.as_mut_ptr(),
                lane_linger_counts.as_mut_ptr(),
                active_route_lane_words.as_mut_ptr(),
                lane_linger_words.as_mut_ptr(),
                lane_offer_linger_words.as_mut_ptr(),
                active_offer_lane_words.as_mut_ptr(),
                lanes,
                lane_words,
                lanes,
                route_depth,
                0,
                scope_count,
            );
        }
        RouteStateFixture {
            state: unsafe { state.assume_init() },
            _route_arm_storage: route_arm_storage,
            _lane_offer_state_storage: lane_offer_state_storage,
            _scope_evidence_slots: scope_evidence_slots,
            _scope_selected_arms: scope_selected_arms,
            _lane_dense_by_lane: lane_dense_by_lane,
            _lane_route_arm_lens: lane_route_arm_lens,
            _lane_linger_counts: lane_linger_counts,
            _active_route_lane_words: active_route_lane_words,
            _lane_linger_words: lane_linger_words,
            _lane_offer_linger_words: lane_offer_linger_words,
            _active_offer_lane_words: active_offer_lane_words,
        }
    }

    #[test]
    fn route_state_keeps_lane_255_addressable_in_full_lane_domain() {
        const LANES: usize = 256;
        let lane_words = lane_word_count(LANES);
        let mut lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal> = (0..LANES)
            .map(|lane| DenseLaneOrdinal::new(lane).expect("test lane dense ordinal"))
            .collect();
        let mut route_arm_storage = std::vec::Vec::with_capacity(LANES);
        route_arm_storage.resize(LANES, RouteArmState::EMPTY);
        let mut lane_offer_state_storage = std::vec::Vec::with_capacity(LANES);
        lane_offer_state_storage.resize(LANES, LaneOfferState::EMPTY);
        let mut scope_evidence_slots = std::vec::Vec::<MaybeUninit<ScopeEvidenceSlot>>::new();
        let mut scope_selected_arms = std::vec::Vec::with_capacity(1);
        scope_selected_arms.resize(1, RouteScopeSelectedArmSlot::EMPTY);
        let mut lane_route_arm_lens = std::vec::Vec::with_capacity(LANES);
        lane_route_arm_lens.resize(LANES, 0u8);
        let mut lane_linger_counts = std::vec::Vec::with_capacity(LANES);
        lane_linger_counts.resize(LANES, 0u8);
        let mut active_route_lane_words = std::vec::Vec::with_capacity(lane_words);
        active_route_lane_words.resize(lane_words, 0usize);
        let mut lane_linger_words = std::vec::Vec::with_capacity(lane_words);
        lane_linger_words.resize(lane_words, 0usize);
        let mut lane_offer_linger_words = std::vec::Vec::with_capacity(lane_words);
        lane_offer_linger_words.resize(lane_words, 0usize);
        let mut active_offer_lane_words = std::vec::Vec::with_capacity(lane_words);
        active_offer_lane_words.resize(lane_words, 0usize);
        let mut state = MaybeUninit::<RouteState>::uninit();
        unsafe {
            RouteState::init_empty(
                state.as_mut_ptr(),
                route_arm_storage.as_mut_ptr(),
                lane_offer_state_storage.as_mut_ptr(),
                scope_evidence_slots
                    .as_mut_ptr()
                    .cast::<ScopeEvidenceSlot>(),
                scope_selected_arms.as_mut_ptr(),
                lane_dense_by_lane.as_mut_ptr(),
                LANES,
                lane_route_arm_lens.as_mut_ptr(),
                lane_linger_counts.as_mut_ptr(),
                active_route_lane_words.as_mut_ptr(),
                lane_linger_words.as_mut_ptr(),
                lane_offer_linger_words.as_mut_ptr(),
                active_offer_lane_words.as_mut_ptr(),
                LANES,
                lane_words,
                LANES,
                1,
                0,
                1,
            );
        }
        let mut state = unsafe { state.assume_init() };
        let scope = ScopeId::route(1);

        assert_eq!(state.lane_route_arm_len(255), 0);
        let proof = state
            .preflight_route_arm_commit(255, scope, 0, 1, false)
            .expect("high lane route arm should preflight");
        state.commit_route_arm_after_preflight(proof);
        assert_eq!(state.lane_route_arm_len(255), 1);
        assert_eq!(state.route_arm_for(255, scope), Some(1));
        assert_eq!(state.selected_arm_for_scope_slot(0), Some(1));
        assert!(state.pop_route_arm(255, scope, 0, false));
        assert_eq!(state.lane_route_arm_len(255), 0);
        assert_eq!(state.selected_arm_for_scope_slot(0), None);
    }

    #[test]
    fn branch_commit_preflight_error_records_no_route_decisions() {
        let mut fixture = route_state_fixture(2, 1, 1);
        let state = &mut fixture.state;
        let scope = ScopeId::route(1);
        let proof = state
            .preflight_route_arm_commit(0, scope, 0, 0, false)
            .expect("first route arm should preflight");
        state.commit_route_arm_after_preflight(proof);
        assert_eq!(state.route_arm_for(0, scope), Some(0));
        assert_eq!(state.selected_arm_for_scope_slot(0), Some(0));

        assert!(
            state
                .preflight_route_arm_commit(1, scope, 0, 1, false)
                .is_none(),
            "conflicting arm must fail in preflight"
        );
        assert_eq!(state.route_arm_for(1, scope), None);
        assert_eq!(state.route_arm_for(0, scope), Some(0));
        assert_eq!(state.selected_arm_for_scope_slot(0), Some(0));
    }

    #[test]
    fn branch_commit_publish_is_infallible_after_preflight_and_preserves_refs() {
        let mut fixture = route_state_fixture(2, 2, 1);
        let state = &mut fixture.state;
        let scope = ScopeId::route(1);
        let first = state
            .preflight_route_arm_commit(0, scope, 0, 1, false)
            .expect("first route arm should preflight");
        state.commit_route_arm_after_preflight(first);
        let second = state
            .preflight_route_arm_commit(1, scope, 0, 1, false)
            .expect("same route arm should preflight");
        state.commit_route_arm_after_preflight(second);
        assert_eq!(state.route_arm_for(0, scope), Some(1));
        assert_eq!(state.route_arm_for(1, scope), Some(1));
        assert_eq!(state.selected_arm_for_scope_slot(0), Some(1));
        assert!(state.pop_route_arm(0, scope, 0, false));
        assert_eq!(
            state.selected_arm_for_scope_slot(0),
            Some(1),
            "selected arm remains while another lane still holds a ref"
        );
        assert!(state.pop_route_arm(1, scope, 0, false));
        assert_eq!(state.selected_arm_for_scope_slot(0), None);
    }

    #[test]
    fn route_commit_proof_workspace_accepts_more_than_64_route_scopes() {
        let mut storage = std::vec::Vec::new();
        storage.resize(71, RouteArmCommitProof::EMPTY);
        let mut workspace = MaybeUninit::<RouteCommitProofWorkspace>::uninit();
        unsafe {
            RouteCommitProofWorkspace::init(workspace.as_mut_ptr(), storage.as_mut_ptr(), 71);
        }
        let mut workspace = unsafe { workspace.assume_init() };
        let list = workspace
            .begin(66)
            .expect("route commit proof workspace derives from route scope count");

        assert_eq!(list.len(), 0);
    }

    #[test]
    fn decode_commit_proof_workspace_accepts_more_than_64_route_scopes() {
        let mut storage = std::vec::Vec::new();
        storage.resize(71, RouteArmCommitProof::EMPTY);
        let mut workspace = MaybeUninit::<RouteCommitProofWorkspace>::uninit();
        unsafe {
            RouteCommitProofWorkspace::init(workspace.as_mut_ptr(), storage.as_mut_ptr(), 71);
        }
        let mut workspace = unsafe { workspace.assume_init() };
        let list = workspace
            .begin(66)
            .expect("decode commit plan uses shared route-scope workspace");

        assert_eq!(list.len(), 0);
    }
}
