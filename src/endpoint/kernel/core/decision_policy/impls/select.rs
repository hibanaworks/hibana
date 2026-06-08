use super::super::super::{
    Arm, CachedRecvMeta, CommitDelta, ControlSemanticKind, CpError, CursorEndpoint, DeferSource,
    DynamicPolicyResolution, EffIndex, EpochTable, LabelUniverse, MintConfigMarker,
    OfferScopeSelection, RecvError, RecvMeta, RecvResult, RendezvousId, ResolvedRouteArm,
    ResolverMode, RouteArmToken, RouteResolveStep, ScopeArmMaterializationMeta, ScopeId, SendMeta,
    TapEvent, Transport, checked_state_index, controller_arm_label, controller_arm_semantic_kind,
    emit, events, preview_selected_arm_for_scope_from_parts,
    require_selected_route_commit_row_from_parts, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    /// Preview recv metadata from a precomputed route-arm entry table.
    fn select_cached_route_arm_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        target_arm: u8,
    ) -> CachedRecvMeta {
        let Some(recv_entry) = materialization_meta.recv_entry(target_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        let idx = state_index_to_usize(recv_entry);
        let Some(meta) = self.cursor.try_recv_meta_at(idx) else {
            return CachedRecvMeta::EMPTY;
        };
        Self::cached_recv_meta_from_recv(idx, meta, Some(target_arm))
    }

    #[inline]
    fn cached_recv_meta_from_recv(
        cursor_index: usize,
        mut meta: RecvMeta,
        route_arm: Option<u8>,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(meta.next) else {
            return CachedRecvMeta::EMPTY;
        };
        if let Some(route_arm) = route_arm {
            meta.route_arm = Some(route_arm);
        }
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            frame_label: meta.frame_label,
            resource: meta.resource,
            semantic: meta.semantic,
            is_control: meta.is_control,
            next,
            scope: meta.scope,
            route_arm: meta.route_arm.unwrap_or(u8::MAX),
            is_choice_determinant: meta.is_choice_determinant,
            shot: meta.shot,
            policy: meta.policy,
            lane: meta.lane,
            flags: CachedRecvMeta::FLAG_RECV_STEP,
        }
    }

    #[inline]
    fn cached_recv_meta_from_send(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        meta: SendMeta,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(meta.next) else {
            return CachedRecvMeta::EMPTY;
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            frame_label: meta.frame_label,
            resource: meta.resource,
            semantic: meta.semantic,
            is_control: meta.is_control,
            next,
            scope: scope_id,
            route_arm,
            is_choice_determinant: false,
            shot: meta.shot,
            policy: meta.policy(),
            lane: meta.lane,
            flags: 0,
        }
    }

    #[inline]
    fn cached_recv_meta_from_local(
        cursor_index: usize,
        route_arm: u8,
        meta: crate::global::typestate::LocalMeta,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(meta.next) else {
            return CachedRecvMeta::EMPTY;
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: ROLE,
            label: meta.label,
            frame_label: meta.frame_label,
            resource: meta.resource,
            semantic: meta.semantic,
            is_control: meta.is_control,
            next,
            scope: meta.scope,
            route_arm,
            is_choice_determinant: false,
            shot: meta.shot,
            policy: meta.policy,
            lane: meta.lane,
            flags: 0,
        }
    }

    #[inline]
    fn synthetic_cached_recv_meta(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        label: u8,
        semantic: ControlSemanticKind,
        next: usize,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            return CachedRecvMeta::EMPTY;
        };
        let Some(next) = checked_state_index(next) else {
            return CachedRecvMeta::EMPTY;
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: EffIndex::ZERO,
            peer: ROLE,
            label,
            frame_label: 0,
            resource: None,
            semantic,
            is_control: true,
            next,
            scope: scope_id,
            route_arm,
            is_choice_determinant: false,
            shot: None,
            policy: ResolverMode::static_mode(),
            lane,
            flags: 0,
        }
    }

    #[inline]
    fn synthetic_cached_recv_meta_for_arm(
        &self,
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        next: usize,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(label) = controller_arm_label(&self.cursor, scope_id, route_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        let semantic = controller_arm_semantic_kind(
            &self.cursor,
            &self.control_semantics(),
            scope_id,
            route_arm,
        )
        .unwrap_or(ControlSemanticKind::DecisionArm);
        Self::synthetic_cached_recv_meta(
            cursor_index,
            scope_id,
            route_arm,
            label,
            semantic,
            next,
            lane,
        )
    }

    fn compute_passive_arm_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        scope_id: ScopeId,
        target_arm: u8,
        offer_lane: u8,
    ) -> CachedRecvMeta {
        let Some(entry) = materialization_meta.passive_arm_entry(target_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        let entry_idx = state_index_to_usize(entry);
        if let Some(recv_meta) = self.cursor.try_recv_meta_at(entry_idx) {
            return Self::cached_recv_meta_from_recv(entry_idx, recv_meta, None);
        }
        if let Some(send_meta) = self.cursor.try_send_meta_at(entry_idx) {
            return Self::cached_recv_meta_from_send(entry_idx, scope_id, target_arm, send_meta);
        }
        if !self.cursor.has_route_scope(scope_id) {
            return CachedRecvMeta::EMPTY;
        }
        if self.cursor.is_jump_at(entry_idx) {
            let Some(scope_end) = self.cursor.jump_target_at(entry_idx) else {
                return CachedRecvMeta::EMPTY;
            };
            if self.cursor.route_scope_linger(scope_id) {
                return self.synthetic_cached_recv_meta_for_arm(
                    scope_end, scope_id, target_arm, scope_end, offer_lane,
                );
            }
            if let Some(recv_meta) = self.cursor.try_recv_meta_at(scope_end) {
                return Self::cached_recv_meta_from_recv(scope_end, recv_meta, None);
            }
            if let Some(send_meta) = self.cursor.try_send_meta_at(scope_end) {
                return Self::cached_recv_meta_from_send(
                    scope_end, scope_id, target_arm, send_meta,
                );
            }
            return CachedRecvMeta::EMPTY;
        }
        if self.cursor.route_scope_linger(scope_id) {
            return self.synthetic_cached_recv_meta_for_arm(
                entry_idx, scope_id, target_arm, entry_idx, offer_lane,
            );
        }
        if let Some(target_idx) =
            self.preview_passive_materialization_index_for_selected_arm(scope_id, target_arm)
        {
            if let Some(recv_meta) = self.cursor.try_recv_meta_at(target_idx) {
                return Self::cached_recv_meta_from_recv(target_idx, recv_meta, Some(target_arm));
            }
            if let Some(send_meta) = self.cursor.try_send_meta_at(target_idx) {
                return Self::cached_recv_meta_from_send(
                    target_idx, scope_id, target_arm, send_meta,
                );
            }
        }
        CachedRecvMeta::EMPTY
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn compute_scope_passive_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        scope_id: ScopeId,
        offer_lane: u8,
    ) -> [CachedRecvMeta; 2] {
        [
            self.compute_passive_arm_recv_meta(materialization_meta, scope_id, 0, offer_lane),
            self.compute_passive_arm_recv_meta(materialization_meta, scope_id, 1, offer_lane),
        ]
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_arm_requires_materialization_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
    ) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        if is_route_controller && selection.at_route_offer_entry {
            if materialization_meta.controller_arm_entry(arm).is_some() {
                return materialization_meta.controller_arm_requires_ready_evidence(arm);
            }
        }
        if selection.at_route_offer_entry && materialization_meta.passive_arm_entry(arm).is_some() {
            if materialization_meta.arm_has_first_recv_dispatch(arm) {
                return !self
                    .selection_arm_dispatch_materializes_without_ready_evidence(selection, arm);
            }
            return false;
        }
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let Some(passive_meta) = passive_recv_meta.get(arm as usize).copied() else {
            return materialization_meta.recv_entry(arm).is_some();
        };
        if passive_meta.is_recv_step() {
            if passive_meta.peer == ROLE {
                return false;
            }
            if passive_meta.is_control {
                if materialization_meta
                    .controller_arm_entry(arm)
                    .map(|(_, label)| label)
                    == Some(passive_meta.label)
                {
                    return false;
                }
                if !is_route_controller
                    && self.control_semantic_kind(passive_meta.semantic).is_loop()
                {
                    return false;
                }
            }
            return true;
        }
        materialization_meta.recv_entry(arm).is_some()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_arm_dispatch_materializes_without_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        arm: u8,
    ) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(entry) = materialization_meta.passive_arm_entry(arm) else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        if self.cursor.is_recv_at(entry_idx)
            || self.cursor.is_send_at(entry_idx)
            || self.cursor.is_local_action_at(entry_idx)
            || self.cursor.is_jump_at(entry_idx)
        {
            return true;
        }
        self.cursor
            .route_scope_for_passive_arm_entry(
                selection.scope_id,
                entry_idx,
                materialization_meta.passive_arm_scope(arm),
            )
            .and_then(|scope| self.preview_selected_arm_for_scope(scope))
            .is_some()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_non_wire_loop_control_recv(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
        label: u8,
    ) -> bool {
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let Some(passive_meta) = passive_recv_meta.get(arm as usize).copied() else {
            return false;
        };
        passive_meta.is_recv_step()
            && passive_meta.is_control
            && passive_meta.label == label
            && (passive_meta.peer == ROLE
                || (!is_route_controller
                    && self.control_semantic_kind(passive_meta.semantic).is_loop()))
    }

    pub(in crate::endpoint::kernel) fn preview_selected_arm_meta(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
    ) -> RecvResult<CachedRecvMeta> {
        let scope_id = selection.scope_id;
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let controller_arm_entry = if self.cursor.is_route_controller(scope_id) {
            materialization_meta.controller_arm_entry(selected_arm)
        } else {
            None
        };

        let direct_meta = if let Some((arm_entry_idx, arm_entry_label)) = controller_arm_entry {
            let arm_entry_idx = state_index_to_usize(arm_entry_idx);
            if let Some(local_meta) = self.cursor.try_local_meta_at(arm_entry_idx) {
                Self::cached_recv_meta_from_local(arm_entry_idx, selected_arm, local_meta)
            } else {
                let semantic = controller_arm_semantic_kind(
                    &self.cursor,
                    &self.control_semantics(),
                    scope_id,
                    selected_arm,
                )
                .unwrap_or(ControlSemanticKind::DecisionArm);
                Self::synthetic_cached_recv_meta(
                    arm_entry_idx,
                    scope_id,
                    selected_arm,
                    arm_entry_label,
                    semantic,
                    arm_entry_idx,
                    selection.offer_lane,
                )
            }
        } else if selected_arm < materialization_meta.arm_count {
            self.select_cached_route_arm_recv_meta(materialization_meta, selected_arm)
        } else {
            CachedRecvMeta::EMPTY
        };

        let meta = if !direct_meta.is_empty() {
            direct_meta
        } else {
            passive_recv_meta
                .get(selected_arm as usize)
                .copied()
                .ok_or(RecvError::PhaseInvariant)?
        };

        Ok(meta)
    }

    pub(in crate::endpoint::kernel) fn descend_selected_passive_route(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
    ) -> RecvResult<bool> {
        if resolved.resolved_hint_frame.is_some() {
            return Ok(false);
        }
        let scope_id = selection.scope_id;
        let selected_arm = resolved.selected_arm;
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(nested_scope) = materialization_meta.passive_arm_scope(selected_arm) else {
            return Ok(false);
        };
        let nested_scope = self.rebase_passive_descendant_scope(scope_id, nested_scope);
        let parent_route_arm_selection_plan =
            self.build_recvless_parent_route_arm_selection_plan(scope_id);
        let (target_index, route_rows) = {
            let Self {
                ports,
                cursor,
                decision_state,
                route_commit_rows,
                ..
            } = self;
            let mut route_rows = route_commit_rows.begin()?;
            let mut route_row_result = Ok(());
            let target_index = cursor
                .visit_passive_route_materialization_rows(
                    scope_id,
                    nested_scope,
                    selected_arm,
                    |target_scope| {
                        preview_selected_arm_for_scope_from_parts::<ROLE, T, E>(
                            ports,
                            decision_state,
                            cursor,
                            target_scope,
                        )
                    },
                    |target_scope, arm| {
                        if route_rows.contains_lane_scope(selection.offer_lane, target_scope) {
                            return true;
                        }
                        route_row_result = require_selected_route_commit_row_from_parts(
                            decision_state,
                            cursor,
                            selection.offer_lane,
                            target_scope,
                            arm,
                        )
                        .and_then(|row| route_rows.push_unique(row));
                        route_row_result.is_ok()
                    },
                )
                .ok_or(RecvError::PhaseInvariant)?;
            route_row_result?;
            (target_index, route_rows.as_commit_rows())
        };
        if let Some(plan) = parent_route_arm_selection_plan {
            self.publish_recvless_parent_route_arm_selection(plan);
        }
        if resolved.route_token.is_poll() {
            self.emit_route_arm_selection(scope_id, resolved.route_token, selection.offer_lane);
        }
        let delta = CommitDelta::route_rows(
            route_rows,
            checked_state_index(target_index).ok_or(RecvError::PhaseInvariant)?,
        );
        let delta = self
            .prepare_commit_delta(delta)
            .map_err(|_| RecvError::PhaseInvariant)?;
        self.commit_prepared_delta(delta);
        self.sync_lane_offer_state();
        Ok(true)
    }

    pub(in crate::endpoint::kernel) fn emit_route_arm_selection(
        &self,
        scope_id: ScopeId,
        token: RouteArmToken,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let causal = TapEvent::make_causal_key(port.lane().as_wire(), token.as_tap_seq());
        let arg0 = self.sid.raw();
        let arg1 = ((scope_id.raw() as u32) << 16) | (token.arm().as_u8() as u32);
        let mut event = events::RouteArmSelection::with_causal(port.now32(), causal, arg0, arg1);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    #[inline]
    pub(crate) fn record_route_arm_selection_for_scope_lanes(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
        decision_lane: u8,
    ) {
        if !self.cursor.has_route_scope(scope_id) {
            self.record_route_arm_selection_for_lane(decision_lane as usize, scope_id, arm);
            return;
        }

        let logical_lane_count = self.cursor.logical_lane_count();
        let Some(candidate_lanes) = self.route_scope_arm_lane_set_for_scope(scope_id, arm) else {
            if (decision_lane as usize) < logical_lane_count {
                self.record_route_arm_selection_for_lane(decision_lane as usize, scope_id, arm);
            }
            return;
        };
        let mut recorded = false;
        let mut next = candidate_lanes.first_set(logical_lane_count);
        while let Some(lane_idx) = next {
            if self
                .cursor
                .route_arm_lane_last_eff(scope_id, arm, lane_idx as u8)
                .is_some()
            {
                self.record_route_arm_selection_for_lane(lane_idx, scope_id, arm);
                recorded = true;
            }
            next = candidate_lanes.next_set_from(lane_idx.saturating_add(1), logical_lane_count);
        }

        if !recorded && (decision_lane as usize) < logical_lane_count {
            self.record_route_arm_selection_for_lane(decision_lane as usize, scope_id, arm);
        }
    }

    pub(in crate::endpoint::kernel) fn prepare_route_arm_selection_from_resolver(
        &mut self,
        scope_id: ScopeId,
        signals: &crate::transport::context::PolicySignals,
    ) -> RecvResult<RouteResolveStep> {
        let (policy, eff_index, _tag, subject) = self
            .cursor
            .route_scope_controller_policy(scope_id)
            .ok_or(RecvError::PhaseInvariant)?;
        if !policy.is_dynamic() {
            return Err(RecvError::PhaseInvariant);
        }
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(RecvError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane = self.offer_lane_for_scope(scope_id);
        self.emit_decision_policy_audit(scope_id, offer_lane, policy_id, signals)
            .map_err(|_| RecvError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let resolution = match cluster.resolve_dynamic_policy(rv_id, eff_index, subject) {
            Ok(resolution) => resolution,
            Err(CpError::PolicyAbort { reason }) => return Ok(RouteResolveStep::Abort(reason)),
            Err(_) => return Err(RecvError::PhaseInvariant),
        };
        let arm = match resolution {
            DynamicPolicyResolution::DecisionArm { arm } => arm,
            DynamicPolicyResolution::Defer => {
                return Ok(RouteResolveStep::Deferred {
                    source: DeferSource::Resolver,
                });
            }
        };
        let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
        self.record_route_arm_selection_for_scope_lanes(scope_id, arm.as_u8(), offer_lane);
        self.record_scope_ack(scope_id, RouteArmToken::from_resolver(arm));
        self.emit_route_arm_selection(scope_id, RouteArmToken::from_resolver(arm), offer_lane);
        Ok(RouteResolveStep::Resolved(arm))
    }
}
