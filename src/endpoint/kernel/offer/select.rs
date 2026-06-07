use super::{
    ActiveEntrySet, Arm, Clock, ControlFlow, CurrentFrontierSelectionState,
    CurrentScopeSelectionMeta, CursorEndpoint, DeferReason, DeferSource, EpochTable,
    FrontierDeferOutcome, FrontierObservationDomain, FrontierObservationKey, FrontierVisitSet,
    LabelUniverse, LaneSetView, MintConfigMarker, ObservedEntrySet, OfferEntryObservedState,
    OfferEntryState, OfferEvidenceOutcome, OfferLaneEntrySlotMasks, OfferProgressState,
    OfferScopeSelection, OfferStagedIngress, Poll, Port, RecvError, RecvResult, RouteDecisionToken,
    ScopeFrameLabelMeta, ScopeId, Transport, frontier_observation_key_view_from_storage,
    frontier_offer_lane_entry_slot_masks_view_from_storage, frontier_snapshot_from_scratch,
    lane_port, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(in crate::endpoint::kernel) fn offer_entry_frame_label_meta(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeFrameLabelMeta> {
        let state = endpoint.offer_entry_state_snapshot(entry_idx)?;
        if !endpoint.offer_entry_has_active_lanes(entry_idx)
            || endpoint.offer_entry_scope_id(entry_idx, state) != scope_id
        {
            return None;
        }
        let loop_meta = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::scope_loop_meta_at(
            &endpoint.cursor,
            &endpoint.control_semantics(),
            scope_id,
            entry_idx,
        );
        Some(
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::scope_frame_label_meta_at(
                &endpoint.cursor,
                &endpoint.control_semantics(),
                scope_id,
                loop_meta,
                entry_idx,
            ),
        )
    }

    #[inline]
    pub(super) fn offer_refresh_mask(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        lane_idx: usize,
    ) -> bool {
        endpoint.cursor.lane_has_pending_step(lane_idx)
            || endpoint
                .decision_state
                .lane_linger_lanes()
                .contains(lane_idx)
            || endpoint
                .decision_state
                .lane_offer_linger_lanes()
                .contains(lane_idx)
    }

    #[inline]
    pub(super) fn for_each_set_lane(
        lane_set: LaneSetView,
        lane_limit: usize,
        mut f: impl FnMut(usize),
    ) {
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            f(lane_idx);
            next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_active_entries(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        domain: FrontierObservationDomain,
    ) -> ActiveEntrySet {
        if domain.uses_root_entries() {
            endpoint.root_frontier_active_entries(domain.root_scope())
        } else {
            endpoint.global_active_entries()
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_offer_lane_entry_slot_masks(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        domain: FrontierObservationDomain,
    ) -> OfferLaneEntrySlotMasks {
        let active_entries = Self::frontier_observation_active_entries(endpoint, domain);
        let port = endpoint.port_for_lane(endpoint.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = endpoint.cursor.frontier_scratch_layout();
        let mut slot_masks =
            frontier_offer_lane_entry_slot_masks_view_from_storage(scratch_ptr, layout);
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::next_slot_in_mask(
                &mut remaining_slots,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(_state) = endpoint.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !endpoint.offer_entry_has_active_lanes(entry_idx) {
                continue;
            }
            let logical_lane_count = endpoint.cursor.logical_lane_count();
            let active_offer_lanes = endpoint.decision_state.active_offer_lanes();
            Self::for_each_set_lane(active_offer_lanes, logical_lane_count, |lane_idx| {
                if state_index_to_usize(endpoint.decision_state.lane_offer_state(lane_idx).entry)
                    == entry_idx
                {
                    slot_masks.set_logical_mask(lane_idx, slot_masks[lane_idx] | (1u8 << slot_idx));
                }
            });
        }
        slot_masks
    }

    pub(in crate::endpoint::kernel) fn frontier_observation_key(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        domain: FrontierObservationDomain,
    ) -> FrontierObservationKey {
        let active_entries = Self::frontier_observation_active_entries(endpoint, domain);
        let port = endpoint.port_for_lane(endpoint.primary_lane);
        let scratch_ptr = lane_port::frontier_scratch_ptr(port);
        let layout = endpoint.cursor.frontier_scratch_layout();
        let mut key = frontier_observation_key_view_from_storage(
            scratch_ptr,
            layout,
            endpoint.cursor.max_frontier_entries(),
        );
        key.clear();
        key.set_active_entries_from(active_entries);
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let scope_id = endpoint
                .offer_entry_representative_lane_from_route_state(entry_idx)
                .map(|pair| pair.1.scope)
                .or_else(|| {
                    endpoint
                        .frontier_state
                        .offer_entry_state
                        .get(entry_idx)
                        .copied()
                        .map(|state| state.scope_id)
                })
                .unwrap_or(ScopeId::none());
            let summary = endpoint.compute_offer_entry_static_summary(entry_idx);
            let slot = key.slot_mut(slot_idx);
            slot.entry_summary_fingerprint = summary.observation_fingerprint();
            slot.scope_generation = endpoint.scope_evidence_generation_for_scope(scope_id);
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::next_slot_in_mask(
                &mut remaining_entries,
            )
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(pair) = endpoint.offer_entry_representative_lane_from_route_state(entry_idx)
            else {
                continue;
            };
            key.slot_mut(slot_idx).route_change_epoch = endpoint
                .ports
                .get(pair.0)
                .and_then(Option::as_ref)
                .map(Port::route_change_epoch)
                .unwrap_or(0);
        }
        let logical_lane_count = endpoint.cursor.logical_lane_count();
        let active_offer_lanes = endpoint.decision_state.active_offer_lanes();
        Self::for_each_set_lane(active_offer_lanes, logical_lane_count, |lane_idx| {
            let info = endpoint.decision_state.lane_offer_state(lane_idx);
            if !info.entry.is_max()
                && active_entries
                    .slot_for_entry(state_index_to_usize(info.entry))
                    .is_some()
            {
                key.insert_offer_lane(lane_idx);
            }
        });
        key
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn ensure_global_frontier_scratch_initialized(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
    ) {
        endpoint.init_global_frontier_scratch_if_needed();
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_cache(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        domain: FrontierObservationDomain,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        endpoint.frontier_observation_cache_snapshot(domain)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn store_frontier_observation(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        domain: FrontierObservationDomain,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        endpoint.write_frontier_observation_snapshot(domain, key, observed_entries);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn cached_offer_entry_observed_state_for_rebuild(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        entry_idx: usize,
        entry_state: &OfferEntryState,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<OfferEntryObservedState> {
        endpoint.reusable_cached_offer_entry_observed_state(
            entry_idx,
            entry_state,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        domain: FrontierObservationDomain,
    ) {
        endpoint.refresh_frontier_observation_cache_impl(domain)
    }

    pub(in crate::endpoint::kernel) fn select_scope(&mut self) -> RecvResult<OfferScopeSelection> {
        self.align_cursor_to_selected_scope()?;
        let node_scope = self.current_offer_scope_id();
        let Some(scope_id) = self.cursor.route_scope_for_offer_node(node_scope) else {
            return Err(RecvError::PhaseInvariant);
        };
        if !self.cursor.route_offer_entry_allows_current(
            scope_id,
            self.cursor.index(),
            self.selected_arm_for_scope(scope_id),
        ) {
            return Err(RecvError::PhaseInvariant);
        }
        let current_idx = self.cursor.index();
        let cached_entry_state = self
            .offer_entry_state_snapshot(current_idx)
            .filter(|state| {
                self.offer_entry_has_active_lanes(current_idx)
                    && self.offer_entry_scope_id(current_idx, *state) == scope_id
            });
        // Route hints are offer-scoped; preview only inspects them here.
        let offer_lane = if let Some(entry_state) = cached_entry_state {
            self.offer_entry_representative_lane_idx(current_idx, entry_state)
                .map(|lane_idx| lane_idx as u8)
        } else {
            self.offer_lane_set_for_scope(scope_id)
                .first_set(self.cursor.logical_lane_count())
                .map(|lane_idx| lane_idx as u8)
        };
        let Some(offer_lane) = offer_lane else {
            return Err(RecvError::PhaseInvariant);
        };
        let at_route_offer_entry = self
            .cursor
            .route_offer_entry_matches_current(scope_id, current_idx)
            .unwrap_or(true);
        let frontier_parallel_root =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::parallel_scope_root(
                &self.cursor,
                scope_id,
            );
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root,
            offer_lane,
            at_route_offer_entry,
        })
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_scope_ack(
        &mut self,
        scope_id: ScopeId,
        token: RouteDecisionToken,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self.decision_state.scope_evidence.record_ack(slot, token)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, true);
    }

    #[inline]
    pub(super) fn mark_scope_materialization_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, false);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        let exact_passive_arm =
            self.passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        let arm = exact_passive_arm.or_else(|| {
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::scope_evidence_frame_label_to_arm(
                frame_label_meta,
                frame_label,
            )
        });
        if let Some(arm) = arm {
            if self.loop_control_evidence_only(frame_label_meta, arm) {
                return;
            }
            if self.static_passive_scope_evidence_materializes_poll(scope_id) {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, lane, frame_label);
            }
        }
    }

    #[inline]
    pub(super) fn mark_static_passive_descendant_path_ready(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) {
        let mut current_scope = scope_id;
        let mut depth = 0usize;
        let depth_bound = self.typestate_walk_bound();
        while depth < depth_bound {
            let Some(arm) = self.static_passive_descendant_dispatch_arm_from_exact_frame_label(
                current_scope,
                lane,
                frame_label,
            ) else {
                break;
            };
            self.mark_scope_ready_arm(current_scope, arm);
            let Some(child_scope) = self
                .cursor
                .passive_descendant_child_scope(current_scope, arm)
            else {
                break;
            };
            if child_scope == current_scope {
                break;
            }
            current_scope = child_scope;
            depth += 1;
        }
    }

    pub(super) fn on_frontier_defer(
        &mut self,
        progress: &mut OfferProgressState,
        scope_id: ScopeId,
        current_parallel: Option<ScopeId>,
        source: DeferSource,
        reason: DeferReason,
        offer_lane: u8,
        ingress_ready: bool,
        selected_arm: Option<u8>,
        visited: &mut FrontierVisitSet,
    ) -> FrontierDeferOutcome {
        let fingerprint = self.evidence_fingerprint(scope_id, ingress_ready);
        let evidence = progress.on_defer(fingerprint);
        let pending = matches!(evidence, OfferEvidenceOutcome::Pending);
        let is_controller = self.cursor.is_route_controller(scope_id);
        let frontier = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::frontier_kind_for_cursor(
            &self.cursor,
            scope_id,
            is_controller,
        );
        let hint = self.peek_scope_frame_hint(scope_id);
        let ready_arm_mask = self.scope_ready_arm_mask(scope_id);
        self.emit_policy_defer_event(
            source,
            reason,
            scope_id,
            frontier,
            selected_arm,
            hint,
            ready_arm_mask,
            ingress_ready,
            pending,
            offer_lane,
        );
        visited.record(scope_id);
        let current_entry_idx = self.cursor.index();
        let current_is_controller = self.cursor.is_route_controller(scope_id);
        let mut scratch = self.frontier_scratch_view();
        let mut snapshot = frontier_snapshot_from_scratch(
            &mut scratch,
            scope_id,
            current_entry_idx,
            current_parallel.unwrap_or(ScopeId::none()),
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::frontier_kind_for_cursor(
                &self.cursor,
                scope_id,
                current_is_controller,
            ),
        );
        self.for_each_active_offer_candidate(current_parallel, |candidate| {
            let _ = snapshot.push_candidate(candidate);
            ControlFlow::<()>::Continue(())
        });
        if pending {
            let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
                return FrontierDeferOutcome::Pending;
            };
            visited.record(candidate.scope_id);
            if candidate.entry_idx as usize != self.cursor.index() {
                if self
                    .commit_cursor_realign_index(candidate.entry_idx as usize)
                    .is_err()
                {
                    return FrontierDeferOutcome::Pending;
                }
            }
            return FrontierDeferOutcome::Yielded;
        }
        let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
            return FrontierDeferOutcome::Continue;
        };
        visited.record(candidate.scope_id);
        if candidate.entry_idx as usize != self.cursor.index() {
            if self
                .commit_cursor_realign_index(candidate.entry_idx as usize)
                .is_err()
            {
                return FrontierDeferOutcome::Continue;
            }
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
        let at_route_entry = self
            .cursor
            .route_offer_entry_matches_current(scope_id, current_idx)?;
        if !at_route_entry {
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

    #[inline]
    pub(super) fn entry_has_route_scope(&self, entry_idx: usize) -> bool {
        let entry_scope = if let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx)
            && self.offer_entry_has_active_lanes(entry_idx)
        {
            let scope_id = self.offer_entry_scope_id(entry_idx, entry_state);
            (!scope_id.is_none()).then_some(scope_id)
        } else {
            None
        };
        self.cursor
            .route_scope_present_for_entry(entry_idx, entry_scope)
    }

    pub(super) fn current_frontier_selection_state(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
    ) -> CurrentFrontierSelectionState {
        if let Some(info) = self.offer_entry_lane_state(scope_id, current_idx) {
            let entry_state = self
                .offer_entry_state_snapshot(current_idx)
                .unwrap_or_else(|| unreachable!("active offer entry must have a runtime snapshot"));
            let summary = self.compute_offer_entry_static_summary(current_idx);
            let entry_parallel =
                self.offer_entry_parallel_root_from_state(current_idx, entry_state);
            let parallel_root = info.parallel_root;
            let current_parallel =
                if !parallel_root.is_none() && self.root_frontier_active_mask(parallel_root) != 0 {
                    Some(parallel_root)
                } else {
                    entry_parallel
                };
            let mut flags = 0u8;
            if summary.is_controller() {
                flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
            }
            if summary.is_dynamic() {
                flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
            }
            return CurrentFrontierSelectionState {
                frontier: self.offer_entry_frontier(current_idx, entry_state),
                parallel_root: current_parallel.unwrap_or(ScopeId::none()),
                ready: summary.static_ready(),
                has_progress_evidence: false,
                flags,
            };
        }
        let current_is_controller = self.cursor.is_route_controller(scope_id);
        let current_is_dynamic = current_is_controller
            && self
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _, _)| policy.is_dynamic())
                .unwrap_or(false);
        let static_facts =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::frontier_static_facts_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                current_is_controller,
                current_is_dynamic,
                current_idx,
            );
        let cursor_parallel = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::parallel_scope_root(
            &self.cursor,
            scope_id,
        );
        let cursor_parallel_has_offer = cursor_parallel
            .map(|root| self.root_frontier_active_mask(root) != 0)
            .unwrap_or(false);
        let current_entry_has_offer = self.offer_entry_has_active_lanes(current_idx);
        let current_entry_parallel = if cursor_parallel_has_offer || !current_entry_has_offer {
            None
        } else {
            self.offer_entry_state_snapshot(current_idx)
                .and_then(|entry_state| {
                    self.offer_entry_parallel_root_from_state(current_idx, entry_state)
                })
        };
        let current_parallel = if cursor_parallel_has_offer {
            cursor_parallel
        } else {
            current_entry_parallel
        };
        let mut flags = 0u8;
        if current_is_controller {
            flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
        }
        if current_is_dynamic {
            flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
        }
        CurrentFrontierSelectionState {
            frontier: static_facts.frontier,
            parallel_root: current_parallel.unwrap_or(ScopeId::none()),
            ready: static_facts.ready,
            has_progress_evidence: false,
            flags,
        }
    }

    pub(super) fn await_static_passive_progress(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        selection: OfferScopeSelection,
        selected_arm: Option<u8>,
        ingress: &mut OfferStagedIngress<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        let materialization_meta = self.selection_materialization_meta(selection);
        let progress_lane = selected_arm
            .and_then(|arm| {
                self.route_scope_arm_lane_set_for_scope(selection.scope_id, arm)
                    .and_then(|lanes| lanes.first_set(self.cursor.logical_lane_count()))
            })
            .map(|lane_idx| lane_idx as u8)
            .unwrap_or(selection.offer_lane);
        if let Some(arm) = selected_arm
            && selection.at_route_offer_entry
            && let Some(entry) = materialization_meta.passive_arm_entry(arm)
        {
            if !self.cursor.is_recv_at(state_index_to_usize(entry)) {
                return Poll::Ready(Ok(()));
            }
        }
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
    pub(super) fn try_poll_route_decision_immediate(
        &self,
        scope_id: ScopeId,
        offer_lanes: LaneSetView,
        cx: &mut core::task::Context<'_>,
    ) -> Option<Arm> {
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        let mut arm = None;
        while let Some(lane_idx) = next {
            let lane = lane_idx as u8;
            let port = self.port_for_lane(lane as usize);
            if let Poll::Ready(route_arm) = port.poll_route_decision(scope_id, ROLE, cx) {
                arm = Some(route_arm);
                break;
            }
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        let arm = arm?;
        Arm::new(arm)
    }
    pub(super) fn try_poll_route_decision_for_offer(
        &self,
        scope_id: ScopeId,
        offer_lanes: LaneSetView,
        cx: &mut core::task::Context<'_>,
    ) -> Option<Arm> {
        if let Some(arm) = self.try_poll_route_decision_immediate(scope_id, offer_lanes, cx) {
            return Some(arm);
        }
        let is_dynamic_route_scope = self
            .cursor
            .route_scope_controller_policy(scope_id)
            .map(|(policy, _, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        if is_dynamic_route_scope {
            return None;
        }
        self.poll_arm_from_ready_mask(scope_id)
    }
}
