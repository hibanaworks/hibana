use super::{
    Arm, CursorEndpoint, EffIndex, EndpointSlot, EpochTable, FrontierScratchView, JumpReason,
    LabelUniverse, LaneSetView, LocalAction, MintConfigMarker, ParentRouteDecisionPlan,
    PassiveArmNavigation, Port, RendezvousId, RouteDecisionSource, RouteDecisionToken, ScopeId,
    ScopeKind, Transport, TryFrom, frontier_scratch_view_from_storage, lane_port,
    state_index_to_usize,
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
    pub(crate) fn publish_scope_settlement(
        &mut self,
        scope: ScopeId,
        route_arm: Option<u8>,
        eff_index: Option<EffIndex>,
        lane: u8,
    ) {
        let region = if scope.kind() == ScopeKind::Route {
            self.cursor.scope_region_by_id(scope)
        } else {
            None
        };
        let linger = region.as_ref().map_or(false, |r| r.linger);
        let lane_wire = lane;
        let mut exited_scope = false;
        let route_arm_done_on_lane = route_arm
            .zip(eff_index)
            .and_then(|(arm, eff)| {
                self.scope_lane_last_eff_for_selected_path(scope, arm, lane_wire, None)
                    .map(|last_eff| last_eff == eff)
            })
            .unwrap_or(false);

        // For linger scopes (loops), if cursor has advanced past the region boundary,
        // rewind to region.start so the next offer() can find the recv node.
        // This is essential for passive observers whose projection has fewer steps.
        // BUT: do NOT rewind if we're in the Break arm (arm > 0 for standard 2-arm loops).
        // The Break arm should exit the loop, not loop back.
        if linger {
            if let Some(ref reg) = region {
                let current_arm = route_arm.or_else(|| self.route_arm_for(lane_wire, scope));
                let is_break_arm = current_arm.map_or(false, |arm| arm > 0);
                if self.cursor.index() >= reg.end || route_arm_done_on_lane {
                    self.clear_descendant_route_state_for_lane(lane_wire, scope);
                    if is_break_arm {
                        self.pop_route_arm(lane_wire, scope);
                        exited_scope = true;
                        let mut current_scope = scope;
                        while let Some(parent) = self.cursor.control_parent_scope(current_scope) {
                            if let Some(parent_region) = self.cursor.scope_region_by_id(parent) {
                                if parent_region.linger {
                                    if let Some(parent_arm) = self.route_arm_for(lane_wire, parent)
                                    {
                                        if parent_arm == 0 {
                                            self.set_cursor_index(parent_region.start);
                                            break;
                                        }
                                    }
                                }
                                let should_advance = self.cursor.index() >= parent_region.end;

                                if should_advance {
                                    self.clear_descendant_route_state_for_lane(lane_wire, parent);
                                    if self.cursor.advance_scope_by_id_in_place(parent) {}
                                    self.pop_route_arm(lane_wire, parent);
                                    current_scope = parent;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        self.set_cursor_index(reg.start);
                    }
                }
                if !is_break_arm {
                    let at_scope_start = self.cursor.index() == reg.start;
                    let at_passive_branch = self.cursor.jump_reason()
                        == Some(JumpReason::PassiveObserverBranch)
                        && self
                            .cursor
                            .scope_region()
                            .map(|region| region.scope_id == scope)
                            .unwrap_or(false);
                    if at_scope_start || at_passive_branch {
                        let first_eff = current_arm
                            .and_then(|arm| {
                                self.scope_lane_first_eff_for_selected_path(
                                    scope, arm, lane_wire, None,
                                )
                            })
                            .or_else(|| self.cursor.scope_lane_first_eff(scope, lane_wire));
                        if let Some(first_eff) = first_eff {
                            let lane_idx = lane_wire as usize;
                            self.set_lane_cursor_to_eff_index(lane_idx, first_eff);
                        }
                    }
                }
            }
        } else if let Some(ref reg) = region {
            if self.cursor.index() >= reg.end || route_arm_done_on_lane {
                if route_arm_done_on_lane && self.cursor.index() < reg.end {
                    self.set_cursor_index(reg.end);
                }
                exited_scope = true;
            }
        }

        if exited_scope {
            let last_eff = route_arm
                .and_then(|arm| {
                    self.scope_lane_last_eff_for_selected_path(scope, arm, lane_wire, None)
                })
                .or_else(|| self.cursor.scope_lane_last_eff(scope, lane_wire));
            if let Some(eff_index) = last_eff {
                let lane_idx = lane_wire as usize;
                if self
                    .cursor
                    .current_phase_contains_eff_index(lane_idx, eff_index)
                {
                    self.advance_lane_cursor(lane_idx, eff_index);
                }
            }
        }

        if scope.kind() == ScopeKind::Route {
            if exited_scope {
                self.clear_scope_route_state_for_other_lanes(scope, lane_wire);
                self.pop_route_arm(lane_wire, scope);
            }
            if exited_scope {
                self.clear_scope_evidence(scope);
            }
        }

        // If we rewound into a parent linger scope, sync its lane cursor to the
        // entry eff_index so offer()/flow() can locate the next iteration.
        let mut parent_scope = scope;
        let mut completed_route = if exited_scope && scope.kind() == ScopeKind::Route {
            route_arm.map(|arm| (scope, arm))
        } else {
            None
        };
        loop {
            let Some(parent) = self.cursor.control_parent_scope(parent_scope) else {
                break;
            };
            if let Some(parent_region) = self.cursor.scope_region_by_id(parent) {
                let parent_arm = self.route_arm_for(lane_wire, parent);
                let parent_arm_done_on_lane = parent_arm
                    .and_then(|arm| {
                        eff_index.and_then(|eff| {
                            self.scope_lane_last_eff_for_selected_path(
                                parent,
                                arm,
                                lane_wire,
                                completed_route,
                            )
                            .map(|last_eff| last_eff == eff)
                        })
                    })
                    .unwrap_or(false);
                if parent.kind() == ScopeKind::Route
                    && !parent_region.linger
                    && (self.cursor.index() >= parent_region.end || parent_arm_done_on_lane)
                {
                    if parent_arm_done_on_lane && self.cursor.index() < parent_region.end {
                        self.set_cursor_index(parent_region.end);
                    }
                    self.pop_route_arm(lane_wire, parent);
                    self.clear_scope_evidence(parent);
                    completed_route = parent_arm.map(|arm| (parent, arm));
                }
                if parent_region.linger {
                    if let Some(parent_arm) = parent_arm {
                        if parent_arm == 0 {
                            if self.cursor.index() >= parent_region.end || parent_arm_done_on_lane {
                                self.set_cursor_index(parent_region.start);
                            }
                            let first_eff = self
                                .scope_lane_first_eff_for_selected_path(
                                    parent,
                                    parent_arm,
                                    lane_wire,
                                    completed_route,
                                )
                                .or_else(|| self.cursor.scope_lane_first_eff(parent, lane_wire));
                            if let Some(first_eff) = first_eff {
                                let lane_idx = lane_wire as usize;
                                self.set_lane_cursor_to_eff_index(lane_idx, first_eff);
                            }
                        } else if self.cursor.index() >= parent_region.end
                            || parent_arm_done_on_lane
                        {
                            self.pop_route_arm(lane_wire, parent);
                            self.clear_scope_evidence(parent);
                            completed_route = Some((parent, parent_arm));
                        }
                    }
                }
            }
            parent_scope = parent;
        }
        self.prune_route_state_to_cursor_path_for_lane(lane_wire);
    }

    fn scope_lane_first_eff_for_selected_path(
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
            if self.node_matches_selected_route_path(idx, scope, arm, completed) {
                return Some(eff);
            }
            idx += 1;
        }
        None
    }

    fn scope_lane_last_eff_for_selected_path(
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
            if self.node_matches_selected_route_path(idx, scope, arm, completed) {
                found = Some(eff);
            }
            idx += 1;
        }
        found
    }

    fn node_matches_selected_route_path(
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

    /// Rendezvous id for the primary port.
    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.port().rv_id()
    }

    /// Get the descriptor-selected primary lane's port.
    ///
    /// # Safety invariant
    /// The primary port is always retained by construction. This is enforced
    /// at attach time and preserved throughout the endpoint's lifetime.
    fn port(&self) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[self.primary_lane].is_some(),
            "port: primary lane {} has no port (invariant violation)",
            self.primary_lane
        );
        // SAFETY: Primary port is always present by construction invariant.
        // In release builds, unwrap_unchecked could be used, but we keep
        // expect for defense-in-depth.
        self.ports[self.primary_lane]
            .as_ref()
            .expect("cursor endpoint retains primary port")
    }

    /// Get port for a specific lane.
    ///
    /// # Panics
    /// Panics if the port for `lane_idx` was not acquired.
    pub(crate) fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[lane_idx].is_some(),
            "port_for_lane: lane {} has no port",
            lane_idx
        );
        self.ports[lane_idx]
            .as_ref()
            .expect("port not acquired for lane")
    }

    #[inline]
    pub(crate) fn frontier_scratch_view(&self) -> FrontierScratchView {
        let port = self.port_for_lane(self.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = self.cursor.frontier_scratch_layout();
        frontier_scratch_view_from_storage(scratch_ptr, layout, self.cursor.max_frontier_entries())
    }

    pub(crate) fn loop_index(scope: ScopeId) -> Option<u8> {
        u8::try_from(scope.ordinal()).ok()
    }

    #[inline]
    pub(crate) fn offer_lane_set_for_scope(&self, scope_id: ScopeId) -> LaneSetView<'static> {
        self.cursor
            .route_scope_offer_lane_set(scope_id)
            .unwrap_or(LaneSetView::EMPTY)
    }

    #[inline]
    pub(crate) fn route_scope_arm_lane_set_for_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.cursor.route_scope_arm_lane_set(scope_id, arm)
    }

    #[inline]
    pub(crate) fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if let Some(lane_idx) = offer_lanes.first_set(self.cursor.logical_lane_count()) {
            lane_idx as u8
        } else {
            self.primary_lane as u8
        }
    }

    pub(in crate::endpoint::kernel) fn build_recvless_parent_route_decision_plan(
        &self,
        child_scope: ScopeId,
    ) -> Option<ParentRouteDecisionPlan> {
        let Some(parent_scope) = self.cursor.route_parent_scope(child_scope) else {
            return None;
        };
        let Some(parent_region) = self.cursor.scope_region_by_id(parent_scope) else {
            return None;
        };
        if !parent_region.linger {
            return None;
        }
        if self.cursor.is_route_controller(parent_scope) {
            return None;
        }
        let parent_is_dynamic = self
            .cursor
            .route_scope_controller_policy(parent_scope)
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        if parent_is_dynamic {
            return None;
        }
        let parent_requires_wire_recv = {
            let mut arm = 0u8;
            let mut requires_wire = false;
            while arm <= 1 {
                if self.arm_has_recv(parent_scope, arm)
                    && !self.is_non_wire_loop_control_arm(parent_scope, arm)
                {
                    requires_wire = true;
                    break;
                }
                if arm == 1 {
                    break;
                }
                arm += 1;
            }
            requires_wire
        };
        if parent_requires_wire_recv {
            return None;
        }
        let Some(parent_arm) = self.cursor.route_parent_arm(child_scope).and_then(Arm::new) else {
            return None;
        };
        Some(ParentRouteDecisionPlan {
            scope: parent_scope,
            arm: parent_arm.as_u8(),
            lane: self.offer_lane_for_scope(parent_scope),
        })
    }

    pub(in crate::endpoint::kernel) fn publish_recvless_parent_route_decision(
        &mut self,
        plan: ParentRouteDecisionPlan,
    ) {
        let Some(parent_arm) = Arm::new(plan.arm) else {
            return;
        };
        self.record_scope_ack(plan.scope, RouteDecisionToken::from_ack(parent_arm));
        self.record_route_decision_for_scope_lanes(plan.scope, plan.arm, plan.lane);
        self.emit_route_decision(plan.scope, plan.arm, RouteDecisionSource::Ack, plan.lane);
    }

    #[inline]
    pub(crate) fn controller_arm_at_cursor(&self, scope_id: ScopeId) -> Option<u8> {
        let idx = self.cursor.index();
        if let Some((entry, _)) = self.cursor.controller_arm_entry_by_arm(scope_id, 0)
            && idx == state_index_to_usize(entry)
        {
            return Some(0);
        }
        if let Some((entry, _)) = self.cursor.controller_arm_entry_by_arm(scope_id, 1)
            && idx == state_index_to_usize(entry)
        {
            return Some(1);
        }
        None
    }

    fn is_non_wire_loop_control_arm(&self, scope_id: ScopeId, arm: u8) -> bool {
        let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
        else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        let Some(recv_meta) = self.cursor.try_recv_meta_at(entry_idx) else {
            return false;
        };
        recv_meta.is_control
            && recv_meta.route_arm == Some(arm)
            && (recv_meta.peer == ROLE
                || (!self.cursor.is_route_controller(scope_id)
                    && self.control_semantic_kind(recv_meta.semantic).is_loop()))
    }

    #[cfg(test)]
    pub(crate) fn is_non_wire_loop_control_recv(
        &self,
        scope_id: ScopeId,
        arm: u8,
        label: u8,
    ) -> bool {
        let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
        else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        let Some(recv_meta) = self.cursor.try_recv_meta_at(entry_idx) else {
            return false;
        };
        if !recv_meta.is_control || recv_meta.label != label {
            return false;
        }
        if recv_meta.peer == ROLE {
            return true;
        }
        // Passive observers model controller self-send loop control as cross-role
        // control recv nodes; treat these labels as non-wire arm selectors.
        !self.cursor.is_route_controller(scope_id)
            && self.control_semantic_kind(recv_meta.semantic).is_loop()
    }
}
