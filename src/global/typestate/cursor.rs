//! Mutable runtime cursor logic for compact role-local execution.

use core::slice;

use super::facts::{
    ARM_SHARED, InboundFrameKey, LocalDependency, LocalMeta, PackedEventConflict,
    PassiveArmChildFact, RecvMeta, RouteScopeRows, SendMeta, StateIndex, state_index_to_usize,
};
#[cfg(all(test, hibana_repo_tests))]
use super::facts::{LocalAction, LocalNode};
use crate::runtime_core::layout::u32_word_count;
use crate::{
    eff::EffIndex,
    global::{
        compiled::images::RoleDescriptorRef,
        const_dsl::{DynamicRouteResolver, ScopeId},
        event_program::{LocalEventProgram, LocalEventRowSet},
        role_program::{LaneSetView, PackedLaneRange},
    },
};

mod first_recv_dispatch;
#[cfg(kani)]
mod kani;
mod lane_progress;
mod navigation;
#[cfg(any(kani, all(test, hibana_repo_tests)))]
mod parallel_reentry_tests;
mod pending_events;
mod receive_handoff;
mod scope_route;
#[cfg(all(test, hibana_repo_tests))]
mod validated_metadata_tests;
pub(crate) use scope_route::{EventAdmissionError, EventArmView, EventBlocker};

#[cfg(all(test, hibana_repo_tests))]
mod compact_cursor_position_tests {
    use super::compact_cursor_position;

