//! Mutable phase and runtime cursor logic for typestate execution.

use core::slice;

use super::{
    builder::{RoleTypestateValue, ScopeRegion},
    facts::{
        JumpError, JumpReason, LocalAction, LocalMeta, LocalNode, PassiveArmNavigation, RecvMeta,
        SendMeta, StateIndex, as_state_index, state_index_to_usize, try_local_meta_value,
        try_recv_meta_value, try_send_meta_value,
    },
};
use crate::endpoint::kernel::FrontierScratchLayout;
use crate::{
    eff::EffIndex,
    global::{
        LoopControlMeaning,
        compiled::{CompiledRoleImage, ControlSemanticsTable, ProgramImage},
        const_dsl::{PolicyMode, ScopeId, ScopeKind},
        role_program::{LaneSetView, LaneSteps, PhaseRouteGuard},
    },
};

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

// =============================================================================
// =============================================================================

/// Maximum phases and steps that PhaseCursor can hold.
const PHASE_CURSOR_MAX_STEPS: usize = crate::eff::meta::MAX_EFF_NODES;
const PHASE_CURSOR_NO_STEP: u16 = u16::MAX;
const PHASE_CURSOR_NO_STATE: StateIndex = StateIndex::MAX;

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, Copy, PartialEq, Eq))]
struct PhaseCursorMachine {
    compiled_role: *const CompiledRoleImage,
    program_image: ProgramImage,
}

impl PhaseCursorMachine {
    #[inline(always)]
    unsafe fn init_from_compiled(
        dst: *mut Self,
        compiled_role: *const CompiledRoleImage,
        program_image: ProgramImage,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).compiled_role).write(compiled_role);
            core::ptr::addr_of_mut!((*dst).program_image).write(program_image);
        }
    }

    #[inline(always)]
    fn compiled_role(&self) -> &CompiledRoleImage {
        debug_assert!(!self.compiled_role.is_null());
        unsafe { &*self.compiled_role }
    }

    #[inline(always)]
    fn role(&self) -> u8 {
        self.compiled_role().role()
    }

    #[inline(always)]
    fn program_image(&self) -> &ProgramImage {
        &self.program_image
    }

    #[inline(always)]
    fn phase_lane_set(&self, idx: usize) -> Option<LaneSetView> {
        self.compiled_role().phase_lane_set(idx)
    }

    #[inline(always)]
    fn phase_min_start(&self, idx: usize) -> Option<u16> {
        self.compiled_role().phase_min_start(idx)
    }

    #[inline(always)]
    fn phase_route_guard(&self, idx: usize) -> Option<PhaseRouteGuard> {
        self.compiled_role().phase_route_guard(idx)
    }

    #[inline(always)]
    fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        self.compiled_role().phase_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    fn local_steps_len(&self) -> usize {
        self.compiled_role().local_len()
    }

    #[inline(always)]
    fn typestate(&self) -> &RoleTypestateValue {
        self.compiled_role().typestate_ref()
    }

    #[inline(always)]
    fn eff_index_to_step(&self) -> &[u16] {
        self.compiled_role().eff_index_to_step()
    }

    #[inline(always)]
    fn step_index_to_state(&self) -> &[StateIndex] {
        self.compiled_role().step_index_to_state()
    }

    #[inline(always)]
    fn control_semantics(&self) -> &ControlSemanticsTable {
        self.program_image().control_semantics()
    }

    #[inline(always)]
    fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        self.program_image().route_controller_role(scope_id)
    }

    #[inline(always)]
    fn route_controller(&self, scope_id: ScopeId) -> Option<(PolicyMode, EffIndex, u8)> {
        self.program_image().route_controller(scope_id)
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, Copy, PartialEq, Eq))]
pub(crate) struct PhaseCursorState {
    /// Primary typestate index used for scope queries.
    idx: u16,
    /// Current phase index (0-based)
    phase_index: u8,
    /// Per-lane step progress within current phase.
    /// `lane_cursors[lane_idx]` = number of steps completed on that lane.
    lane_cursors: *mut u16,
    /// Current label for each lane's pending step.
    current_step_labels: *mut u8,
}

impl PhaseCursorState {
    #[inline(always)]
    pub(crate) unsafe fn init_empty(
        dst: *mut Self,
        lane_cursors: *mut u16,
        current_step_labels: *mut u8,
        logical_lane_count: usize,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).idx).write(0);
            core::ptr::addr_of_mut!((*dst).phase_index).write(0);
            core::ptr::addr_of_mut!((*dst).lane_cursors).write(lane_cursors);
            core::ptr::addr_of_mut!((*dst).current_step_labels).write(current_step_labels);
            let mut lane_idx = 0usize;
            while lane_idx < logical_lane_count {
                lane_cursors.add(lane_idx).write(0);
                current_step_labels.add(lane_idx).write(0);
                lane_idx += 1;
            }
        }
    }
}

