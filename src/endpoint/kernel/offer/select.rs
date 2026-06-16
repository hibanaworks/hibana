use super::super::evidence_store::ReadyArmEvidence;
use super::{
    ActiveEntrySet, Arm, ControlFlow, CurrentFrontierSelectionState, CurrentScopeSelectionMeta,
    CursorEndpoint, DeferReason, FrontierDeferOutcome, FrontierObservationDomain,
    FrontierObservationKey, FrontierVisitSet, IngressEvidenceState, LaneSetView, ObservedEntrySet,
    OfferEntryObservedState, OfferEntryPosition, OfferEvidenceOutcome, OfferLaneEntrySlotMasks,
    OfferProgressState, OfferScopeSelection, OfferStagedIngress, Poll, Port, RecvError, RecvResult,
    ResolverDeferProgress, RouteArmToken, ScopeFrameLabelScratch, ScopeFrameLabelView, ScopeId,
    Transport, frontier_observation_key_view_from_storage,
    frontier_offer_lane_entry_slot_masks_view_from_storage, frontier_snapshot_from_scratch,
    lane_port, state_index_to_usize,
};
use crate::global::typestate::PackedEventConflict;

pub(super) struct FrontierDeferRequest {
    pub(super) scope_id: ScopeId,
    pub(super) current_parallel: Option<ScopeId>,
    pub(super) reason: DeferReason,
    pub(super) offer_lane: u8,
    pub(super) ingress: IngressEvidenceState,
    pub(super) selected_arm: Option<u8>,
}