    #[test]
    fn terminal_cursor_position_uses_the_full_u16_value_domain() {
        assert_eq!(compact_cursor_position(0), Some(0));
        assert_eq!(compact_cursor_position(u16::MAX as usize), Some(u16::MAX));
        assert_eq!(compact_cursor_position(u16::MAX as usize + 1), None);
    }
}

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
pub(crate) enum SendPreviewError {
    Invariant,
    OutOfOrder,
    OtherViewPending,
    LabelMismatch {
        expected: u8,
        actual: u8,
    },
    PayloadMismatch {
        expected: crate::eff::EventPayload,
        actual: crate::eff::EventPayload,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EnabledEventCommit {
    event_index: StateIndex,
    progress_step: RelocatableResidentLaneStep,
    cursor_after: StateIndex,
}

impl EnabledEventCommit {
    #[inline(always)]
    const fn new(
        event_index: StateIndex,
        progress_step: RelocatableResidentLaneStep,
        cursor_after: StateIndex,
    ) -> Self {
        Self {
            event_index,
            progress_step,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn event_index(self) -> StateIndex {
        self.event_index
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

// =============================================================================
// =============================================================================

const EVENT_CURSOR_STATE_NONE: StateIndex = StateIndex::ABSENT;

#[derive(Debug)]
struct EventCursorMachine {
    role: u8,
    event_program: LocalEventProgram,
}

#[cfg(hibana_profile)]
use crate::global::event_program::work;

impl EventCursorMachine {
    #[inline(always)]
    unsafe fn init_from_event_rows(dst: *mut Self, role: u8, event_program: LocalEventProgram) {
        /* SAFETY: `EventCursor::init_from_compiled` passes an unpublished
        cursor-machine field; role id and local event-program slice are written
        before cursor methods can read the machine. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).role).write(role);
            core::ptr::addr_of_mut!((*dst).event_program).write(event_program);
        }
    }

    #[inline(always)]
    fn event_program(&self) -> &LocalEventProgram {
        &self.event_program
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
    fn local_steps_len(&self) -> usize {
        self.event_program().local_len()
    }

    #[inline(always)]
    fn route_chain_bound(&self) -> usize {
        crate::invariant_some(
            self.event_program()
                .footprint()
                .route_scope_count
                .checked_add(1),
        )
    }

    #[inline(always)]
    fn node_len(&self) -> usize {
        self.event_program().node_len()
    }

    #[cfg(all(test, hibana_repo_tests))]
    fn node(&self, idx: usize) -> LocalNode {
        self.event_program().node(idx)
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
    fn route_scope_rows_by_slot(&self, slot: usize) -> Option<RouteScopeRows> {
        self.event_program().route_scope_rows_by_slot(slot)
    }

    #[inline(always)]
    fn route_scope_slot_count(&self) -> usize {
        self.event_program().route_scope_slot_count()
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

    fn route_commit_bytes(&self, range: PackedLaneRange) -> &'static [u8] {
        self.event_program().route_commit_bytes(range)
    }

    #[inline(always)]
    fn passive_arm_child_fact_by_slot(&self, slot: usize, arm: u8) -> Option<PassiveArmChildFact> {
        self.event_program()
            .passive_arm_child_fact_by_slot(slot, arm)
    }

    #[inline(always)]
    fn logical_lane_count(&self) -> usize {
        self.event_program().footprint().logical_lane_count
    }

    #[inline(always)]
    fn route_scope_reentry(&self, scope_id: ScopeId) -> bool {
        self.event_program().route_scope_reentry(scope_id)
    }

    #[inline(always)]
    fn has_reentry_scopes(&self) -> bool {
        self.event_program().has_reentry_scopes()
    }

    #[inline(always)]
    fn roll_scope_row(&self, scope_id: ScopeId) -> Option<LocalEventRowSet> {
        self.event_program().roll_scope_row(scope_id)
    }

    #[inline(always)]
    fn roll_scope_row_by_slot(&self, slot: usize) -> Option<(ScopeId, LocalEventRowSet)> {
        self.event_program().roll_scope_row_by_slot(slot)
    }

    #[inline(always)]
    fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        let row = self
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)?;
        (row.start() < row.end()).then(|| StateIndex::from_usize(row.start()))
    }

    #[inline(always)]
    fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        self.event_program()
            .route_arm_first_operation(slot, arm, true)
            .map(StateIndex::from_usize)
    }

    #[inline(always)]
    fn route_scope_offer_entry_by_slot(&self, slot: usize) -> StateIndex {
        let mut start: Option<usize> = None;
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self.event_program().route_arm_event_row_by_slot(slot, arm)
                && row.start() < row.end()
            {
                start = Some(match start {
                    Some(current) => current.min(row.start()),
                    None => row.start(),
                });
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        match start {
            Some(start) => StateIndex::from_usize(start),
            None => StateIndex::ABSENT,
        }
    }

    #[inline(always)]
    fn route_scope_dense_ordinal(&self, scope_id: ScopeId) -> Option<usize> {
        self.event_program().route_scope_slot(scope_id)
    }

    #[inline(always)]
    fn controller_arm_entry_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<(StateIndex, u8)> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        let idx = self
            .event_program()
            .route_arm_first_operation(slot, arm, false)?;
        let event = crate::invariant_some(self.event_program().event_row_at(idx));
        Some((StateIndex::from_usize(idx), event.atom().label))
    }

    #[inline(always)]
    fn route_controller_role(&self, scope_id: ScopeId) -> u8 {
        self.program_ref().route_controller_role(scope_id)
    }

    #[inline(always)]
    fn route_resolver(&self, scope_id: ScopeId) -> Option<DynamicRouteResolver> {
        self.program_ref().route_resolver(scope_id)
    }
}

#[derive(Debug)]
pub(crate) struct EventCursorState {
    /// Lookup context used for scope queries; it does not position lane heads.
    idx: u16,
    /// Per-lane descriptor event index; the whole image's end means no head.
    /// Completion is tracked by `completed_event_words`, not by this locator.
    lane_cursors: *mut u16,
    /// Encoded current logical label for each lane's pending step.
    current_step_label_codes: *mut u16,
    /// Current event completions followed by one retained completion per roll.
    completed_event_words: *mut u32,
    /// Qualification-only observation. It never authorizes or schedules an
    /// operation and is absent from production storage and generated images.
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    completion_mutations: u64,
}

const CURRENT_STEP_UNLABELED_CODE: u16 = u16::MAX;

#[inline(always)]
const fn compact_cursor_position(position: usize) -> Option<u16> {
    if position <= u16::MAX as usize {
        Some(position as u16)
    } else {
        None
    }
}

#[inline(always)]
const fn local_event_bit(step_idx: usize, local_step_count: usize) -> Option<(usize, u32)> {
    if step_idx >= local_step_count {
        None
    } else {
        Some((
            step_idx / u32::BITS as usize,
            1u32 << (step_idx % u32::BITS as usize),
        ))
    }
}

impl EventCursorState {
    pub(crate) unsafe fn init_empty(
        dst: *mut Self,
        lane_cursors: *mut u16,
        current_step_label_codes: *mut u16,
        completed_event_words: *mut u32,
        logical_lane_count: usize,
        completed_event_word_count: usize,
    ) {
        /* SAFETY: endpoint cursor initialization passes an unpublished
        `EventCursorState` plus three resident columns. Lane cursor and current
        label columns are initialized for `logical_lane_count`; completed-event
        words are initialized for the computed word count before publication. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).idx).write(0);
            core::ptr::addr_of_mut!((*dst).lane_cursors).write(lane_cursors);
            core::ptr::addr_of_mut!((*dst).current_step_label_codes)
                .write(current_step_label_codes);
            core::ptr::addr_of_mut!((*dst).completed_event_words).write(completed_event_words);
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            core::ptr::addr_of_mut!((*dst).completion_mutations).write(0);
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
/// Each lane has an independent locator into the whole compiled image. Joins
/// and route-arm liveness are decided by dependency/conflict facts before a
/// commit delta is accepted, independently of these lookup positions.
#[derive(Debug)]
pub(crate) struct EventCursor {
    machine: EventCursorMachine,
    state: *mut EventCursorState,
}

impl EventCursor {
    #[cfg(all(hibana_profile, test, hibana_repo_tests))]
    pub(crate) fn work_counts(&self) -> [(&'static str, u64); work::NAMES.len()] {
        self.machine.event_program.work.report()
    }

    #[inline(always)]
    pub(crate) fn role(&self) -> u8 {
        self.machine().role()
    }

    #[inline(always)]
    const fn encode_index(idx: usize) -> u16 {
        match compact_cursor_position(idx) {
            Some(encoded) => encoded,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    fn machine(&self) -> &EventCursorMachine {
        &self.machine
    }

    #[inline(always)]
    pub(crate) fn program_ref(
        &self,
    ) -> &'static crate::global::compiled::images::CompiledProgramRef {
        self.machine().program_ref()
    }

    pub(crate) fn parallel_borrow_mask(&self, view: u8, start: usize, stop: usize) -> Option<u16> {
        self.machine()
            .event_program()
            .parallel_borrow_mask(view, start, stop)
    }

    pub(crate) fn offer_candidate_lanes(&self, borrow_mask: Option<u16>) -> LaneSetView<'static> {
        self.machine()
            .event_program()
            .offer_candidate_lanes(borrow_mask)
    }

    #[inline]
    pub(crate) fn parallel_borrow_owns_lane(&self, mask: u16, lane: usize) -> bool {
        self.machine()
            .event_program()
            .parallel_borrow_owns_lane(mask, lane)
    }

    #[inline(always)]
    fn state(&self) -> &EventCursorState {
        if self.state.is_null() {
            crate::invariant();
        }
        /* SAFETY: `self.state` is the cursor-state section installed from the
        endpoint arena during cursor initialization; shared access is tied to
        `&self`. */
        unsafe { &*self.state }
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut EventCursorState {
        if self.state.is_null() {
            crate::invariant();
        }
        /* SAFETY: `&mut self` is the cursor mutation token, so this is the only
        mutable borrow of the resident cursor-state section for the operation. */
        unsafe { &mut *self.state }
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.machine().logical_lane_count()
    }

    #[inline(always)]
    fn has_reentry_scopes(&self) -> bool {
        self.machine().has_reentry_scopes()
    }

    #[inline(always)]
    fn lane_cursors(&self) -> &[u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &[]
        } else {
            /* SAFETY: `lane_cursors` was initialized with one u16 per logical
            lane in this cursor state, and `len` comes from the same role image. */
            unsafe { slice::from_raw_parts(self.state().lane_cursors, len) }
        }
    }

    #[inline(always)]
    fn lane_cursors_mut(&mut self) -> &mut [u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
            /* SAFETY: `lane_cursors` has one initialized u16 per logical lane,
            and `&mut self` owns mutable cursor progress for that slice. */
            unsafe { slice::from_raw_parts_mut(self.state_mut().lane_cursors, len) }
        }
    }

    #[inline(always)]
    fn completed_event_word_count(&self) -> usize {
        u32_word_count(
            self.machine()
                .event_program()
                .footprint()
                .completion_bit_count(),
        )
    }

    #[inline(always)]
    fn completed_event_words(&self) -> &[u32] {
        let len = self.completed_event_word_count();
        if len == 0 {
            &[]
        } else {
            /* SAFETY: `completed_event_words` was initialized with the word
            count derived from this cursor's event and roll rows. */
            unsafe { slice::from_raw_parts(self.state().completed_event_words, len) }
        }
    }

    #[inline(always)]
    fn completed_event_words_mut(&mut self) -> &mut [u32] {
        let len = self.completed_event_word_count();
        if len == 0 {
            &mut []
        } else {
            /* SAFETY: `completed_event_words` has the initialized word count
            derived from the event and roll rows, and `&mut self` owns updates to
            that bitset. */
            unsafe { slice::from_raw_parts_mut(self.state_mut().completed_event_words, len) }
        }
    }

    #[inline(always)]
    fn local_event_words(&self) -> &[u32] {
        let len = u32_word_count(self.local_steps_len());
        if len == 0 {
            &[]
        } else {
            // SAFETY: the initialized completion allocation covers every local
            // event bit. Retained roll bits and epochs are outside the query's
            // index range. The shared slice stays within this cursor borrow.
            unsafe { slice::from_raw_parts(self.state().completed_event_words, len) }
        }
    }

    #[inline(always)]
    fn local_event_done(&self, step_idx: usize) -> bool {
        let Some((word_idx, bit)) = local_event_bit(step_idx, self.local_steps_len()) else {
            crate::invariant();
        };
        let Some(word) = self.local_event_words().get(word_idx) else {
            crate::invariant();
        };
        (word & bit) != 0
    }

    #[inline(always)]
    fn clear_local_event_done(&mut self, step_idx: usize) {
        if step_idx >= self.local_steps_len() {
            crate::invariant();
        }
        self.write_completion_bit(step_idx, false);
    }

    #[inline(always)]
    fn mark_local_event_done(&mut self, step_idx: usize) {
        if step_idx >= self.local_steps_len() {
            crate::invariant();
        }
        self.write_completion_bit(step_idx, true);
    }

    #[inline(always)]
    fn write_completion_bit(&mut self, index: usize, completed: bool) {
        #[cfg(any(kani, all(test, hibana_repo_tests)))]
        self.record_completion_mutation();
        let count = self
            .machine()
            .event_program()
            .footprint()
            .completion_bit_count();
        let Some((word_idx, bit)) = local_event_bit(index, count) else {
            crate::invariant();
        };
        let Some(word) = self.completed_event_words_mut().get_mut(word_idx) else {
            crate::invariant();
        };
        if completed {
            *word |= bit;
        } else {
            *word &= !bit;
        }
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    fn record_completion_mutation(&mut self) {
        self.state_mut().completion_mutations = self
            .state()
            .completion_mutations
            .checked_add(1)
            .expect("qualification mutation count exhausted");
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) fn completion_mutations(&self) -> u64 {
        self.state().completion_mutations
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
            /* SAFETY: `current_step_label_codes` was initialized with one u16
            per logical lane in this cursor state. */
            unsafe { slice::from_raw_parts(self.state().current_step_label_codes, len) }
        }
    }

    #[inline(always)]
    fn current_step_label_codes_mut(&mut self) -> &mut [u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
            /* SAFETY: `current_step_label_codes` has one initialized u16 per
            logical lane, and `&mut self` owns current-step label updates. */
            unsafe { slice::from_raw_parts_mut(self.state_mut().current_step_label_codes, len) }
        }
    }

    #[inline(always)]
    pub(crate) fn local_steps_len(&self) -> usize {
        self.machine().local_steps_len()
    }

    #[inline(always)]
    pub(crate) fn route_chain_bound(&self) -> usize {
        self.machine().route_chain_bound()
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
        /* SAFETY: endpoint initialization passes an unpublished `EventCursor`
        field and disjoint cursor backing columns from the endpoint arena.
        Machine and state are fully initialized before the cursor is exposed. */
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
                u32_word_count(
                    role_descriptor
                        .local_event_rows()
                        .footprint()
                        .completion_bit_count(),
                ),
            );
            (&mut *dst).seed_lane_heads();
            (&mut *dst).rebuild_current_step_label_codes();
        }
    }

    // =========================================================================
    // =========================================================================

    // =========================================================================
    // Lane Access
    // =========================================================================

    fn resolved_label_for_lane(&self, lane_idx: usize) -> Option<u8> {
        let state_idx = self.step_state_index_at_lane(lane_idx)?;
        self.event_row_at(state_index_to_usize(state_idx))
            .map(|row| row.atom().label)
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
