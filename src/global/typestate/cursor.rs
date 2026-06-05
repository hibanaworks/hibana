//! Mutable phase and runtime cursor logic for typestate execution.

use core::slice;

use super::facts::{
    ARM_SHARED, FirstRecvDispatchSpec, JumpError, JumpReason, LocalAction, LocalMeta, LocalNode,
    MAX_FIRST_RECV_DISPATCH, PassiveArmNavigation, RecvMeta, ScopeRegion, SendMeta, StateIndex,
    as_state_index, state_index_to_usize,
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
pub(crate) struct ResidentLaneStep {
    phase: u8,
    lane: u8,
    ordinal: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResidentLaneStepError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorRefresh {
    Lane(u8),
    Phase,
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
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
    fn phase_lane_set(&self, idx: usize) -> Option<LaneSetView<'static>> {
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
    fn phase_lane_step_at(&self, idx: usize, lane_idx: usize, ordinal: usize) -> Option<u16> {
        self.role_descriptor()
            .phase_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    fn phase_lane_step_ordinal(&self, idx: usize, lane_idx: usize, step_idx: usize) -> Option<u16> {
        self.role_descriptor()
            .phase_lane_step_ordinal(idx, lane_idx, step_idx)
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
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { &*self.state }
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut PhaseCursorState {
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
        state: *mut PhaseCursorState,
        lane_cursors: *mut u16,
        current_step_label_codes: *mut u16,
        role_descriptor: RoleDescriptorRef,
    ) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
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
    pub(crate) fn current_phase_lane_set(&self) -> LaneSetView<'static> {
        self.machine()
            .phase_lane_set(self.phase_index_usize())
            .unwrap_or(LaneSetView::EMPTY)
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

    #[inline(always)]
    fn current_phase_lane_step_at(&self, lane_idx: usize, ordinal: usize) -> Option<usize> {
        self.machine()
            .phase_lane_step_at(self.phase_index_usize(), lane_idx, ordinal)
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
