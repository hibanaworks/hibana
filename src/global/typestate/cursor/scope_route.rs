use super::{
    ARM_SHARED, ControlSemanticKind, EffIndex, FirstRecvDispatchSpec, LaneSetView, LocalAction,
    LocalMeta, LoopControlMeaning, LoopMetadata, LoopRole, MAX_FIRST_RECV_DISPATCH, PhaseCursor,
    PolicyMode, RecvMeta, ScopeId, ScopeKind, ScopeRegion, SendMeta, StateIndex, as_state_index,
    state_index_to_usize,
};
impl PhaseCursor {
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

    pub(super) fn passive_arm_jump(&self, _scope_id: ScopeId, _arm: u8) -> Option<StateIndex> {
        None
    }

    pub(super) fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
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

    fn route_scope_offer_lane_set_inner(&self, scope_id: ScopeId) -> Option<LaneSetView<'static>> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        self.machine()
            .role_descriptor_ref()
            .route_scope_offer_lane_set_by_slot(slot)
    }

    fn route_scope_arm_lane_set_inner(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
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

    pub(super) fn first_recv_dispatch_target_for_lane_frame_label(
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
    ) -> Option<FirstRecvDispatchSpec> {
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
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        self.machine()
            .role_descriptor_ref()
            .first_recv_dispatch_table(scope_id)
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
    pub(crate) fn route_scope_offer_lane_set(
        &self,
        scope_id: ScopeId,
    ) -> Option<LaneSetView<'static>> {
        self.route_scope_offer_lane_set_inner(scope_id)
    }

    /// Get the compiled lane mask for one arm of a route scope.
    pub(crate) fn route_scope_arm_lane_set(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
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
    ) -> Option<FirstRecvDispatchSpec> {
        self.first_recv_dispatch_entry_inner(scope_id, idx)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        self.first_recv_dispatch_table_inner(scope_id)
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
