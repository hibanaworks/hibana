use super::super::super::{
    Arm, CachedRecvMeta, CachedRouteArm, ClusterError, CommitDelta, CursorEndpoint, EffIndex,
    EventSemanticKind, OfferScopeSelection, RecvError, RecvMeta, RecvResult, RendezvousId,
    ResolvedRouteArm, RouteArmToken, RouteResolveStep, RouteResolver, ScopeId, SendMeta, TapEvent,
    Transport, checked_state_index, controller_arm_label, controller_arm_semantic_kind, emit,
    events, prepare_route_site_materialization_rows_from_resident_route_commit_range,
    preview_selected_arm_for_scope_from_parts, state_index_to_usize,
};
use crate::eff::EventOrigin;
use crate::global::typestate::RouteChoiceMark;
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    /// Preview recv metadata from a precomputed route-arm entry table.
    fn select_cached_route_arm_recv_meta(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> CachedRecvMeta {
        let Some(recv_entry) = self
            .cursor
            .route_scope_arm_recv_index(scope_id, target_arm)
            .and_then(checked_state_index)
        else {
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
            crate::invariant();
        };
        let Some(next) = checked_state_index(meta.next) else {
            crate::invariant();
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
            semantic: meta.semantic,
            origin: meta.origin,
            next,
            scope: meta.scope,
            route_arm: CachedRouteArm::from_option(meta.route_arm),
            choice: meta.choice,
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
            crate::invariant();
        };
        let Some(next) = checked_state_index(meta.next) else {
            crate::invariant();
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            frame_label: meta.frame_label,
            semantic: meta.semantic,
            origin: meta.origin,
            next,
            scope: scope_id,
            route_arm: CachedRouteArm::some(route_arm),
            choice: RouteChoiceMark::Ordinary,
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
            crate::invariant();
        };
        let Some(next) = checked_state_index(meta.next) else {
            crate::invariant();
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: ROLE,
            label: meta.label,
            frame_label: meta.frame_label,
            semantic: meta.semantic,
            origin: meta.origin,
            next,
            scope: meta.scope,
            route_arm: CachedRouteArm::some(route_arm),
            choice: RouteChoiceMark::Ordinary,
            lane: meta.lane,
            flags: 0,
        }
    }

    #[inline]
    fn route_arm_cached_recv_meta(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        label: u8,
        semantic: EventSemanticKind,
        next: usize,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            crate::invariant();
        };
        let Some(next) = checked_state_index(next) else {
            crate::invariant();
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: EffIndex::ZERO,
            peer: ROLE,
            label,
            frame_label: 0,
            semantic,
            origin: EventOrigin::Session,
            next,
            scope: scope_id,
            route_arm: CachedRouteArm::some(route_arm),
            choice: RouteChoiceMark::Ordinary,
            lane,
            flags: 0,
        }
    }

    #[inline]
    fn route_arm_cached_recv_meta_for_arm(
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
        let semantic = crate::invariant_some(controller_arm_semantic_kind(
            &self.cursor,
            scope_id,
            route_arm,
        ));
        Self::route_arm_cached_recv_meta(
            cursor_index,
            scope_id,
            route_arm,
            label,
            semantic,
            next,
            lane,
        )
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn compute_passive_arm_recv_meta(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
        offer_lane: u8,
    ) -> CachedRecvMeta {
        let Some(entry) = self.cursor.passive_observer_arm_entry(scope_id, target_arm) else {
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
        if self.cursor.route_scope_reentry(scope_id) {
            return self.route_arm_cached_recv_meta_for_arm(
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

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn selection_arm_requires_materialization_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
    ) -> bool {
        let scope_id = selection.scope_id;
        let at_route_entry = selection.entry_position.is_route_entry();
        let controller_arm_entry = self
            .cursor
            .shared_controller_arm_entry_by_arm(scope_id, arm);
        if is_route_controller
            && at_route_entry
            && let Some((entry, _)) = controller_arm_entry
        {
            return self
                .cursor
                .try_recv_meta_at(state_index_to_usize(entry))
                .is_some_and(|recv_meta| recv_meta.peer != ROLE);
        }
        if at_route_entry
            && self
                .cursor
                .passive_observer_arm_entry(scope_id, arm)
                .is_some()
        {
            if self.selected_arm_has_first_recv_dispatch(scope_id, arm) {
                return !self
                    .selection_arm_dispatch_materializes_without_ready_evidence(scope_id, arm);
            }
            return false;
        }
        if arm as usize >= 2 {
            return self
                .cursor
                .route_scope_arm_recv_index(scope_id, arm)
                .is_some();
        }
        if let Some(passive_recv) = self.selected_passive_arm_recv_ready_meta(scope_id, arm) {
            if passive_recv.peer == ROLE {
                return false;
            }
            if passive_recv.origin.is_session()
                && controller_arm_entry.map(|(_, label)| label) == Some(passive_recv.label)
            {
                return false;
            }
            return true;
        }
        self.cursor
            .route_scope_arm_recv_index(scope_id, arm)
            .is_some()
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_arm_dispatch_materializes_without_ready_evidence(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> bool {
        let Some(entry) = self.cursor.passive_observer_arm_entry(scope_id, arm) else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        if self.cursor.is_recv_at(entry_idx)
            || self.cursor.is_send_at(entry_idx)
            || self.cursor.is_local_action_at(entry_idx)
        {
            return true;
        }
        self.cursor
            .passive_child_scope(scope_id, arm)
            .and_then(|scope| self.preview_selected_arm_for_scope(scope))
            .is_some()
    }

    #[inline]
    fn selected_arm_has_first_recv_dispatch(&self, scope_id: ScopeId, arm: u8) -> bool {
        self.cursor
            .route_scope_first_recv_dispatch_arm_mask(scope_id)
            .is_some_and(|mask| arm < 2 && (mask & (1u8 << arm)) != 0)
    }

    #[inline(never)]
    fn selected_passive_arm_recv_ready_meta(&self, scope_id: ScopeId, arm: u8) -> Option<RecvMeta> {
        let entry_idx = self
            .cursor
            .passive_observer_arm_entry_index(scope_id, arm)?;
        self.cursor.try_recv_meta_at(entry_idx)
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn preview_selected_arm_meta(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
    ) -> RecvResult<CachedRecvMeta> {
        let scope_id = selection.scope_id;
        let controller_arm_entry = if self.cursor.is_route_controller(scope_id) {
            self.cursor
                .shared_controller_arm_entry_by_arm(scope_id, selected_arm)
        } else {
            None
        };

        let direct_meta = if let Some((arm_entry_idx, arm_entry_label)) = controller_arm_entry {
            let arm_entry_idx = state_index_to_usize(arm_entry_idx);
            if let Some(local_meta) = self.cursor.try_local_meta_at(arm_entry_idx) {
                Self::cached_recv_meta_from_local(arm_entry_idx, selected_arm, local_meta)
            } else {
                let semantic = controller_arm_semantic_kind(&self.cursor, scope_id, selected_arm)
                    .ok_or(RecvError::PhaseInvariant)?;
                Self::route_arm_cached_recv_meta(
                    arm_entry_idx,
                    scope_id,
                    selected_arm,
                    arm_entry_label,
                    semantic,
                    arm_entry_idx,
                    selection.offer_lane,
                )
            }
        } else if selected_arm
            < self
                .cursor
                .route_scope_arm_count(scope_id)
                .ok_or(RecvError::PhaseInvariant)?
        {
            self.select_cached_route_arm_recv_meta(scope_id, selected_arm)
        } else {
            CachedRecvMeta::EMPTY
        };

        let meta = if !direct_meta.is_empty() {
            direct_meta
        } else {
            if selected_arm as usize >= 2 {
                return Err(RecvError::PhaseInvariant);
            }
            self.compute_passive_arm_recv_meta(scope_id, selected_arm, selection.offer_lane)
        };

        Ok(meta)
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn descend_selected_passive_route(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
        observed_frame_label: Option<u8>,
    ) -> RecvResult<bool> {
        let scope_id = selection.scope_id;
        let selected_arm = resolved.selected_arm;
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(nested_scope) = materialization_meta.passive_child_scope(selected_arm) else {
            return Ok(false);
        };
        if let Some(observed_label) = observed_frame_label {
            let meta = self.preview_selected_arm_meta(selection, selected_arm)?;
            if !meta.is_empty() && meta.frame_label == observed_label {
                return Ok(false);
            }
        }
        let nested_scope = self.rebase_passive_descendant_scope(scope_id, nested_scope);
        let (target_index, route_rows) = {
            let Self {
                ports,
                cursor,
                decision_state,
                route_commit_rows,
                ..
            } = self;
            let mut route_rows = route_commit_rows.begin();
            let mut route_row_result = Ok(());
            let target_index = cursor
                .visit_passive_route_materialization_rows(
                    scope_id,
                    nested_scope,
                    selected_arm,
                    |target_scope| {
                        preview_selected_arm_for_scope_from_parts::<ROLE, T>(
                            ports,
                            decision_state,
                            cursor,
                            target_scope,
                        )
                    },
                    |target_scope, arm| {
                        route_row_result =
                            prepare_route_site_materialization_rows_from_resident_route_commit_range(
                                decision_state,
                                cursor,
                                selection.offer_lane,
                                target_scope,
                                arm,
                                &mut route_rows,
                            );
                        route_row_result.is_ok()
                    },
                )
                .ok_or(RecvError::PhaseInvariant)?;
            route_row_result?;
            (
                target_index,
                route_rows.as_route_only_commit_rows(selection.offer_lane),
            )
        };
        let emit_poll_selection = resolved.route_token.is_poll();
        let delta = CommitDelta::route_rows(
            route_rows,
            checked_state_index(target_index).ok_or(RecvError::PhaseInvariant)?,
        );
        let delta = self
            .prepare_commit_delta(delta)
            .map_err(|_| RecvError::PhaseInvariant)?;
        self.commit_prepared_delta(delta);
        if emit_poll_selection {
            self.emit_route_arm_selection(scope_id, resolved.route_token, selection.offer_lane);
        }
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
        let event = events::route_arm_selection_with_causal(
            port.now32(),
            causal,
            arg0,
            scope_id,
            token.arm().as_u8(),
        );
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
            next = candidate_lanes.next_set_from(lane_idx + 1, logical_lane_count);
        }

        if !recorded && (decision_lane as usize) < logical_lane_count {
            self.record_route_arm_selection_for_lane(decision_lane as usize, scope_id, arm);
        }
    }

    pub(in crate::endpoint::kernel) fn prepare_route_arm_selection_from_resolver(
        &mut self,
        scope_id: ScopeId,
    ) -> RecvResult<RouteResolveStep> {
        let (resolver, _tag) = self
            .cursor
            .route_scope_controller_resolver(scope_id)
            .ok_or(RecvError::PhaseInvariant)?;
        let RouteResolver::Dynamic {
            resolver_id,
            scope: resolver_scope,
        } = resolver
        else {
            return Err(RecvError::PhaseInvariant);
        };
        if scope_id.is_none() || scope_id != resolver_scope {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane = self.offer_lane_for_scope(scope_id);
        let cluster = self.session.cluster();
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let resolution = match cluster.resolve_dynamic_resolver(rv_id, scope_id, resolver_id) {
            Ok(resolution) => resolution,
            Err(ClusterError::ResolverReject { resolver_id }) => {
                self.emit_dynamic_resolver_reject_audit(offer_lane, scope_id, resolver_id);
                return Ok(RouteResolveStep::Reject(resolver_id));
            }
            Err(
                ClusterError::RendezvousMismatch { .. }
                | ClusterError::RendezvousUnregistered { .. }
                | ClusterError::RendezvousBusy { .. }
                | ClusterError::ResourceExhausted { .. }
                | ClusterError::DynamicResolverInvariant { .. },
            ) => return Err(RecvError::PhaseInvariant),
        };
        let arm = Arm::new(resolution.index()).ok_or(RecvError::PhaseInvariant)?;
        Ok(RouteResolveStep::Resolved(arm))
    }
}
