use super::*;

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
    pub(crate) fn evaluate_dynamic_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        control: Option<ControlDesc>,
    ) -> SendResult<()> {
        if !meta.policy().is_dynamic() {
            return Ok(());
        }
        if let Some(control) = control
            && control_policy_is_validated_during_handle_preparation(control.op())
        {
            return Ok(());
        }
        let dynamic_kind = self.control_semantic_kind(meta.semantic);
        let route_signals = self.policy_signals_for_slot(PolicySlot::Route).into_owned();
        match dynamic_kind {
            ControlSemanticKind::LoopContinue | ControlSemanticKind::LoopBreak => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_loop_policy(meta, op, &route_signals)
            }
            ControlSemanticKind::RouteArm => {
                let op = control.ok_or(SendError::PhaseInvariant)?.op();
                self.evaluate_route_policy(meta, target_label, op, &route_signals)
            }
            ControlSemanticKind::Other => {
                if control.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                let op = if meta.scope.is_none() {
                    ControlOp::RouteDecision
                } else {
                    self.cursor
                        .route_scope_controller_policy(meta.scope)
                        .map(|(_, _, _, op)| op)
                        .unwrap_or(ControlOp::RouteDecision)
                };
                self.evaluate_route_policy(meta, target_label, op, &route_signals)
            }
        }
    }

    fn emit_route_policy_audit(
        &self,
        scope_id: ScopeId,
        lane: u8,
        policy_id: u16,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) {
        let port = self.port_for_lane(lane as usize);
        let _ = port.flush_transport_events();
        let transport_attrs = port.transport().metrics().attrs();
        let mut policy_attrs = *signals.attrs();
        policy_attrs.copy_from(&transport_attrs);
        let policy_input = signals.input;
        let arg0 = route_policy_input_arg0(&policy_input);
        let mut event = events::RawEvent::new(port.now32(), ids::ROUTE_DECISION)
            .with_arg0(arg0)
            .with_arg1(policy_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let policy_digest = port.policy_digest(PolicySlot::Route);
        let event_hash = policy_runtime::hash_tap_event(&event);
        let signals_input_hash = policy_runtime::hash_policy_input(policy_input);
        let policy_attrs_hash = policy_attrs.hash32();
        let transport_snapshot_hash = policy_runtime::hash_transport_attrs(&policy_attrs);
        let replay_transport = policy_runtime::replay_transport_inputs(&policy_attrs);
        let replay_transport_presence = policy_runtime::replay_transport_presence(&policy_attrs);
        let mode_id = policy_runtime::POLICY_MODE_AUDIT_ONLY_TAG;
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            policy_attrs_hash,
            transport_snapshot_hash,
            ((policy_runtime::slot_tag(PolicySlot::Route) as u32) << 24) | ((mode_id as u32) << 16),
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_input[0],
            policy_input[1],
            policy_input[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT1,
            policy_input[3],
            0,
            0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT0,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT1,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
            port.lane(),
        );
        let verdict = policy_runtime::PolicyVerdict::NoEngine;
        let verdict_meta = ((policy_runtime::verdict_tag(verdict) as u32) << 24)
            | ((policy_runtime::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            policy_runtime::verdict_reason(verdict) as u32,
            policy_runtime::POLICY_FUEL_NONE as u32,
            port.lane(),
        );
    }

    fn evaluate_route_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        op: ControlOp,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        // Route decisions are fixed at the offer/decode decision point.
        // Re-evaluating dynamic route policy for local self-send can diverge from
        // the selected arm and introduce non-deterministic PolicyAbort.
        if meta.peer == ROLE {
            return Ok(());
        }

        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        self.emit_route_policy_audit(scope_id, meta.lane, policy_id, signals);

        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                op,
                signals.input,
                &attrs,
            )
            .map_err(Self::map_cp_error)?;

        match resolution {
            DynamicPolicyResolution::RouteArm { arm } if arm == arm_index => Ok(()),
            DynamicPolicyResolution::RouteArm { .. } => {
                Err(SendError::PolicyAbort { reason: policy_id })
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    fn evaluate_loop_policy(
        &mut self,
        meta: &SendMeta,
        op: ControlOp,
        signals: &crate::transport::context::PolicySignals<'_>,
    ) -> SendResult<()> {
        // For local control (self-send), the caller explicitly chooses continue/break.
        // No resolver validation is needed - the caller's choice is authoritative.
        if meta.peer == ROLE {
            return Ok(());
        }

        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;

        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                op,
                signals.input,
                &attrs,
            )
            .map_err(Self::map_cp_error)?;

        if meta.scope.is_none() || meta.scope != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match resolution {
            DynamicPolicyResolution::Loop { decision } => {
                let disposition = if decision {
                    LoopDisposition::Continue
                } else {
                    LoopDisposition::Break
                };
                if !loop_control_kind_matches_disposition(meta.semantic, disposition) {
                    return Err(SendError::PolicyAbort { reason: policy_id });
                }
                Ok(())
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

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
        .unwrap_or(ControlSemanticKind::RouteArm);
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
    pub(crate) fn compute_scope_passive_recv_meta(
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
    pub(crate) fn selection_arm_requires_materialization_ready_evidence(
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
    pub(crate) fn selection_arm_dispatch_materializes_without_ready_evidence(
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
    pub(crate) fn selection_non_wire_loop_control_recv(
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

    pub(crate) fn preview_selected_arm_meta(
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
                .unwrap_or(ControlSemanticKind::RouteArm);
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

    pub(crate) fn descend_selected_passive_route(
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
                route_state,
                route_commit_proofs,
                ..
            } = self;
            let mut route_arm_proofs = route_commit_proofs.begin(required)?;
            route_arm_proofs.push_unique(require_route_arm_commit_proof_from_parts(
                route_state,
                cursor,
                selection.offer_lane,
                scope_id,
                selected_arm,
            )?)?;
            let target_index = loop {
                let target_preview_arm = preview_selected_arm_for_scope_from_parts::<ROLE, T, E>(
                    ports,
                    route_state,
                    cursor,
                    target_scope,
                );
                if let Some(arm) = target_preview_arm {
                    if !route_arm_proofs.contains_lane_scope(selection.offer_lane, target_scope) {
                        route_arm_proofs.push_unique(require_route_arm_commit_proof_from_parts(
                            route_state,
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
                route_state.commit_route_arm_after_preflight(proof);
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

    pub(crate) fn emit_route_decision(
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

    pub(crate) fn prepare_route_decision_from_resolver(
        &mut self,
        scope_id: ScopeId,
        signals: &crate::transport::context::PolicySignals<'_>,
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
        self.emit_route_policy_audit(scope_id, offer_lane, policy_id, signals);
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(offer_lane as usize);
        let lane = Lane::new(port.lane().raw());
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            eff_index,
            tag,
            op,
            signals.input,
            &attrs,
        ) {
            Ok(resolution) => resolution,
            Err(CpError::PolicyAbort { reason }) => return Ok(RouteResolveStep::Abort(reason)),
            Err(_) => return Err(RecvError::PhaseInvariant),
        };
        let arm = match resolution {
            DynamicPolicyResolution::RouteArm { arm } => arm,
            DynamicPolicyResolution::Loop { .. } => return Err(RecvError::PhaseInvariant),
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