/// Phase-aware cursor for multi-lane parallel execution.
///
/// Provides explicit phase/lane tracking for typestate navigation. Each phase represents
/// a fork-join barrier; lanes within a phase execute independently. All lanes must
/// complete before advancing to the next phase.
///
/// # Design Philosophy
///
/// What is expressed in types must be realized at runtime.
///
/// `PhaseCursor` ensures that the parallel structure expressed by `g::par` in the
/// choreography is faithfully represented at runtime, with independent lane cursors
/// and proper barrier semantics.
#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub(crate) struct PhaseCursor {
    machine: PhaseCursorMachine,
    state: *mut PhaseCursorState,
}

impl PhaseCursor {
    #[inline(always)]
    const fn encode_index(idx: usize) -> u16 {
        debug_assert!(idx < PHASE_CURSOR_MAX_STEPS);
        idx as u16
    }

    #[inline(always)]
    fn idx_usize(&self) -> usize {
        self.state().idx as usize
    }

    #[inline(always)]
    fn phase_index_usize(&self) -> usize {
        self.state().phase_index as usize
    }

    #[inline(always)]
    fn machine(&self) -> &PhaseCursorMachine {
        &self.machine
    }

    #[inline(always)]
    fn state(&self) -> &PhaseCursorState {
        debug_assert!(!self.state.is_null());
        unsafe { &*self.state }
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut PhaseCursorState {
        debug_assert!(!self.state.is_null());
        unsafe { &mut *self.state }
    }

    #[inline(always)]
    pub(crate) fn control_semantics(&self) -> ControlSemanticsTable {
        *self.machine().control_semantics()
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        self.machine().compiled_role().frontier_scratch_layout()
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.machine().compiled_role().max_frontier_entries()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.machine().compiled_role().logical_lane_count()
    }

    #[inline(always)]
    fn lane_cursors(&self) -> &[u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.state().lane_cursors, len) }
        }
    }

    #[inline(always)]
    fn lane_cursors_mut(&mut self) -> &mut [u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.state_mut().lane_cursors, len) }
        }
    }

    #[inline(always)]
    fn current_step_labels(&self) -> &[u8] {
        let len = self.logical_lane_count();
        if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.state().current_step_labels, len) }
        }
    }

    #[inline(always)]
    fn current_step_labels_mut(&mut self) -> &mut [u8] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.state_mut().current_step_labels, len) }
        }
    }

    #[inline(always)]
    fn typestate(&self) -> &RoleTypestateValue {
        self.machine().typestate()
    }

    #[inline(always)]
    fn local_steps_len(&self) -> usize {
        self.machine().local_steps_len()
    }

    // =========================================================================
    // Construction
    // =========================================================================

    #[inline(never)]
    pub(crate) unsafe fn init_from_compiled(
        dst: *mut Self,
        state: *mut PhaseCursorState,
        lane_cursors: *mut u16,
        current_step_labels: *mut u8,
        compiled_role: *const CompiledRoleImage,
        program_image: ProgramImage,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).state).write(state);
            PhaseCursorMachine::init_from_compiled(
                core::ptr::addr_of_mut!((*dst).machine),
                compiled_role,
                program_image,
            );
            PhaseCursorState::init_empty(
                state,
                lane_cursors,
                current_step_labels,
                (&*compiled_role).logical_lane_count(),
            );
            (&mut *dst).rebuild_current_step_labels();
        }
    }

    // =========================================================================
    // =========================================================================

    #[inline(always)]
    pub(crate) fn current_phase_lane_set(&self) -> LaneSetView {
        self.machine()
            .phase_lane_set(self.phase_index_usize())
            .unwrap_or(LaneSetView::from_parts(core::ptr::null(), 0))
    }

    #[inline(always)]
    pub(crate) fn current_phase_route_guard(&self) -> Option<PhaseRouteGuard> {
        self.machine().phase_route_guard(self.phase_index_usize())
    }

    #[inline(always)]
    fn current_phase_min_start(&self) -> Option<u16> {
        self.machine().phase_min_start(self.phase_index_usize())
    }

    #[inline(always)]
    fn current_phase_lane_steps(&self, lane_idx: usize) -> Option<LaneSteps> {
        self.machine()
            .phase_lane_steps(self.phase_index_usize(), lane_idx)
    }

    // =========================================================================
    // Lane Access
    // =========================================================================

    fn resolved_label_for_lane(&self, lane_idx: usize) -> Option<u8> {
        let state_idx = self.step_state_index_at_lane(lane_idx)?;
        let node = self.typestate().node(state_index_to_usize(state_idx));
        match node.action() {
            LocalAction::Send { label, .. }
            | LocalAction::Recv { label, .. }
            | LocalAction::Local { label, .. } => Some(label),
            LocalAction::Terminate | LocalAction::Jump { .. } => None,
        }
    }

    fn rebuild_current_step_labels(&mut self) {
        self.current_step_labels_mut().fill(0);
        let lane_limit = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            let label = self.resolved_label_for_lane(lane_idx);
            if let Some(label) = label {
                self.current_step_labels_mut()[lane_idx] = label;
            }
            lane_idx += 1;
        }
    }

    fn refresh_current_step_label(&mut self, lane_idx: usize) {
        let label = self.resolved_label_for_lane(lane_idx);
        if let Some(label) = label {
            self.current_step_labels_mut()[lane_idx] = label;
        } else {
            self.current_step_labels_mut()[lane_idx] = 0;
        }
    }

    // =========================================================================
    // =========================================================================

    /// Find the lane that has a pending step with the given label.
    ///
    /// This is the core of Phase-driven execution: we use the label→lane mask
    /// for the current phase to resolve the lane without scanning.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn find_step_for_label(&self, target_label: u8) -> Option<(usize, StateIndex)> {
        let phase_idx = self.phase_index_usize();
        let lane_entries = self.machine().compiled_role().phase_lane_entries(phase_idx);
        let mut entry_idx = 0usize;
        while entry_idx < lane_entries.len() {
            let lane_idx = lane_entries[entry_idx].lane as usize;
            if self.current_step_labels()[lane_idx] != target_label {
                entry_idx += 1;
                continue;
            }
            let state_idx = self.step_state_index_at_lane(lane_idx)?;
            let node = self.typestate().node(state_index_to_usize(state_idx));
            let Some(label) = (match node.action() {
                LocalAction::Send { label, .. }
                | LocalAction::Recv { label, .. }
                | LocalAction::Local { label, .. } => Some(label),
                LocalAction::Terminate | LocalAction::Jump { .. } => None,
            }) else {
                debug_assert!(false, "current step label cache pointed at unlabeled step");
                return None;
            };
            if label != target_label {
                debug_assert!(false, "current step label cache out of sync");
                return None;
            }
            return Some((lane_idx, state_idx));
        }
        None
    }

    /// Get the step index at the current cursor position for a specific lane.
    pub(crate) fn step_index_at_lane(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }

        let lane_steps = self.current_phase_lane_steps(lane_idx)?;
        if !lane_steps.is_active() {
            return None;
        }

        let cursor_pos = self.lane_cursors()[lane_idx] as usize;
        let start = lane_steps.start as usize;
        let len = lane_steps.len as usize;
        let step_idx = start + cursor_pos;
        if cursor_pos >= len || step_idx >= self.local_steps_len() {
            return None;
        }

        Some(step_idx)
    }

    pub(crate) fn index_for_lane_step(&self, lane_idx: usize) -> Option<usize> {
        let state_idx = self.step_state_index_at_lane(lane_idx)?;
        Some(state_index_to_usize(state_idx))
    }

    #[inline]
    fn step_state_index_at_lane(&self, lane_idx: usize) -> Option<StateIndex> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        let state_idx = self
            .machine()
            .step_index_to_state()
            .get(step_idx)
            .copied()?;
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(
                false,
                "missing typestate index for lane step idx={}",
                step_idx
            );
            return None;
        }
        Some(state_idx)
    }

    // =========================================================================
    // =========================================================================

    /// Set cursor for a specific lane to the step matching `eff_index`.
    ///
    /// Unlike `advance_lane_to_eff_index`, this positions the lane cursor at the
    /// step itself (not past it). Used for loop rewinds.
    pub(crate) fn set_lane_cursor_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        if lane_idx >= self.logical_lane_count() {
            return;
        }
        let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
            return;
        };
        if !lane_steps.is_active() {
            return;
        }
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for phase cursor");
            return;
        }
        let step_idx = self.machine().eff_index_to_step()[eff_idx];
        if step_idx == PHASE_CURSOR_NO_STEP {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "step index out of bounds for local steps");
            return;
        }
        let start = lane_steps.start as usize;
        let end = start.saturating_add(lane_steps.len as usize);
        if step_idx < start || step_idx >= end {
            debug_assert!(
                false,
                "eff_index not in current lane scope: eff_index={} lane={}",
                eff_index, lane_idx
            );
            return;
        }
        let target = step_idx.saturating_sub(start);
        self.lane_cursors_mut()[lane_idx] = Self::encode_index(target);
        self.refresh_current_step_label(lane_idx);
    }

    /// Advance cursor for a specific lane to the step matching `eff_index`.
    pub(crate) fn advance_lane_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        if lane_idx >= self.logical_lane_count() {
            return;
        }
        let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
            return;
        };
        if !lane_steps.is_active() {
            return;
        }
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for phase cursor");
            return;
        }
        let step_idx = self.machine().eff_index_to_step()[eff_idx];
        if step_idx == PHASE_CURSOR_NO_STEP {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "step index out of bounds for local steps");
            return;
        }
        let start = lane_steps.start as usize;
        let end = start.saturating_add(lane_steps.len as usize);
        if step_idx < start || step_idx >= end {
            debug_assert!(
                false,
                "eff_index not in current lane scope: eff_index={} lane={}",
                eff_index, lane_idx
            );
            return;
        }
        let target = step_idx.saturating_sub(start) + 1;
        if target > self.lane_cursors()[lane_idx] as usize {
            self.lane_cursors_mut()[lane_idx] = Self::encode_index(target);
            self.refresh_current_step_label(lane_idx);
        }
    }

    /// Advance to next phase without syncing the primary typestate index.
    #[inline]
    pub(crate) fn advance_phase_without_sync(&mut self) {
        let state = self.state_mut();
        state.phase_index = state.phase_index.saturating_add(1);
        self.lane_cursors_mut().fill(0);
        self.rebuild_current_step_labels();
    }

    pub(crate) fn sync_idx_to_phase_start(&mut self) {
        let phase_lane_set = self.current_phase_lane_set();
        if phase_lane_set.word_len() == 0 {
            return;
        }
        let Some(phase_min_start) = self.current_phase_min_start() else {
            return;
        };
        let step_idx = phase_min_start as usize;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "phase start out of local steps range");
            return;
        }
        let state_idx = self.machine().step_index_to_state()[step_idx];
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        }
        self.state_mut().idx = state_idx.raw();
    }

    /// Check if all lanes in current phase are complete.
    pub(crate) fn is_phase_complete(&self) -> bool {
        let phase_idx = self.phase_index_usize();
        let lane_entries = self.machine().compiled_role().phase_lane_entries(phase_idx);
        if lane_entries.is_empty() {
            return true; // No more phases
        }
        let mut entry_idx = 0usize;
        while entry_idx < lane_entries.len() {
            let lane_idx = lane_entries[entry_idx].lane as usize;
            let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
                debug_assert!(false, "compiled phase lane mask missing lane entry");
                return false;
            };
            if (self.lane_cursors()[lane_idx] as usize) < lane_steps.len as usize {
                return false;
            }
            entry_idx += 1;
        }
        true
    }

    // =========================================================================
    // Core Typestate Navigation
    // =========================================================================

    /// Current typestate index.
    #[inline(always)]
    pub(crate) fn index(&self) -> usize {
        self.state().idx as usize
    }

    /// Access a typestate node by index.
    #[inline(always)]
    pub(crate) fn typestate_node(&self, index: usize) -> LocalNode {
        self.typestate().node(index)
    }

    #[inline(always)]
    fn action(&self) -> LocalAction {
        self.typestate().node(self.idx_usize()).action()
    }

    /// Returns `true` when the cursor points at a send action.
    #[inline(always)]
    pub(crate) fn is_send(&self) -> bool {
        self.action().is_send()
    }

    /// Returns `true` when the cursor points at a receive action.
    #[inline(always)]
    pub(crate) fn is_recv(&self) -> bool {
        self.action().is_recv()
    }

    /// Returns `true` when the cursor points at a local action.
    #[inline(always)]
    pub(crate) fn is_local_action(&self) -> bool {
        self.action().is_local_action()
    }

    /// Returns `true` when the cursor points at a Jump action.
    #[inline(always)]
    pub(crate) fn is_jump(&self) -> bool {
        self.action().is_jump()
    }

    /// Returns the jump reason if the current node is a Jump action.
    #[inline(always)]
    pub(crate) fn jump_reason(&self) -> Option<JumpReason> {
        self.action().jump_reason()
    }

    /// Advance typestate index to the successor.
    #[inline(always)]
    pub(crate) fn advance_in_place(&mut self) {
        let next = self.typestate().node(self.idx_usize()).next();
        self.state_mut().idx = next.raw();
    }

    /// Follow Jump nodes until reaching a non-Jump or PassiveObserverBranch.
    ///
    /// Jump nodes are control flow instructions that redirect the cursor to
    /// their target (stored in the `next` field). This method follows the
    /// chain of Jump nodes until reaching a non-Jump node.
    ///
    /// **Decision point**: Only `PassiveObserverBranch` Jumps are NOT followed
    /// automatically. The passive observer must use `offer()` to determine
    /// which arm was selected before the Jump can be followed.
    ///
    /// **Auto-followed Jumps**:
    /// - `LoopContinue`: Returns cursor to loop_start for next iteration
    /// - `LoopBreak`: Exits the loop scope to terminal
    /// - `RouteArmEnd`: Exits the route arm to scope_end
    ///
    /// Returns `Err(JumpError)` if the Jump chain exceeds MAX_EFF_NODES iterations,
    /// indicating a CFG cycle bug in the typestate compiler.
    #[inline(always)]
    pub(crate) fn try_follow_jumps_in_place(&mut self) -> Result<(), JumpError> {
        let mut iter = 0u32;
        while self.is_jump() {
            match self.action().jump_reason() {
                Some(JumpReason::PassiveObserverBranch) => {
                    // Decision point: stop for offer() to handle arm selection.
                    // Even when an arm is τ-eliminated, the decision is still required.
                    return Ok(());
                }
                _ => {
                    // Follow all other Jump nodes (LoopContinue, LoopBreak, RouteArmEnd)
                    self.advance_in_place();
                    iter += 1;
                    if iter > crate::eff::meta::MAX_EFF_NODES as u32 {
                        return Err(JumpError {
                            iterations: iter,
                            idx: self.idx_usize(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Advance to the next node, then follow Jump nodes.
    ///
    /// Returns `Err(JumpError)` if the Jump chain exceeds MAX_EFF_NODES iterations.
    #[inline(always)]
    pub(crate) fn try_advance_past_jumps_in_place(&mut self) -> Result<(), JumpError> {
        self.advance_in_place();
        self.try_follow_jumps_in_place()
    }

    /// Follow a PassiveObserverBranch Jump to the specified arm's target.
    ///
    /// Uses O(1) registry lookup to find the PassiveObserverBranch Jump for the
    /// specified arm, then follows it to the target node.
    ///
    /// Returns `None` if:
    /// - Not in a scope
    /// - No PassiveObserverBranch Jump found for the specified arm
    pub(crate) fn follow_passive_observer_arm(
        &self,
        target_arm: u8,
    ) -> Option<PassiveArmNavigation> {
        let scope_region = self.scope_region()?;
        self.follow_passive_observer_arm_for_scope(scope_region.scope_id, target_arm)
    }

    /// Follow a PassiveObserverBranch Jump to the specified arm's target for a given scope.
    ///
    /// Unlike `follow_passive_observer_arm()`, this takes an explicit `scope_id` parameter
    /// instead of deriving it from the cursor's current node. Use this when you already
    /// know the scope (e.g., in `offer()` after scope decision).
    ///
    /// Returns `PassiveArmNavigation::WithinArm` containing the arm entry index.
    /// For τ-eliminated arms (no cross-role content), returns the ArmEmpty placeholder.
    ///
    /// Navigation priority:
    /// 1. PassiveObserverBranch Jump (if available) - follows the jump to arm entry
    /// 2. passive_arm_entry (direct entry index)
    ///
    /// The direct entry path is needed for nested routes where the inner route may have
    /// controller_arm_entry set (causing PassiveObserverBranch generation to skip),
    /// but passive_arm_entry is still valid for navigation.
    ///
    /// Note: τ-eliminated arms (no cross-role content) are handled at compile time
    /// by generating ArmEmpty (RouteArmEnd) placeholder nodes, ensuring
    /// passive_arm_entry is always set.
    pub(crate) fn follow_passive_observer_arm_for_scope(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> Option<PassiveArmNavigation> {
        // O(1) registry lookup for the PassiveObserverBranch Jump node index
        let typestate = self.typestate();
        let jump_node_idx = typestate.passive_arm_jump(scope_id, target_arm);

        if let Some(jump_idx) = jump_node_idx {
            // Primary path: follow PassiveObserverBranch Jump to target
            let jump_node = typestate.node(state_index_to_usize(jump_idx));
            let target = jump_node.next();
            Some(PassiveArmNavigation::WithinArm { entry: target })
        } else if !self.is_route_controller(scope_id)
            && let Some(entry_idx) = typestate.passive_arm_entry(scope_id, target_arm)
        {
            // Secondary path: use passive_arm_entry directly
            // This is needed for nested routes where the inner route may be incorrectly
            // classified as "controller" (due to some nodes having controller_arm_entry set),
            // preventing PassiveObserverBranch generation. However, passive_arm_entry is
            // still correctly tracking the first cross-role node of each arm.
            //
            // For τ-eliminated arms, passive_arm_entry points to the ArmEmpty
            // (RouteArmEnd) placeholder generated at compile time.
            Some(PassiveArmNavigation::WithinArm { entry: entry_idx })
        } else {
            // No valid arm entry found - this should not happen with CFG-pure design.
            // All arms (including τ-eliminated) should have passive_arm_entry set.
            None
        }
    }

    /// Find the route arm containing a Recv node with the specified label.
    ///
    /// Uses FIRST-recv dispatch for O(1) lookup. The dispatch table now includes
    /// the arm directly as `(label, arm, target_idx)`, eliminating positional inference.
    ///
    /// Returns `None` if label not found in dispatch table.
    ///
    /// Direct entry via passive_arm_entry is only needed for τ-eliminated arms or
    /// arms with no recv nodes (which have no FIRST entries).
    #[cfg(test)]
    pub(crate) fn find_arm_for_recv_label(&self, target_label: u8) -> Option<u8> {
        let scope_region = self.scope_region()?;
        let scope_id = scope_region.scope_id;
        // FIRST-recv dispatch: O(1) lookup returns (arm, target_idx) directly.
        // The arm is stored in the compiled dispatch table, eliminating positional inference.
        if let Some((arm, _target_idx)) = self
            .typestate()
            .first_recv_dispatch_target_for_label(scope_id, target_label)
        {
            if arm == super::super::typestate::ARM_SHARED {
                return Some(0);
            }
            return Some(arm);
        }

        let typestate = self.typestate();

        // Bounded O(4) scan of arm entry node labels for τ-eliminated or local-only arms.
        for arm in 0..2u8 {
            let entry_idx = if let Some(jump_node_idx) = typestate.passive_arm_jump(scope_id, arm) {
                let jump_node = typestate.node(state_index_to_usize(jump_node_idx));
                Some(state_index_to_usize(jump_node.next()))
            } else {
                if self.is_route_controller(scope_id) {
                    None
                } else {
                    typestate
                        .passive_arm_entry(scope_id, arm)
                        .map(state_index_to_usize)
                }
            };

            if let Some(target_idx) = entry_idx {
                let entry_node = typestate.node(target_idx);
                if let LocalAction::Recv { label, .. } = entry_node.action() {
                    if label == target_label {
                        return Some(arm);
                    }
                }
            }
        }
        None
    }

    #[inline(always)]
    pub(crate) fn set_index(&mut self, idx: usize) {
        debug_assert!(idx < self.typestate().len());
        self.state_mut().idx = Self::encode_index(idx);
    }

    #[inline(always)]
    pub(crate) fn action_at(&self, idx: usize) -> LocalAction {
        self.typestate().node(idx).action()
    }

    #[inline(always)]
    pub(crate) fn is_recv_at(&self, idx: usize) -> bool {
        self.action_at(idx).is_recv()
    }

    #[inline(always)]
    pub(crate) fn is_send_at(&self, idx: usize) -> bool {
        self.action_at(idx).is_send()
    }

    #[inline(always)]
    pub(crate) fn is_local_action_at(&self, idx: usize) -> bool {
        self.action_at(idx).is_local_action()
    }

    #[inline(always)]
    pub(crate) fn is_jump_at(&self, idx: usize) -> bool {
        self.action_at(idx).is_jump()
    }

    #[inline(always)]
    pub(crate) fn jump_reason_at(&self, idx: usize) -> Option<JumpReason> {
        self.action_at(idx).jump_reason()
    }

    #[inline(always)]
    pub(crate) fn jump_target_at(&self, idx: usize) -> Option<usize> {
        if self.is_jump_at(idx) {
            Some(state_index_to_usize(self.typestate().node(idx).next()))
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn node_scope_id_at(&self, idx: usize) -> ScopeId {
        self.typestate().node(idx).scope()
    }

    #[inline(always)]
    pub(crate) fn try_send_meta_at(&self, idx: usize) -> Option<SendMeta> {
        try_send_meta_value(self.typestate(), idx)
    }

    #[inline(always)]
    pub(crate) fn try_recv_meta_at(&self, idx: usize) -> Option<RecvMeta> {
        try_recv_meta_value(self.typestate(), idx)
    }

    #[inline(always)]
    pub(crate) fn try_local_meta_at(&self, idx: usize) -> Option<LocalMeta> {
        try_local_meta_value(self.typestate(), idx)
    }

    // =========================================================================
    // Scope Queries (delegated to typestate)
    // =========================================================================

    /// Get scope region for current node.
    pub(crate) fn scope_region(&self) -> Option<ScopeRegion> {
        let typestate = self.typestate();
        let scope_id = typestate.node(self.idx_usize()).scope();
        if scope_id.is_none() {
            None
        } else {
            self.scope_region_by_id(scope_id)
        }
    }

    /// Get scope region by scope ID.
    #[inline(always)]
    pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        let mut region = self.typestate().scope_region_for(scope_id)?;
        region.controller_role = self.machine().route_controller_role(scope_id);
        Some(region)
    }

    /// FIRST-recv dispatch lookup for passive observers.
    ///
    /// Given a recv label, returns the route arm and leaf recv StateIndex.
    /// Returns `(arm, target_idx)` for O(1) dispatch without extra inference.
    ///
    /// Returns `None` if label not found.
    #[inline]
    pub(crate) fn first_recv_target(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        if let Some((policy, _, _)) = self.route_scope_controller_policy(scope_id)
            && policy.is_dynamic()
        {
            return None;
        }
        self.typestate()
            .first_recv_dispatch_target_for_label(scope_id, label)
    }

    #[inline]
    pub(crate) fn first_recv_target_evidence(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.typestate()
            .first_recv_dispatch_target_for_label(scope_id, label)
    }

    /// Check if this role is the controller for the given route scope.
    ///
    /// Uses the shared program route atlas to compare the route controller role
    /// against the attached role image. This keeps controller authority program-wide
    /// instead of duplicating it in every role-local scope record.
    ///
    /// Returns `true` if `controller_role == self.compiled.role()`, `false` otherwise.
    #[inline]
    pub(crate) fn is_route_controller(&self, scope_id: ScopeId) -> bool {
        self.machine()
            .route_controller_role(scope_id)
            .map_or(false, |ctrl| ctrl == self.machine().role())
    }

    /// Scope ID stored on the current node (no parent traversal).
    #[inline(always)]
    pub(crate) fn node_scope_id(&self) -> ScopeId {
        self.typestate().node(self.idx_usize()).scope()
    }

    /// Advance past the current scope if it matches the given kind.
    pub(crate) fn advance_scope_if_kind_in_place(&mut self, kind: ScopeKind) -> bool {
        if let Some(region) = self.scope_region()
            && region.kind == kind
        {
            self.set_index(region.end);
            return true;
        }
        false
    }

    /// Advance past a scope by ID.
    ///
    /// If cursor is already at or beyond scope.end, returns None since no
    /// advancement is needed (cursor has already exited the scope).
    pub(crate) fn advance_scope_by_id_in_place(&mut self, scope_id: ScopeId) -> bool {
        if let Some(region) = self.scope_region_by_id(scope_id) {
            // Only advance if cursor is still inside the scope
            if self.idx_usize() < region.end {
                self.set_index(region.end);
                return true;
            }
        }
        // Cursor already at or beyond scope.end - no advancement needed
        false
    }

    /// Get parent scope.
    pub(crate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.typestate().scope_parent(scope_id)
    }

    #[inline]
    pub(crate) fn enclosing_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        let mut current = scope_id;
        while !current.is_none() {
            let region = self.scope_region_by_id(current)?;
            if matches!(region.kind, ScopeKind::Loop) {
                return Some(current);
            }
            current = match self.scope_parent(current) {
                Some(parent) => parent,
                None => ScopeId::none(),
            };
        }
        None
    }

    #[inline]
    pub(crate) fn node_loop_scope(&self, index: usize) -> Option<ScopeId> {
        let scope = self.typestate_node(index).scope();
        if scope.is_none() {
            None
        } else {
            self.enclosing_loop_scope(scope)
        }
    }

    // =========================================================================
    // Label Seeking
    // =========================================================================

    /// Find cursor at node with given label.
    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn seek_label_index(&self, label: u8) -> Option<usize> {
        let typestate = self.typestate();
        for i in 0..typestate.len() {
            let node = typestate.node(i);
            let node_label = match node.action() {
                LocalAction::Send { label: l, .. }
                | LocalAction::Recv { label: l, .. }
                | LocalAction::Local { label: l, .. } => Some(l),
                LocalAction::Terminate | LocalAction::Jump { .. } => None,
            };
            if node_label == Some(label) {
                return Some(i);
            }
        }
        None
    }

    fn try_index_for_loop_control(&self, meaning: LoopControlMeaning) -> Option<usize> {
        let typestate = self.typestate();
        for i in 0..typestate.len() {
            let node = typestate.node(i);
            let resource = match node.action() {
                LocalAction::Send { resource, .. }
                | LocalAction::Recv { resource, .. }
                | LocalAction::Local { resource, .. } => resource,
                LocalAction::Terminate | LocalAction::Jump { .. } => None,
            };
            if LoopControlMeaning::from_resource_tag(resource) == Some(meaning) {
                return Some(i);
            }
        }
        None
    }

    fn successor_index_for_loop_control(&self, meaning: LoopControlMeaning) -> usize {
        let index = self
            .try_index_for_loop_control(meaning)
            .expect("loop control not found in typestate");
        state_index_to_usize(self.typestate().node(index).next())
    }

    // =========================================================================
    // Route Scope Methods
    // =========================================================================

    /// Get recv node index for a route arm.
    pub(crate) fn route_scope_arm_recv_index(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> Option<usize> {
        let state = self.typestate().route_recv_state(scope_id, target_arm)?;
        Some(state_index_to_usize(state))
    }

    /// Get arm count for a route scope.
    pub(crate) fn route_scope_arm_count(&self, scope_id: ScopeId) -> Option<u8> {
        self.typestate()
            .route_arm_count(scope_id)
            .map(|count| count as u8)
    }

    /// Get the compiled offer-lane mask for a route scope.
    pub(crate) fn route_scope_offer_lane_set(&self, scope_id: ScopeId) -> Option<LaneSetView> {
        self.typestate().route_offer_lane_mask(scope_id)
    }

    /// Get offer entry index for a route scope.
    /// u16::MAX indicates the entry check is disabled (e.g., linger routes).
    pub(crate) fn route_scope_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        self.typestate().route_offer_entry(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        let sparse_slot = self.typestate().route_scope_slot(scope_id)?;
        self.typestate().route_scope_dense_ordinal(sparse_slot)
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.typestate().route_scope_count()
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.typestate().first_recv_dispatch_entry(scope_id, idx)
    }

    pub(crate) fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        self.typestate().scope_lane_first_eff(scope_id, lane)
    }

    pub(crate) fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        self.typestate().scope_lane_last_eff(scope_id, lane)
    }

    pub(crate) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        self.typestate()
            .scope_lane_last_eff_for_arm(scope_id, arm, lane)
    }

    /// Get the controller arm entry index for a given label.
    /// Returns the StateIndex of the arm whose label matches, used by flow() for O(1) lookup.
    pub(crate) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        if !self.is_route_controller(scope_id) {
            return None;
        }
        self.typestate()
            .controller_arm_entry_for_label(scope_id, label)
    }

    /// Get the controller arm entry (index, label) for a given arm number.
    /// Used by offer() to navigate to the selected arm's entry point.
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        if !self.is_route_controller(scope_id) {
            return None;
        }
        self.typestate().controller_arm_entry_by_arm(scope_id, arm)
    }

    #[inline]
    pub(crate) fn passive_arm_scope_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        self.typestate().passive_arm_scope(scope_id, arm)
    }

    /// Get route controller policy metadata.
    ///
    /// The tuple `(PolicyMode, EffIndex, u8)` corresponds to the controller-provided
    /// policy mode, the effect index of the send action that declared it, and the
    /// control resource tag embedded in the DSL. Route policies are tracked for both
    /// generic route decisions and loop-based routing (LoopContinue/LoopBreak).
    pub(crate) fn route_scope_controller_policy(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
        self.machine().route_controller(scope_id)
    }

    // =========================================================================
    // Metadata Extraction
    // =========================================================================

    /// Try to get send metadata at the current cursor location.
    /// Returns `None` if the current node is not a Send action.
    pub(crate) fn try_send_meta(&self) -> Option<SendMeta> {
        try_send_meta_value(self.typestate(), self.idx_usize())
    }

    /// Try to get receive metadata at the current cursor location.
    /// Returns `None` if the current node is not a Recv action.
    pub(crate) fn try_recv_meta(&self) -> Option<RecvMeta> {
        try_recv_meta_value(self.typestate(), self.idx_usize())
    }

    /// Try to get local action metadata at the current cursor location.
    /// Returns `None` if the current node is not a Local action.
    pub(crate) fn try_local_meta(&self) -> Option<LocalMeta> {
        try_local_meta_value(self.typestate(), self.idx_usize())
    }

    // =========================================================================
    // Loop Metadata
    // =========================================================================

    /// Get loop metadata for current scope.
    pub(crate) fn loop_metadata_inner(&self) -> Option<LoopMetadata> {
        let node = self.typestate().node(self.idx_usize());
        let action = node.action();
        let role = self.machine().role();
        let (resource, eff_index, controller, target, role_kind) = match action {
            LocalAction::Send {
                resource,
                eff_index,
                peer,
                ..
            } => (resource, eff_index, role, peer, LoopRole::Controller),
            LocalAction::Recv {
                resource,
                eff_index,
                peer,
                ..
            } => (resource, eff_index, peer, role, LoopRole::Target),
            _ => return None,
        };
        if LoopControlMeaning::from_resource_tag(resource) != Some(LoopControlMeaning::Continue) {
            return None;
        }
        let scope = self.node_loop_scope(self.idx_usize())?;
        let continue_index = self.successor_index_for_loop_control(LoopControlMeaning::Continue);
        let break_index = self.successor_index_for_loop_control(LoopControlMeaning::Break);
        Some(LoopMetadata {
            scope,
            controller,
            target,
            role: role_kind,
            eff_index,
            decision_index: as_state_index(self.idx_usize()),
            continue_index: as_state_index(continue_index),
            break_index: as_state_index(break_index),
        })
    }
}
