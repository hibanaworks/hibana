//! Mutable phase and runtime cursor logic for typestate execution.

use core::slice;

use super::facts::{
    ARM_SHARED, JumpError, JumpReason, LocalAction, LocalMeta, LocalNode, MAX_FIRST_RECV_DISPATCH,
    PassiveArmNavigation, RecvMeta, ScopeRegion, SendMeta, StateIndex, as_state_index,
    state_index_to_usize,
};
use crate::endpoint::kernel::FrontierScratchLayout;
use crate::{
    eff::EffIndex,
    global::{
        LoopControlMeaning,
        compiled::images::{ControlSemanticKind, ControlSemanticsTable, RoleDescriptorRef},
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

const PHASE_CURSOR_NO_STATE: StateIndex = StateIndex::MAX;

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, Copy, PartialEq, Eq))]
struct PhaseCursorMachine {
    role_descriptor: RoleDescriptorRef,
}

impl PhaseCursorMachine {
    #[inline(always)]
    unsafe fn init_from_descriptor(dst: *mut Self, role_descriptor: RoleDescriptorRef) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).role_descriptor).write(role_descriptor);
        }
    }

    #[inline(always)]
    fn role_descriptor(&self) -> RoleDescriptorRef {
        self.role_descriptor
    }

    #[inline(always)]
    fn role_descriptor_ref(&self) -> &RoleDescriptorRef {
        &self.role_descriptor
    }

    #[inline(always)]
    fn role(&self) -> u8 {
        self.role_descriptor().role()
    }

    #[inline(always)]
    fn program_ref(&self) -> crate::global::compiled::images::CompiledProgramRef {
        self.role_descriptor().program()
    }

    #[inline(always)]
    fn phase_lane_set(&self, idx: usize) -> Option<LaneSetView> {
        self.role_descriptor().phase_lane_set(idx)
    }

    #[inline(always)]
    fn phase_min_start(&self, idx: usize) -> Option<u16> {
        self.role_descriptor().phase_min_start(idx)
    }

    #[inline(always)]
    fn phase_route_guard(&self, idx: usize) -> Option<PhaseRouteGuard> {
        self.role_descriptor().phase_route_guard(idx)
    }

    #[inline(always)]
    fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        self.role_descriptor().phase_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    fn local_steps_len(&self) -> usize {
        self.role_descriptor().local_len()
    }

    #[inline(always)]
    fn node_len(&self) -> usize {
        self.role_descriptor_ref().node_len()
    }

    #[inline(always)]
    fn node(&self, idx: usize) -> LocalNode {
        self.role_descriptor_ref().node(idx)
    }

    #[inline(always)]
    fn checked_node(&self, idx: usize) -> Option<LocalNode> {
        self.role_descriptor_ref().checked_node(idx)
    }

    #[inline(always)]
    fn state_for_step_index(&self, step_idx: usize) -> Option<StateIndex> {
        self.role_descriptor_ref().state_for_step_index(step_idx)
    }

    #[inline(always)]
    fn step_for_eff_index(&self, eff_index: EffIndex) -> Option<usize> {
        self.role_descriptor_ref().step_for_eff_index(eff_index)
    }

    #[inline(always)]
    fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.role_descriptor_ref().scope_region_by_id(scope_id)
    }

    #[inline(always)]
    fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.role_descriptor_ref().scope_parent(scope_id)
    }

    #[inline(always)]
    fn control_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.role_descriptor_ref().control_parent(scope_id)
    }

    #[inline(always)]
    fn route_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.role_descriptor_ref().route_parent(scope_id)
    }

    #[inline(always)]
    fn route_parent_arm(&self, scope_id: ScopeId) -> Option<u8> {
        self.role_descriptor_ref().route_parent_arm(scope_id)
    }

    #[inline(always)]
    fn parallel_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.role_descriptor_ref().parallel_root(scope_id)
    }

    #[inline(always)]
    fn enclosing_loop(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.role_descriptor_ref().enclosing_loop(scope_id)
    }

    #[inline(always)]
    fn control_semantics(&self) -> &ControlSemanticsTable {
        self.program_ref().control_semantics()
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
pub(crate) struct PhaseCursorState {
    /// Primary typestate index used for scope queries.
    idx: u16,
    /// Current phase index (0-based)
    phase_index: u8,
    /// Per-lane step progress within current phase.
    /// `lane_cursors[lane_idx]` = number of steps completed on that lane.
    lane_cursors: *mut u16,
    /// Encoded current logical label for each lane's pending step.
    current_step_label_codes: *mut u16,
}

const CURRENT_STEP_UNLABELED_CODE: u16 = u16::MAX;

impl PhaseCursorState {
    #[inline(always)]
    pub(crate) unsafe fn init_empty(
        dst: *mut Self,
        lane_cursors: *mut u16,
        current_step_label_codes: *mut u16,
        logical_lane_count: usize,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).idx).write(0);
            core::ptr::addr_of_mut!((*dst).phase_index).write(0);
            core::ptr::addr_of_mut!((*dst).lane_cursors).write(lane_cursors);
            core::ptr::addr_of_mut!((*dst).current_step_label_codes)
                .write(current_step_label_codes);
            let mut lane_idx = 0usize;
            while lane_idx < logical_lane_count {
                lane_cursors.add(lane_idx).write(0);
                current_step_label_codes
                    .add(lane_idx)
                    .write(CURRENT_STEP_UNLABELED_CODE);
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
        debug_assert!(idx < u16::MAX as usize);
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
        self.machine().role_descriptor().frontier_scratch_layout()
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.machine().role_descriptor().max_frontier_entries()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.machine().role_descriptor().logical_lane_count()
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
    const fn encode_current_step_label(label: u8) -> u16 {
        label as u16
    }

    #[inline(always)]
    fn current_step_label_codes(&self) -> &[u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.state().current_step_label_codes, len) }
        }
    }

    #[inline(always)]
    fn current_step_label_codes_mut(&mut self) -> &mut [u16] {
        let len = self.logical_lane_count();
        if len == 0 {
            &mut []
        } else {
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
        state: *mut PhaseCursorState,
        lane_cursors: *mut u16,
        current_step_label_codes: *mut u16,
        role_descriptor: RoleDescriptorRef,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).state).write(state);
            PhaseCursorMachine::init_from_descriptor(
                core::ptr::addr_of_mut!((*dst).machine),
                role_descriptor,
            );
            PhaseCursorState::init_empty(
                state,
                lane_cursors,
                current_step_label_codes,
                role_descriptor.logical_lane_count(),
            );
            (&mut *dst).rebuild_current_step_label_codes();
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
    // =========================================================================

    /// Find the lane that has a pending step with the given label.
    ///
    /// This is the core of Phase-driven execution: we use the label→lane mask
    /// for the current phase to resolve the lane without scanning.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn find_step_for_label(&self, target_label: u8) -> Option<(usize, StateIndex)> {
        let target_code = Self::encode_current_step_label(target_label);
        let phase_idx = self.phase_index_usize();
        let role_descriptor = self.machine().role_descriptor();
        let lane_entries = role_descriptor.phase_lane_entries(phase_idx);
        if lane_entries.is_empty() {
            let lane_set = self.current_phase_lane_set();
            let lane_limit = self.logical_lane_count();
            let mut next = lane_set.first_set(lane_limit);
            while let Some(lane_idx) = next {
                if self.current_step_label_codes()[lane_idx] == target_code {
                    let Some(state_idx) = self.step_state_index_at_lane(lane_idx) else {
                        debug_assert!(
                            false,
                            "current step label cache pointed at completed resident lane"
                        );
                        return None;
                    };
                    let node = self.machine().node(state_index_to_usize(state_idx));
                    let Some(label) = (match node.action() {
                        LocalAction::Send { label, .. }
                        | LocalAction::Recv { label, .. }
                        | LocalAction::Local { label, .. } => Some(label),
                        LocalAction::Terminate => None,
                    }) else {
                        debug_assert!(
                            false,
                            "current step label cache pointed at unlabeled resident step"
                        );
                        return None;
                    };
                    if label != target_label {
                        debug_assert!(false, "resident current step label cache out of sync");
                        return None;
                    }
                    return Some((lane_idx, state_idx));
                }
                next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
            }
            return None;
        }
        let mut entry_idx = 0usize;
        while entry_idx < lane_entries.len() {
            let lane_idx = lane_entries[entry_idx].lane as usize;
            if self.current_step_label_codes()[lane_idx] != target_code {
                entry_idx += 1;
                continue;
            }
            let Some(state_idx) = self.step_state_index_at_lane(lane_idx) else {
                debug_assert!(false, "current step label cache pointed at completed lane");
                entry_idx += 1;
                continue;
            };
            let node = self.machine().node(state_index_to_usize(state_idx));
            let Some(label) = (match node.action() {
                LocalAction::Send { label, .. }
                | LocalAction::Recv { label, .. }
                | LocalAction::Local { label, .. } => Some(label),
                LocalAction::Terminate => None,
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
        let state_idx = self.machine().state_for_step_index(step_idx)?;
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
        let Some(step_idx) = self.machine().step_for_eff_index(eff_index) else {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        };
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
        self.refresh_current_step_label_code(lane_idx);
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
        let Some(step_idx) = self.machine().step_for_eff_index(eff_index) else {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        };
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
            self.refresh_current_step_label_code(lane_idx);
        }
    }

    pub(crate) fn current_phase_contains_eff_index(
        &self,
        lane_idx: usize,
        eff_index: EffIndex,
    ) -> bool {
        if lane_idx >= self.logical_lane_count() {
            return false;
        }
        let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
            return false;
        };
        if !lane_steps.is_active() {
            return false;
        }
        let Some(step_idx) = self.machine().step_for_eff_index(eff_index) else {
            return false;
        };
        if step_idx >= self.local_steps_len() {
            return false;
        }
        let start = lane_steps.start as usize;
        let end = start.saturating_add(lane_steps.len as usize);
        step_idx >= start && step_idx < end
    }

    pub(crate) fn complete_lane_phase(&mut self, lane_idx: usize) {
        if lane_idx >= self.logical_lane_count() {
            return;
        }
        let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
            return;
        };
        if !lane_steps.is_active() {
            return;
        }
        self.lane_cursors_mut()[lane_idx] = Self::encode_index(lane_steps.len as usize);
        self.refresh_current_step_label_code(lane_idx);
    }

    /// Advance to next phase without syncing the primary typestate index.
    #[inline]
    pub(crate) fn advance_phase_without_sync(&mut self) {
        let state = self.state_mut();
        state.phase_index = state.phase_index.saturating_add(1);
        self.lane_cursors_mut().fill(0);
        self.rebuild_current_step_label_codes();
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
        let Some(state_idx) = self.machine().state_for_step_index(step_idx) else {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        };
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        }
        self.state_mut().idx = state_idx.raw();
    }

    /// Check if all lanes in current phase are complete.
    pub(crate) fn is_phase_complete(&self) -> bool {
        let phase_idx = self.phase_index_usize();
        let role_descriptor = self.machine().role_descriptor();
        let lane_entries = role_descriptor.phase_lane_entries(phase_idx);
        if lane_entries.is_empty() {
            let lane_set = self.current_phase_lane_set();
            if lane_set.is_empty() {
                return true;
            }
            let lane_limit = self.logical_lane_count();
            let mut next = lane_set.first_set(lane_limit);
            while let Some(lane_idx) = next {
                let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
                    debug_assert!(false, "resident phase lane mask missing lane steps");
                    return false;
                };
                if (self.lane_cursors()[lane_idx] as usize) < lane_steps.len as usize {
                    return false;
                }
                next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
            }
            return true;
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
        self.machine().node(index)
    }

    #[inline(always)]
    fn action(&self) -> LocalAction {
        self.machine().node(self.idx_usize()).action()
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

    #[inline(always)]
    pub(crate) fn is_terminal(&self) -> bool {
        self.action().is_terminal()
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

    /// Return the index reached after following non-decision Jump nodes.
    ///
    /// This is a preview operation: it does not mutate the cursor, so callers can
    /// validate jump traversal before publishing route commits or consuming preview
    /// resources.
    #[inline(never)]
    pub(crate) fn try_follow_jumps_from_index(
        &self,
        idx: StateIndex,
    ) -> Result<StateIndex, JumpError> {
        let _ = self.checked_typestate_node(idx, 0)?;
        Ok(idx)
    }

    /// Return the index reached by advancing once, then following Jump nodes.
    #[inline(never)]
    pub(crate) fn try_next_index_past_jumps(&self) -> Result<StateIndex, JumpError> {
        let next = self.machine().node(self.idx_usize()).next();
        self.try_follow_jumps_from_index(next)
    }

    /// Advance to the next node, then follow Jump nodes.
    ///
    /// Returns `Err(JumpError)` if the Jump chain exceeds the compiled local
    /// typestate bound.
    #[inline(never)]
    pub(crate) fn try_advance_past_jumps_in_place(&mut self) -> Result<(), JumpError> {
        let target = self.try_next_index_past_jumps()?;
        self.state_mut().idx = target.raw();
        Ok(())
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
        let jump_node_idx = self.passive_arm_jump(scope_id, target_arm);

        if let Some(jump_idx) = jump_node_idx {
            // Primary path: follow PassiveObserverBranch Jump to target
            let jump_node = self.machine().node(state_index_to_usize(jump_idx));
            let target = jump_node.next();
            Some(PassiveArmNavigation::WithinArm { entry: target })
        } else if !self.is_route_controller(scope_id)
            && let Some(entry_idx) = self.passive_arm_entry(scope_id, target_arm)
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

    /// Find the route arm containing a Recv node with the specified lane-local frame label.
    ///
    /// Uses FIRST-recv dispatch for direct lookup. The dispatch table includes
    /// `(frame_label, lane, arm, target_idx)`.
    ///
    /// Returns `None` if the frame label is not found in the dispatch table.
    ///
    /// Direct entry via passive_arm_entry is only needed for τ-eliminated arms or
    /// arms with no recv nodes (which have no FIRST entries).
    #[cfg(test)]
    pub(crate) fn find_arm_for_recv_lane_frame_label(
        &self,
        lane: u8,
        frame_label: u8,
    ) -> Option<u8> {
        let scope_region = self.scope_region()?;
        let scope_id = scope_region.scope_id;
        // FIRST-recv dispatch: O(1) lookup returns (arm, target_idx) directly.
        // The arm is stored in the compiled dispatch table, eliminating positional lookup.
        if let Some((arm, _target_idx)) =
            self.first_recv_dispatch_target_for_lane_frame_label(scope_id, lane, frame_label)
        {
            if arm == super::super::typestate::ARM_SHARED {
                return Some(0);
            }
            return Some(arm);
        }

        // Bounded O(4) scan of arm entry node labels for τ-eliminated or local-only arms.
        for arm in 0..2u8 {
            let entry_idx = if let Some(jump_node_idx) = self.passive_arm_jump(scope_id, arm) {
                let jump_node = self.machine().node(state_index_to_usize(jump_node_idx));
                Some(state_index_to_usize(jump_node.next()))
            } else {
                if self.is_route_controller(scope_id) {
                    None
                } else {
                    self.passive_arm_entry(scope_id, arm)
                        .map(state_index_to_usize)
                }
            };

            if let Some(target_idx) = entry_idx {
                let entry_node = self.machine().node(target_idx);
                if let LocalAction::Recv {
                    lane: entry_lane,
                    frame_label: entry_frame_label,
                    ..
                } = entry_node.action()
                {
                    if entry_lane == lane && entry_frame_label == frame_label {
                        return Some(arm);
                    }
                }
            }
        }
        None
    }

    #[inline(always)]
    pub(crate) fn set_index(&mut self, idx: usize) {
        debug_assert!(idx < self.machine().node_len());
        self.state_mut().idx = Self::encode_index(idx);
    }

    #[inline(always)]
    pub(crate) fn contains_node_index(&self, idx: usize) -> bool {
        idx < self.machine().node_len()
    }

    #[inline(always)]
    pub(crate) fn action_at(&self, idx: usize) -> LocalAction {
        self.machine().node(idx).action()
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
            Some(state_index_to_usize(self.machine().node(idx).next()))
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn node_scope_id_at(&self, idx: usize) -> ScopeId {
        self.machine().node(idx).scope()
    }

    #[inline(always)]
    pub(crate) fn try_send_meta_at(&self, idx: usize) -> Option<SendMeta> {
        self.try_send_meta_from_node(idx)
    }

    #[inline(always)]
    pub(crate) fn try_recv_meta_at(&self, idx: usize) -> Option<RecvMeta> {
        self.try_recv_meta_from_node(idx)
    }

    #[inline(always)]
    pub(crate) fn try_local_meta_at(&self, idx: usize) -> Option<LocalMeta> {
        self.try_local_meta_from_node(idx)
    }

    fn try_send_meta_from_node(&self, idx: usize) -> Option<SendMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy,
                lane,
            } => Some(SendMeta::new(
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                node.control_semantic(),
                is_control,
                state_index_to_usize(node.next()),
                node.scope(),
                node.route_arm(),
                shot,
                policy,
                lane,
            )),
            _ => None,
        }
    }

    fn try_recv_meta_from_node(&self, idx: usize) -> Option<RecvMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy,
                lane,
            } => Some(RecvMeta {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                semantic: node.control_semantic(),
                is_control,
                next: state_index_to_usize(node.next()),
                scope: node.scope(),
                route_arm: node.route_arm(),
                is_choice_determinant: node.is_choice_determinant(),
                shot,
                policy,
                lane,
            }),
            _ => None,
        }
    }

    fn try_local_meta_from_node(&self, idx: usize) -> Option<LocalMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Local {
                eff_index,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy,
                lane,
            } => Some(LocalMeta {
                eff_index,
                label,
                frame_label,
                resource,
                semantic: node.control_semantic(),
                is_control,
                next: state_index_to_usize(node.next()),
                scope: node.scope(),
                route_arm: node.route_arm(),
                shot,
                policy,
                lane,
            }),
            _ => None,
        }
    }

    // =========================================================================
    // Scope Queries (delegated to typestate)
    // =========================================================================

    /// Get scope region for current node.
    pub(crate) fn scope_region(&self) -> Option<ScopeRegion> {
        let scope_id = self.machine().node(self.idx_usize()).scope();
        if scope_id.is_none() {
            None
        } else {
            self.scope_region_by_id(scope_id)
        }
    }

    /// Get scope region by scope ID.
    #[inline(always)]
    pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        let mut region = self.machine().scope_region_by_id(scope_id)?;
        region.controller_role = self.machine().route_controller_role(scope_id);
        Some(region)
    }

    #[inline]
    pub(crate) fn first_recv_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        if let Some((policy, _, _, _)) = self.route_scope_controller_policy(scope_id)
            && policy.is_dynamic()
        {
            return None;
        }
        self.first_recv_descendant_target_for_lane_frame_label(scope_id, lane, frame_label)
    }

    /// Resolve an already-observed wire frame label to the branch-local first
    /// recv target recorded by projection metadata.
    ///
    /// This does not grant route authority. Dynamic routes still require a
    /// resolver/controller decision; this lookup is only for validating that
    /// an observed frame belongs to the selected arm before committing decode
    /// progress in split images.
    #[inline]
    pub(crate) fn observed_recv_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.first_recv_descendant_target_for_lane_frame_label(scope_id, lane, frame_label)
    }

    fn first_recv_descendant_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let depth_bound = self
            .machine()
            .role_descriptor()
            .route_scope_count()
            .saturating_add(1);
        self.first_recv_descendant_target_for_lane_frame_label_inner(
            scope_id,
            lane,
            frame_label,
            0,
            depth_bound,
        )
    }

    fn first_recv_descendant_target_for_lane_frame_label_inner(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        depth: usize,
        depth_bound: usize,
    ) -> Option<(u8, StateIndex)> {
        if depth > depth_bound {
            return None;
        }
        let direct =
            self.first_recv_dispatch_target_for_lane_frame_label(scope_id, lane, frame_label);
        if let Some((arm, target)) = direct
            && arm != ARM_SHARED
        {
            return Some((arm, target));
        }

        let mut matched = None;
        let mut arm = 0u8;
        while arm < 2 {
            if let Some(child_scope) = self.passive_arm_scope_by_arm(scope_id, arm)
                && child_scope != scope_id
                && let Some((_child_arm, target)) = self
                    .first_recv_descendant_target_for_lane_frame_label_inner(
                        child_scope,
                        lane,
                        frame_label,
                        depth.saturating_add(1),
                        depth_bound,
                    )
            {
                if matched.is_some_and(|(prev, _)| prev != arm) {
                    return None;
                }
                matched = Some((arm, target));
            }
            arm += 1;
        }
        matched.or(direct)
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
        self.machine().node(self.idx_usize()).scope()
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
        self.machine().scope_parent(scope_id)
    }

    #[inline]
    pub(crate) fn control_parent_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().control_parent(scope_id)
    }

    #[inline]
    pub(crate) fn route_parent_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().route_parent(scope_id)
    }

    #[inline]
    pub(crate) fn route_parent_arm(&self, scope_id: ScopeId) -> Option<u8> {
        self.machine().route_parent_arm(scope_id)
    }

    #[inline]
    pub(crate) fn parallel_scope_root(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().parallel_root(scope_id)
    }

    #[inline]
    pub(crate) fn enclosing_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().enclosing_loop(scope_id)
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
        for i in 0..self.machine().node_len() {
            let node = self.machine().node(i);
            let node_label = match node.action() {
                LocalAction::Send { label: l, .. }
                | LocalAction::Recv { label: l, .. }
                | LocalAction::Local { label: l, .. } => Some(l),
                LocalAction::Terminate => None,
            };
            if node_label == Some(label) {
                return Some(i);
            }
        }
        None
    }

    fn try_index_for_loop_control(&self, meaning: LoopControlMeaning) -> Option<usize> {
        for i in 0..self.machine().node_len() {
            let node = self.machine().node(i);
            let semantic = match node.action() {
                LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Local { .. } => {
                    node.control_semantic()
                }
                LocalAction::Terminate => continue,
            };
            if LoopControlMeaning::from_semantic(semantic) == Some(meaning) {
                return Some(i);
            }
        }
        None
    }

    fn successor_index_for_loop_control(&self, meaning: LoopControlMeaning) -> usize {
        let index = self
            .try_index_for_loop_control(meaning)
            .expect("loop control not found in typestate");
        state_index_to_usize(self.machine().node(index).next())
    }

    fn passive_arm_jump(&self, _scope_id: ScopeId, _arm: u8) -> Option<StateIndex> {
        None
    }

    fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.machine()
            .role_descriptor_ref()
            .passive_arm_entry(scope_id, arm)
    }

    fn route_recv_state(&self, scope_id: ScopeId, target_arm: u8) -> Option<StateIndex> {
        self.machine()
            .role_descriptor_ref()
            .route_recv_state(scope_id, target_arm)
    }

    fn route_arm_count_inner(&self, scope_id: ScopeId) -> Option<u8> {
        self.scope_region_by_id(scope_id).map(|_| 2)
    }

    fn route_scope_offer_lane_set_inner(&self, scope_id: ScopeId) -> Option<LaneSetView> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine()
            .role_descriptor_ref()
            .route_scope_offer_lane_set_by_slot(slot)
    }

    fn route_scope_arm_lane_set_inner(&self, scope_id: ScopeId, arm: u8) -> Option<LaneSetView> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine()
            .role_descriptor_ref()
            .route_scope_arm_lane_set_by_slot(slot, arm)
    }

    fn route_scope_offer_entry_inner(&self, scope_id: ScopeId) -> Option<StateIndex> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine()
            .role_descriptor_ref()
            .route_scope_offer_entry_by_slot(slot)
    }

    fn route_scope_slot_inner(&self, scope_id: ScopeId) -> Option<usize> {
        self.machine()
            .role_descriptor_ref()
            .route_scope_dense_ordinal(scope_id)
    }

    fn first_recv_dispatch_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_target_for_lane_frame_label(scope_id, lane, frame_label)
    }

    #[cfg(test)]
    fn first_recv_dispatch_entry_inner(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, u8, StateIndex)> {
        let len = self.first_recv_dispatch_table_inner(scope_id)?.1 as usize;
        if idx >= len {
            return None;
        }
        let table = self.first_recv_dispatch_table_inner(scope_id)?.0;
        Some(table[idx])
    }

    fn first_recv_dispatch_table_inner(
        &self,
        scope_id: ScopeId,
    ) -> Option<([(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH], u8)> {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_table(scope_id)
    }

    fn first_recv_dispatch_frame_label_mask_inner(
        &self,
        scope_id: ScopeId,
    ) -> crate::transport::FrameLabelMask {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_frame_label_mask(scope_id)
    }

    fn first_recv_dispatch_arm_mask_inner(&self, scope_id: ScopeId) -> u8 {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_arm_mask(scope_id)
    }

    fn first_recv_dispatch_lane_mask_inner(&self, scope_id: ScopeId, arm: u8) -> u8 {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_lane_mask(scope_id, arm)
    }

    fn first_recv_dispatch_arm_frame_label_mask_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> crate::transport::FrameLabelMask {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_arm_frame_label_mask(scope_id, arm)
    }

    fn scope_lane_first_eff_inner(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let region = self.scope_region_by_id(scope_id)?;
        let mut idx = region.start;
        while idx < region.end && idx < self.machine().node_len() {
            match self.machine().node(idx).action() {
                LocalAction::Send {
                    eff_index, lane: l, ..
                }
                | LocalAction::Recv {
                    eff_index, lane: l, ..
                }
                | LocalAction::Local {
                    eff_index, lane: l, ..
                } if l == lane => return Some(eff_index),
                _ => {}
            }
            idx += 1;
        }
        None
    }

    fn scope_lane_last_eff_inner(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let region = self.scope_region_by_id(scope_id)?;
        let mut found = None;
        let mut idx = region.start;
        while idx < region.end && idx < self.machine().node_len() {
            match self.machine().node(idx).action() {
                LocalAction::Send {
                    eff_index, lane: l, ..
                }
                | LocalAction::Recv {
                    eff_index, lane: l, ..
                }
                | LocalAction::Local {
                    eff_index, lane: l, ..
                } if l == lane => found = Some(eff_index),
                _ => {}
            }
            idx += 1;
        }
        found
    }

    fn scope_lane_last_eff_for_arm_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        let region = self.scope_region_by_id(scope_id)?;
        let mut found = None;
        let mut idx = region.start;
        while idx < region.end && idx < self.machine().node_len() {
            let node = self.machine().node(idx);
            if self.node_belongs_to_route_arm(idx, scope_id, arm) {
                match node.action() {
                    LocalAction::Send {
                        eff_index, lane: l, ..
                    }
                    | LocalAction::Recv {
                        eff_index, lane: l, ..
                    }
                    | LocalAction::Local {
                        eff_index, lane: l, ..
                    } if l == lane => found = Some(eff_index),
                    _ => {}
                }
            }
            idx += 1;
        }
        found
    }

    fn node_belongs_to_route_arm(&self, idx: usize, scope_id: ScopeId, arm: u8) -> bool {
        let node = self.machine().node(idx);
        let mut current = node.scope();
        if current.is_none() {
            return false;
        }
        if current == scope_id {
            return node.route_arm() == Some(arm);
        }
        let mut depth = 0usize;
        let depth_bound = self
            .machine()
            .role_descriptor()
            .route_scope_count()
            .saturating_add(1);
        while !current.is_none() && current != scope_id && depth < depth_bound {
            if current.kind() != ScopeKind::Route {
                let Some(parent) = self.scope_parent(current) else {
                    return false;
                };
                current = parent;
                depth += 1;
                continue;
            }
            let Some(parent) = self.route_parent_scope(current) else {
                return false;
            };
            if parent == scope_id {
                return self.route_parent_arm(current) == Some(arm);
            }
            current = parent;
            depth += 1;
        }
        false
    }

    fn controller_arm_entry_for_label_inner(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        self.machine()
            .role_descriptor_ref()
            .controller_arm_entry_for_label(scope_id, label)
    }

    fn controller_arm_entry_by_arm_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.machine()
            .role_descriptor_ref()
            .controller_arm_entry_by_arm(scope_id, arm)
    }

    fn passive_arm_scope_inner(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        let entry = self.passive_arm_entry(scope_id, arm)?;
        let mut current = self.machine().node(state_index_to_usize(entry)).scope();
        if current.is_none() || current == scope_id {
            return None;
        }
        if current.kind() != ScopeKind::Route {
            current = self.route_parent_scope(current)?;
        }
        let mut depth = 0usize;
        let depth_bound = self
            .machine()
            .role_descriptor()
            .route_scope_count()
            .saturating_add(1);
        while !current.is_none() && current != scope_id && depth < depth_bound {
            let parent = self.route_parent_scope(current)?;
            if parent == scope_id {
                return Some(current);
            }
            if parent == current {
                return None;
            }
            current = parent;
            depth += 1;
        }
        None
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
        self.route_recv_state(scope_id, target_arm)
            .map(state_index_to_usize)
    }

    /// Get arm count for a route scope.
    pub(crate) fn route_scope_arm_count(&self, scope_id: ScopeId) -> Option<u8> {
        self.route_arm_count_inner(scope_id)
    }

    /// Get the compiled offer-lane mask for a route scope.
    pub(crate) fn route_scope_offer_lane_set(&self, scope_id: ScopeId) -> Option<LaneSetView> {
        self.route_scope_offer_lane_set_inner(scope_id)
    }

    /// Get the compiled lane mask for one arm of a route scope.
    pub(crate) fn route_scope_arm_lane_set(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView> {
        self.route_scope_arm_lane_set_inner(scope_id, arm)
    }

    /// Get offer entry index for a route scope.
    /// u16::MAX indicates the entry check is disabled (e.g., linger routes).
    pub(crate) fn route_scope_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        self.route_scope_offer_entry_inner(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_slot_inner(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.machine().role_descriptor().route_scope_count()
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, u8, StateIndex)> {
        self.first_recv_dispatch_entry_inner(scope_id, idx)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH], u8)> {
        self.first_recv_dispatch_table_inner(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_frame_label_mask(
        &self,
        scope_id: ScopeId,
    ) -> crate::transport::FrameLabelMask {
        self.first_recv_dispatch_frame_label_mask_inner(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_arm_mask(&self, scope_id: ScopeId) -> u8 {
        self.first_recv_dispatch_arm_mask_inner(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_lane_mask(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> u8 {
        self.first_recv_dispatch_lane_mask_inner(scope_id, arm)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_arm_frame_label_mask(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> crate::transport::FrameLabelMask {
        self.first_recv_dispatch_arm_frame_label_mask_inner(scope_id, arm)
    }

    pub(crate) fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        self.scope_lane_first_eff_inner(scope_id, lane)
    }

    pub(crate) fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        self.scope_lane_last_eff_inner(scope_id, lane)
    }

    pub(crate) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_lane_last_eff_for_arm_inner(scope_id, arm, lane)
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
        self.controller_arm_entry_for_label_inner(scope_id, label)
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
        self.controller_arm_entry_by_arm_inner(scope_id, arm)
    }

    #[inline]
    pub(crate) fn shared_controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.controller_arm_entry_by_arm_inner(scope_id, arm)
    }

    #[inline]
    pub(crate) fn control_semantic_at(&self, idx: usize) -> ControlSemanticKind {
        self.machine().node(idx).control_semantic()
    }

    #[inline]
    pub(crate) fn passive_arm_scope_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        self.passive_arm_scope_inner(scope_id, arm)
    }

    /// Get route controller policy metadata.
    ///
    /// The tuple `(PolicyMode, EffIndex, u8, ControlOp)` corresponds to the
    /// controller-provided
    /// policy mode, the effect index of the send action that declared it, and the
    /// control descriptor metadata embedded in the DSL. Route policies are tracked
    /// for both generic route decisions and loop-based routing.
    pub(crate) fn route_scope_controller_policy(
        &self,
        scope_id: ScopeId,
    ) -> Option<(
        PolicyMode,
        EffIndex,
        u8,
        crate::control::cap::mint::ControlOp,
    )> {
        self.machine().route_controller(scope_id)
    }

    // =========================================================================
    // Metadata Extraction
    // =========================================================================

    /// Try to get send metadata at the current cursor location.
    /// Returns `None` if the current node is not a Send action.
    pub(crate) fn try_send_meta(&self) -> Option<SendMeta> {
        self.try_send_meta_from_node(self.idx_usize())
    }

    /// Try to get receive metadata at the current cursor location.
    /// Returns `None` if the current node is not a Recv action.
    pub(crate) fn try_recv_meta(&self) -> Option<RecvMeta> {
        self.try_recv_meta_from_node(self.idx_usize())
    }

    /// Try to get local action metadata at the current cursor location.
    /// Returns `None` if the current node is not a Local action.
    pub(crate) fn try_local_meta(&self) -> Option<LocalMeta> {
        self.try_local_meta_from_node(self.idx_usize())
    }

    // =========================================================================
    // Loop Metadata
    // =========================================================================

    /// Get loop metadata for current scope.
    pub(crate) fn loop_metadata_inner(&self) -> Option<LoopMetadata> {
        let node = self.machine().node(self.idx_usize());
        let action = node.action();
        let role = self.machine().role();
        let (eff_index, controller, target, role_kind) = match action {
            LocalAction::Send {
                eff_index, peer, ..
            } => (eff_index, role, peer, LoopRole::Controller),
            LocalAction::Recv {
                eff_index, peer, ..
            } => (eff_index, peer, role, LoopRole::Target),
            _ => return None,
        };
        if LoopControlMeaning::from_semantic(node.control_semantic())
            != Some(LoopControlMeaning::Continue)
        {
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
