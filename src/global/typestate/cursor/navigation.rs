use super::{
    EventCursor, LocalAction, LocalMeta, LocalNode, RecvMeta, ScopeId, SendMeta, StateIndex,
    state_index_to_usize,
};
impl EventCursor {
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
    pub(crate) fn node_next_index_at(&self, index: usize) -> StateIndex {
        self.machine().node(index).next()
    }

    #[inline(always)]
    pub(crate) fn node_route_arm_at(&self, index: usize) -> Option<u8> {
        self.machine().node(index).route_arm()
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

    /// Navigate to the projected passive arm entry for a given route scope.
    ///
    /// This takes an explicit `scope_id` instead of deriving it from the
    /// cursor's current node, keeping route-arm navigation tied to descriptor
    /// facts chosen by the caller.
    ///
    /// Returns the arm entry index.
    /// For τ-eliminated arms with no cross-role content, returns the terminal arm row.
    ///
    /// Navigation uses the projected passive arm-entry authority. τ-eliminated
    /// arms with no cross-role content are sealed at compile time with an
    /// terminal RouteArmEnd row, so the entry remains present through the same
    /// descriptor row.
    pub(crate) fn follow_passive_observer_arm_for_scope(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> Option<StateIndex> {
        if !self.is_route_controller(scope_id)
            && let Some(entry_idx) = self.passive_arm_entry(scope_id, target_arm)
        {
            Some(entry_idx)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn set_index(&mut self, idx: usize) {
        if idx >= self.machine().node_len() {
            crate::invariant();
        }
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
    pub(crate) fn node_scope_id_at(&self, idx: usize) -> ScopeId {
        self.machine().node(idx).scope()
    }

    #[inline(always)]
    pub(crate) fn checked_node_scope_id_at(&self, idx: usize) -> Option<ScopeId> {
        Some(self.machine().checked_node(idx)?.scope())
    }

    #[inline(always)]
    pub(crate) fn route_arm_at(&self, idx: usize) -> Option<u8> {
        self.machine().node(idx).route_arm()
    }

    #[inline(always)]
    pub(crate) fn current_route_arm(&self) -> Option<u8> {
        self.route_arm_at(self.idx_usize())
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

    #[inline]
    fn route_authority_at(&self, idx: usize, route_arm: Option<u8>) -> (ScopeId, Option<u8>) {
        let Some(region) = self.enclosing_route_scope_rows_at(idx) else {
            return (ScopeId::none(), None);
        };
        let scope = region.scope();
        let selected_arm = route_arm.or_else(|| self.route_arm_for_index(scope, idx));
        (scope, selected_arm)
    }

    pub(super) fn try_send_meta_from_node(&self, idx: usize) -> Option<SendMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                resolver,
                lane,
            } => {
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, selected_route_arm) = self.route_authority_at(idx, route_arm);
                Some(SendMeta {
                    eff_index,
                    peer,
                    label,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    selected_route_arm,
                    resolver,
                    lane,
                })
            }
            LocalAction::Recv { .. } | LocalAction::Local { .. } | LocalAction::Terminate => None,
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
                origin,
                resolver,
                lane,
            } => {
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, _) = self.route_authority_at(idx, route_arm);
                Some(RecvMeta {
                    eff_index,
                    peer,
                    label,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    choice: node.choice_mark(),
                    resolver,
                    lane,
                })
            }
            LocalAction::Send { .. } | LocalAction::Local { .. } | LocalAction::Terminate => None,
        }
    }

    pub(super) fn try_local_meta_from_node(&self, idx: usize) -> Option<LocalMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Local {
                eff_index,
                label,
                frame_label,
                origin,
                resolver,
                lane,
            } => {
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, _) = self.route_authority_at(idx, route_arm);
                Some(LocalMeta {
                    eff_index,
                    label,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    resolver,
                    lane,
                })
            }
            LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Terminate => None,
        }
    }
}
