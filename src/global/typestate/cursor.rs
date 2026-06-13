//! Mutable runtime cursor logic for compact role-local execution.

use core::slice;

use super::facts::{
    ARM_SHARED, FirstRecvDispatchSpec, LocalAction, LocalDependency, LocalMeta, LocalNode,
    MAX_FIRST_RECV_DISPATCH, PackedEventConflict, PassiveArmChildFact, RecvMeta, RouteScopeRows,
    SendMeta, StateIndex, state_index_to_usize,
};
use crate::endpoint::kernel::FrontierScratchLayout;
use crate::{
    eff::EffIndex,
    global::{
        compiled::images::{EventSemanticKind, RoleDescriptorRef},
        const_dsl::{ResolverMode, ScopeId, ScopeKind},
        event_program::{LocalEventProgram, LocalEventRowSet},
        role_program::{LaneSetView, LaneSteps, PackedLaneRange, lane_word_count},
    },
};

mod first_recv_dispatch;
mod lane_progress;
mod navigation;
mod scope_route;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResidentLaneStep {
    step_idx: u16,
    lane: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RelocatableResidentLaneStep(ResidentLaneStep);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CursorInvariantError {
    _sealed: (),
}

impl CursorInvariantError {
    pub(crate) const INVARIANT: Self = Self { _sealed: () };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlowPreviewError {
    Invariant,
    LabelMismatch { expected: u8, actual: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EnabledEventCommit {
    progress_step: RelocatableResidentLaneStep,
    cursor_after: StateIndex,
}

impl EnabledEventCommit {
    #[inline(always)]
    pub(crate) const fn new(
        progress_step: RelocatableResidentLaneStep,
        cursor_after: StateIndex,
    ) -> Self {
        Self {
            progress_step,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn progress_step(self) -> RelocatableResidentLaneStep {
        self.progress_step
    }

    #[inline(always)]
    pub(crate) const fn cursor_after(self) -> StateIndex {
        self.cursor_after
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteOfferCursorState {
    scope_id: ScopeId,
    entry_idx: usize,
}

impl RouteOfferCursorState {
    #[inline(always)]
    pub(crate) const fn new(scope_id: ScopeId, entry_idx: usize) -> Self {
        Self {
            scope_id,
            entry_idx,
        }
    }

    #[inline(always)]
    pub(crate) const fn scope_id(self) -> ScopeId {
        self.scope_id
    }

    #[inline(always)]
    pub(crate) const fn entry_idx(self) -> usize {
        self.entry_idx
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorRefresh {
    Lane(u8),
    AllLanes,
}

// =============================================================================
// =============================================================================

const EVENT_CURSOR_NO_STATE: StateIndex = StateIndex::MAX;

#[derive(Debug)]
struct EventCursorMachine {
    role: u8,
    event_program: LocalEventProgram,
}

impl EventCursorMachine {
    #[inline(always)]
    unsafe fn init_from_event_rows(dst: *mut Self, role: u8, event_program: LocalEventProgram) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).role).write(role);
            core::ptr::addr_of_mut!((*dst).event_program).write(event_program);
        }
    }

    #[inline(always)]
    fn event_program(&self) -> LocalEventProgram {
        self.event_program
    }

    #[inline(always)]
    fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    fn program_ref(&self) -> &'static crate::global::compiled::images::CompiledProgramRef {
        self.event_program().program_ref()
    }

    #[inline(always)]
    fn resident_row_min_start(&self, idx: usize) -> Option<u16> {
        self.event_program().resident_row_min_start(idx)
    }

    #[inline(always)]
    fn resident_row_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        self.event_program().resident_row_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    fn resident_row_lane_step_at(
        &self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        self.event_program()
            .resident_row_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    fn resident_row_lane_step_ordinal(
        &self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        self.event_program()
            .resident_row_lane_step_ordinal(idx, lane_idx, step_idx)
    }

    #[inline(always)]
    fn local_steps_len(&self) -> usize {
        self.event_program().local_len()
    }

    #[inline(always)]
    fn node_len(&self) -> usize {
        self.event_program().node_len()
    }

    #[inline(always)]
    fn node(&self, idx: usize) -> LocalNode {
        self.event_program().node(idx)
    }

    #[inline(always)]
    fn checked_node(&self, idx: usize) -> Option<LocalNode> {
        self.event_program().checked_node(idx)
    }

    #[inline(always)]
    fn state_for_step_index(&self, step_idx: usize) -> Option<StateIndex> {
        self.event_program().state_for_step_index(step_idx)
    }

    #[inline(always)]
    fn route_scope_rows(&self, scope_id: ScopeId) -> Option<RouteScopeRows> {
        self.event_program().route_scope_rows(scope_id)
    }

    #[inline(always)]
    fn parallel_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        matches!(scope_id.kind(), ScopeKind::Parallel).then_some(scope_id)
    }

    #[inline(always)]
    fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        self.event_program().dependency_for_index(current_idx)
    }

    #[inline(always)]
    fn event_conflict_for_index(&self, current_idx: usize) -> PackedEventConflict {
        self.event_program().event_conflict_for_index(current_idx)
    }

    #[inline(always)]
    fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        self.event_program().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    fn route_commit_range_by_slot(&self, slot: usize, arm: u8) -> PackedLaneRange {
        self.event_program().route_commit_range_by_slot(slot, arm)
    }

    #[inline(always)]
    fn route_commit_row_at(&self, idx: usize) -> PackedEventConflict {
        self.event_program().route_commit_row_at(idx)
    }

    #[inline(always)]
    fn passive_arm_child_fact_by_slot(&self, slot: usize, arm: u8) -> Option<PassiveArmChildFact> {
        self.event_program()
            .passive_arm_child_fact_by_slot(slot, arm)
    }

    #[inline(always)]
    fn enclosing_loop(&self, scope_id: ScopeId) -> Option<ScopeId> {
        matches!(scope_id.kind(), ScopeKind::Loop).then_some(scope_id)
    }

    #[inline(always)]
    fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        FrontierScratchLayout::new(
            self.max_frontier_entries(),
            self.logical_lane_count(),
            lane_word_count(self.logical_lane_count()),
        )
    }

    #[inline(always)]
    fn max_frontier_entries(&self) -> usize {
        self.event_program().footprint().frontier_entry_count()
    }

    #[inline(always)]
    fn logical_lane_count(&self) -> usize {
        let footprint = self.event_program().footprint();
        footprint
            .logical_lane_count
            .max(footprint.endpoint_lane_slot_count.max(1))
    }

    #[inline(always)]
    fn route_scope_linger(&self, scope_id: ScopeId) -> bool {
        self.event_program().route_scope_linger(scope_id)
    }

    #[inline(always)]
    fn loop_scope_row(&self, scope_id: ScopeId) -> Option<LocalEventRowSet> {
        self.event_program().loop_scope_row(scope_id)
    }

    #[inline(always)]
    fn loop_scope_row_by_slot(&self, slot: usize) -> Option<(ScopeId, LocalEventRowSet)> {
        self.event_program().loop_scope_row_by_slot(slot)
    }

    #[inline(always)]
    fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        let row = self
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)?;
        let mut idx = row.start();
        while idx < row.end() {
            match self.node(idx).action() {
                LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Local { .. } => {
                    return Some(StateIndex::from_usize(idx));
                }
                LocalAction::Terminate => idx += 1,
            }
        }
        None
    }

