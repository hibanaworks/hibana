use super::super::{
    ARM_SHARED, EventCursor, LocalAction, PackedEventConflict, RouteScopeRows, ScopeId, StateIndex,
    state_index_to_usize,
};
use crate::global::typestate::LocalConflict;

impl EventCursor {
    #[inline(always)]
    pub(crate) fn route_scope_rows(&self, scope_id: ScopeId) -> Option<RouteScopeRows> {
        self.machine().route_scope_rows(scope_id)
    }

    #[inline(always)]
    pub(crate) fn route_scope_rows_at(&self, idx: usize) -> Option<RouteScopeRows> {
        self.route_scope_rows(self.node_scope_id_at(idx))
    }

    pub(crate) fn enclosing_route_scope_rows_at(&self, idx: usize) -> Option<RouteScopeRows> {
        let mut selected = None;
        let mut selected_len = usize::MAX;
        let mut slot = 0usize;
        while let Some(region) = self.machine().route_scope_rows_by_slot(slot) {
            if idx >= region.start() && idx < region.end() {
                let len = region.end() - region.start();
                if len < selected_len {
                    selected = Some(region);
                    selected_len = len;
                }
            }
            slot += 1;
        }
        selected
    }

    #[inline(always)]
    pub(crate) fn checked_route_scope_rows_at(&self, idx: usize) -> Option<RouteScopeRows> {
        self.route_scope_rows(self.checked_node_scope_id_at(idx)?)
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
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
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

    #[inline]
    pub(crate) fn selected_route_arm_recv_entry_index(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        if let Some(idx) = self.route_scope_arm_recv_index(scope_id, arm) {
            return Some(idx);
        }
        self.passive_observer_arm_entry_index(scope_id, arm)
    }

    pub(crate) fn route_arm_for_index(&self, scope_id: ScopeId, idx: usize) -> Option<u8> {
        let slot = self.route_scope_slot_inner(scope_id)?;
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self
                .machine()
                .event_program()
                .route_arm_event_row_by_slot(slot, arm)
                && idx >= row.start()
                && idx < row.end()
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
    pub(crate) fn first_recv_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        if let Some((resolver, _)) = self.route_scope_controller_resolver(scope_id)
            && resolver.is_dynamic()
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

    #[inline]
    pub(crate) fn current_recv_matches_scope_arm(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        arm: u8,
    ) -> bool {
        self.first_recv_target_for_lane_frame_label(scope_id, lane, frame_label)
            .is_some_and(|(target_arm, _)| target_arm == arm)
    }

    #[inline]
    pub(crate) fn intrinsic_passive_scope_evidence_materializes_poll(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        !self.is_route_controller(scope_id)
            && !self
                .route_scope_controller_resolver(scope_id)
                .is_some_and(|(resolver, _)| resolver.is_dynamic())
    }

    pub(crate) fn passive_descendant_dispatch_arm_from_exact_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<u8> {
        self.first_recv_descendant_target_for_lane_frame_label(scope_id, lane, frame_label)
            .and_then(|(dispatch_arm, _)| (dispatch_arm != ARM_SHARED).then_some(dispatch_arm))
    }

    fn first_recv_descendant_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let (dispatch, len) = self.route_scope_first_recv_dispatch_table(scope_id)?;
        let mut matched = None;
        let mut idx = 0usize;
        while idx < len as usize {
            let entry = dispatch[idx];
            if entry.lane() == lane
                && entry.frame_label() == frame_label
                && entry.arm() < 2
                && !entry.target().is_absent()
            {
                let target = (entry.arm(), entry.target());
                if matched.is_some_and(|prev| prev != target) {
                    return None;
                }
                matched = Some(target);
            }
            idx += 1;
        }
        matched
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
            .is_some_and(|ctrl| ctrl == self.machine().role())
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

    #[inline(always)]
    pub(crate) fn node_in_selected_route_arm(
        &self,
        idx: usize,
        scope: ScopeId,
        arm: u8,
        selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.event_conflict_row_contains_route_arm(
            self.machine().event_conflict_for_index(idx),
            scope,
            arm,
            selected_arm_for_scope,
        )
    }

    #[inline(always)]
    pub(crate) fn event_conflict_row_allows(
        &self,
        mut conflict: PackedEventConflict,
        preview_scope: ScopeId,
        preview_arm: Option<u8>,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                return true;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                return true;
            };
            let selected = selected_arm_for_scope(scope).or_else(|| {
                if scope == preview_scope && preview_arm.is_some() {
                    preview_arm
                } else {
                    None
                }
            });
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

    #[inline(always)]
    pub(crate) fn event_conflict_row_allows_with_preview(
        &self,
        mut conflict: PackedEventConflict,
        preview_conflict: PackedEventConflict,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                return true;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                return true;
            };
            let selected = selected_arm_for_scope(scope)
                .or_else(|| self.preview_conflict_arm(preview_conflict, scope));
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

    #[inline(always)]
    fn preview_conflict_arm(
        &self,
        mut conflict: PackedEventConflict,
        target_scope: ScopeId,
    ) -> Option<u8> {
        if target_scope.is_none() {
            return None;
        }
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
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
    fn event_conflict_row_contains_route_arm(
        &self,
        mut conflict: PackedEventConflict,
        target_scope: ScopeId,
        target_arm: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        if target_scope.is_none() {
            return false;
        }
        let mut matched = false;
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                return matched;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                return matched;
            };
            if scope == target_scope {
                if arm != target_arm {
                    return false;
                }
                matched = true;
            } else if let Some(selected) = selected_arm_for_scope(scope)
                && selected != arm
            {
                return false;
            }
            conflict = self.route_scope_conflict_row(scope);
            depth += 1;
        }
        false
    }

    #[inline(always)]
    fn route_scope_conflict_row(&self, scope_id: ScopeId) -> PackedEventConflict {
        let Some(slot) = self.route_scope_slot_inner(scope_id) else {
            return PackedEventConflict::none();
        };
        self.machine().route_scope_conflict_by_slot(slot)
    }

    pub(crate) fn selected_route_label_index(
        &self,
        scope_id: ScopeId,
        target_label: u8,
        selected_arm: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let lane_limit = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            let Some(idx) = self.index_for_lane_step(lane_idx) else {
                lane_idx += 1;
                continue;
            };
            let node = self.typestate_node(idx);
            let label = match node.action() {
                LocalAction::Send { label, .. }
                | LocalAction::Recv { label, .. }
                | LocalAction::Local { label, .. } => label,
                LocalAction::Terminate => {
                    lane_idx += 1;
                    continue;
                }
            };
            if label == target_label
                && self.node_in_selected_route_arm(idx, scope_id, selected_arm, |candidate| {
                    selected_arm_for_scope(candidate)
                })
            {
                return Some(idx);
            }
            lane_idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn current_offer_scope_id(
        &self,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut preview_selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
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
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                break;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                break;
            };
            let Some(selected) =
                selected_arm_for_scope(scope).or_else(|| preview_selected_arm_for_scope(scope))
            else {
                return scope;
            };
            if selected != arm {
                return scope;
            }
            conflict = self.route_scope_conflict_row(scope);
            depth += 1;
        }
        node_scope
    }

    #[inline(always)]
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
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
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