impl<'r, const ROLE: u8, T, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn write_offer_entry_frame_label_meta(
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
        scope_id: ScopeId,
        entry_idx: usize,
        out: &mut ScopeFrameLabelScratch,
    ) -> bool {
        if endpoint.offer_entry_state_snapshot(entry_idx).is_none() {
            return false;
        }
        if !endpoint.offer_entry_has_active_lanes(entry_idx)
            || endpoint.offer_entry_scope_id(entry_idx) != scope_id
        {
            return false;
        }
        let reentry_meta = CursorEndpoint::<ROLE, T, MAX_RV>::scope_reentry_meta_at(
            &endpoint.cursor,
            scope_id,
            entry_idx,
        );
        CursorEndpoint::<ROLE, T, MAX_RV>::write_scope_frame_label_meta_at(
            &endpoint.cursor,
            scope_id,
            reentry_meta,
            entry_idx,
            out,
        );
        true
    }

    #[inline]
    pub(super) fn offer_refresh_mask(
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
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

    #[inline]
    pub(super) fn for_each_set_lane(
        lane_set: LaneSetView,
        lane_limit: usize,
        mut f: impl FnMut(usize),
    ) {
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            f(lane_idx);
            next = lane_set.next_set_from(lane_idx + 1, lane_limit);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_active_entries(
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
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
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
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
            CursorEndpoint::<ROLE, T, MAX_RV>::next_slot_in_mask(&mut remaining_slots)
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            if endpoint.offer_entry_state_snapshot(entry_idx).is_none() {
                continue;
            }
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
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
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
            CursorEndpoint::<ROLE, T, MAX_RV>::next_slot_in_mask(&mut remaining_entries)
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let scope_id = match endpoint
                .offer_entry_representative_lane_from_route_state(entry_idx)
                .map(|pair| pair.1.scope)
                .or_else(|| {
                    endpoint
                        .frontier_state
                        .offer_entry_state
                        .get(entry_idx)
                        .copied()
                        .map(|state| state.scope_id)
                }) {
                Some(scope_id) => scope_id,
                None => ScopeId::none(),
            };
            let summary = endpoint.compute_offer_entry_summary(entry_idx);
            let slot = key.slot_mut(slot_idx);
            slot.entry_summary_fingerprint = summary.observation_fingerprint();
            slot.scope_generation = endpoint.scope_evidence_generation_for_scope(scope_id);
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, MAX_RV>::next_slot_in_mask(&mut remaining_entries)
        {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(pair) = endpoint.offer_entry_representative_lane_from_route_state(entry_idx)
            else {
                continue;
            };
            let Some(route_change_generation) = endpoint
                .ports
                .get(pair.0)
                .and_then(Option::as_ref)
                .map(Port::route_change_generation)
            else {
                crate::invariant();
            };
            key.slot_mut(slot_idx).route_change_generation = route_change_generation;
        }
        let logical_lane_count = endpoint.cursor.logical_lane_count();
        let active_offer_lanes = endpoint.decision_state.active_offer_lanes();
        Self::for_each_set_lane(active_offer_lanes, logical_lane_count, |lane_idx| {
            let info = endpoint.decision_state.lane_offer_state(lane_idx);
            if !info.entry.is_absent()
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
    pub(in crate::endpoint::kernel) fn ensure_global_frontier_scratch_ready(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, MAX_RV>,
    ) {
        endpoint.init_global_frontier_scratch_if_needed();
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn frontier_observation_cache(
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
        domain: FrontierObservationDomain,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        endpoint.frontier_observation_cache_snapshot(domain)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn store_frontier_observation(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, MAX_RV>,
        domain: FrontierObservationDomain,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        endpoint.write_frontier_observation_snapshot(domain, key, observed_entries);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn cached_offer_entry_observed_state_for_rebuild(
        endpoint: &CursorEndpoint<'r, ROLE, T, MAX_RV>,
        entry_idx: usize,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<OfferEntryObservedState> {
        endpoint.reusable_cached_offer_entry_observed_state(
            entry_idx,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache(
        endpoint: &mut CursorEndpoint<'r, ROLE, T, MAX_RV>,
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
        let cached_entry_state = self.offer_entry_state_snapshot(current_idx).filter(|_| {
            self.offer_entry_has_active_lanes(current_idx)
                && self.offer_entry_scope_id(current_idx) == scope_id
        });
        // Route hints are offer-scoped; preview only inspects them here.
        let offer_lane = if cached_entry_state.is_some() {
            self.offer_entry_representative_lane_idx(current_idx)
                .map(|lane_idx| lane_idx as u8)
        } else {
            self.offer_lane_set_for_scope(scope_id)
                .first_set(self.cursor.logical_lane_count())
                .map(|lane_idx| lane_idx as u8)
        };
        let Some(offer_lane) = offer_lane else {
            return Err(RecvError::PhaseInvariant);
        };
        let route_offer_entry_matches = crate::invariant_some(
            self.cursor
                .route_offer_entry_cursor_position(scope_id, current_idx),
        )
        .is_at_entry();
        let entry_position = if route_offer_entry_matches {
            OfferEntryPosition::RouteEntry
        } else {
            OfferEntryPosition::AfterRouteEntry
        };
        let frontier_parallel_root =
            CursorEndpoint::<ROLE, T, MAX_RV>::parallel_scope_root(&self.cursor, scope_id);
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root,
            offer_lane,
            entry_position,
        })
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn record_scope_ack(
        &mut self,
        scope_id: ScopeId,
        token: RouteArmToken,
    ) {
        if let Some(slot) = self.scope_slot_for_route(scope_id)
            && self.decision_state.scope_evidence.record_ack(slot, token)
        {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, ReadyArmEvidence::Poll);
    }

    #[inline]
    pub(super) fn mark_scope_materialization_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, ReadyArmEvidence::Materialization);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: &ScopeFrameLabelView<'_>,
    ) {
        let exact_passive_arm = self
            .cursor
            .passive_descendant_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label);
        let arm = exact_passive_arm
            .or_else(|| frame_label_meta.evidence_arm_for_frame_label(frame_label));
        if let Some(arm) = arm {
            if self.intrinsic_passive_scope_evidence_materializes_poll(scope_id) {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_passive_arm.is_some() {
                self.mark_intrinsic_passive_descendant_path_ready(scope_id, lane, frame_label);
            }
        }
    }

    #[inline]
    pub(super) fn mark_intrinsic_passive_descendant_path_ready(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) {
        let mut current_scope = scope_id;
        let mut depth = 0usize;
        let depth_bound = PackedEventConflict::MAX_CHAIN_DEPTH;
        while depth < depth_bound {
            let Some(arm) = self
                .cursor
                .passive_descendant_dispatch_arm_from_exact_frame_label(
                    current_scope,
                    lane,
                    frame_label,
                )
            else {
                break;
            };
            self.mark_scope_ready_arm(current_scope, arm);
            let Some(child_scope) = self.cursor.passive_child_scope(current_scope, arm) else {
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
            reason,
            offer_lane,
            ingress,
            selected_arm,
        } = request;
        let fingerprint = self.evidence_fingerprint(scope_id, ingress);
        let evidence = progress.on_defer(fingerprint);
        let defer_progress = if matches!(evidence, OfferEvidenceOutcome::Pending) {
            ResolverDeferProgress::Pending
        } else {
            ResolverDeferProgress::Settled
        };
        let is_controller = self.cursor.is_route_controller(scope_id);
        let frontier = CursorEndpoint::<ROLE, T, MAX_RV>::frontier_kind_for_cursor(
            &self.cursor,
            scope_id,
            is_controller,
        );
        let hint = self.peek_scope_frame_hint(scope_id);
        self.emit_resolver_defer_event(super::super::core::ResolverDeferAudit {
            reason,
            scope_id,
            frontier,
            selected_arm,
            hint,
            ingress,
            progress: defer_progress,
            lane: offer_lane,
        });
        visited.record(scope_id);
        let current_entry_idx = self.cursor.index();
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
            CursorEndpoint::<ROLE, T, MAX_RV>::frontier_kind_for_cursor(
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
        if defer_progress.is_pending() {
            let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
                return FrontierDeferOutcome::Pending;
            };
            visited.record(candidate.scope_id);
            if candidate.entry_idx as usize != self.cursor.index()
                && self
                    .commit_cursor_realign_index(candidate.entry_idx as usize)
                    .is_err()
            {
                return FrontierDeferOutcome::Pending;
            }
            return FrontierDeferOutcome::Yielded;
        }
        let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
            return FrontierDeferOutcome::Continue;
        };
        visited.record(candidate.scope_id);
        if candidate.entry_idx as usize != self.cursor.index()
            && self
                .commit_cursor_realign_index(candidate.entry_idx as usize)
                .is_err()
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
        let at_route_entry = self
            .cursor
            .route_offer_entry_cursor_position(scope_id, current_idx)?;
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

    #[inline]
    pub(super) fn entry_has_route_scope(&self, entry_idx: usize) -> bool {
        let entry_scope = if self.offer_entry_state_snapshot(entry_idx).is_some()
            && self.offer_entry_has_active_lanes(entry_idx)
        {
            let scope_id = self.offer_entry_scope_id(entry_idx);
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
            let summary = self.compute_offer_entry_summary(current_idx);
            let entry_parallel = self.offer_entry_parallel_root(current_idx);
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
            if summary.intrinsic_ready() {
                flags |= CurrentFrontierSelectionState::FLAG_READY;
            }
            return CurrentFrontierSelectionState {
                frontier: self.offer_entry_frontier(current_idx),
                parallel_root: match current_parallel {
                    Some(root) => root,
                    None => ScopeId::none(),
                },
                flags,
            };
        }
        let current_is_controller = self.cursor.is_route_controller(scope_id);
        let current_is_dynamic = current_is_controller
            && self
                .cursor
                .route_scope_controller_resolver(scope_id)
                .is_some_and(|(resolver, _, _)| resolver.is_dynamic());
        let frontier_facts = CursorEndpoint::<ROLE, T, MAX_RV>::frontier_facts_at(
            &self.cursor,
            scope_id,
            current_is_controller,
            current_is_dynamic,
            current_idx,
        );
        let cursor_parallel =
            CursorEndpoint::<ROLE, T, MAX_RV>::parallel_scope_root(&self.cursor, scope_id);
        let cursor_parallel_has_offer =
            cursor_parallel.is_some_and(|root| self.root_frontier_active_mask(root) != 0);
        let current_entry_has_offer = self.offer_entry_has_active_lanes(current_idx);
        let current_entry_parallel = if cursor_parallel_has_offer || !current_entry_has_offer {
            None
        } else {
            self.offer_entry_state_snapshot(current_idx)
                .and_then(|_| self.offer_entry_parallel_root(current_idx))
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
        let progress_lane = match selected_arm {
            Some(arm) => self
                .route_scope_arm_lane_set_for_scope(selection.scope_id, arm)
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
    pub(super) fn try_poll_route_arm_selection_immediate(
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
            if let Poll::Ready(route_arm) = port.poll_route_arm_selection(scope_id, ROLE, cx) {
                arm = Some(route_arm);
                break;
            }
            next = offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        let arm = arm?;
        Arm::new(arm)
    }
    pub(super) fn try_poll_route_arm_selection_for_offer(
        &self,
        scope_id: ScopeId,
        offer_lanes: LaneSetView,
        cx: &mut core::task::Context<'_>,
    ) -> Option<Arm> {
        if let Some(arm) = self.try_poll_route_arm_selection_immediate(scope_id, offer_lanes, cx) {
            return Some(arm);
        }
        let is_dynamic_route_scope = self
            .cursor
            .route_scope_controller_resolver(scope_id)
            .is_some_and(|(resolver, _, _)| resolver.is_dynamic());
        if is_dynamic_route_scope {
            return None;
        }
        self.poll_arm_from_ready_mask(scope_id)
    }
}
