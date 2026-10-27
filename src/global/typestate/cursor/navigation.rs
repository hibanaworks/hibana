use super::{
    EventCursor, LocalMeta, RecvMeta, ScopeId, SendMeta, StateIndex, state_index_to_usize,
};
use crate::global::event_program::LocalEventRow;
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

    pub(crate) fn framed_receive_candidates(&self, frame_label: u16) -> core::ops::Range<usize> {
        self.machine()
            .event_program()
            .source_effect_candidates(usize::from(frame_label))
    }
    /// Current typestate index.
    #[inline(always)]
    pub(crate) fn index(&self) -> usize {
        self.state().idx as usize
    }

    #[inline(always)]
    pub(crate) fn event_row_at(&self, index: usize) -> Option<LocalEventRow> {
        if index > self.local_steps_len() {
            crate::invariant();
        }
        self.machine().event_program().event_row_at(index)
    }

    #[inline(always)]
    pub(crate) fn node_next_index_at(&self, index: usize) -> StateIndex {
        self.event_row_at(index)
            .map_or_else(|| StateIndex::from_usize(index), LocalEventRow::next)
    }

    #[inline(always)]
    pub(crate) fn node_route_arm_at(&self, index: usize) -> Option<u8> {
        self.event_row_at(index).and_then(LocalEventRow::route_arm)
    }

    /// Returns `true` when the cursor points at a receive action.
    #[inline(always)]
    pub(crate) fn is_recv(&self) -> bool {
        self.is_recv_at(self.idx_usize())
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

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn action_at(&self, idx: usize) -> super::LocalAction {
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
        self.event_row_at(idx)
            .map_or(ScopeId::none(), LocalEventRow::scope)
    }

    #[inline(always)]
    pub(crate) fn try_send_meta_at(&self, idx: usize) -> Option<SendMeta> {
        if idx > self.local_steps_len() {
            crate::invariant();
        }
        let row = self.machine().event_program().event_row_at(idx)?;
        if row.direction() != RoleLocalDirection::Send {
            return None;
        }
        let atom = row.atom();
        let route_arm = row.route_arm();
        Some(SendMeta {
            eff_index: row.effect(),
            peer: atom.to,
            label: atom.label,
            payload_kind: atom.payload,
            frame_label: row.effect().dense_ordinal() as u16,
            origin: atom.origin,
            next: state_index_to_usize(row.next()),
            scope: row.scope(),
            route_scope: row.route_scope(),
            route_arm,
            selected_route_arm: route_arm,
            lane: atom.lane,
        })
    }

    #[inline(always)]
    pub(crate) fn try_recv_meta_at(&self, idx: usize) -> Option<RecvMeta> {
        if idx > self.local_steps_len() {
            crate::invariant();
        }
        let row = self.machine().event_program().event_row_at(idx)?;
        if row.direction() != RoleLocalDirection::Recv {
            return None;
        }
        let atom = row.atom();
        Some(RecvMeta {
            eff_index: row.effect(),
            peer: atom.from,
            label: atom.label,
            payload_kind: atom.payload,
            frame_label: row.effect().dense_ordinal() as u16,
            origin: atom.origin,
            next: state_index_to_usize(row.next()),
            scope: row.scope(),
            route_scope: row.route_scope(),
            route_arm: row.route_arm(),
            choice: row.choice(),
            lane: atom.lane,
        })
    }

    #[inline(always)]
    pub(crate) fn try_local_meta_at(&self, idx: usize) -> Option<LocalMeta> {
        if idx > self.local_steps_len() {
            crate::invariant();
        }
        let row = self.machine().event_program().event_row_at(idx)?;
        if row.direction() != RoleLocalDirection::Local {
            return None;
        }
        let atom = row.atom();
        Some(LocalMeta {
            eff_index: row.effect(),
            label: atom.label,
            payload_kind: atom.payload,
            frame_label: row.effect().dense_ordinal() as u16,
            origin: atom.origin,
            next: state_index_to_usize(row.next()),
            scope: row.scope(),
            route_scope: row.route_scope(),
            route_arm: row.route_arm(),
            lane: atom.lane,
        })
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
}