    #[inline(always)]
    fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        let row = self
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)?;
        let mut idx = row.start();
        while idx < row.end() {
            if let LocalAction::Recv { .. } = self.node(idx).action() {
                return Some(StateIndex::from_usize(idx));
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    fn route_scope_offer_entry_by_slot(&self, slot: usize) -> Option<StateIndex> {
        let mut start = usize::MAX;
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self.event_program().route_arm_event_row_by_slot(slot, arm)
                && row.start() < start
            {
                start = row.start();
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        if start == usize::MAX {
            Some(StateIndex::MAX)
        } else {
            Some(StateIndex::from_usize(start))
        }
    }

    #[inline(always)]
    fn route_scope_dense_ordinal(&self, scope_id: ScopeId) -> Option<usize> {
        self.event_program().route_scope_slot(scope_id)
    }

    #[inline(always)]
    fn controller_arm_entry_for_label(&self, scope_id: ScopeId, label: u8) -> Option<StateIndex> {
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some((entry, entry_label)) = self.controller_arm_entry_by_arm(scope_id, arm)
                && entry_label == label
            {
                return Some(entry);
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    #[inline(always)]
    fn controller_arm_entry_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<(StateIndex, u8)> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        let row = self
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)?;
        let mut idx = row.start();
        while idx < row.end() {
            match self.node(idx).action() {
                LocalAction::Send { label, .. } | LocalAction::Local { label, .. } => {
                    return Some((StateIndex::from_usize(idx), label));
                }
                LocalAction::Recv { .. } | LocalAction::Terminate => idx += 1,
            }
        }
        None
    }

    #[inline(always)]
    fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        self.program_ref().route_controller_role(scope_id)
    }

    #[inline(always)]
    fn route_controller(&self, scope_id: ScopeId) -> Option<(ResolverMode, EffIndex, u8)> {
        self.program_ref().route_controller(scope_id)
    }
}

#[derive(Debug)]
pub(crate) struct EventCursorState {
    /// Primary typestate index used for scope queries.
    idx: u16,
    /// Cached resident-row locator for compact lane rows.
    resident_row_index: u8,
    /// Per-lane cursor within the cached resident row.
    /// Completion is tracked by `completed_event_words`, not by this locator.
    lane_cursors: *mut u16,
    /// Encoded current logical label for each lane's pending step.
    current_step_label_codes: *mut u16,
    /// Bitset of committed local event rows.
    completed_event_words: *mut u32,
}

const CURRENT_STEP_UNLABELED_CODE: u16 = u16::MAX;

impl EventCursorState {
    #[inline(always)]
    pub(crate) unsafe fn init_empty(
        dst: *mut Self,
        lane_cursors: *mut u16,
        current_step_label_codes: *mut u16,
        completed_event_words: *mut u32,
        logical_lane_count: usize,
        completed_event_word_count: usize,
    ) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).idx).write(0);
            core::ptr::addr_of_mut!((*dst).resident_row_index).write(0);
            core::ptr::addr_of_mut!((*dst).lane_cursors).write(lane_cursors);
            core::ptr::addr_of_mut!((*dst).current_step_label_codes)
                .write(current_step_label_codes);
            core::ptr::addr_of_mut!((*dst).completed_event_words).write(completed_event_words);
            let mut lane_idx = 0usize;
            while lane_idx < logical_lane_count {
                lane_cursors.add(lane_idx).write(0);
                current_step_label_codes
                    .add(lane_idx)
                    .write(CURRENT_STEP_UNLABELED_CODE);
                lane_idx += 1;
            }
            let mut word_idx = 0usize;
            while word_idx < completed_event_word_count {
                completed_event_words.add(word_idx).write(0);
                word_idx += 1;
            }
        }
    }
}

