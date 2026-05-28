use super::super::super::{
    ARM_SHARED, Arm, BindingSlot, CachedRecvMeta, ControlSemanticKind, CpError, CursorEndpoint,
    DeferSource, DynamicPolicyResolution, EffIndex, EpochTable, LabelUniverse, Lane,
    MintConfigMarker, OfferScopeSelection, PolicyMode, RecvError, RecvMeta, RecvResult,
    RendezvousId, ResolvedRouteDecision, RouteDecisionSource, RouteDecisionToken, RouteResolveStep,
    ScopeArmMaterializationMeta, ScopeId, ScopeKind, SendMeta, TapEvent, Transport,
    checked_state_index, controller_arm_label, controller_arm_semantic_kind, emit, events,
    preview_selected_arm_for_scope_from_parts, require_route_arm_commit_proof_from_parts,
    route_scope_materialization_index_from_cursor, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
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
            policy: PolicyMode::static_mode(),
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
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return CachedRecvMeta::EMPTY;
        };
        if self.cursor.is_jump_at(entry_idx) {
            let Some(scope_end) = self.cursor.jump_target_at(entry_idx) else {
                return CachedRecvMeta::EMPTY;
            };
            if region.linger {
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
        if region.linger {
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
        materialization_meta
            .passive_arm_scope(arm)
            .or_else(|| {
                let scope = self.cursor.node_scope_id_at(entry_idx);
                (scope != selection.scope_id && scope.kind() == ScopeKind::Route).then_some(scope)
            })
            .filter(|scope| scope.kind() == ScopeKind::Route)
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

    /// Preview recv metadata from a precomputed first-recv dispatch table.
    fn select_cached_dispatch_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        target_arm: u8,
        lane: u8,
        resolved_hint_frame_label: Option<u8>,
    ) -> CachedRecvMeta {
        let Some(frame_label) = resolved_hint_frame_label else {
            return CachedRecvMeta::EMPTY;
        };
        let Some((dispatch_arm, target_idx)) =
            materialization_meta.first_recv_target_for_lane_frame_label(lane, frame_label)
        else {
            return CachedRecvMeta::EMPTY;
        };
        if dispatch_arm != ARM_SHARED && dispatch_arm != target_arm {
            return CachedRecvMeta::EMPTY;
        }
        let target_idx = state_index_to_usize(target_idx);
        let route_arm = if dispatch_arm == ARM_SHARED {
            target_arm
        } else {
            dispatch_arm
        };
        let Some(meta) = self.cursor.try_recv_meta_at(target_idx) else {
            return CachedRecvMeta::EMPTY;
        };
        Self::cached_recv_meta_from_recv(target_idx, meta, Some(route_arm))
    }

    pub(in crate::endpoint::kernel) fn preview_selected_arm_meta(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
        resolved_hint_frame_label: Option<u8>,
    ) -> RecvResult<CachedRecvMeta> {
        let scope_id = selection.scope_id;
        let materialization_meta = self.selection_materialization_meta(selection);
        let passive_recv_meta = self.selection_passive_recv_meta(selection, materialization_meta);
        let controller_arm_entry =
            if selection.at_route_offer_entry && self.cursor.is_route_controller(scope_id) {
                materialization_meta.controller_arm_entry(selected_arm)
            } else {
                None
            };
        let dispatch_meta = if controller_arm_entry.is_none() {
            self.select_cached_dispatch_recv_meta(
                materialization_meta,
                selected_arm,
                selection.offer_lane,
                resolved_hint_frame_label,
            )
        } else {
            CachedRecvMeta::EMPTY
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
        } else if !dispatch_meta.is_empty() {
            dispatch_meta
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
        resolved: ResolvedRouteDecision,
    ) -> RecvResult<bool> {
        if resolved.resolved_hint_frame_label.is_some() {
            return Ok(false);
        }
        let scope_id = selection.scope_id;
        let selected_arm = resolved.selected_arm;
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(nested_scope) = materialization_meta.passive_arm_scope(selected_arm) else {
            return Ok(false);
        };
        let nested_scope = self.rebase_passive_descendant_scope(scope_id, nested_scope);
        if nested_scope == scope_id || nested_scope.kind() != ScopeKind::Route {
            return Ok(false);
        }
        let parent_route_decision_plan = self.build_recvless_parent_route_decision_plan(scope_id);
        let mut target_scope = nested_scope;
        let target_index = {
            let required = self.route_scope_depth_bound();
            let Self {
                ports,
                cursor,
                decision_state,
                route_commit_proofs,
                ..
            } = self;
            let mut route_arm_proofs = route_commit_proofs.begin(required)?;
            route_arm_proofs.push_unique(require_route_arm_commit_proof_from_parts(
                decision_state,
                cursor,
                selection.offer_lane,
                scope_id,
                selected_arm,
            )?)?;
            let target_index = loop {
                let target_preview_arm = preview_selected_arm_for_scope_from_parts::<ROLE, T, E>(
                    ports,
                    decision_state,
                    cursor,
                    target_scope,
                );
                if let Some(arm) = target_preview_arm {
                    if !route_arm_proofs.contains_lane_scope(selection.offer_lane, target_scope) {
                        route_arm_proofs.push_unique(require_route_arm_commit_proof_from_parts(
                            decision_state,
                            cursor,
                            selection.offer_lane,
                            target_scope,
                            arm,
                        )?)?;
                    }
                    if let Some(child_scope) = cursor.passive_arm_scope_by_arm(target_scope, arm)
                        && child_scope.kind() == ScopeKind::Route
                    {
                        target_scope = child_scope;
                        continue;
                    }
                }
                break route_scope_materialization_index_from_cursor(cursor, target_scope)
                    .ok_or(RecvError::PhaseInvariant)?;
            };
            for proof in route_arm_proofs.iter() {
                decision_state.commit_route_arm_after_preflight(proof);
            }
            target_index
        };
        if let Some(plan) = parent_route_decision_plan {
            self.publish_recvless_parent_route_decision(plan);
        }
        if matches!(resolved.route_token.source(), RouteDecisionSource::Poll) {
            self.emit_route_decision(
                scope_id,
                selected_arm,
                RouteDecisionSource::Poll,
                selection.offer_lane,
            );
        }
        self.set_cursor_index(target_index);
        self.sync_lane_offer_state();
        Ok(true)
    }

    pub(in crate::endpoint::kernel) fn emit_route_decision(
        &self,
        scope_id: ScopeId,
        arm: u8,
        source: RouteDecisionSource,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let causal = TapEvent::make_causal_key(port.lane().as_wire(), source.as_tap_seq());
        let arg0 = self.sid.raw();
        let arg1 = ((scope_id.raw() as u32) << 16) | (arm as u32);
        let mut event = events::RouteDecision::with_causal(port.now32(), causal, arg0, arg1);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    #[inline]
    pub(crate) fn record_route_decision_for_scope_lanes(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
        decision_lane: u8,
    ) {
        if scope_id.is_none() || scope_id.kind() != ScopeKind::Route {
            self.record_route_decision_for_lane(decision_lane as usize, scope_id, arm);
            return;
        }

        let logical_lane_count = self.cursor.logical_lane_count();
        let Some(candidate_lanes) = self.route_scope_arm_lane_set_for_scope(scope_id, arm) else {
            if (decision_lane as usize) < logical_lane_count {
                self.record_route_decision_for_lane(decision_lane as usize, scope_id, arm);
            }
            return;
        };
        let mut recorded = false;
        let mut next = candidate_lanes.first_set(logical_lane_count);
        while let Some(lane_idx) = next {
            if self
                .cursor
                .scope_lane_last_eff_for_arm(scope_id, arm, lane_idx as u8)
                .is_some()
            {
                self.record_route_decision_for_lane(lane_idx, scope_id, arm);
                recorded = true;
            }
            next = candidate_lanes.next_set_from(lane_idx.saturating_add(1), logical_lane_count);
        }

        if !recorded && (decision_lane as usize) < logical_lane_count {
            self.record_route_decision_for_lane(decision_lane as usize, scope_id, arm);
        }
    }

    pub(in crate::endpoint::kernel) fn prepare_route_decision_from_resolver(
        &mut self,
        scope_id: ScopeId,
        signals: &crate::transport::context::PolicySignals,
    ) -> RecvResult<RouteResolveStep> {
        let (policy, eff_index, tag, op) = self
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
        let port = self.port_for_lane(offer_lane as usize);
        let lane = Lane::new(port.lane().raw());
        let attrs = *signals.attrs();
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            eff_index,
            tag,
            op,
            signals.input(),
            &attrs,
        ) {
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
        self.record_route_decision_for_scope_lanes(scope_id, arm.as_u8(), offer_lane);
        self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
        self.emit_route_decision(
            scope_id,
            arm.as_u8(),
            RouteDecisionSource::Resolver,
            offer_lane,
        );
        Ok(RouteResolveStep::Resolved(arm))
    }
}
