//! Mutable phase and runtime cursor logic for typestate execution.

use core::ptr::NonNull;

use super::{
    builder::{RoleTypestateValue, ScopeRegion},
    facts::{
        JumpError, JumpReason, LocalAction, LocalMeta, LocalNode, PassiveArmNavigation, RecvMeta,
        SendMeta, StateIndex, as_state_index, state_index_to_usize, try_local_meta, try_recv_meta,
        try_send_meta,
    },
};
use crate::{
    eff::EffIndex,
    global::{
        LoopControlMeaning,
        compiled::CompiledRole,
        const_dsl::{PolicyMode, ScopeId, ScopeKind},
        role_program::{LocalStep, MAX_LANES, Phase},
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
    pub decision_index: usize,
    pub continue_index: usize,
    pub break_index: usize,
}

// =============================================================================
// =============================================================================

/// Maximum phases and steps that PhaseCursor can hold.
const PHASE_CURSOR_MAX_PHASES: usize = 32;
const PHASE_CURSOR_MAX_STEPS: usize = crate::eff::meta::MAX_EFF_NODES;
const PHASE_CURSOR_NO_STEP: u16 = u16::MAX;
const PHASE_CURSOR_NO_STATE: StateIndex = StateIndex::MAX;

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
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseCursor {
    compiled: NonNull<CompiledRole>,
    /// Primary typestate index used for scope queries.
    idx: usize,

    /// Current phase index (0-based)
    phase_index: usize,
    /// Per-lane step progress within current phase.
    /// `lane_cursors[lane_idx]` = number of steps completed on that lane.
    lane_cursors: [usize; MAX_LANES],
    /// Label → lane bitmask for the current step on each lane.
    /// Updated when lane cursors advance.
    label_lane_mask: [u8; 256],
}

impl PhaseCursor {
    #[inline(always)]
    fn compiled(&self) -> &CompiledRole {
        // SAFETY: CursorEndpoint holds a pinned compiled-cache lease for the
        // entire endpoint lifetime, and PhaseCursor never mutates the compiled
        // facts behind this stable slot.
        unsafe { self.compiled.as_ref() }
    }

    #[inline(always)]
    fn typestate(&self) -> &RoleTypestateValue {
        self.compiled().typestate()
    }

    #[inline(always)]
    fn phase_len(&self) -> usize {
        self.compiled().phase_count().min(PHASE_CURSOR_MAX_PHASES)
    }

    #[inline(always)]
    fn local_steps_len(&self) -> usize {
        self.compiled().step_count().min(PHASE_CURSOR_MAX_STEPS)
    }

    // =========================================================================
    // Construction
    // =========================================================================

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn new(compiled: &CompiledRole) -> Self {
        Self::from_pinned_role_ptr(NonNull::from(compiled))
    }

    #[inline(always)]
    pub(crate) fn from_pinned_role_ptr(compiled: NonNull<CompiledRole>) -> Self {
        let mut cursor = Self {
            compiled,
            idx: 0,
            phase_index: 0,
            lane_cursors: [0; MAX_LANES],
            label_lane_mask: [0; 256],
        };
        cursor.rebuild_label_lane_mask();
        cursor
    }

    // =========================================================================
    // =========================================================================

    /// Get the current phase, if any.
    #[inline]
    pub(crate) fn current_phase(&self) -> Option<Phase> {
        if self.phase_index < self.phase_len() {
            self.compiled().phase(self.phase_index).copied()
        } else {
            None
        }
    }

    // =========================================================================
    // Lane Access
    // =========================================================================

    fn current_label_for_lane(&self, lane_idx: usize) -> Option<u8> {
        self.step_at_lane(lane_idx).map(|step| step.label())
    }

    fn rebuild_label_lane_mask(&mut self) {
        self.label_lane_mask = [0; 256];
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(label) = self.current_label_for_lane(lane_idx) {
                self.label_lane_mask[label as usize] |= 1u8 << (lane_idx as u32);
            }
            lane_idx += 1;
        }
    }

    fn update_label_lane_mask(
        &mut self,
        lane_idx: usize,
        old_label: Option<u8>,
        new_label: Option<u8>,
    ) {
        let bit = 1u8 << (lane_idx as u32);
        if let Some(label) = old_label {
            self.label_lane_mask[label as usize] &= !bit;
        }
        if let Some(label) = new_label {
            self.label_lane_mask[label as usize] |= bit;
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
    pub(crate) fn find_step_for_label(&self, target_label: u8) -> Option<(usize, LocalStep)> {
        let lane_mask = self.label_lane_mask[target_label as usize];
        if lane_mask == 0 {
            return None;
        }
        let lane_idx = lane_mask.trailing_zeros() as usize;
        let step = self.step_at_lane(lane_idx)?;
        if step.label() != target_label {
            debug_assert!(false, "label lane mask out of sync");
            return None;
        }
        Some((lane_idx, step))
    }

    /// Get the step at the current cursor position for a specific lane.
    pub(crate) fn step_at_lane(&self, lane_idx: usize) -> Option<LocalStep> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        self.compiled().step(step_idx).copied()
    }

    /// Get the step index at the current cursor position for a specific lane.
    pub(crate) fn step_index_at_lane(&self, lane_idx: usize) -> Option<usize> {
        let phase = self.current_phase()?;

        if lane_idx >= MAX_LANES {
            return None;
        }

        let lane_steps = &phase.lanes[lane_idx];
        if !lane_steps.is_active() {
            return None;
        }

        let cursor_pos = self.lane_cursors[lane_idx];
        let step_idx = lane_steps.start + cursor_pos;
        if cursor_pos >= lane_steps.len || step_idx >= self.local_steps_len() {
            return None;
        }

        Some(step_idx)
    }

    pub(crate) fn index_for_lane_step(&self, lane_idx: usize) -> Option<usize> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        let state_idx = self.compiled().state_for_step_index(step_idx)?;
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(
                false,
                "missing typestate index for lane step idx={}",
                step_idx
            );
            return None;
        }
        Some(state_idx.as_usize())
    }

    // =========================================================================
    // =========================================================================

    /// Set cursor for a specific lane to the step matching `eff_index`.
    ///
    /// Unlike `advance_lane_to_eff_index`, this positions the lane cursor at the
    /// step itself (not past it). Used for loop rewinds.
    pub(crate) fn set_lane_cursor_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        let Some(phase) = self.current_phase() else {
            return;
        };
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_steps = &phase.lanes[lane_idx];
        if !lane_steps.is_active() {
            return;
        }
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for phase cursor");
            return;
        }
        let step_idx = self
            .compiled()
            .step_for_eff_index(eff_idx)
            .unwrap_or(PHASE_CURSOR_NO_STEP);
        if step_idx == PHASE_CURSOR_NO_STEP {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "step index out of bounds for local steps");
            return;
        }
        let start = lane_steps.start;
        let end = start.saturating_add(lane_steps.len);
        if step_idx < start || step_idx >= end {
            debug_assert!(
                false,
                "eff_index not in current lane scope: eff_index={} lane={}",
                eff_index, lane_idx
            );
            return;
        }
        let old_label = self.current_label_for_lane(lane_idx);
        let target = step_idx.saturating_sub(start);
        self.lane_cursors[lane_idx] = target;
        let new_label = self.current_label_for_lane(lane_idx);
        self.update_label_lane_mask(lane_idx, old_label, new_label);
    }

    /// Advance cursor for a specific lane to the step matching `eff_index`.
    pub(crate) fn advance_lane_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        let Some(phase) = self.current_phase() else {
            return;
        };
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_steps = &phase.lanes[lane_idx];
        if !lane_steps.is_active() {
            return;
        }
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for phase cursor");
            return;
        }
        let step_idx = self
            .compiled()
            .step_for_eff_index(eff_idx)
            .unwrap_or(PHASE_CURSOR_NO_STEP);
        if step_idx == PHASE_CURSOR_NO_STEP {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "step index out of bounds for local steps");
            return;
        }
        let start = lane_steps.start;
        let end = start.saturating_add(lane_steps.len);
        if step_idx < start || step_idx >= end {
            debug_assert!(
                false,
                "eff_index not in current lane scope: eff_index={} lane={}",
                eff_index, lane_idx
            );
            return;
        }
        let target = step_idx.saturating_sub(start) + 1;
        if target > self.lane_cursors[lane_idx] {
            let old_label = self.current_label_for_lane(lane_idx);
            self.lane_cursors[lane_idx] = target;
            let new_label = self.current_label_for_lane(lane_idx);
            self.update_label_lane_mask(lane_idx, old_label, new_label);
        }
    }

    /// Advance to next phase without syncing the primary typestate index.
    #[inline]
    pub(crate) fn advance_phase_without_sync(&mut self) {
        self.phase_index += 1;
        self.lane_cursors = [0; MAX_LANES];
        self.rebuild_label_lane_mask();
    }

    pub(crate) fn sync_idx_to_phase_start(&mut self) {
        let Some(phase) = self.current_phase() else {
            return;
        };
        if phase.lane_mask == 0 {
            return;
        };
        let step_idx = phase.min_start;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "phase start out of local steps range");
            return;
        }
        let state_idx = self
            .compiled()
            .state_for_step_index(step_idx)
            .unwrap_or(PHASE_CURSOR_NO_STATE);
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        }
        self.idx = state_idx.as_usize();
    }

    /// Check if all lanes in current phase are complete.
    pub(crate) fn is_phase_complete(&self) -> bool {
        let Some(phase) = self.current_phase() else {
            return true; // No more phases
        };

        let mut lane_mask = phase.lane_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= lane_mask - 1;
            let lane_steps = &phase.lanes[lane_idx];
            if self.lane_cursors[lane_idx] < lane_steps.len {
                return false;
            }
        }
        true
    }

    // =========================================================================
    // Core Typestate Navigation
    // =========================================================================

    /// Current typestate index.
    #[inline(always)]
    pub(crate) const fn index(&self) -> usize {
        self.idx
    }

    /// Access a typestate node by index.
    #[inline(always)]
    pub(crate) fn typestate_node(&self, index: usize) -> LocalNode {
        self.typestate().node(index)
    }

    #[inline(always)]
    fn action(&self) -> LocalAction {
        self.typestate().node(self.idx).action()
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

    /// Returns the jump target index if the current node is a Jump action.
    #[inline(always)]
    pub(crate) fn jump_target(&self) -> Option<usize> {
        if self.is_jump() {
            Some(state_index_to_usize(self.typestate().node(self.idx).next()))
        } else {
            None
        }
    }

    /// Returns the label associated with the current typestate node.
    #[inline(always)]
    pub(crate) fn label(&self) -> Option<u8> {
        match self.action() {
            LocalAction::Send { label, .. }
            | LocalAction::Recv { label, .. }
            | LocalAction::Local { label, .. } => Some(label),
            LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => None,
        }
    }

    /// Advance typestate index to the successor.
    #[inline(always)]
    pub(crate) fn advance(self) -> Self {
        let next = state_index_to_usize(self.typestate().node(self.idx).next());
        Self { idx: next, ..self }
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
    pub(crate) fn try_follow_jumps(self) -> Result<Self, JumpError> {
        let mut cursor = self;
        let mut iter = 0u32;
        while cursor.is_jump() {
            match cursor.action().jump_reason() {
                Some(JumpReason::PassiveObserverBranch) => {
                    // Decision point: stop for offer() to handle arm selection.
                    // Even when an arm is τ-eliminated, the decision is still required.
                    return Ok(cursor);
                }
                _ => {
                    // Follow all other Jump nodes (LoopContinue, LoopBreak, RouteArmEnd)
                    cursor = cursor.advance();
                    iter += 1;
                    if iter > crate::eff::meta::MAX_EFF_NODES as u32 {
                        return Err(JumpError {
                            iterations: iter,
                            idx: cursor.idx,
                        });
                    }
                }
            }
        }
        Ok(cursor)
    }

    /// Advance to the next node, then follow Jump nodes.
    ///
    /// Returns `Err(JumpError)` if the Jump chain exceeds MAX_EFF_NODES iterations.
    #[inline(always)]
    pub(crate) fn try_advance_past_jumps(self) -> Result<Self, JumpError> {
        self.advance().try_follow_jumps()
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
        } else if let Some(entry_idx) = typestate.passive_arm_entry(scope_id, target_arm) {
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

    /// Find the route arm containing a Send/Local node with the specified label.
    ///
    /// Uses O(1) registry lookup via `passive_arm_jump()` or `passive_arm_entry()`
    /// to check each arm's entry point label, avoiding full scope scan.
    ///
    /// For 2-arm routes, this performs at most 2 registry lookups + 2 node reads.
    pub(crate) fn find_arm_for_send_label(&self, target_label: u8) -> Option<u8> {
        let scope_region = self.scope_region()?;
        let scope_id = scope_region.scope_id;
        let typestate = self.typestate();

        // O(1) per arm: check arm entry node labels
        // 2-arm route constraint means at most 2 iterations
        for arm in 0..2u8 {
            // First try PassiveObserverBranch Jump (for linger routes)
            let entry_idx = if let Some(jump_node_idx) = typestate.passive_arm_jump(scope_id, arm) {
                let jump_node = typestate.node(state_index_to_usize(jump_node_idx));
                Some(state_index_to_usize(jump_node.next()))
            } else {
                // Direct entry path for non-linger routes.
                typestate
                    .passive_arm_entry(scope_id, arm)
                    .map(state_index_to_usize)
            };

            if let Some(target_idx) = entry_idx {
                let entry_node = typestate.node(target_idx);

                // Check arm entry node's label
                match entry_node.action() {
                    LocalAction::Send { label, .. } | LocalAction::Local { label, .. }
                        if label == target_label =>
                    {
                        return Some(arm);
                    }
                    _ => {}
                }
            }
        }
        None
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
        let typestate = self.typestate();

        // FIRST-recv dispatch: O(1) lookup returns (arm, target_idx) directly.
        // The arm is now stored in the dispatch table, eliminating positional inference.
        if let Some((arm, _target_idx)) = typestate.first_recv_target(scope_id, target_label) {
            if arm == super::super::typestate::ARM_SHARED {
                return Some(0);
            }
            return Some(arm);
        }

        // Bounded O(4) scan of arm entry node labels for τ-eliminated or local-only arms.
        for arm in 0..2u8 {
            let entry_idx = if let Some(jump_node_idx) = typestate.passive_arm_jump(scope_id, arm) {
                let jump_node = typestate.node(state_index_to_usize(jump_node_idx));
                Some(state_index_to_usize(jump_node.next()))
            } else {
                typestate
                    .passive_arm_entry(scope_id, arm)
                    .map(state_index_to_usize)
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

    /// Follow a PassiveObserverBranch to the arm containing the specified label.
    ///
    /// This combines `find_arm_for_send_label` and `follow_passive_observer_arm`
    /// to directly navigate to the correct position for a passive observer.
    pub(crate) fn follow_passive_observer_for_label(&self, label: u8) -> Option<Self> {
        let target_arm = self.find_arm_for_send_label(label)?;
        let PassiveArmNavigation::WithinArm { entry } =
            self.follow_passive_observer_arm(target_arm)?;
        Some(self.with_index(state_index_to_usize(entry)))
    }

    /// Create a cursor at a specific typestate index.
    pub(crate) fn with_index(&self, idx: usize) -> Self {
        debug_assert!(idx < self.typestate().len());
        Self { idx, ..*self }
    }

    // =========================================================================
    // Scope Queries (delegated to typestate)
    // =========================================================================

    /// Get scope region for current node.
    pub(crate) fn scope_region(&self) -> Option<ScopeRegion> {
        let typestate = self.typestate();
        let scope_id = typestate.node(self.idx).scope();
        if scope_id.is_none() {
            None
        } else {
            typestate.scope_region_for(scope_id)
        }
    }

    /// Get scope region by scope ID.
    #[inline(always)]
    pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.typestate().scope_region_for(scope_id)
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
        self.typestate().first_recv_target(scope_id, label)
    }

    #[inline]
    pub(crate) fn first_recv_target_evidence(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.typestate().first_recv_target(scope_id, label)
    }

    /// Check if this role is the controller for the given route scope.
    ///
    /// Uses type-level `controller_role` from `ScopeRegion` (propagated from
    /// binary route construction via `ScopeMarker`). This eliminates runtime
    /// inference based on `controller_arm_entry` presence.
    ///
    /// Returns `true` if `controller_role == self.compiled.role()`, `false` otherwise.
    #[inline]
    pub(crate) fn is_route_controller(&self, scope_id: ScopeId) -> bool {
        self.scope_region_by_id(scope_id)
            .and_then(|region| region.controller_role)
            .map_or(false, |ctrl| ctrl == self.compiled().role())
    }

    /// Get scope ID at current position.
    #[cfg(test)]
    pub(crate) fn scope_id(&self) -> Option<ScopeId> {
        self.scope_region().map(|region| region.scope_id)
    }

    /// Scope ID stored on the current node (no parent traversal).
    #[inline(always)]
    pub(crate) fn node_scope_id(&self) -> ScopeId {
        self.typestate().node(self.idx).scope()
    }

    /// Get scope kind at current position.
    #[cfg(test)]
    pub(crate) fn scope_kind(&self) -> Option<ScopeKind> {
        self.scope_region().map(|region| region.kind)
    }

    /// Advance past the current scope if it matches the given kind.
    pub(crate) fn advance_scope_if_kind(&self, kind: ScopeKind) -> Option<Self> {
        let region = self.scope_region()?;
        if region.kind == kind {
            Some(self.with_index(region.end))
        } else {
            None
        }
    }

    /// Advance past a scope by ID.
    ///
    /// If cursor is already at or beyond scope.end, returns None since no
    /// advancement is needed (cursor has already exited the scope).
    pub(crate) fn advance_scope_by_id(&self, scope_id: ScopeId) -> Option<Self> {
        let region = self.scope_region_by_id(scope_id)?;
        // Only advance if cursor is still inside the scope
        if self.idx < region.end {
            Some(self.with_index(region.end))
        } else {
            // Cursor already at or beyond scope.end - no advancement needed
            None
        }
    }

    /// Get parent scope.
    pub(crate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.typestate().scope_parent(scope_id)
    }

    // =========================================================================
    // Label Seeking
    // =========================================================================

    /// Find cursor at node with given label.
    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn seek_label(&self, label: u8) -> Option<Self> {
        let typestate = self.typestate();
        for i in 0..typestate.len() {
            let node = typestate.node(i);
            let node_label = match node.action() {
                LocalAction::Send { label: l, .. }
                | LocalAction::Recv { label: l, .. }
                | LocalAction::Local { label: l, .. } => Some(l),
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => None,
            };
            if node_label == Some(label) {
                return Some(self.with_index(i));
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
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => None,
            };
            if LoopControlMeaning::from_resource_tag(resource) == Some(meaning) {
                return Some(i);
            }
        }
        None
    }

    fn successor_for_loop_control(&self, meaning: LoopControlMeaning) -> Self {
        let index = self
            .try_index_for_loop_control(meaning)
            .expect("loop control not found in typestate");
        self.with_index(index).advance()
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

    /// Get offer lanes list for a route scope.
    /// Returns the lane list and its length for the first recv nodes in the scope.
    pub(crate) fn route_scope_offer_lane_list(
        &self,
        scope_id: ScopeId,
    ) -> Option<([u8; MAX_LANES], usize)> {
        self.typestate().route_offer_lane_list(scope_id)
    }

    /// Get offer entry index for a route scope.
    /// u16::MAX indicates the entry check is disabled (e.g., linger routes).
    pub(crate) fn route_scope_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        self.typestate().route_offer_entry(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.compiled().first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        self.typestate().route_scope_slot(scope_id)
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
        self.typestate()
            .controller_arm_entry_for_label(scope_id, label)
    }

    /// Check if the cursor is at a controller arm entry for the given scope.
    /// Used by flow() to determine if arm repositioning is valid.
    pub(crate) fn is_at_controller_arm_entry(&self, scope_id: ScopeId) -> bool {
        self.typestate()
            .is_at_controller_arm_entry(scope_id, as_state_index(self.idx))
    }

    /// Get the controller arm entry (index, label) for a given arm number.
    /// Used by offer() to navigate to the selected arm's entry point.
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.compiled().controller_arm_entry_by_arm(scope_id, arm)
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
        self.typestate().route_controller(scope_id)
    }

    // =========================================================================
    // Metadata Extraction
    // =========================================================================

    /// Try to get send metadata at the current cursor location.
    /// Returns `None` if the current node is not a Send action.
    pub(crate) fn try_send_meta(&self) -> Option<SendMeta> {
        try_send_meta(self.typestate(), self.idx)
    }

    /// Try to get receive metadata at the current cursor location.
    /// Returns `None` if the current node is not a Recv action.
    pub(crate) fn try_recv_meta(&self) -> Option<RecvMeta> {
        try_recv_meta(self.typestate(), self.idx)
    }

    /// Try to get local action metadata at the current cursor location.
    /// Returns `None` if the current node is not a Local action.
    pub(crate) fn try_local_meta(&self) -> Option<LocalMeta> {
        try_local_meta(self.typestate(), self.idx)
    }

    // =========================================================================
    // Loop Metadata
    // =========================================================================

    /// Get loop metadata for current scope.
    pub(crate) fn loop_metadata_inner(&self) -> Option<LoopMetadata> {
        let node = self.typestate().node(self.idx);
        let action = node.action();
        let role = self.compiled().role();
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
        let scope = node.loop_scope()?;
        let continue_index = self
            .successor_for_loop_control(LoopControlMeaning::Continue)
            .index();
        let break_index = self
            .successor_for_loop_control(LoopControlMeaning::Break)
            .index();
        Some(LoopMetadata {
            scope,
            controller,
            target,
            role: role_kind,
            eff_index,
            decision_index: self.idx,
            continue_index,
            break_index,
        })
    }

    // =========================================================================
    // Terminal State
    // =========================================================================

    /// Assert that the cursor is at a terminal state.
    #[cfg(test)]
    pub(crate) fn assert_terminal(&self) {
        assert!(
            matches!(self.action(), LocalAction::Terminate),
            "cursor at index {} is not terminal",
            self.idx
        );
    }
}
