use super::super::{
    ARM_SHARED, EventCursor, LocalAction, PackedEventConflict, PassiveArmNavigation,
    RouteScopeRegion, ScopeId, ScopeKind, ScopeRegion, StateIndex, state_index_to_usize,
};
use crate::global::typestate::LocalConflict;

impl EventCursor {
    /// Get scope region by scope ID.
    #[inline(always)]
    pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        let mut region = self.machine().scope_region_by_id(scope_id)?;
        region.controller_role = self.machine().route_controller_role(scope_id);
        Some(region)
    }

    #[inline(always)]
    pub(crate) fn route_scope_region_by_id(&self, scope_id: ScopeId) -> Option<RouteScopeRegion> {
        RouteScopeRegion::from_region(self.scope_region_by_id(scope_id)?)
    }

    #[inline(always)]
    pub(crate) fn route_scope_region_at(&self, idx: usize) -> Option<RouteScopeRegion> {
        self.route_scope_region_by_id(self.node_scope_id_at(idx))
    }

    #[inline(always)]
    pub(crate) fn route_scope_end_by_id(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_region_by_id(scope_id)
            .map(|region| region.end())
    }

    #[inline(always)]
    pub(crate) fn route_scope_for_passive_arm_entry(
        &self,
        parent_scope: ScopeId,
        entry_idx: usize,
        passive_arm_scope: Option<ScopeId>,
    ) -> Option<ScopeId> {
        let scope = passive_arm_scope.or_else(|| {
            let scope = self.node_scope_id_at(entry_idx);
            (scope != parent_scope && self.route_scope_region_by_id(scope).is_some())
                .then_some(scope)
        })?;
        self.route_scope_region_by_id(scope)
            .map(RouteScopeRegion::scope)
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
        let depth_bound = self.local_steps_len().saturating_add(1);
        while depth < depth_bound {
            if let Some(entry) = self.route_scope_arm_recv_index(scope, selected_arm) {
                return Some(entry);
            }
            let entry_idx = self.passive_observer_arm_entry_index(scope, selected_arm)?;
            if self.is_recv_at(entry_idx)
                || self.is_send_at(entry_idx)
                || self.is_local_action_at(entry_idx)
                || self.is_jump_at(entry_idx)
            {
                return Some(entry_idx);
            }
            let child_scope = self.route_scope_for_passive_arm_entry(
                scope,
                entry_idx,
                self.passive_arm_scope_by_arm(scope, selected_arm),
            )?;
            selected_arm = selected_arm_for_scope(child_scope)?;
            scope = child_scope;
            depth += 1;
        }
        None
    }

    #[inline]
    pub(crate) fn passive_observer_arm_entry(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        match self.follow_passive_observer_arm_for_scope(scope_id, arm)? {
            PassiveArmNavigation::WithinArm { entry } => Some(entry),
        }
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

    #[inline(always)]
    pub(crate) fn route_scope_linger(&self, scope_id: ScopeId) -> bool {
        self.machine().event_program().route_scope_linger(scope_id)
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

    #[inline]
    pub(crate) fn current_recv_matches_scope_arm(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        loop_label_scope: bool,
        semantic: crate::global::compiled::images::ControlSemanticKind,
        arm: u8,
    ) -> bool {
        self.first_recv_target_for_lane_frame_label(scope_id, lane, frame_label)
            .map(|(target_arm, _)| target_arm == arm)
            .unwrap_or(false)
            || (loop_label_scope && semantic.is_loop())
    }

    #[inline]
    pub(crate) fn static_passive_scope_evidence_materializes_poll(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        !self.is_route_controller(scope_id)
            && !self
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _, _)| policy.is_dynamic())
                .unwrap_or(false)
    }

    pub(crate) fn passive_descendant_dispatch_arm_from_exact_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<u8> {
        if let Some((dispatch_arm, _)) =
            self.first_recv_target_for_lane_frame_label(scope_id, lane, frame_label)
            && dispatch_arm != ARM_SHARED
        {
            return Some(dispatch_arm);
        }
        let mut matched_arm = None;
        for arm in [0u8, 1u8] {
            let Some(child_scope) = self.passive_arm_scope_by_arm(scope_id, arm) else {
                continue;
            };
            if self
                .passive_descendant_dispatch_arm_from_exact_frame_label(
                    child_scope,
                    lane,
                    frame_label,
                )
                .is_some()
            {
                if matched_arm.is_some_and(|prev| prev != arm) {
                    return None;
                }
                matched_arm = Some(arm);
            }
        }
        matched_arm
    }

    fn first_recv_descendant_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let depth_bound = self
            .machine()
            .event_program()
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

    /// Get parent scope.
    pub(crate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.machine().scope_parent(scope_id)
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
    pub(crate) fn route_ancestor_arm(&self, scope_id: ScopeId, ancestor: ScopeId) -> Option<u8> {
        self.machine().route_ancestor_arm(scope_id, ancestor)
    }

    #[inline]
    pub(crate) fn route_scope_for_selected_child_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<ScopeId> {
        self.machine()
            .route_scope_for_selected_child_arm(scope_id, arm)
    }

    #[inline(always)]
    pub(crate) fn node_in_selected_route_arm(
        &self,
        idx: usize,
        scope: ScopeId,
        arm: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let node = self.typestate_node(idx);
        let mut current = node.scope();
        if current.is_none() {
            return false;
        }
        let node_arm = node.route_arm();
        if current == scope {
            return node_arm == Some(arm);
        }
        if current.kind() == ScopeKind::Route
            && let Some(selected) = selected_arm_for_scope(current)
            && node_arm != Some(selected)
        {
            return false;
        }
        let mut depth = 0usize;
        let depth_bound = self.local_steps_len().saturating_add(1);
        while !current.is_none() && current != scope && depth < depth_bound {
            if current.kind() != ScopeKind::Route {
                let Some(parent) = self.scope_parent(current) else {
                    return false;
                };
                if parent == scope {
                    return self.route_ancestor_arm(current, scope) == Some(arm);
                }
                current = parent;
                depth += 1;
                continue;
            }
            let Some(parent) = self.route_parent_scope(current) else {
                return false;
            };
            let relation_arm = self.route_parent_arm(current);
            if parent == scope {
                return relation_arm == Some(arm);
            }
            if let Some(parent_selected) = selected_arm_for_scope(parent)
                && relation_arm != Some(parent_selected)
            {
                return false;
            }
            current = parent;
            depth += 1;
        }
        false
    }

    #[inline(always)]
    pub(crate) fn node_conflict_allows(
        &self,
        idx: usize,
        selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.event_conflict_row_allows(
            self.machine().event_conflict_for_index(idx),
            ScopeId::none(),
            None,
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
        let depth_bound = self.route_scope_count().saturating_add(1);
        while depth < depth_bound {
            let Some(row) = conflict.to_conflict() else {
                return true;
            };
            let LocalConflict::RouteArm { scope, arm } = row else {
                return true;
            };
            let selected = if scope == preview_scope && preview_arm.is_some() {
                preview_arm
            } else {
                selected_arm_for_scope(scope)
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
                lane_idx = lane_idx.saturating_add(1);
                continue;
            };
            let node = self.typestate_node(idx);
            let label = match node.action() {
                LocalAction::Send { label, .. }
                | LocalAction::Recv { label, .. }
                | LocalAction::Local { label, .. } => label,
                LocalAction::Terminate => {
                    lane_idx = lane_idx.saturating_add(1);
                    continue;
                }
            };
            if label == target_label {
                if node.scope() == scope_id {
                    if node.route_arm() == Some(selected_arm) {
                        return Some(idx);
                    }
                } else if self.node_in_selected_route_arm(
                    idx,
                    scope_id,
                    selected_arm,
                    |candidate| selected_arm_for_scope(candidate),
                ) {
                    return Some(idx);
                }
            }
            lane_idx = lane_idx.saturating_add(1);
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
        if node_scope.kind() == ScopeKind::Route && selected_arm_for_scope(node_scope).is_some() {
            return node_scope;
        }
        let mut child_scope = node_scope;
        while let Some(parent_scope) = self.route_parent_scope(child_scope) {
            if parent_scope == child_scope {
                return parent_scope;
            }
            let child_selected_arm = selected_arm_for_scope(child_scope);
            let Some(parent_arm) = selected_arm_for_scope(parent_scope)
                .or_else(|| {
                    // Once execution has entered a selected child route, the
                    // parent arm is descriptor-derived from that child path.
                    child_selected_arm
                        .is_some()
                        .then(|| self.route_ancestor_arm(child_scope, parent_scope))
                        .flatten()
                })
                .or_else(|| preview_selected_arm_for_scope(parent_scope))
            else {
                return parent_scope;
            };
            if self.route_ancestor_arm(child_scope, parent_scope) != Some(parent_arm) {
                return parent_scope;
            }
            child_scope = parent_scope;
        }
        node_scope
    }

    #[inline(always)]
    pub(crate) fn rebase_passive_descendant_scope(
        &self,
        stop_scope: ScopeId,
        initial_scope: ScopeId,
        mut selected_or_preview_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
        mut materialization_index_for_selected_arm: impl FnMut(ScopeId, u8) -> Option<usize>,
    ) -> ScopeId {
        let mut target_scope = initial_scope;
        let mut attempts = 0usize;
        let depth_bound = self.local_steps_len().saturating_add(1);
        'rebase: while attempts < depth_bound {
            let mut child_scope = target_scope;
            let mut depth = 0usize;
            while depth < depth_bound {
                let Some(parent_scope) = self.route_parent_scope(child_scope) else {
                    break 'rebase;
                };
                if parent_scope == child_scope || parent_scope == stop_scope {
                    break 'rebase;
                }
                if parent_scope.kind() == ScopeKind::Route
                    && let Some(parent_arm) = selected_or_preview_arm_for_scope(parent_scope)
                    && self.route_ancestor_arm(child_scope, parent_scope) != Some(parent_arm)
                {
                    if let Some(scope) = self.passive_arm_scope_by_arm(parent_scope, parent_arm)
                        && scope != child_scope
                    {
                        target_scope = scope;
                        attempts += 1;
                        continue 'rebase;
                    }
                    if let Some(entry_idx) =
                        materialization_index_for_selected_arm(parent_scope, parent_arm)
                    {
                        let scope = self.node_scope_id_at(entry_idx);
                        if scope.kind() == ScopeKind::Route
                            && scope != parent_scope
                            && scope != child_scope
                        {
                            target_scope = scope;
                            attempts += 1;
                            continue 'rebase;
                        }
                    }
                    break 'rebase;
                }
                child_scope = parent_scope;
                depth += 1;
            }
            break;
        }
        target_scope
    }
}
