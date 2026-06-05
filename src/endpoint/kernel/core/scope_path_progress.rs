use super::{
    CursorEndpoint, EffIndex, EndpointSlot, EpochTable, LabelUniverse, LocalAction,
    MintConfigMarker, ScopeId, ScopeKind, SelectedRoutePhaseProgress, Transport,
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    pub(in crate::endpoint::kernel) fn selected_route_phase_progress(
        &self,
        scope: ScopeId,
        arm: u8,
        completed: Option<(ScopeId, u8)>,
    ) -> SelectedRoutePhaseProgress {
        let lane_set = self.cursor.current_phase_lane_set();
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
                let node = self.cursor.typestate_node(idx);
                if node.scope() == scope {
                    if node.route_arm() == Some(arm) {
                        return SelectedRoutePhaseProgress::PendingResidentStep;
                    }
                } else if self.node_matches_selected_route_path(idx, scope, arm, completed) {
                    return SelectedRoutePhaseProgress::PendingResidentStep;
                }
            }
            next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        SelectedRoutePhaseProgress::Complete
    }

    pub(in crate::endpoint::kernel) fn scope_lane_first_eff_for_selected_path(
        &self,
        scope: ScopeId,
        arm: u8,
        lane: u8,
        completed: Option<(ScopeId, u8)>,
    ) -> Option<EffIndex> {
        let region = self.cursor.scope_region_by_id(scope)?;
        let mut idx = region.start;
        while idx < region.end && self.cursor.contains_node_index(idx) {
            let node = self.cursor.typestate_node(idx);
            let eff = match node.action() {
                LocalAction::Send {
                    eff_index, lane: l, ..
                }
                | LocalAction::Recv {
                    eff_index, lane: l, ..
                }
                | LocalAction::Local {
                    eff_index, lane: l, ..
                } if l == lane => eff_index,
                _ => {
                    idx += 1;
                    continue;
                }
            };
            if node.scope() == scope {
                if node.route_arm() == Some(arm) {
                    return Some(eff);
                }
            } else if self.node_matches_selected_route_path(idx, scope, arm, completed) {
                return Some(eff);
            }
            idx += 1;
        }
        None
    }

    pub(in crate::endpoint::kernel) fn scope_lane_last_eff_for_selected_path(
        &self,
        scope: ScopeId,
        arm: u8,
        lane: u8,
        completed: Option<(ScopeId, u8)>,
    ) -> Option<EffIndex> {
        let region = self.cursor.scope_region_by_id(scope)?;
        let mut found = None;
        let mut idx = region.start;
        while idx < region.end && self.cursor.contains_node_index(idx) {
            let node = self.cursor.typestate_node(idx);
            let eff = match node.action() {
                LocalAction::Send {
                    eff_index, lane: l, ..
                }
                | LocalAction::Recv {
                    eff_index, lane: l, ..
                }
                | LocalAction::Local {
                    eff_index, lane: l, ..
                } if l == lane => eff_index,
                _ => {
                    idx += 1;
                    continue;
                }
            };
            if node.scope() == scope {
                if node.route_arm() == Some(arm) {
                    found = Some(eff);
                }
            } else if self.node_matches_selected_route_path(idx, scope, arm, completed) {
                found = Some(eff);
            }
            idx += 1;
        }
        found
    }

    pub(in crate::endpoint::kernel) fn has_selected_route_path_step_from(
        &self,
        scope: ScopeId,
        arm: u8,
        start_idx: usize,
        completed: Option<(ScopeId, u8)>,
    ) -> bool {
        let Some(region) = self.cursor.scope_region_by_id(scope) else {
            return false;
        };
        let mut idx = start_idx.max(region.start);
        while idx < region.end && self.cursor.contains_node_index(idx) {
            let node = self.cursor.typestate_node(idx);
            if !matches!(
                node.action(),
                LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Local { .. }
            ) {
                idx += 1;
                continue;
            }
            if node.scope() == scope {
                if node.route_arm() == Some(arm) {
                    return true;
                }
            } else if self.node_matches_selected_route_path(idx, scope, arm, completed) {
                return true;
            }
            idx += 1;
        }
        false
    }

    pub(in crate::endpoint::kernel) fn node_matches_selected_route_path(
        &self,
        idx: usize,
        scope: ScopeId,
        arm: u8,
        completed: Option<(ScopeId, u8)>,
    ) -> bool {
        let node = self.cursor.typestate_node(idx);
        let mut current = node.scope();
        if current.is_none() {
            return false;
        }
        let node_arm = node.route_arm();
        if current == scope {
            return node_arm == Some(arm);
        }
        if current.kind() == ScopeKind::Route {
            if let Some(selected) = self.selected_arm_for_scope_with_completed(current, completed)
                && node_arm != Some(selected)
            {
                return false;
            }
        }
        let mut depth = 0usize;
        let depth_bound = self.route_scope_depth_bound();
        while !current.is_none() && current != scope && depth < depth_bound {
            if current.kind() != ScopeKind::Route {
                let Some(parent) = self.cursor.scope_parent(current) else {
                    return false;
                };
                if parent == scope {
                    return self.scope_route_arm_relation_matches(current, scope, arm);
                }
                current = parent;
                depth += 1;
                continue;
            }
            let Some(parent) = self.cursor.route_parent_scope(current) else {
                return false;
            };
            let relation_arm = self.cursor.route_parent_arm(current);
            if parent == scope {
                return relation_arm == Some(arm);
            }
            if let Some(parent_selected) =
                self.selected_arm_for_scope_with_completed(parent, completed)
                && relation_arm != Some(parent_selected)
            {
                return false;
            }
            current = parent;
            depth += 1;
        }
        false
    }

    fn scope_route_arm_relation_matches(
        &self,
        child: ScopeId,
        parent_route: ScopeId,
        arm: u8,
    ) -> bool {
        self.cursor.route_parent_scope(child) == Some(parent_route)
            && self.cursor.route_parent_arm(child) == Some(arm)
    }

    fn selected_arm_for_scope_with_completed(
        &self,
        scope: ScopeId,
        completed: Option<(ScopeId, u8)>,
    ) -> Option<u8> {
        if let Some((completed_scope, completed_arm)) = completed
            && completed_scope == scope
        {
            return Some(completed_arm);
        }
        self.selected_arm_for_scope(scope)
    }
}
