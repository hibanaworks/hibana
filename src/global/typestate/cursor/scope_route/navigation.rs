use super::super::{
    ARM_SHARED, CursorInvariantError, EventCursor, InboundFrameKey, LocalAction,
    PackedEventConflict, RouteScopeRows, ScopeId, StateIndex, state_index_to_usize,
};
use crate::global::{const_dsl::ScopeKind, typestate::LocalConflict};

impl EventCursor {
    pub(crate) fn deeper_route_scope(&self, current: ScopeId, candidate: ScopeId) -> ScopeId {
        if current.same(candidate) || self.route_scope_descends_from(current, candidate) {
            return current;
        }
        if self.route_scope_descends_from(candidate, current) {
            return candidate;
        }
        crate::invariant();
    }

    fn route_scope_descends_from(&self, scope: ScopeId, ancestor: ScopeId) -> bool {
        let mut current = scope;
        let mut depth = 0usize;
        let depth_bound = self.route_chain_bound();
        while depth < depth_bound {
            if current.same(ancestor) {
                return true;
            }
            let Some((parent, _)) = self.route_conflict_parent_arm(current) else {
                return false;
            };
            if parent.same(current) {
                crate::invariant();
            }
            current = parent;
            depth += 1;
        }
        crate::invariant();
    }

    #[inline(always)]
    pub(crate) fn route_scope_rows(&self, scope_id: ScopeId) -> Option<RouteScopeRows> {
        self.machine().route_scope_rows(scope_id)
    }

    #[inline(always)]
    pub(crate) fn route_scope_rows_at(&self, idx: usize) -> Option<RouteScopeRows> {
        self.route_scope_rows(self.node_scope_id_at(idx))
    }

    pub(crate) fn enclosing_route_scope_rows_at(&self, idx: usize) -> Option<RouteScopeRows> {
        if matches!(self.action_at(idx), LocalAction::Terminate) {
            return None;
        }
        let conflict = self.machine().event_conflict_for_index(idx);
        let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() else {
            let mut slot = 0usize;
            while slot < self.machine().route_scope_slot_count() {
                if self
                    .machine()
                    .route_scope_rows_by_slot(slot)
                    .is_some_and(|region| region.start() <= idx && idx < region.end())
                {
                    crate::invariant();
                }
                slot += 1;
            }
            return None;
        };
        let Some(region) = self.route_scope_rows(scope) else {
            crate::invariant();
        };
        if idx < region.start()
            || idx >= region.end()
            || self.route_arm_for_index(scope, idx) != Some(arm)
        {
            crate::invariant();
        }
        Some(region)
    }

    pub(crate) fn visit_route_arms_for_index(
        &self,
        idx: usize,
        mut visit: impl FnMut(ScopeId, u8),
    ) {
        let mut slot = 0usize;
        while slot < self.machine().route_scope_slot_count() {
            if let Some(region) = self.machine().route_scope_rows_by_slot(slot)
                && region.start() <= idx
                && idx < region.end()
                && let Some(arm) = self.route_arm_for_index(region.scope(), idx)
            {
                visit(region.scope(), arm);
            }
            slot += 1;
        }
    }

    #[inline(always)]
    pub(crate) fn route_scope_end_by_id(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_rows(scope_id).map(|region| region.end())
    }

