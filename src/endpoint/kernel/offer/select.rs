use super::super::evidence_store::ReadyArmEvidence;
use super::{
    Arm, ControlFlow, CurrentFrontierSelectionState, CurrentScopeSelectionMeta, CursorEndpoint,
    FrontierDeferOutcome, FrontierVisitSet, IngressEvidenceState, OfferEntryKey,
    OfferEvidenceOutcome, OfferProgressState, OfferScopeSelection, OfferStagedIngress, Poll,
    RecvError, RecvResult, RouteArmToken, ScopeId, Transport, frontier_snapshot_from_scratch,
    lane_port, state_index_to_usize,
};
use crate::global::typestate::InboundFrameKey;

pub(super) struct FrontierDeferRequest {
    pub(super) scope_id: ScopeId,
    pub(super) current_parallel: Option<ScopeId>,
    pub(super) ingress: IngressEvidenceState,
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(super) fn offer_refresh_mask(
        endpoint: &CursorEndpoint<'r, ROLE, T>,
        lane_idx: usize,
    ) -> bool {
        endpoint.cursor.lane_has_pending_step(lane_idx)
            || endpoint
                .decision_state
                .lane_reentry_lanes()
                .contains(lane_idx)
            || endpoint
                .decision_state
                .lane_offer_reentry_lanes()
                .contains(lane_idx)
    }

