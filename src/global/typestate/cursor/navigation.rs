#[cfg(test)]
use super::ARM_SHARED;
use super::{
    JumpError, JumpReason, LocalAction, LocalMeta, LocalNode, PassiveArmNavigation, PhaseCursor,
    RecvMeta, ScopeId, SendMeta, StateIndex, state_index_to_usize,
};
impl PhaseCursor {
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

    /// Returns `true` when the cursor points at a receive action.
    #[inline(always)]
    pub(crate) fn is_recv(&self) -> bool {
        self.action().is_recv()
    }

    #[inline(always)]
    pub(crate) fn is_terminal(&self) -> bool {
        self.action().is_terminal()
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

    /// Return the index reached by advancing once from a preview index, then
    /// following Jump nodes. This is a preview operation and does not mutate the
    /// cursor.
    #[inline(never)]
    pub(crate) fn try_next_index_past_jumps_from(
        &self,
        idx: StateIndex,
    ) -> Result<StateIndex, JumpError> {
        let next = self.machine().node(state_index_to_usize(idx)).next();
        self.try_follow_jumps_from_index(next)
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
            if arm == ARM_SHARED {
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

    pub(super) fn try_send_meta_from_node(&self, idx: usize) -> Option<SendMeta> {
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

    pub(super) fn try_recv_meta_from_node(&self, idx: usize) -> Option<RecvMeta> {
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

    pub(super) fn try_local_meta_from_node(&self, idx: usize) -> Option<LocalMeta> {
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
}