    pub(crate) fn passive_materialization_index_for_selected_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut scope = scope_id;
        let mut selected_arm = arm;
        let mut depth = 0usize;
        let depth_bound = self.route_chain_bound();
        while depth < depth_bound {
            if let Some(entry) = self.route_scope_arm_recv_index(scope, selected_arm) {
                return Some(entry);
            }
            let entry_idx = self.passive_observer_arm_entry_index(scope, selected_arm)?;
            if self.is_recv_at(entry_idx)
                || self.is_send_at(entry_idx)
                || self.is_local_action_at(entry_idx)
            {
                return Some(entry_idx);
            }
            let child_scope = self.passive_child_scope(scope, selected_arm)?;
            selected_arm = selected_arm_for_scope(child_scope)?;
            scope = child_scope;
            depth += 1;
        }
        crate::invariant();
    }

    #[inline]
    pub(crate) fn passive_observer_arm_entry(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.follow_passive_observer_arm_for_scope(scope_id, arm)
    }

    #[inline]
    pub(crate) fn passive_observer_arm_entry_index(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        self.passive_observer_arm_entry(scope_id, arm)
            .map(state_index_to_usize)
    }

    pub(crate) fn route_arm_for_index(&self, scope_id: ScopeId, idx: usize) -> Option<u8> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        let lane = match self.action_at(idx) {
            LocalAction::Send { lane, .. }
            | LocalAction::Recv { lane, .. }
            | LocalAction::Local { lane, .. } => lane,
            LocalAction::Terminate => return None,
        };
        let Ok(step) = self.relocatable_resident_lane_step_at_index(idx, lane as usize) else {
            crate::invariant();
        };
        let step_idx = step.0.step_idx as usize;
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self
                .machine()
                .event_program()
                .route_arm_event_row_by_slot(slot, arm)
                && step_idx >= row.start()
                && step_idx < row.end()
            {
                return Some(arm);
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn route_scope_reentry(&self, scope_id: ScopeId) -> bool {
        self.machine().route_scope_reentry(scope_id)
    }

    #[inline]
    pub(crate) fn intrinsic_passive_scope_evidence_materializes_poll(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        !self.is_route_controller(scope_id) && self.route_scope_resolver(scope_id).is_none()
    }

    pub(crate) fn passive_descendant_dispatch_arm_for_key(
        &self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> Result<Option<u8>, CursorInvariantError> {
        self.first_recv_descendant_target_for_key(scope_id, key)
            .map(|target| {
                target.and_then(|(dispatch_arm, _)| {
                    (dispatch_arm != ARM_SHARED).then_some(dispatch_arm)
                })
            })
    }

    pub(crate) fn passive_descendant_target_index_for_key(
        &self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> Result<Option<usize>, CursorInvariantError> {
        self.first_recv_descendant_target_for_key(scope_id, key)
            .map(|target| {
                target.and_then(|(dispatch_arm, target)| {
                    (dispatch_arm != ARM_SHARED).then_some(state_index_to_usize(target))
                })
            })
    }

    pub(crate) fn enclosing_passive_route_scope_for_key(
        &self,
        idx: usize,
        key: InboundFrameKey,
    ) -> Result<Option<ScopeId>, CursorInvariantError> {
        let mut selected = None;
        let mut slot = 0usize;
        while slot < self.machine().route_scope_slot_count() {
            if let Some(region) = self.machine().route_scope_rows_by_slot(slot)
                && region.start() <= idx
                && idx < region.end()
            {
                let scope = region.scope();
                if self
                    .first_recv_descendant_target_for_key(scope, key)?
                    .is_some_and(|(dispatch_arm, _)| dispatch_arm != ARM_SHARED)
                {
                    selected = Some(match selected {
                        Some(current) => self.deeper_route_scope(current, scope),
                        None => scope,
                    });
                }
            }
            slot += 1;
        }
        Ok(selected)
    }

    fn first_recv_descendant_target_for_key(
        &self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> Result<Option<(u8, StateIndex)>, CursorInvariantError> {
        self.machine()
            .first_recv_descendant_target_for_key(scope_id, key)
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
        if !matches!(scope_id.kind(), Some(ScopeKind::Route)) {
            return false;
        }
        self.machine().route_controller_role(scope_id) == self.machine().role()
    }

    /// Scope ID stored on the current node (no parent traversal).
    #[inline(always)]
    pub(crate) fn node_scope_id(&self) -> ScopeId {
        self.machine().node(self.idx_usize()).scope()
    }

    #[inline(always)]
    pub(crate) fn route_conflict_parent_arm(&self, scope_id: ScopeId) -> Option<(ScopeId, u8)> {
        let LocalConflict::RouteArm { scope, arm } =
            self.route_scope_conflict_row(scope_id).to_conflict()?
        else {
            return None;
        };
        (!scope.is_none()).then_some((scope, arm))
    }

    #[inline(always)]
    pub(crate) fn event_conflict_for_index(&self, idx: usize) -> PackedEventConflict {
        self.machine().event_conflict_for_index(idx)
    }

    #[inline(never)]
    pub(crate) fn event_conflict_row_allows(
        &self,
        mut conflict: PackedEventConflict,
        preview_scope: ScopeId,
        preview_arm: Option<u8>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut depth = 0usize;
        let depth_bound = self.route_chain_bound();
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                return true;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                return true;
            };
            let selected = match selected_arm_for_scope(scope) {
                Some(arm) => Some(arm),
                None if scope == preview_scope && preview_arm.is_some() => preview_arm,
                None => None,
            };
            if let Some(selected) = selected
                && selected != arm
            {
                return false;
            }
            conflict = self.route_scope_conflict_row(scope);
            depth += 1;
        }
        false
    }

    #[inline(never)]
    pub(crate) fn event_conflict_row_allows_with_preview(
        &self,
        mut conflict: PackedEventConflict,
        preview_conflict: PackedEventConflict,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut depth = 0usize;
        let depth_bound = self.route_chain_bound();
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                return true;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                return true;
            };
            let selected = match selected_arm_for_scope(scope) {
                Some(arm) => Some(arm),
                None => self.preview_conflict_arm(preview_conflict, scope),
            };
            if let Some(selected) = selected
                && selected != arm
            {
                return false;
            }
            conflict = self.route_scope_conflict_row(scope);
            depth += 1;
        }
        false
    }

    #[inline(never)]
    pub(super) fn preview_conflict_arm(
        &self,
        mut conflict: PackedEventConflict,
        target_scope: ScopeId,
    ) -> Option<u8> {
        if target_scope.is_none() {
            return None;
        }
        let mut depth = 0usize;
        let depth_bound = self.route_chain_bound();
        while depth < depth_bound {
            let LocalConflict::RouteArm { scope, arm } = conflict.to_conflict()? else {
                return None;
            };
            if scope == target_scope {
                return Some(arm);
            }
            conflict = self.route_scope_conflict_row(scope);
            depth += 1;
        }
        None
    }

    #[inline(always)]
    fn route_scope_conflict_row(&self, scope_id: ScopeId) -> PackedEventConflict {
        let Some(slot) = self.route_scope_slot_inner(scope_id) else {
            return PackedEventConflict::none();
        };
        self.machine().route_scope_conflict_by_slot(slot)
    }

    pub(crate) fn current_offer_scope_id(
        &self,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        preview_selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> ScopeId {
        let node_scope = self.node_scope_id();
        if node_scope.is_none() {
            return node_scope;
        }
        if self.route_scope_slot_inner(node_scope).is_some()
            && selected_arm_for_scope(node_scope).is_some()
        {
            return node_scope;
        }
        let mut conflict = self.machine().event_conflict_for_index(self.idx_usize());
        let mut first_unresolved = ScopeId::none();
        let mut depth = 0usize;
        let depth_bound = self.route_chain_bound();
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                break;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                break;
            };
            let selected = match selected_arm_for_scope(scope) {
                Some(arm) => Some(arm),
                None => preview_selected_arm_for_scope(scope),
            };
            let Some(selected) = selected else {
                if first_unresolved.is_none() {
                    first_unresolved = scope;
                }
                conflict = self.route_scope_conflict_row(scope);
                depth += 1;
                continue;
            };
            if selected != arm {
                return scope;
            }
            conflict = self.route_scope_conflict_row(scope);
            depth += 1;
        }
        if !first_unresolved.is_none() {
            return first_unresolved;
        }
        node_scope
    }

    pub(crate) fn rebase_passive_descendant_scope(
        &self,
        stop_scope: ScopeId,
        initial_scope: ScopeId,
        mut selected_or_preview_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> ScopeId {
        let Some(stop_arm) = selected_or_preview_arm_for_scope(stop_scope) else {
            return initial_scope;
        };
        let Some(mut selected_scope) = self.passive_child_scope(stop_scope, stop_arm) else {
            return initial_scope;
        };
        if selected_scope == initial_scope {
            return initial_scope;
        }
        let depth_bound = self.route_chain_bound();
        let mut depth = 0usize;
        while depth < depth_bound {
            let Some(arm) = selected_or_preview_arm_for_scope(selected_scope) else {
                return selected_scope;
            };
            let Some(child_scope) = self.passive_child_scope(selected_scope, arm) else {
                return selected_scope;
            };
            if child_scope == initial_scope {
                return initial_scope;
            }
            selected_scope = child_scope;
            depth += 1;
        }
        crate::invariant();
    }
}
