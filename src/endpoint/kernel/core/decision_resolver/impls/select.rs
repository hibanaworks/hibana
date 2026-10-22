use super::super::super::{
    Arm, CachedRecvMeta, ClusterError, CommitDelta, EffIndex, OfferScopeSelection, OperationKernel,
    RecvError, RecvMeta, RecvResult, ResolvedRouteArm, RouteArmToken, RouteResolveStep, ScopeId,
    SendMeta, TapEvent, Transport, checked_state_index, controller_arm_label, emit, events,
    prepare_route_site_materialization_rows_from_resident_route_commit_range,
    preview_selected_arm_for_scope_from_parts, state_index_to_usize,
};
use crate::eff::EventOrigin;
use crate::global::typestate::{InboundFrameKey, LocalAction};
impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    /// Preview recv metadata from a precomputed route-arm entry table.
    fn select_cached_route_arm_recv_meta(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> CachedRecvMeta {
        let target_arm = Arm::from_raw(target_arm).as_u8();
        let Some(recv_entry) = self
            .progress
            .cursor
            .route_scope_arm_recv_index(scope_id, target_arm)
            .and_then(checked_state_index)
        else {
            return CachedRecvMeta::EMPTY;
        };
        let idx = state_index_to_usize(recv_entry);
        let Some(meta) = self.progress.cursor.try_recv_meta_at(idx) else {
            return CachedRecvMeta::EMPTY;
        };
        Self::cached_recv_meta_from_recv(idx, meta)
    }

    #[inline]
    fn cached_recv_meta_from_recv(cursor_index: usize, meta: RecvMeta) -> CachedRecvMeta {
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
            origin: meta.origin,
            lane: meta.lane,
            flags: CachedRecvMeta::FLAG_RECV_STEP | CachedRecvMeta::next_presence_flag(next),
        }
    }

    #[inline]
    fn cached_recv_meta_from_send(cursor_index: usize, meta: SendMeta) -> CachedRecvMeta {
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
            origin: meta.origin,
            lane: meta.lane,
            flags: CachedRecvMeta::next_presence_flag(next),
        }
    }

    #[inline]
    fn cached_recv_meta_from_local(
        &self,
        cursor_index: usize,
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
            peer: self.public_header.role(),
            label: meta.label,
            origin: meta.origin,
            lane: meta.lane,
            flags: CachedRecvMeta::next_presence_flag(next),
        }
    }

    #[inline]
    fn route_arm_cached_recv_meta(
        &self,
        cursor_index: usize,
        label: u8,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(cursor_index) = checked_state_index(cursor_index) else {
            crate::invariant();
        };
        CachedRecvMeta {
            cursor_index,
            eff_index: EffIndex::ZERO,
            peer: self.public_header.role(),
            label,
            origin: EventOrigin::Session,
            lane,
            flags: CachedRecvMeta::FLAG_NEXT_PRESENT,
        }
    }

    #[inline]
    fn route_arm_cached_recv_meta_for_arm(
        &self,
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        lane: u8,
    ) -> CachedRecvMeta {
        let Some(label) = controller_arm_label(&self.progress.cursor, scope_id, route_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        self.route_arm_cached_recv_meta(cursor_index, label, lane)
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn compute_passive_arm_recv_meta(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
        offer_lane: u8,
    ) -> CachedRecvMeta {
        let target_arm = Arm::from_raw(target_arm).as_u8();
        let Some(entry) = self
            .progress
            .cursor
            .passive_observer_arm_entry(scope_id, target_arm)
        else {
            return CachedRecvMeta::EMPTY;
        };
        let entry_idx = state_index_to_usize(entry);
        if let Some(recv_meta) = self.progress.cursor.try_recv_meta_at(entry_idx) {
            return Self::cached_recv_meta_from_recv(entry_idx, recv_meta);
        }
        if let Some(send_meta) = self.progress.cursor.try_send_meta_at(entry_idx) {
            return Self::cached_recv_meta_from_send(entry_idx, send_meta);
        }
        if !self.progress.cursor.has_route_scope(scope_id) {
            return CachedRecvMeta::EMPTY;
        }
        if self.progress.cursor.route_scope_reentry(scope_id) {
            return self
                .route_arm_cached_recv_meta_for_arm(entry_idx, scope_id, target_arm, offer_lane);
        }
        if let Some(target_idx) =
            self.preview_passive_materialization_index_for_selected_arm(scope_id, target_arm)
        {
            if let Some(recv_meta) = self.progress.cursor.try_recv_meta_at(target_idx) {
                return Self::cached_recv_meta_from_recv(target_idx, recv_meta);
            }
            if let Some(send_meta) = self.progress.cursor.try_send_meta_at(target_idx) {
                return Self::cached_recv_meta_from_send(target_idx, send_meta);
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
        let arm = Arm::from_raw(arm);
        let arm_raw = arm.as_u8();
        let scope_id = selection.scope_id;
        let at_route_entry = selection.entry_position.is_route_entry();
        let controller_arm_entry = self
            .progress
            .cursor
            .shared_controller_arm_entry_by_arm(scope_id, arm_raw);
        if is_route_controller
            && at_route_entry
            && let Some((entry, _)) = controller_arm_entry
        {
            return self
                .progress
                .cursor
                .try_recv_meta_at(state_index_to_usize(entry))
                .is_some_and(|recv_meta| recv_meta.peer != self.public_header.role());
        }
        if at_route_entry
            && self
                .progress
                .cursor
                .passive_observer_arm_entry(scope_id, arm_raw)
                .is_some()
        {
            if self.selected_arm_has_first_recv_dispatch(scope_id, arm) {
                return !self
                    .selection_arm_dispatch_materializes_without_ready_evidence(scope_id, arm);
            }
            return false;
        }
        if let Some(passive_recv) = self.selected_passive_arm_recv_ready_meta(scope_id, arm) {
            if passive_recv.peer == self.public_header.role() {
                return false;
            }
            if passive_recv.origin.is_session()
                && controller_arm_entry.map(|(_, label)| label) == Some(passive_recv.label)
            {
                return false;
            }
            return true;
        }
        self.progress
            .cursor
            .route_scope_arm_recv_index(scope_id, arm_raw)
            .is_some()
    }

    #[inline]
    fn selection_arm_dispatch_materializes_without_ready_evidence(
        &self,
        scope_id: ScopeId,
        arm: Arm,
    ) -> bool {
        let arm = arm.as_u8();
        let Some(entry) = self
            .progress
            .cursor
            .passive_observer_arm_entry(scope_id, arm)
        else {
            return false;
        };
        let entry_idx = state_index_to_usize(entry);
        if self.progress.cursor.is_recv_at(entry_idx)
            || self.progress.cursor.is_send_at(entry_idx)
            || self.progress.cursor.is_local_action_at(entry_idx)
        {
            return true;
        }
        self.progress
            .cursor
            .passive_child_scope(scope_id, arm)
            .and_then(|scope| self.preview_live_selected_arm_for_scope(scope))
            .is_some()
    }

    #[inline]
    fn selected_arm_has_first_recv_dispatch(&self, scope_id: ScopeId, arm: Arm) -> bool {
        let Some(mask) = self
            .progress
            .cursor
            .route_scope_first_recv_dispatch_arm_mask(scope_id)
        else {
            return false;
        };
        if mask & !0b11 != 0 {
            crate::invariant();
        }
        (mask & (1u8 << arm.as_u8())) != 0
    }

    #[inline(never)]
    fn selected_passive_arm_recv_ready_meta(
        &self,
        scope_id: ScopeId,
        arm: Arm,
    ) -> Option<RecvMeta> {
        let entry_idx = self
            .progress
            .cursor
            .passive_observer_arm_entry_index(scope_id, arm.as_u8())?;
        self.progress.cursor.try_recv_meta_at(entry_idx)
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn preview_selected_arm_meta(
        &self,
        selection: OfferScopeSelection,
        selected_arm: u8,
    ) -> RecvResult<CachedRecvMeta> {
        let selected_arm = Arm::from_raw(selected_arm).as_u8();
        let scope_id = selection.scope_id;
        if let Some(target_idx) = selection.observed_target_index() {
            let target_arm = self
                .progress
                .cursor
                .route_arm_for_index(scope_id, target_idx)
                .ok_or(RecvError::PhaseInvariant)?;
            if target_arm != selected_arm {
                return Err(RecvError::PhaseInvariant);
            }
            let meta = self
                .progress
                .cursor
                .try_recv_meta_at(target_idx)
                .ok_or(RecvError::PhaseInvariant)?;
            return Ok(Self::cached_recv_meta_from_recv(target_idx, meta));
        }
        let controller_arm_entry = if self.is_route_controller(scope_id) {
            self.progress
                .cursor
                .shared_controller_arm_entry_by_arm(scope_id, selected_arm)
        } else {
            None
        };

        let direct_meta = if let Some((arm_entry_idx, arm_entry_label)) = controller_arm_entry {
            let arm_entry_idx = state_index_to_usize(arm_entry_idx);
            // Every real event retains its exact resident identity, including
            // its wire color. Synthetic arm metadata belongs only to terminal arms.
            match self.progress.cursor.action_at(arm_entry_idx) {
                LocalAction::Send { .. } => Self::cached_recv_meta_from_send(
                    arm_entry_idx,
                    crate::invariant_some(self.progress.cursor.try_send_meta_at(arm_entry_idx)),
                ),
                LocalAction::Recv { .. } => Self::cached_recv_meta_from_recv(
                    arm_entry_idx,
                    crate::invariant_some(self.progress.cursor.try_recv_meta_at(arm_entry_idx)),
                ),
                LocalAction::Local { .. } => self.cached_recv_meta_from_local(
                    arm_entry_idx,
                    crate::invariant_some(self.progress.cursor.try_local_meta_at(arm_entry_idx)),
                ),
                LocalAction::Terminate => self.route_arm_cached_recv_meta(
                    arm_entry_idx,
                    arm_entry_label,
                    selection.offer_lane,
                ),
            }
        } else if selected_arm
            < self
                .progress
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
            self.compute_passive_arm_recv_meta(scope_id, selected_arm, selection.offer_lane)
        };

        Ok(meta)
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn descend_selected_passive_route(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
        observed_frame: Option<InboundFrameKey>,
    ) -> RecvResult<bool> {
        let scope_id = selection.scope_id;
        let selected_arm = resolved.selected_arm;
        // An exact enabled receive already names the operation to preview.
        // Its arm can also contain a nested choice on an independent lane.
        if selection.observed_target_index().is_some() {
            return Ok(false);
        }
        let materialization_meta = self.selection_materialization_meta(selection);
        let Some(nested_scope) =
            materialization_meta.passive_child_scope(Arm::from_raw(selected_arm))
        else {
            return Ok(false);
        };
        if let Some(key) = observed_frame {
            let meta = self.preview_selected_arm_meta(selection, selected_arm)?;
            let exact_target = self
                .progress
                .cursor
                .passive_descendant_target_index_for_key(scope_id, key)
                .map_err(|_| RecvError::PhaseInvariant)?;
            if !meta.is_empty()
                && meta.peer == key.source_role
                && meta.lane == key.lane
                && meta.eff_index.dense_ordinal() == usize::from(key.frame_label)
                && exact_target == Some(state_index_to_usize(meta.cursor_index))
            {
                return Ok(false);
            }
        }
        let nested_scope = self.rebase_passive_descendant_scope(scope_id, nested_scope);
        let (target_index, route_rows) = {
            let crate::endpoint::kernel::core::ActorProgress {
                cursor,
                decision_state,
                route_commit_rows,
                ..
            } = &mut *self.progress;
            let mut route_rows = route_commit_rows.begin();
            let mut route_row_result = Ok(());
            let target_index = cursor
                .visit_passive_route_materialization_rows(
                    scope_id,
                    nested_scope,
                    selected_arm,
                    |target_scope| {
                        preview_selected_arm_for_scope_from_parts(
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
                route_rows.finish_route_only_for_lane(selection.offer_lane)?,
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
        let _ = self.commit_prepared_delta(delta, None);
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
        let arg0 = self.context.sid.raw();
        let event =
            events::route_arm_selection_with_causal(causal, arg0, scope_id, token.arm().as_u8());
        emit(port.tap(), event);
    }

    pub(in crate::endpoint::kernel) fn prepare_route_arm_selection_from_resolver(
        &mut self,
        scope_id: ScopeId,
    ) -> RecvResult<RouteResolveStep> {
        let resolver = self
            .progress
            .cursor
            .route_scope_resolver(scope_id)
            .ok_or(RecvError::PhaseInvariant)?;
        let offer_lane = self.offer_lane_for_scope(scope_id);
        let cluster = self.context.session.cluster();
        let rv_id = self.context.public_rv;
        self.context
            .session
            .rendezvous()
            .seal_session_membership(self.context.sid);
        let resolver_result =
            cluster.resolve_dynamic_resolver(rv_id, self.progress.cursor.program_ref(), resolver);
        if let Some(kind) = self.session_fault() {
            return Err(RecvError::SessionFault(kind));
        }
        let resolution = match resolver_result {
            Ok(resolution) => resolution,
            Err(ClusterError::ResolverReject { resolver_id }) => {
                self.emit_dynamic_resolver_reject_audit(offer_lane, scope_id, resolver_id);
                return Ok(RouteResolveStep::Reject(resolver_id));
            }
            Err(
                ClusterError::RendezvousMismatch { .. }
                | ClusterError::RendezvousUnregistered { .. }
                | ClusterError::RendezvousBusy { .. }
                | ClusterError::SessionProgramMismatch { .. }
                | ClusterError::SessionMembershipSealed { .. }
                | ClusterError::ResourceExhausted { .. }
                | ClusterError::DynamicResolverInvariant { .. },
            ) => return Err(RecvError::PhaseInvariant),
        };
        let arm = Arm::from_raw(resolution.index());
        Ok(RouteResolveStep::Resolved(arm))
    }
}
