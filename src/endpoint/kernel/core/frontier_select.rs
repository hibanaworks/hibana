use super::{
    ActiveEntrySlot, ActiveOfferEntry, ControlFlow, CurrentScopeSelectionMeta, CursorEndpoint,
    FrontierCandidate, LaneOfferState, OfferEntryEvidence, OfferEntryKey, OfferEntryObservedState,
    ScopeArmMaterializationMeta, ScopeId, Transport, offer_entry_frontier_candidate,
    offer_entry_observed_state,
};
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn active_offer_entry_from_owner_lane(
        &self,
        key: OfferEntryKey,
        owner_lane_idx: usize,
    ) -> ActiveOfferEntry {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        if owner_lane_idx >= lane_limit || !active_offer_lanes.contains(owner_lane_idx) {
            crate::invariant();
        }
        let owner = self.decision_state.lane_offer_state(owner_lane_idx);
        if owner.key() != Some(key) {
            crate::invariant();
        }
        let owner_lane = match u8::try_from(owner_lane_idx) {
            Ok(lane) => lane,
            Err(_) => crate::invariant(),
        };
        crate::invariant_some(ActiveOfferEntry::new(owner_lane, owner))
    }

    pub(in crate::endpoint::kernel) fn active_offer_entry_excluding(
        &self,
        key: OfferEntryKey,
        excluded_lane_idx: Option<usize>,
    ) -> Option<ActiveOfferEntry> {
        if key.is_absent() {
            crate::invariant();
        }
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.decision_state.active_offer_lanes();
        let mut active: Option<ActiveOfferEntry> = None;
        let mut next = active_offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let info = self.decision_state.lane_offer_state(lane_idx);
            if info.entry.is_absent() || info.scope.is_none() {
                crate::invariant();
            }
            if excluded_lane_idx != Some(lane_idx) && info.key() == Some(key) {
                if let Some(active) = active {
                    if !active.accepts_lane(info) {
                        crate::invariant();
                    }
                } else {
                    let lane = match u8::try_from(lane_idx) {
                        Ok(lane) => lane,
                        Err(_) => crate::invariant(),
                    };
                    active = Some(crate::invariant_some(ActiveOfferEntry::new(lane, info)));
                }
            }
            next = active_offer_lanes.next_set_from(lane_idx + 1, lane_limit);
        }
        active
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn active_offer_entry(
        &self,
        key: OfferEntryKey,
    ) -> Option<ActiveOfferEntry> {
        self.active_offer_entry_excluding(key, None)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn active_offer_entry_from_slot(
        &self,
        slot: ActiveEntrySlot,
    ) -> ActiveOfferEntry {
        if slot.key.is_absent() {
            crate::invariant();
        }
        self.active_offer_entry_from_owner_lane(slot.key, slot.lane_idx as usize)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<CurrentScopeSelectionMeta> {
        let key = OfferEntryKey::from_index(scope_id, entry_idx)?;
        let active = self.active_offer_entry(key)?;
        Some(self.compute_offer_entry_selection_meta(
            active.representative(),
            !self.offer_lane_set_for_scope(scope_id).is_empty(),
        ))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_materialization_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeArmMaterializationMeta> {
        let key = OfferEntryKey::from_index(scope_id, entry_idx)?;
        self.active_offer_entry(key)?;
        Some(self.compute_scope_arm_materialization_meta(scope_id))
    }

    pub(in crate::endpoint::kernel) fn compute_offer_entry_selection_meta(
        &self,
        info: LaneOfferState,
        has_offer_lanes: bool,
    ) -> CurrentScopeSelectionMeta {
        let scope_id = info.scope;
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
        if self.cursor.route_scope_arm_count(scope_id).is_none() {
            crate::invariant();
        }
        let mut meta = ScopeArmMaterializationMeta::EMPTY;
        let mut arm = 0u8;
        while arm <= 1 {
            let arm_idx = arm as usize;
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
        if let Some(arm_mask) = self
            .cursor
            .route_scope_first_recv_dispatch_arm_mask(scope_id)
        {
            meta.record_first_recv_dispatch(arm_mask);
        }
        meta
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn preview_offer_entry_evidence_non_consuming(
        &mut self,
        scope_id: ScopeId,
    ) -> OfferEntryEvidence {
        let mut evidence = OfferEntryEvidence::empty();
        if self.peek_live_scope_ack(scope_id).is_some() {
            evidence = evidence.with_ack();
        }
        if self.scope_has_ready_arm_evidence(scope_id) {
            evidence = evidence.with_ready_arm();
        }
        evidence
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_candidate_from_observation(
        &self,
        active: ActiveOfferEntry,
        evidence: OfferEntryEvidence,
    ) -> (OfferEntryObservedState, FrontierCandidate) {
        let info = active.representative();
        let observed = offer_entry_observed_state(info, evidence);
        let candidate = offer_entry_frontier_candidate(info, observed);
        (observed, candidate)
    }

    pub(in crate::endpoint::kernel) fn scan_active_offer_entry_non_consuming(
        &mut self,
        slot: ActiveEntrySlot,
    ) -> (ActiveOfferEntry, OfferEntryObservedState, FrontierCandidate) {
        let active = self.active_offer_entry_from_slot(slot);
        let evidence = self.preview_offer_entry_evidence_non_consuming(active.scope());
        let (observed, candidate) = self.offer_entry_candidate_from_observation(active, evidence);
        (active, observed, candidate)
    }

    pub(in crate::endpoint::kernel) fn for_each_active_offer_candidate<R>(
        &mut self,
        current_parallel: Option<ScopeId>,
        mut visitor: impl FnMut(FrontierCandidate) -> ControlFlow<R>,
    ) -> Option<R> {
        let active_entries = self.active_frontier_entries(current_parallel);
        let mut next_slot = 0usize;
        while next_slot < active_entries.len() {
            let slot = crate::invariant_some(active_entries.slot_at(next_slot));
            next_slot += 1;
            let (_, _, candidate) = self.scan_active_offer_entry_non_consuming(slot);
            if let ControlFlow::Break(result) = visitor(candidate) {
                return Some(result);
            }
        }
        None
    }
}