    pub(in crate::endpoint::kernel) fn select_scope(
        &mut self,
        carried_lane: Option<u8>,
        carried_key: Option<InboundFrameKey>,
        carried_observation: Option<lane_port::FrameObservation>,
    ) -> RecvResult<OfferScopeSelection> {
        if let Some(selection) = self.select_current_materialized_ingress_scope(carried_key)? {
            return Ok(selection);
        }
        if let Some(selection) =
            self.select_observed_ingress_route_scope(carried_key, carried_observation)?
        {
            return Ok(selection);
        }
        if let Some(observed) = carried_observation {
            let lane = carried_lane.ok_or(RecvError::PhaseInvariant)?;
            self.emit_materialization_mismatch_observation(
                usize::from(lane),
                lane,
                lane_port::FrameMismatch::label_mismatch(observed),
            );
            return Err(RecvError::PhaseInvariant);
        }
        if let Some(selection) = self.select_carried_ingress_scope(carried_lane)? {
            return Ok(selection);
        }
        let node_scope = self.align_cursor_to_selected_scope()?;
        let current_idx = self.cursor.index();
        let Some(scope_id) = self
            .cursor
            .route_scope_for_offer_node(node_scope, current_idx)
        else {
            return Err(RecvError::PhaseInvariant);
        };
        if !self.cursor.route_offer_entry_allows_current(
            scope_id,
            self.cursor.index(),
            self.preview_live_selected_arm_for_scope(scope_id),
        ) {
            return Err(RecvError::PhaseInvariant);
        }
        let current_key = crate::invariant_some(OfferEntryKey::from_index(scope_id, current_idx));
        let current_active_entry = self.active_offer_entry(current_key);
        let current_entry_active = current_active_entry.is_some();
        // Offer-lane choice remains local to the selected route scope.
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        let lane_limit = self.cursor.logical_lane_count();
        let carried_offer_lane = carried_lane
            .map(usize::from)
            .filter(|&lane_idx| lane_idx < lane_limit && offer_lanes.contains(lane_idx))
            .map(|lane_idx| lane_idx as u8);
        let offer_lane = if let Some(lane) = carried_offer_lane {
            Some(lane)
        } else if current_entry_active {
            Some(crate::invariant_some(current_active_entry).representative_lane())
        } else {
            offer_lanes
                .first_set(lane_limit)
                .map(|lane_idx| lane_idx as u8)
        };
        let Some(offer_lane) = offer_lane else {
            return Err(RecvError::PhaseInvariant);
        };
        self.offer_scope_selection_for_scope_lane(scope_id, current_idx, offer_lane)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_scope_ack(
        &mut self,
        scope_id: ScopeId,
        token: RouteArmToken,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            self.decision_state.scope_evidence.record_ack(slot, token);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(
        &mut self,
        scope_id: ScopeId,
        arm: Arm,
    ) {
        self.mark_scope_ready_arm_inner(scope_id, arm, ReadyArmEvidence::Poll);
    }

    #[inline]
    pub(super) fn mark_scope_materialization_ready_arm(&mut self, scope_id: ScopeId, arm: Arm) {
        self.mark_scope_ready_arm_inner(scope_id, arm, ReadyArmEvidence::Materialization);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_frame_key(
        &mut self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> bool {
        let Some(arm) = self
            .cursor
            .passive_descendant_dispatch_arm_for_key(scope_id, key)
            .map(Arm::from_raw)
        else {
            return false;
        };
        self.mark_scope_ready_arm_from_exact_passive_arm(scope_id, arm);
        self.mark_intrinsic_passive_descendant_path_ready(scope_id, key);
        true
    }

    #[inline]
    pub(super) fn mark_scope_ready_arm_from_exact_passive_arm(
        &mut self,
        scope_id: ScopeId,
        arm: Arm,
    ) {
        if self.intrinsic_passive_scope_evidence_materializes_poll(scope_id) {
            self.mark_scope_ready_arm(scope_id, arm);
        } else {
            self.mark_scope_materialization_ready_arm(scope_id, arm);
        }
    }

    #[inline]
    pub(super) fn mark_intrinsic_passive_descendant_path_ready(
        &mut self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) {
        let mut current_scope = scope_id;
        let mut depth = 0usize;
        let depth_bound = self.cursor.route_chain_bound();
        while depth < depth_bound {
            let Some(arm) = self
                .cursor
                .passive_descendant_dispatch_arm_for_key(current_scope, key)
            else {
                break;
            };
            let arm = Arm::from_raw(arm);
            self.mark_scope_ready_arm(current_scope, arm);
            let Some(child_scope) = self.cursor.passive_child_scope(current_scope, arm.as_u8())
            else {
                break;
            };
            current_scope = child_scope;
            depth += 1;
        }
    }

    pub(super) fn on_frontier_defer(
        &mut self,
        progress: &mut OfferProgressState,
        request: FrontierDeferRequest,
        visited: &mut FrontierVisitSet,
    ) -> FrontierDeferOutcome {
        let FrontierDeferRequest {
            scope_id,
            current_parallel,
            ingress,
        } = request;
        let fingerprint = self.evidence_fingerprint(scope_id, ingress);
        let evidence = progress.on_defer(fingerprint);
        let is_pending = matches!(evidence, OfferEvidenceOutcome::Pending);
        let current_entry_idx = self.cursor.index();
        visited.record(current_entry_idx);
        let current_is_controller = self.cursor.is_route_controller(scope_id);
        let mut scratch = self.frontier_scratch_view();
        let mut snapshot = frontier_snapshot_from_scratch(
            &mut scratch,
            scope_id,
            current_entry_idx,
            match current_parallel {
                Some(root) => root,
                None => ScopeId::none(),
            },
            CursorEndpoint::<ROLE, T>::frontier_kind_for_cursor(
                &self.cursor,
                scope_id,
                current_is_controller,
            ),
        );
        self.for_each_active_offer_candidate(current_parallel, |candidate| {
            if !snapshot.push_candidate(candidate) {
                crate::invariant();
            }
            ControlFlow::<()>::Continue(())
        });
        if is_pending {
            let Some(candidate) = snapshot.select_yield_candidate(visited) else {
                return FrontierDeferOutcome::Pending;
            };
            let candidate_entry = candidate.entry.as_usize();
            visited.record(candidate_entry);
            if candidate_entry != self.cursor.index()
                && self.commit_cursor_realign_index(candidate_entry).is_err()
            {
                return FrontierDeferOutcome::Pending;
            }
            return FrontierDeferOutcome::Yielded;
        }
        let Some(candidate) = snapshot.select_yield_candidate(visited) else {
            return FrontierDeferOutcome::Continue;
        };
        let candidate_entry = candidate.entry.as_usize();
        visited.record(candidate_entry);
        if candidate_entry != self.cursor.index()
            && self.commit_cursor_realign_index(candidate_entry).is_err()
        {
            return FrontierDeferOutcome::Continue;
        }
        FrontierDeferOutcome::Yielded
    }
    pub(super) fn current_scope_selection_meta(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        current_frontier: CurrentFrontierSelectionState,
    ) -> Option<CurrentScopeSelectionMeta> {
        if let Some(meta) = self.offer_entry_selection_meta(scope_id, current_idx) {
            return Some(meta);
        }
        if !self.cursor.has_route_scope(scope_id) {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let at_route_entry = self.cursor.route_offer_entry_cursor_position(
            scope_id,
            current_idx,
            self.preview_live_selected_arm_for_scope(scope_id),
        )?;
        if !at_route_entry.is_at_entry() {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let mut flags = CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY;
        if !self.offer_lane_set_for_scope(scope_id).is_empty() {
            flags |= CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES;
        }
        if current_frontier.is_controller() {
            flags |= CurrentScopeSelectionMeta::FLAG_CONTROLLER;
        }
        Some(CurrentScopeSelectionMeta { flags })
    }

    pub(super) fn current_frontier_selection_state(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
    ) -> CurrentFrontierSelectionState {
        let active_entry = OfferEntryKey::from_index(scope_id, current_idx)
            .and_then(|key| self.active_offer_entry(key));
        if let Some(active) = active_entry {
            let info = active.representative();
            let parallel_root = active.parallel_root();
            let mut flags = 0u8;
            if info.is_controller() {
                flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
            }
            if info.is_dynamic() {
                flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
            }
            if info.intrinsic_ready() {
                flags |= CurrentFrontierSelectionState::FLAG_READY;
            }
            return CurrentFrontierSelectionState {
                frontier: active.frontier(),
                parallel_root,
                flags,
            };
        }
        let current_is_controller = self.cursor.is_route_controller(scope_id);
        let current_is_dynamic =
            current_is_controller && self.cursor.route_scope_resolver(scope_id).is_some();
        let frontier_facts = CursorEndpoint::<ROLE, T>::frontier_facts_at(
            &self.cursor,
            scope_id,
            current_is_controller,
            current_is_dynamic,
            current_idx,
        );
        let current_parallel =
            CursorEndpoint::<ROLE, T>::parallel_scope_root(&self.cursor, scope_id)
                .filter(|&root| self.root_frontier_has_active_entries(root));
        let mut flags = 0u8;
        if current_is_controller {
            flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
        }
        if current_is_dynamic {
            flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
        }
        if frontier_facts.ready() {
            flags |= CurrentFrontierSelectionState::FLAG_READY;
        }
        CurrentFrontierSelectionState {
            frontier: frontier_facts.frontier,
            parallel_root: match current_parallel {
                Some(root) => root,
                None => ScopeId::none(),
            },
            flags,
        }
    }

    pub(super) fn await_intrinsic_passive_progress(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        selection: OfferScopeSelection,
        selected_arm: Option<u8>,
        ingress: &mut OfferStagedIngress<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        let materialization_meta = self.selection_materialization_meta(selection);
        let selected_arm = selected_arm.map(Arm::from_raw);
        let progress_lane = match selected_arm {
            Some(arm) => self
                .route_scope_arm_lane_set_for_scope(selection.scope_id, arm.as_u8())
                .and_then(|lanes| lanes.first_set(self.cursor.logical_lane_count()))
                .map(|lane_idx| lane_idx as u8),
            None => Some(selection.offer_lane),
        };
        if let Some(arm) = selected_arm
            && selection.entry_position.is_route_entry()
            && let Some(entry) = materialization_meta.passive_arm_entry(arm)
            && !self.cursor.is_recv_at(state_index_to_usize(entry))
        {
            return Poll::Ready(Ok(()));
        }
        let Some(progress_lane) = progress_lane else {
            return Poll::Ready(Ok(()));
        };
        if !ingress.has_transport() {
            return self.await_transport_payload_for_offer_lane(
                pending_recv,
                progress_lane,
                ingress,
                cx,
            );
        }
        Poll::Ready(Ok(()))
    }
}
