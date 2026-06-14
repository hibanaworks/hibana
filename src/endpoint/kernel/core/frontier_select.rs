use super::{
    ActiveEntrySet, ControlFlow, CurrentScopeSelectionMeta, CursorEndpoint, FrontierCandidate,
    FrontierKind, LaneOfferState, ObservedEntrySet, OfferEntryEvidence, OfferEntryObservedState,
    OfferEntryState, OfferEntrySummary, ScopeArmMaterializationMeta, ScopeId, Transport,
    checked_state_index, offer_entry_frontier_candidate, offer_entry_observed_state,
    state_index_to_usize,
};
impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: crate::runtime_core::config::Clock,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_has_active_lanes(
        &self,
        entry_idx: usize,
    ) -> bool {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if !info.entry.is_absent() && state_index_to_usize(info.entry) == entry_idx {
                return true;
            }
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        self.frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
            .is_some_and(OfferEntryState::is_active)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_has_other_active_lanes(
        &self,
        entry_idx: usize,
        excluded_lane_idx: usize,
    ) -> bool {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            if lane_idx != excluded_lane_idx {
                let info = self.decision_state.lane_offer_state(lane_idx);
                if !info.entry.is_absent() && state_index_to_usize(info.entry) == entry_idx {
                    return true;
                }
            }
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        false
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_state_snapshot(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryState> {
        let has_active_lanes = self.offer_entry_has_active_lanes(entry_idx);
        if let Some(state) = self.offer_entry_state_from_route_state(entry_idx) {
            return Some(state);
        }
        if let Some(state) = self
            .frontier_state
            .offer_entry_state
            .get(entry_idx)
            .copied()
        {
            if state.is_active() || has_active_lanes {
                return Some(state);
            }
            return None;
        }
        has_active_lanes.then_some(OfferEntryState::active(
            u8::MAX,
            ScopeId::none(),
            FrontierKind::Route,
            ScopeId::none(),
            CurrentScopeSelectionMeta::EMPTY,
            OfferEntrySummary::EMPTY,
        ))
    }

    #[inline]
    fn offer_entry_state_from_route_state(&self, entry_idx: usize) -> Option<OfferEntryState> {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
                continue;
            }
            let cached_state = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
                .filter(|state| state.scope_id == info.scope);
            let selection_meta = match cached_state {
                Some(state) => state.selection_meta,
                None => self.compute_offer_entry_selection_meta(
                    info.scope,
                    info,
                    !self.offer_lane_set_for_scope(info.scope).is_empty(),
                ),
            };
            let summary = match cached_state {
                Some(state) => state.summary,
                None => self.compute_offer_entry_summary_from_route_state(entry_idx),
            };
            return Some(OfferEntryState::active(
                lane_idx as u8,
                info.parallel_root,
                info.frontier,
                info.scope,
                selection_meta,
                summary,
            ));
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn observed_ready_reentry_entry_idx(
        &self,
        observed_entries: ObservedEntrySet,
        current_idx: usize,
    ) -> Option<usize> {
        let mask = observed_entries.ready_mask & !observed_entries.entry_bit(current_idx);
        observed_entries.first_entry_idx(mask)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<CurrentScopeSelectionMeta> {
        let state = self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx)
            || self.offer_entry_scope_id(entry_idx) != scope_id
        {
            return None;
        }
        Some(state.selection_meta)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_materialization_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeArmMaterializationMeta> {
        self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx)
            || self.offer_entry_scope_id(entry_idx) != scope_id
        {
            return None;
        }
        Some(self.compute_scope_arm_materialization_meta(scope_id))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_lane_state(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<LaneOfferState> {
        self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx)
            || self.offer_entry_scope_id(entry_idx) != scope_id
        {
            return None;
        }
        self.offer_entry_representative_lane_state(entry_idx)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_representative_lane_from_route_state(
        &self,
        entry_idx: usize,
    ) -> Option<(usize, LaneOfferState)> {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                return Some((lane_idx, info));
            }
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_parallel_root(
        &self,
        entry_idx: usize,
    ) -> Option<ScopeId> {
        if let Some(info) = self.offer_entry_representative_lane_state(entry_idx) {
            let parallel_root = info.parallel_root;
            return (!parallel_root.is_none()).then_some(parallel_root);
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_representative_lane_state(
        &self,
        entry_idx: usize,
    ) -> Option<LaneOfferState> {
        if let Some((lane_idx, info)) =
            self.offer_entry_representative_lane_from_route_state(entry_idx)
        {
            let lane_limit = self.cursor.logical_lane_count();
            if lane_idx < lane_limit {
                return Some(info);
            }
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_representative_lane_idx(
        &self,
        entry_idx: usize,
    ) -> Option<usize> {
        if let Some(pair) = self.offer_entry_representative_lane_from_route_state(entry_idx) {
            return Some(pair.0);
        }
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_scope_id(&self, entry_idx: usize) -> ScopeId {
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return ScopeId::none();
        }
        if let Some(info) = self.offer_entry_representative_lane_state(entry_idx) {
            return info.scope;
        }
        ScopeId::none()
    }

    pub(in crate::endpoint::kernel) fn compute_offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        info: LaneOfferState,
        has_offer_lanes: bool,
    ) -> CurrentScopeSelectionMeta {
        if !self.cursor.has_route_scope(scope_id) {
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
        let Some(arm_count) = self.cursor.route_scope_arm_count(scope_id) else {
            crate::invariant();
        };
        let mut meta = ScopeArmMaterializationMeta {
            arm_count,
            ..ScopeArmMaterializationMeta::EMPTY
        };
        let mut arm = 0u8;
        while arm <= 1 {
            let arm_idx = arm as usize;
            if let Some((entry, label)) = self
                .cursor
                .shared_controller_arm_entry_by_arm(scope_id, arm)
            {
                meta.controller_arm_entry[arm_idx] = entry;
                meta.controller_arm_label[arm_idx] = label;
                if let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry))
                    && recv_meta.peer != ROLE
                {
                    meta.controller_cross_role_recv_mask |= 1u8 << arm_idx;
                }
            }
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope_id, arm)
                && let Some(entry) = checked_state_index(entry)
            {
                meta.recv_entry[arm_idx] = entry;
            }
            if let Some(entry) = self.cursor.passive_observer_arm_entry(scope_id, arm) {
                meta.passive_arm_entry[arm_idx] = entry;
            }
            if let Some(scope) = self.cursor.passive_child_scope(scope_id, arm) {
                meta.passive_child_scope[arm_idx] = scope;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        if let Some((dispatch, dispatch_len)) =
            self.cursor.route_scope_first_recv_dispatch_table(scope_id)
        {
            let mut arm_mask = 0u8;
            let mut idx = 0usize;
            while idx < dispatch_len as usize {
                let entry = dispatch[idx];
                if entry.arm() < 2 && !entry.target().is_absent() {
                    arm_mask |= 1u8 << entry.arm();
                }
                idx += 1;
            }
            meta.record_first_recv_dispatch(arm_mask);
        }
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
            if self.offer_entry_state_snapshot(entry_idx).is_none() {
                continue;
            }
            if self.offer_entry_has_active_lanes(entry_idx)
                && self
                    .offer_entry_representative_lane_idx(entry_idx)
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
    ) -> FrontierKind {
        if let Some(info) = self.offer_entry_representative_lane_state(entry_idx) {
            return info.frontier;
        }
        FrontierKind::Route
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn preview_offer_entry_evidence_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> OfferEntryEvidence {
        let scope_id = self.offer_entry_scope_id(entry_idx);
        let offer_lanes = if scope_id.is_none() {
            crate::global::role_program::LaneSetView::EMPTY
        } else {
            self.offer_lane_set_for_scope(scope_id)
        };
        let mut evidence = OfferEntryEvidence::empty();
        if !scope_id.is_none()
            && (self.peek_scope_ack(scope_id).is_some()
                || self
                    .preview_scope_ack_token_non_consuming(scope_id, offer_lanes)
                    .is_some())
        {
            evidence = evidence.with_ack();
        }
        if !scope_id.is_none() && self.scope_has_ready_arm_evidence(scope_id) {
            evidence = evidence.with_ready_arm();
        }
        evidence
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_candidate_from_observation(
        &self,
        entry_idx: usize,
        evidence: OfferEntryEvidence,
    ) -> (OfferEntryObservedState, FrontierCandidate) {
        let scope_id = self.offer_entry_scope_id(entry_idx);
        let summary = self.compute_offer_entry_summary(entry_idx);
        let observed = offer_entry_observed_state(scope_id, summary, evidence);
        let candidate = offer_entry_frontier_candidate(
            scope_id,
            entry_idx,
            match self.offer_entry_parallel_root(entry_idx) {
                Some(root) => root,
                None => ScopeId::none(),
            },
            self.offer_entry_frontier(entry_idx),
            observed,
        );
        (observed, candidate)
    }

    pub(in crate::endpoint::kernel) fn scan_offer_entry_candidate_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<FrontierCandidate> {
        self.offer_entry_state_snapshot(entry_idx)?;
        if !self.offer_entry_has_active_lanes(entry_idx) {
            return None;
        }
        let evidence = self.preview_offer_entry_evidence_non_consuming(entry_idx);
        let (_observed, candidate) =
            self.offer_entry_candidate_from_observation(entry_idx, evidence);
        Some(candidate)
    }

    pub(in crate::endpoint::kernel) fn for_each_active_offer_candidate<R>(
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
}
