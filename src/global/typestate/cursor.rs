//! Mutable runtime cursor logic for compact role-local execution.

use core::slice;

use super::facts::{
    ARM_SHARED, FirstRecvDispatchSpec, JumpError, JumpReason, LocalAction, LocalDependency,
    LocalMeta, LocalNode, MAX_FIRST_RECV_DISPATCH, PackedEventConflict, PassiveArmNavigation,
    RecvMeta, RecvlessParentRouteDecision, RouteScopeRegion, ScopeRegion, SendMeta, StateIndex,
    as_state_index, state_index_to_usize,
};
use crate::endpoint::kernel::FrontierScratchLayout;
use crate::{
    eff::EffIndex,
    global::{
        LoopControlMeaning,
        compiled::images::{ControlSemanticKind, ControlSemanticsTable, RoleDescriptorRef},
        const_dsl::{PolicyMode, ScopeId, ScopeKind},
        event_program::LocalEventProgram,
        role_program::{LaneSetView, LaneSteps},
    },
};

mod lane_progress;
mod navigation;
mod scope_route;

/// Role perspective for a loop decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopRole {
    Controller,
    Target,
}

/// Metadata associated with a loop decision site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopMetadata {
    pub scope: ScopeId,
    pub controller: u8,
    pub target: u8,
    pub role: LoopRole,
    pub eff_index: EffIndex,
    pub decision_index: StateIndex,
    pub continue_index: StateIndex,
    pub break_index: StateIndex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResidentLaneStep {
    step_idx: u16,
    lane: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RelocatableResidentLaneStep(ResidentLaneStep);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResidentLaneStepError;

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
#[cfg_attr(test, derive(Clone, Copy, PartialEq, Eq))]
struct EventCursorMachine {
    descriptor: RoleDescriptorRef,
    event_program: LocalEventProgram,
}

impl EventCursorMachine {
    #[inline(always)]
    unsafe fn init_from_descriptor(dst: *mut Self, role_descriptor: RoleDescriptorRef) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).descriptor).write(role_descriptor);
            core::ptr::addr_of_mut!((*dst).event_program)
                .write(LocalEventProgram::from_descriptor(role_descriptor));
        }
    }

    #[inline(always)]
    fn event_program(&self) -> LocalEventProgram {
        self.event_program
    }

    #[inline(always)]
    fn descriptor(&self) -> RoleDescriptorRef {
        self.descriptor
    }

    #[inline(always)]
    fn role(&self) -> u8 {
        self.descriptor().role()
    }

    #[inline(always)]
    fn program_ref(&self) -> crate::global::compiled::images::CompiledProgramRef {
        self.descriptor().program()
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
        self.descriptor().local_len()
    }

    #[inline(always)]
    fn node_len(&self) -> usize {
        self.descriptor().node_len()
    }

    #[inline(always)]
    fn node(&self, idx: usize) -> LocalNode {
        self.descriptor().node(idx)
    }

    #[inline(always)]
    fn checked_node(&self, idx: usize) -> Option<LocalNode> {
        self.descriptor().checked_node(idx)
    }

    #[inline(always)]
    fn state_for_step_index(&self, step_idx: usize) -> Option<StateIndex> {
        self.descriptor().state_for_step_index(step_idx)
    }

    #[inline(always)]
    fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.descriptor().scope_region_by_id(scope_id)
    }

    #[inline(always)]
    fn route_scope_for_selected_child_arm(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        self.descriptor()
            .route_scope_for_selected_child_arm(scope_id, arm)
    }

    #[inline(always)]
    fn parallel_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.descriptor().parallel_root(scope_id)
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
    fn enclosing_loop(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.descriptor().enclosing_loop(scope_id)
    }

    #[inline(always)]
    fn control_semantics(&self) -> &ControlSemanticsTable {
        self.program_ref().control_semantics()
    }

    #[inline(always)]
    fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        self.descriptor().frontier_scratch_layout()
    }

    #[inline(always)]
    fn max_frontier_entries(&self) -> usize {
        self.descriptor().max_frontier_entries()
    }

    #[inline(always)]
    fn logical_lane_count(&self) -> usize {
        self.descriptor().logical_lane_count()
    }

    #[inline(always)]
    fn route_scope_linger(&self, scope_id: ScopeId) -> bool {
        self.descriptor().route_scope_linger(scope_id)
    }

    #[inline(always)]
    fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.descriptor().passive_arm_entry(scope_id, arm)
    }

    #[inline(always)]
    fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.descriptor().route_recv_state(scope_id, arm)
    }

    #[inline(always)]
    fn route_scope_offer_entry_by_slot(&self, slot: usize) -> Option<StateIndex> {
        self.descriptor().route_scope_offer_entry_by_slot(slot)
    }

    #[inline(always)]
    fn route_scope_dense_ordinal(&self, scope_id: ScopeId) -> Option<usize> {
        self.descriptor().route_scope_dense_ordinal(scope_id)
    }

    #[inline(always)]
    fn first_recv_dispatch_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.descriptor()
            .first_recv_dispatch_target_for_lane_frame_label(scope_id, lane, frame_label)
    }

    #[inline(always)]
    fn first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        self.descriptor().first_recv_dispatch_table(scope_id)
    }

    #[inline(always)]
    fn controller_arm_entry_for_label(&self, scope_id: ScopeId, label: u8) -> Option<StateIndex> {
        self.descriptor()
            .controller_arm_entry_for_label(scope_id, label)
    }

    #[inline(always)]
    fn controller_arm_entry_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<(StateIndex, u8)> {
        self.descriptor().controller_arm_entry_by_arm(scope_id, arm)
    }

    #[inline(always)]
    fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        self.program_ref().route_controller_role(scope_id)
    }

    #[inline(always)]
    fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(
        PolicyMode,
        EffIndex,
        u8,
        crate::control::cap::mint::ControlOp,
    )> {
        self.program_ref().route_controller(scope_id)
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, Copy, PartialEq, Eq))]
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
#[cfg_attr(test, derive(Clone))]
pub(crate) struct EventCursor {
    machine: EventCursorMachine,
    state: *mut EventCursorState,
}

