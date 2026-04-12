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
    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_active_mask_from_route_state(
        &self,
        entry_idx: usize,
    ) -> u8 {
        let mut active_mask = 0u8;
        let mut remaining_lanes = self.route_state.active_offer_mask;
        while remaining_lanes != 0 {
            let lane_idx = remaining_lanes.trailing_zeros() as usize;
            remaining_lanes &= !(1u8 << lane_idx);
            if lane_idx >= MAX_LANES {
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if !info.entry.is_max() && state_index_to_usize(info.entry) == entry_idx {
                active_mask |= 1u8 << lane_idx;
            }
        }
        active_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_state_snapshot(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryState> {
        let active_mask = self.offer_entry_active_mask_from_route_state(entry_idx);
        #[cfg(test)]
        {
            if let Some(mut state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            {
                if state.active_mask != 0 {
                    return Some(state);
                }
                if active_mask != 0 {
                    state.active_mask = active_mask;
                    return Some(state);
                }
                return None;
            }
        }
        (active_mask != 0).then_some(OfferEntryState {
            active_mask,
            ..OfferEntryState::EMPTY
        })
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_reentry_entry_idx(
        &self,
        observed_entries: ObservedEntrySet,
        current_idx: usize,
        ready_only: bool,
    ) -> Option<usize> {
        let mut mask = if ready_only {
            observed_entries.ready_mask
        } else {
            observed_entries.occupancy_mask()
        };
        mask &= !observed_entries.entry_bit(current_idx);
        observed_entries.first_entry_idx(mask)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<CurrentScopeSelectionMeta> {
        let state = self.offer_entry_state_snapshot(entry_idx)?;
        if state.active_mask == 0 || self.offer_entry_scope_id(entry_idx, state) != scope_id {
            return None;
        }
        if let Some(info) = self.offer_entry_lane_state(scope_id, entry_idx) {
            return Some(self.compute_offer_entry_selection_meta(
                scope_id,
                info,
                self.offer_lanes_for_scope(scope_id).1 != 0,
            ));
        }
        #[cfg(test)]
        {
            return Some(state.selection_meta);
        }
        #[cfg(not(test))]
        {
            None
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_label_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeLabelMeta> {
        let state = self.offer_entry_state_snapshot(entry_idx)?;
        if state.active_mask == 0 || self.offer_entry_scope_id(entry_idx, state) != scope_id {
            return None;
        }
        if let Some(info) = self.offer_entry_lane_state(scope_id, entry_idx) {
            let representative_idx = state_index_to_usize(info.entry);
            let loop_meta = Self::scope_loop_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                representative_idx,
            );
            return Some(Self::scope_label_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                loop_meta,
                representative_idx,
            ));
        }
        let loop_meta = Self::scope_loop_meta(&self.cursor, &self.control_semantics(), scope_id);
        #[cfg(test)]
        {
            if !state.label_meta.scope_id().is_none() {
                return Some(state.label_meta);
            }
        }
        Some(Self::scope_label_meta(
            &self.cursor,
            &self.control_semantics(),
            scope_id,
            loop_meta,
        ))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_materialization_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeArmMaterializationMeta> {
        let state = self.offer_entry_state_snapshot(entry_idx)?;
        if state.active_mask == 0 || self.offer_entry_scope_id(entry_idx, state) != scope_id {
            return None;
        }
        #[cfg(test)]
        {
            if state.materialization_meta.arm_count != 0 {
                return Some(state.materialization_meta);
            }
        }
        Some(self.compute_scope_arm_materialization_meta(scope_id))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_lane_state(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<LaneOfferState> {
        let state = self.offer_entry_state_snapshot(entry_idx)?;
        if state.active_mask == 0 || self.offer_entry_scope_id(entry_idx, state) != scope_id {
            return None;
        }
        self.offer_entry_representative_lane_state(entry_idx, state)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_parallel_root_from_state(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> Option<ScopeId> {
        if let Some(info) = self.offer_entry_representative_lane_state(entry_idx, entry_state) {
            let parallel_root = info.parallel_root;
            return (!parallel_root.is_none()).then_some(parallel_root);
        }
        #[cfg(test)]
        {
            return (!entry_state.parallel_root.is_none()).then_some(entry_state.parallel_root);
        }
        #[cfg(not(test))]
        {
            None
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_representative_lane_state(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> Option<LaneOfferState> {
        let mut remaining_lanes = entry_state.active_mask;
        while remaining_lanes != 0 {
            let lane_idx = remaining_lanes.trailing_zeros() as usize;
            remaining_lanes &= !(1u8 << lane_idx);
            if lane_idx >= MAX_LANES {
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                return Some(info);
            }
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_representative_lane_idx(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> Option<usize> {
        let mut remaining_lanes = entry_state.active_mask;
        while remaining_lanes != 0 {
            let lane_idx = remaining_lanes.trailing_zeros() as usize;
            remaining_lanes &= !(1u8 << lane_idx);
            if lane_idx >= MAX_LANES {
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                return Some(lane_idx);
            }
        }
        #[cfg(test)]
        {
            let lane_idx = entry_state.lane_idx as usize;
            if lane_idx < MAX_LANES {
                return Some(lane_idx);
            }
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_scope_id(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> ScopeId {
        if entry_state.active_mask == 0 {
            return ScopeId::none();
        }
        if let Some(info) = self.offer_entry_representative_lane_state(entry_idx, entry_state) {
            return info.scope;
        }
        #[cfg(test)]
        {
            return entry_state.scope_id;
        }
        #[cfg(not(test))]
        {
            ScopeId::none()
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_lane_mask_for_scope_id(
        &self,
        scope_id: ScopeId,
    ) -> u8 {
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
        let mut offer_lane_mask = 0u8;
        let mut offer_lane_idx = 0usize;
        while offer_lane_idx < offer_lanes_len {
            let lane_idx = offer_lanes[offer_lane_idx] as usize;
            if lane_idx < MAX_LANES {
                offer_lane_mask |= 1u8 << lane_idx;
            }
            offer_lane_idx += 1;
        }
        offer_lane_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_offer_lane_mask(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> u8 {
        if entry_state.active_mask == 0 {
            return 0;
        }
        #[cfg(test)]
        if self
            .offer_entry_representative_lane_state(entry_idx, entry_state)
            .is_none()
        {
            return entry_state.offer_lane_mask;
        }
        let scope_id = self.offer_entry_scope_id(entry_idx, entry_state);
        if scope_id.is_none() {
            return 0;
        }
        let offer_lane_mask = self.offer_lane_mask_for_scope_id(scope_id);
        if offer_lane_mask != 0 {
            return offer_lane_mask;
        }
        #[cfg(test)]
        {
            return entry_state.offer_lane_mask;
        }
        #[cfg(not(test))]
        {
            0
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_lane_mask_for_active_entries(
        &self,
        active_entries: ActiveEntrySet,
    ) -> u8 {
        let mut offer_lane_mask = 0u8;
        let mut remaining_entries = active_entries.occupancy_mask();
        while remaining_entries != 0 {
            let slot_idx = remaining_entries.trailing_zeros() as usize;
            remaining_entries &= !(1u8 << slot_idx);
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if state.active_mask == 0 {
                continue;
            }
            offer_lane_mask |= self.offer_entry_offer_lane_mask(entry_idx, state);
        }
        offer_lane_mask
    }

    pub(in crate::endpoint::kernel) fn compute_offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        info: LaneOfferState,
        has_offer_lanes: bool,
    ) -> CurrentScopeSelectionMeta {
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return CurrentScopeSelectionMeta::EMPTY;
        };
        if region.kind != ScopeKind::Route {
            return CurrentScopeSelectionMeta::EMPTY;
        }
        let mut flags = CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY;
        if has_offer_lanes {
            flags |= CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES;
        }
        if info.is_controller() {
            flags |= CurrentScopeSelectionMeta::FLAG_CONTROLLER;
        }
        CurrentScopeSelectionMeta { flags }
    }

    pub(in crate::endpoint::kernel) fn compute_scope_arm_materialization_meta(
        &self,
        scope_id: ScopeId,
    ) -> ScopeArmMaterializationMeta {
        let mut meta = ScopeArmMaterializationMeta {
            arm_count: self.cursor.route_scope_arm_count(scope_id).unwrap_or(0),
            ..ScopeArmMaterializationMeta::EMPTY
        };
        let mut arm = 0u8;
        while arm <= 1 {
            let arm_idx = arm as usize;
            if let Some((entry, label)) = self.cursor.controller_arm_entry_by_arm(scope_id, arm) {
                meta.controller_arm_entry[arm_idx] = entry;
                meta.controller_arm_label[arm_idx] = label;
                if let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry)) {
                    meta.controller_recv_mask |= 1u8 << arm_idx;
                    if recv_meta.peer != ROLE {
                        meta.controller_cross_role_recv_mask |= 1u8 << arm_idx;
                    }
                    meta.record_binding_demux_lane(arm, recv_meta.lane);
                }
            }
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope_id, arm)
                && let Some(entry) = checked_state_index(entry)
            {
                meta.recv_entry[arm_idx] = entry;
                if let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry)) {
                    meta.record_binding_demux_lane(arm, recv_meta.lane);
                }
            }
            if let Some(PassiveArmNavigation::WithinArm { entry }) = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope_id, arm)
            {
                meta.passive_arm_entry[arm_idx] = entry;
            }
            if let Some(scope) = self.cursor.passive_arm_scope_by_arm(scope_id, arm) {
                meta.passive_arm_scope[arm_idx] = scope;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        let mut dispatch_idx = 0usize;
        while let Some(dispatch) = self
            .cursor
            .route_scope_first_recv_dispatch_entry(scope_id, dispatch_idx)
        {
            let (_label, dispatch_arm, target) = dispatch;
            meta.first_recv_dispatch[dispatch_idx] = dispatch;
            if let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(target)) {
                meta.record_binding_demux_lane(dispatch_arm, recv_meta.lane);
            }
            dispatch_idx += 1;
        }
        meta.first_recv_len = dispatch_idx as u8;
        meta
    }

    pub(in crate::endpoint::kernel) fn next_active_frontier_entry(
        &self,
        active_entries: ActiveEntrySet,
        remaining_mask: &mut u8,
    ) -> Option<usize> {
        while *remaining_mask != 0 {
            let slot_idx = remaining_mask.trailing_zeros() as usize;
            *remaining_mask &= !(1u8 << slot_idx);
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if state.active_mask != 0
                && self
                    .offer_entry_representative_lane_idx(entry_idx, state)
                    .is_some()
            {
                return Some(entry_idx);
            }
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_frontier(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> FrontierKind {
        if let Some(info) = self.offer_entry_representative_lane_state(entry_idx, entry_state) {
            return info.frontier;
        }
        #[cfg(test)]
        {
            return entry_state.frontier;
        }
        #[cfg(not(test))]
        {
            FrontierKind::Route
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn preview_offer_entry_evidence_non_consuming(
        &mut self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> (bool, bool, bool) {
        let scope_id = self.offer_entry_scope_id(entry_idx, entry_state);
        let offer_lane_mask = self.offer_entry_offer_lane_mask(entry_idx, entry_state);
        let binding_ready = self
            .binding_inbox
            .has_buffered_for_lane_mask(offer_lane_mask);
        let mut has_ack = !scope_id.is_none() && self.peek_scope_ack(scope_id).is_some();
        let pending_ack_mask = if let Some(lane_idx) =
            self.offer_entry_representative_lane_idx(entry_idx, entry_state)
        {
            if scope_id.is_none() {
                0
            } else {
                self.pending_scope_ack_lane_mask(lane_idx, scope_id, offer_lane_mask)
            }
        } else {
            0
        };
        if !has_ack {
            has_ack = pending_ack_mask != 0;
        }
        let has_ready_arm_evidence =
            !scope_id.is_none() && self.scope_has_ready_arm_evidence(scope_id);
        (binding_ready, has_ack, has_ready_arm_evidence)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_candidate_from_observation(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
        binding_ready: bool,
        has_ack: bool,
        has_ready_arm_evidence: bool,
    ) -> (OfferEntryObservedState, FrontierCandidate) {
        let scope_id = self.offer_entry_scope_id(entry_idx, entry_state);
        let summary = self.compute_offer_entry_static_summary(entry_state.active_mask, entry_idx);
        let loop_meta = if let Some(info) =
            self.offer_entry_representative_lane_state(entry_idx, entry_state)
        {
            Self::scope_loop_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                state_index_to_usize(info.entry),
            )
        } else {
            #[cfg(test)]
            {
                entry_state.label_meta.loop_meta()
            }
            #[cfg(not(test))]
            {
                if scope_id.is_none() {
                    ScopeLoopMeta::EMPTY
                } else {
                    Self::scope_loop_meta(&self.cursor, &self.control_semantics(), scope_id)
                }
            }
        };
        let ack_is_progress = Self::ack_is_progress_evidence(loop_meta, has_ack);
        let observed = offer_entry_observed_state(
            scope_id,
            summary,
            has_ready_arm_evidence,
            ack_is_progress,
            binding_ready,
        );
        let candidate = offer_entry_frontier_candidate(
            scope_id,
            entry_idx,
            self.offer_entry_parallel_root_from_state(entry_idx, entry_state)
                .unwrap_or(ScopeId::none()),
            self.offer_entry_frontier(entry_idx, entry_state),
            observed,
        );
        (observed, candidate)
    }

    pub(in crate::endpoint::kernel) fn scan_offer_entry_candidate_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<FrontierCandidate> {
        let entry_state = self.offer_entry_state_snapshot(entry_idx)?;
        if entry_state.active_mask == 0 {
            return None;
        }
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            self.preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);
        let (_observed, candidate) = self.offer_entry_candidate_from_observation(
            entry_idx,
            entry_state,
            binding_ready,
            has_ack,
            has_ready_arm_evidence,
        );
        Some(candidate)
    }

    pub(super) fn for_each_active_offer_candidate<R>(
        &mut self,
        current_parallel: Option<ScopeId>,
        mut visitor: impl FnMut(FrontierCandidate) -> ControlFlow<R>,
    ) -> Option<R> {
        let active_entries = self.active_frontier_entries(current_parallel);
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(entry_idx) =
            self.next_active_frontier_entry(active_entries, &mut remaining_entries)
        {
            let Some(candidate) = self.scan_offer_entry_candidate_non_consuming(entry_idx) else {
                continue;
            };
            if let ControlFlow::Break(result) = visitor(candidate) {
                return Some(result);
            }
        }
        None
    }

    pub(super) fn on_frontier_defer(
        &mut self,
        liveness: &mut OfferLivenessState,
        scope_id: ScopeId,
        current_parallel: Option<ScopeId>,
        source: DeferSource,
        reason: DeferReason,
        retry_hint: u8,
        offer_lane: u8,
        binding_ready: bool,
        selected_arm: Option<u8>,
        visited: &mut FrontierVisitSet,
    ) -> FrontierDeferOutcome {
        let fingerprint = self.evidence_fingerprint(scope_id, binding_ready);
        let budget = liveness.on_defer(fingerprint);
        let exhausted = matches!(budget, DeferBudgetOutcome::Exhausted);
        let is_controller = self.cursor.is_route_controller(scope_id);
        let frontier = Self::frontier_kind_for_cursor(&self.cursor, scope_id, is_controller);
        let hint = self.peek_scope_hint(scope_id);
        let ready_arm_mask = self.scope_ready_arm_mask(scope_id);
        self.emit_policy_defer_event(
            source,
            reason,
            scope_id,
            frontier,
            selected_arm,
            hint,
            retry_hint,
            *liveness,
            ready_arm_mask,
            binding_ready,
            exhausted,
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
            Self::frontier_kind_for_cursor(&self.cursor, scope_id, current_is_controller),
        );
        self.for_each_active_offer_candidate(current_parallel, |candidate| {
            let _ = snapshot.push_candidate(candidate);
            ControlFlow::<()>::Continue(())
        });
        if exhausted {
            let Some(candidate) = snapshot.select_exhausted_controller_candidate(*visited) else {
                return FrontierDeferOutcome::Exhausted;
            };
            visited.record(candidate.scope_id);
            if candidate.entry_idx as usize != self.cursor.index() {
                self.set_cursor_index(candidate.entry_idx as usize);
            }
            return FrontierDeferOutcome::Yielded;
        }
        let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
            return FrontierDeferOutcome::Continue;
        };
        visited.record(candidate.scope_id);
        if candidate.entry_idx as usize != self.cursor.index() {
            self.set_cursor_index(candidate.entry_idx as usize);
        }
        FrontierDeferOutcome::Yielded
    }

    fn current_scope_selection_meta(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        current_frontier: CurrentFrontierSelectionState,
    ) -> Option<CurrentScopeSelectionMeta> {
        if let Some(meta) = self.offer_entry_selection_meta(scope_id, current_idx) {
            return Some(meta);
        }
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        };
        if region.kind != ScopeKind::Route {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let offer_entry = self.cursor.route_scope_offer_entry(region.scope_id)?;
        let route_entry_idx = if offer_entry.is_max() {
            current_idx
        } else {
            state_index_to_usize(offer_entry)
        };
        if !offer_entry.is_max() && route_entry_idx != current_idx {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let mut flags = CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY;
        if self.offer_lanes_for_scope(region.scope_id).1 != 0 {
            flags |= CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES;
        }
        if current_frontier.is_controller() {
            flags |= CurrentScopeSelectionMeta::FLAG_CONTROLLER;
        }
        Some(CurrentScopeSelectionMeta { flags })
    }

    fn current_frontier_selection_state(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
    ) -> CurrentFrontierSelectionState {
        if let Some(info) = self.offer_entry_lane_state(scope_id, current_idx) {
            let entry_state = self
                .offer_entry_state_snapshot(current_idx)
                .unwrap_or_else(|| unreachable!("active offer entry must have a runtime snapshot"));
            let summary =
                self.compute_offer_entry_static_summary(entry_state.active_mask, current_idx);
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
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false);
        let static_facts = Self::frontier_static_facts_at(
            &self.cursor,
            &self.control_semantics(),
            scope_id,
            current_is_controller,
            current_is_dynamic,
            current_idx,
        );
        let cursor_parallel = Self::parallel_scope_root(&self.cursor, scope_id);
        let cursor_parallel_has_offer = cursor_parallel
            .map(|root| self.root_frontier_active_mask(root) != 0)
            .unwrap_or(false);
        let current_entry_has_offer = self.offer_entry_active_mask(current_idx) != 0;
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

    pub(super) fn align_cursor_to_selected_scope(&mut self) -> RecvResult<()> {
        let node_scope = self.cursor.node_scope_id();
        let current_scope = self.current_offer_scope_id();
        if current_scope != node_scope
            && let Some(entry_idx) = self.route_scope_offer_entry_index(current_scope)
            && entry_idx != self.cursor.index()
        {
            self.set_cursor_index(entry_idx);
            self.sync_lane_offer_state();
            return self.align_cursor_to_selected_scope();
        }
        let node_scope = self.current_offer_scope_id();
        let current_idx = self.cursor.index();
        let mut current_frontier_state =
            self.current_frontier_selection_state(node_scope, current_idx);
        let current_frontier = current_frontier_state.frontier;
        let current_parallel = current_frontier_state.parallel();
        let current_parallel_root = current_frontier_state.parallel_root;
        let current_scope_selected = self.selected_arm_for_scope(node_scope).is_some();
        if current_scope_selected
            && self
                .current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
                .map(|meta| meta.is_route_entry())
                .unwrap_or(false)
        {
            return Ok(());
        }
        let use_root_observed_entries = current_parallel.is_some();
        let active_entries = self.active_frontier_entries(current_parallel);
        if active_entries.contains_only(current_idx) {
            let Some(current_scope_meta) =
                self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
            else {
                return Ok(());
            };
            if current_scope_meta.is_route_entry() && current_scope_meta.has_offer_lanes() {
                return Ok(());
            }
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let mut observed_entries = if use_root_observed_entries {
            self.root_frontier_observed_entries(current_parallel_root)
        } else {
            self.global_frontier_observed_entries()
        };
        let cached_entries = self.cached_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
        );
        if cached_entries.is_none() && observed_entries.len() != 0 {
            self.refresh_frontier_observation_cache(
                current_parallel_root,
                use_root_observed_entries,
            );
            observed_entries = if use_root_observed_entries {
                self.root_frontier_observed_entries(current_parallel_root)
            } else {
                self.global_frontier_observed_entries()
            };
        }
        let reentry_ready_entry_idx =
            self.observed_reentry_entry_idx(observed_entries, current_idx, true);
        let reentry_any_entry_idx =
            self.observed_reentry_entry_idx(observed_entries, current_idx, false);
        let loop_controller_without_evidence =
            current_frontier_state.loop_controller_without_evidence();
        let progress_sibling_exists = if current_parallel_root.is_none() {
            self.global_frontier_progress_sibling_exists(
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        } else {
            self.root_frontier_progress_sibling_exists(
                current_parallel_root,
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        };
        let Some(current_scope_meta) =
            self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
        else {
            return Ok(());
        };
        let current_is_route_entry = current_scope_meta.is_route_entry();
        let current_has_offer_lanes = current_scope_meta.has_offer_lanes();
        let current_is_controller = current_scope_meta.is_controller();
        let observed_mask = observed_entries.occupancy_mask();
        let current_entry_bit = observed_entries.entry_bit(current_idx);
        if current_entry_bit != 0 {
            current_frontier_state.ready |= (current_entry_bit & observed_entries.ready_mask) != 0;
            current_frontier_state.has_progress_evidence |=
                (current_entry_bit & observed_entries.progress_mask) != 0;
        }
        let current_matches_candidate = current_entry_bit != 0;
        let mut current_has_evidence = (current_entry_bit & observed_entries.progress_mask) != 0;
        let suppress_current_controller_without_evidence = current_is_controller
            && current_matches_candidate
            && (current_entry_bit & observed_entries.ready_arm_mask) == 0
            && (current_entry_bit & observed_entries.progress_mask) == 0
            && progress_sibling_exists;
        let controller_progress_sibling_exists = (observed_entries.progress_mask
            & observed_entries.controller_mask
            & !current_entry_bit)
            != 0;
        let mut static_controller_ready_mask = observed_mask & !observed_entries.controller_mask;
        static_controller_ready_mask |= current_entry_bit & observed_entries.controller_mask;
        static_controller_ready_mask |=
            observed_entries.progress_mask & observed_entries.controller_mask;
        if suppress_current_controller_without_evidence {
            static_controller_ready_mask &= !current_entry_bit;
        }
        let current_entry_unrunnable = current_is_route_entry && !current_has_offer_lanes;
        let mut candidate_mask = current_entry_bit | observed_entries.progress_mask;
        if current_entry_unrunnable {
            candidate_mask |= observed_mask & !current_entry_bit;
        }
        candidate_mask &= static_controller_ready_mask;
        let hinted_mask = candidate_mask & observed_entries.ready_arm_mask;
        let hinted_count = hinted_mask.count_ones() as usize;
        let hint_filter_mask = if hinted_count == 1 { hinted_mask } else { 0 };
        let hint_filter = observed_entries.first_entry_idx(hint_filter_mask);
        let candidate_mask = if hint_filter_mask != 0 {
            hinted_mask
        } else {
            candidate_mask
        };
        let controller_mask = candidate_mask & observed_entries.controller_mask;
        let dynamic_controller_mask = controller_mask & observed_entries.dynamic_controller_mask;
        let candidate_count = candidate_mask.count_ones() as usize;
        let controller_count = controller_mask.count_ones() as usize;
        let dynamic_controller_count = dynamic_controller_mask.count_ones() as usize;
        let candidate_idx = observed_entries.first_entry_idx(candidate_mask);
        let controller_idx = observed_entries.first_entry_idx(controller_mask);
        let dynamic_controller_idx = observed_entries.first_entry_idx(dynamic_controller_mask);
        current_has_evidence |= current_frontier_state.has_progress_evidence;
        let suppress_current_passive_without_evidence =
            should_suppress_current_passive_without_evidence(
                current_frontier,
                current_is_controller,
                current_has_evidence,
                controller_progress_sibling_exists,
            );
        let current_matches_filtered = current_entry_matches_after_filter(
            current_matches_candidate && !suppress_current_passive_without_evidence,
            current_has_offer_lanes,
            current_idx,
            hint_filter,
        );
        let current_is_candidate = current_entry_is_candidate(
            current_matches_filtered,
            current_is_controller,
            current_has_evidence,
            candidate_count,
            progress_sibling_exists,
        );
        let selection = match choose_offer_priority(
            current_is_candidate,
            dynamic_controller_count,
            controller_count,
            candidate_count,
        ) {
            Some(OfferSelectPriority::CurrentOfferEntry) => {
                Some((OfferSelectPriority::CurrentOfferEntry, current_idx))
            }
            Some(OfferSelectPriority::DynamicControllerUnique) => dynamic_controller_idx
                .map(|idx| (OfferSelectPriority::DynamicControllerUnique, idx)),
            Some(OfferSelectPriority::ControllerUnique) => {
                controller_idx.map(|idx| (OfferSelectPriority::ControllerUnique, idx))
            }
            Some(OfferSelectPriority::CandidateUnique) => {
                candidate_idx.map(|idx| (OfferSelectPriority::CandidateUnique, idx))
            }
            None => None,
        };
        if let Some((_priority, entry_idx)) = selection {
            if entry_idx != self.cursor.index() {
                self.set_cursor_index(entry_idx);
            }
            return Ok(());
        }
        if self.ensure_current_route_arm_state()?.is_some() {
            return Ok(());
        }
        if current_is_route_entry && current_has_offer_lanes {
            return Ok(());
        }
        if !current_is_route_entry {
            if let Some(entry_idx) = reentry_ready_entry_idx.or(reentry_any_entry_idx) {
                if entry_idx != self.cursor.index() {
                    self.set_cursor_index(entry_idx);
                }
                return Ok(());
            }
        }
        Err(RecvError::PhaseInvariant)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn align_cursor_to_lane_progress(
        &mut self,
        preferred_lane_idx: usize,
    ) -> bool {
        if let Some(idx) = self.cursor.index_for_lane_step(preferred_lane_idx) {
            self.set_cursor_index(idx);
            return true;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
                self.set_cursor_index(idx);
                return true;
            }
            lane_idx += 1;
        }
        false
    }

    pub(in crate::endpoint::kernel) fn has_ready_frontier_candidate(&mut self) -> bool {
        if self.route_state.active_offer_mask == 0 {
            return false;
        }
        let scope_id = self.current_offer_scope_id();
        if scope_id.is_none() {
            return false;
        }
        let cursor_parallel = Self::parallel_scope_root(&self.cursor, scope_id);
        let mut has_ready = false;
        self.for_each_active_offer_candidate(cursor_parallel, |candidate| {
            has_ready |= candidate.ready();
            ControlFlow::<()>::Continue(())
        });
        has_ready
    }
}