/// Cursor storage for role-local event progress.
///
/// The resident row index is a compact locator for compiled lane rows. It is
/// not a correctness barrier: joins and route-arm liveness are decided by
/// dependency/conflict facts before a commit delta is accepted.
#[derive(Debug)]
pub(crate) struct EventCursor {
    machine: EventCursorMachine,
    state: *mut EventCursorState,
}

impl EventCursor {
    #[inline(always)]
    const fn encode_index(idx: usize) -> u16 {
        if idx >= u16::MAX as usize {
            crate::invariant();
        }
        idx as u16
    }

    #[inline(always)]
    fn idx_usize(&self) -> usize {
        self.state().idx as usize
    }

    #[inline(always)]
    fn resident_row_index_usize(&self) -> usize {
        self.state().resident_row_index as usize
    }

    #[inline(always)]
    fn machine(&self) -> &EventCursorMachine {
        &self.machine
    }

    #[inline(always)]
    fn state(&self) -> &EventCursorState {
        if self.state.is_null() {
            crate::invariant();
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { &*self.state }
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut EventCursorState {
        if self.state.is_null() {
            crate::invariant();
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe { &mut *self.state }
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        self.machine().frontier_scratch_layout()
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.machine().max_frontier_entries()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.machine().logical_lane_count()
    }

    #[inline(always)]
    fn lane_cursors(&self) -> &[u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &[]
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts(self.state().lane_cursors, len) }
        }
    }

    #[inline(always)]
    fn lane_cursors_mut(&mut self) -> &mut [u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts_mut(self.state_mut().lane_cursors, len) }
        }
    }

    #[inline(always)]
    fn completed_event_word_count(&self) -> usize {
        completed_event_word_count(self.local_steps_len())
    }

    #[inline(always)]
    fn completed_event_words(&self) -> &[u32] {
        let len = self.completed_event_word_count();
        if len == 0 {
            &[]
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts(self.state().completed_event_words, len) }
        }
    }

    #[inline(always)]
    fn completed_event_words_mut(&mut self) -> &mut [u32] {
        let len = self.completed_event_word_count();
        if len == 0 {
            &mut []
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts_mut(self.state_mut().completed_event_words, len) }
        }
    }

    #[inline(always)]
    fn local_event_done(&self, step_idx: usize) -> bool {
        if step_idx >= self.local_steps_len() {
            return false;
        }
        let word_idx = step_idx / u32::BITS as usize;
        let bit = step_idx % u32::BITS as usize;
        let Some(word) = self.completed_event_words().get(word_idx) else {
            crate::invariant();
        };
        (word & (1u32 << bit)) != 0
    }

    #[inline(always)]
    fn clear_local_event_done(&mut self, step_idx: usize) {
        if step_idx >= self.local_steps_len() {
            return;
        }
        let word_idx = step_idx / u32::BITS as usize;
        let bit = step_idx % u32::BITS as usize;
        let Some(word) = self.completed_event_words_mut().get_mut(word_idx) else {
            crate::invariant();
        };
        *word &= !(1u32 << bit);
    }

    #[inline(always)]
    fn mark_local_event_done(&mut self, step_idx: usize) {
        if step_idx >= self.local_steps_len() {
            return;
        }
        let word_idx = step_idx / u32::BITS as usize;
        let bit = step_idx % u32::BITS as usize;
        let Some(word) = self.completed_event_words_mut().get_mut(word_idx) else {
            crate::invariant();
        };
        *word |= 1u32 << bit;
    }

    #[inline(always)]
    const fn encode_current_step_label(label: u8) -> u16 {
        label as u16
    }

    #[inline(always)]
    fn current_step_label_codes(&self) -> &[u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &[]
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts(self.state().current_step_label_codes, len) }
        }
    }

    #[inline(always)]
    fn current_step_label_codes_mut(&mut self) -> &mut [u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe { slice::from_raw_parts_mut(self.state_mut().current_step_label_codes, len) }
        }
    }

    #[inline(always)]
    pub(crate) fn local_steps_len(&self) -> usize {
        self.machine().local_steps_len()
    }

    // =========================================================================
    // Construction
    // =========================================================================

    #[inline(never)]
    pub(crate) unsafe fn init_from_compiled(
        dst: *mut Self,
        state: *mut EventCursorState,
        lane_cursors: *mut u16,
        current_step_label_codes: *mut u16,
        completed_event_words: *mut u32,
        role_descriptor: RoleDescriptorRef,
    ) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).state).write(state);
            EventCursorMachine::init_from_event_rows(
                core::ptr::addr_of_mut!((*dst).machine),
                role_descriptor.role(),
                LocalEventProgram::from_rows(role_descriptor.local_event_rows()),
            );
            EventCursorState::init_empty(
                state,
                lane_cursors,
                current_step_label_codes,
                completed_event_words,
                role_descriptor.logical_lane_count(),
                completed_event_word_count(role_descriptor.local_len()),
            );
            (&mut *dst).rebuild_current_step_label_codes();
        }
    }

    // =========================================================================
    // =========================================================================

    #[inline(always)]
    fn current_resident_row_lane_steps(&self, lane_idx: usize) -> Option<LaneSteps> {
        self.machine()
            .resident_row_lane_steps(self.resident_row_index_usize(), lane_idx)
    }

    #[inline(always)]
    fn current_resident_row_lane_step_at(&self, lane_idx: usize, ordinal: usize) -> Option<usize> {
        self.machine()
            .resident_row_lane_step_at(self.resident_row_index_usize(), lane_idx, ordinal)
            .map(usize::from)
    }

    // =========================================================================
    // Lane Access
    // =========================================================================

    fn resolved_label_for_lane(&self, lane_idx: usize) -> Option<u8> {
        let state_idx = self.step_state_index_at_lane(lane_idx)?;
        let node = self.machine().node(state_index_to_usize(state_idx));
        match node.action() {
            LocalAction::Send { label, .. }
            | LocalAction::Recv { label, .. }
            | LocalAction::Local { label, .. } => Some(label),
            LocalAction::Terminate => None,
        }
    }

    fn rebuild_current_step_label_codes(&mut self) {
        self.current_step_label_codes_mut()
            .fill(CURRENT_STEP_UNLABELED_CODE);
        let lane_limit = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            let label = self.resolved_label_for_lane(lane_idx);
            if let Some(label) = label {
                self.current_step_label_codes_mut()[lane_idx] =
                    Self::encode_current_step_label(label);
            }
            lane_idx += 1;
        }
    }

    fn refresh_current_step_label_code(&mut self, lane_idx: usize) {
        let label = self.resolved_label_for_lane(lane_idx);
        if let Some(label) = label {
            self.current_step_label_codes_mut()[lane_idx] = Self::encode_current_step_label(label);
        } else {
            self.current_step_label_codes_mut()[lane_idx] = CURRENT_STEP_UNLABELED_CODE;
        }
    }

    // =========================================================================
}

#[inline(always)]
const fn completed_event_word_count(bits: usize) -> usize {
    let pad = u32::BITS as usize - 1;
    if bits > usize::MAX - pad {
        crate::invariant();
    }
    (bits + pad) / u32::BITS as usize
}
