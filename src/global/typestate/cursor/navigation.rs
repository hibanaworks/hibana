use super::{
    EventCursor, LocalAction, LocalMeta, LocalNode, RecvMeta, ScopeId, SendMeta, StateIndex,
    state_index_to_usize,
};
use crate::global::role_program::RoleLocalDirection;
impl EventCursor {
    #[inline]
    pub(super) fn outbound_candidates<'a>(
        &'a self,
        selector: crate::global::typestate::MessageSelector,
        source: core::ops::Range<usize>,
        indexed: impl Iterator<Item = usize> + 'a,
    ) -> impl Iterator<Item = usize> + 'a {
        let repeatable = self.has_reentry_scopes();
        indexed.filter(
            #[inline(always)]
            move |&index| {
                if !repeatable && self.local_event_done(index) {
                    return false;
                }
                let (event, direction) =
                    crate::invariant_some(self.machine().event_program().operation_at(index));
                if direction == RoleLocalDirection::Recv || !source.contains(&event.dense_ordinal())
                {
                    return false;
                }
                let atom = self.program_ref().event_atom_at(event.dense_ordinal());
                atom.from == selector.role
                    && atom.label == selector.label
                    && atom.payload == selector.payload_kind
            },
        )
    }

    /// Completed rows still identify a contract before live admission.
    pub(super) fn source_has_outbound_contract(
        &self,
        selector: crate::global::typestate::MessageSelector,
        source: core::ops::Range<usize>,
        mut indexed: impl Iterator<Item = usize>,
    ) -> bool {
        indexed.any(|index| {
            let (event, _) =
                crate::invariant_some(self.machine().event_program().operation_at(index));
            source.contains(&event.dense_ordinal())
                && self
                    .program_ref()
                    .event_atom_at(event.dense_ordinal())
                    .payload
                    == selector.payload_kind
        })
    }

    #[inline]
    pub(crate) fn receive_candidates(
        &self,
        role: u8,
        label: u8,
    ) -> impl Iterator<Item = usize> + '_ {
        // Only explicit reentry can make a completed row available again.
        // Reuse the existing completion authority before decoding candidates.
        let repeatable = self.has_reentry_scopes();
        let mut candidates = self
            .machine()
            .event_program()
            .receive_candidates(role, label);
        core::iter::from_fn(
            #[inline(always)]
            move || loop {
                let index = candidates.next()?;
                if repeatable || !self.local_event_done(index) {
                    return Some(index);
                }
            },
        )
    }
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
        if !self.contains_route_controller(scope_id)
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
        self.machine()
            .event_program()
            .operation_at(idx)
            .is_some_and(|(_, direction)| direction == RoleLocalDirection::Recv)
    }

    #[inline(always)]
    pub(crate) fn is_send_at(&self, idx: usize) -> bool {
        self.machine()
            .event_program()
            .operation_at(idx)
            .is_some_and(|(_, direction)| direction == RoleLocalDirection::Send)
    }

    #[inline(always)]
    pub(crate) fn is_local_action_at(&self, idx: usize) -> bool {
        self.machine()
            .event_program()
            .operation_at(idx)
            .is_some_and(|(_, direction)| direction == RoleLocalDirection::Local)
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

    /// Logical operation owner, independent of the physical actor holding it.
    pub(crate) fn logical_role_at(&self, idx: usize) -> Option<u8> {
        let (eff, direction) = self.machine().event_program().operation_at(idx)?;
        let atom = self.program_ref().event_atom_at(eff.dense_ordinal());
        Some(if direction == RoleLocalDirection::Recv {
            atom.to
        } else {
            atom.from
        })
    }

    /// An operation borrows its carrier view; an owned send also consumes its
    /// payload view. Waiting while holding either prerequisite cannot progress.
    pub(super) fn uses_view_at(&self, idx: usize, role: u8) -> bool {
        let Some((effect, direction)) = self.machine().event_program().operation_at(idx) else {
            return false;
        };
        let atom = self.program_ref().event_atom_at(effect.dense_ordinal());
        let owner = if direction == RoleLocalDirection::Recv {
            atom.to
        } else {
            atom.from
        };
        owner == role
            || (direction == RoleLocalDirection::Send
                && atom.payload == crate::eff::EventPayload::Endpoint(role))
    }

    /// Select an immutable actor binding through the existing route decisions.
    /// An undecided binding is not an operation right or a completed event.
    pub(super) fn event_actor_is_local(
        &self,
        idx: usize,
        selected_arm: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<bool> {
        let count = self.local_steps_len();
        if idx > count {
            crate::invariant();
        }
        if idx == count {
            return Some(false);
        }
        let program = self.program_ref();
        // Construction checks every projected row's actor. Only conditional
        // ownership needs a choice lookup; unconditional loans remain static.
        if program.actor_decision_count() == 0 {
            return Some(true);
        }
        let (eff, direction) =
            crate::invariant_some(self.machine().event_program().operation_at(idx));
        let bindings = crate::invariant_some(program.actor_binding_at(eff.dense_ordinal()));
        program
            .selected_actor(
                bindings[usize::from(direction == RoleLocalDirection::Recv)],
                selected_arm,
            )
            .map(|actor| actor == self.role())
    }

    #[inline]
    fn route_authority_at(&self, idx: usize, route_arm: Option<u8>) -> (ScopeId, Option<u8>) {
        // RoleImageRef construction proves exact conflict/scope/arm membership,
        // including exclusion from every route envelope when no conflict exists.
        // Metadata reads never need to rediscover that immutable authority.
        match self.machine().event_conflict_for_index(idx).to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { scope, .. }) => {
                (scope, route_arm)
            }
            _ => (ScopeId::none(), None),
        }
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
                lane,
            } => {
                let payload_kind = self
                    .machine()
                    .program_ref()
                    .event_atom_at(eff_index.dense_ordinal())
                    .payload;
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, selected_route_arm) = self.route_authority_at(idx, route_arm);
                Some(SendMeta {
                    eff_index,
                    peer,
                    label,
                    payload_kind,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    selected_route_arm,
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
                lane,
            } => {
                let payload_kind = self
                    .machine()
                    .program_ref()
                    .event_atom_at(eff_index.dense_ordinal())
                    .payload;
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, _) = self.route_authority_at(idx, route_arm);
                Some(RecvMeta {
                    eff_index,
                    peer,
                    label,
                    payload_kind,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    choice: node.choice_mark(),
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
                lane,
            } => {
                let payload_kind = self
                    .machine()
                    .program_ref()
                    .event_atom_at(eff_index.dense_ordinal())
                    .payload;
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, _) = self.route_authority_at(idx, route_arm);
                Some(LocalMeta {
                    eff_index,
                    label,
                    payload_kind,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    lane,
                })
            }
            LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Terminate => None,
        }
    }
}
