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
    ) -> crate::global::role_program::LaneMask {
        let mut active_mask = 0;
        let lane_limit = self.cursor.logical_lane_count();
        let mut remaining_lanes = self.route_state.active_offer_mask;
        while remaining_lanes != 0 {
            let lane_idx = remaining_lanes.trailing_zeros() as usize;
            remaining_lanes &= !crate::global::role_program::lane_mask_bit(lane_idx);
            if lane_idx >= lane_limit {
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if !info.entry.is_max() && state_index_to_usize(info.entry) == entry_idx {
                active_mask |= crate::global::role_program::lane_mask_bit(lane_idx);
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
                self.offer_lane_mask_for_scope(scope_id) != 0,
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
        let lane_limit = self.cursor.logical_lane_count();
        while remaining_lanes != 0 {
            let lane_idx = remaining_lanes.trailing_zeros() as usize;
            remaining_lanes &= !crate::global::role_program::lane_mask_bit(lane_idx);
            if lane_idx >= lane_limit {
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
        let lane_limit = self.cursor.logical_lane_count();
        while remaining_lanes != 0 {
            let lane_idx = remaining_lanes.trailing_zeros() as usize;
            remaining_lanes &= !crate::global::role_program::lane_mask_bit(lane_idx);
            if lane_idx >= lane_limit {
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
            if lane_idx < lane_limit {
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
    ) -> crate::global::role_program::LaneMask {
        self.offer_lane_mask_for_scope(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_offer_lane_mask(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> crate::global::role_program::LaneMask {
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
    ) -> crate::global::role_program::LaneMask {
        let mut offer_lane_mask = 0;
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

    #[inline]
    pub(in crate::endpoint::kernel) fn align_cursor_to_lane_progress(
        &mut self,
        preferred_lane_idx: usize,
    ) -> bool {
        if let Some(idx) = self.cursor.index_for_lane_step(preferred_lane_idx) {
            self.set_cursor_index(idx);
            return true;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
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