impl EventCursor {
    #[inline(always)]
    const fn encode_index(idx: usize) -> u16 {
        debug_assert!(idx < u16::MAX as usize);
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
        debug_assert!(!self.state.is_null());
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { &*self.state }
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut EventCursorState {
        debug_assert!(!self.state.is_null());
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe { &mut *self.state }
    }

    #[inline(always)]
    pub(crate) fn control_semantics(&self) -> ControlSemanticsTable {
        *self.machine().control_semantics()
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
        self.completed_event_words()
            .get(word_idx)
            .map(|word| (word & (1u32 << bit)) != 0)
            .unwrap_or(false)
    }

    #[inline(always)]
    fn mark_local_event_done(&mut self, step_idx: usize) {
        if step_idx >= self.local_steps_len() {
            return;
        }
        let word_idx = step_idx / u32::BITS as usize;
        let bit = step_idx % u32::BITS as usize;
        if let Some(word) = self.completed_event_words_mut().get_mut(word_idx) {
            *word |= 1u32 << bit;
        }
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
    fn checked_typestate_node(
        &self,
        idx: StateIndex,
        iterations: u32,
    ) -> Result<LocalNode, JumpError> {
        if idx.is_max() {
            return Err(JumpError {
                iterations,
                idx: state_index_to_usize(idx),
            });
        }
        let raw = state_index_to_usize(idx);
        self.machine().checked_node(raw).ok_or(JumpError {
            iterations,
            idx: raw,
        })
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
            EventCursorMachine::init_from_descriptor(
                core::ptr::addr_of_mut!((*dst).machine),
                role_descriptor,
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
    bits.saturating_add(u32::BITS as usize - 1) / u32::BITS as usize
}
