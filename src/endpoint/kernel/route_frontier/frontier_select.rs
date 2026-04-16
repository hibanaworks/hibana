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
    pub(in crate::endpoint::kernel) fn offer_entry_has_active_lanes(
        &self,
        entry_idx: usize,
    ) -> bool {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if active_offer_lanes.contains(lane_idx) {
                let info = self.route_state.lane_offer_state(lane_idx);
                if !info.entry.is_max() && state_index_to_usize(info.entry) == entry_idx {
                    return true;
                }
            }
            lane_idx += 1;
        }
        #[cfg(test)]
        {
            return self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
                .map(|state| state.active_mask != 0)
                .unwrap_or(false);
        }
        #[cfg(not(test))]
        false
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_has_other_active_lanes(
        &self,
        entry_idx: usize,
        excluded_lane_idx: usize,
    ) -> bool {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if lane_idx != excluded_lane_idx && active_offer_lanes.contains(lane_idx) {
                let info = self.route_state.lane_offer_state(lane_idx);
                if !info.entry.is_max() && state_index_to_usize(info.entry) == entry_idx {
                    return true;
                }
            }
            lane_idx += 1;
        }
        false
    }

    #[inline]
    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn offer_entry_active_mask_from_route_state(
        &self,
        entry_idx: usize,
    ) -> u32 {
        let mut active_mask = 0u32;
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let projected_lane_limit = core::cmp::min(lane_limit, u32::BITS as usize);
        let mut lane_idx = 0usize;
        while lane_idx < projected_lane_limit {
            if !active_offer_lanes.contains(lane_idx) {
                lane_idx += 1;
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if !info.entry.is_max() && state_index_to_usize(info.entry) == entry_idx {
                active_mask |= 1u32 << lane_idx;
            }
            lane_idx += 1;
        }
        active_mask
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_state_snapshot(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryState> {
        let has_active_lanes = self.offer_entry_has_active_lanes(entry_idx);
        #[cfg(test)]
        {
            if let Some(state) = self.offer_entry_state_from_route_state(entry_idx) {
                return Some(state);
            }
            if let Some(state) = self
                .frontier_state
                .offer_entry_state
                .get(entry_idx)
                .copied()
            {
                if state.active_mask != 0 || has_active_lanes {
                    return Some(state);
                }
                return None;
            }
        }
        #[cfg(not(test))]
        {
            has_active_lanes.then_some(OfferEntryState::EMPTY)
        }
        #[cfg(test)]
        {
            let active_mask = self.offer_entry_active_mask_from_route_state(entry_idx);
            has_active_lanes.then_some(OfferEntryState {
                active_mask,
                ..OfferEntryState::EMPTY
            })
        }
    }

    #[cfg(test)]
    #[inline]
    fn offer_entry_state_from_route_state(&self, entry_idx: usize) -> Option<OfferEntryState> {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if !active_offer_lanes.contains(lane_idx) {
                lane_idx += 1;
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                lane_idx += 1;
                continue;
            }
            let selection_meta = self.compute_offer_entry_selection_meta(
                info.scope,
                info,
                !self.offer_lane_set_for_scope(info.scope).is_empty(),
            );
            let representative_idx = state_index_to_usize(info.entry);
            let loop_meta = Self::scope_loop_meta_at(
                &self.cursor,
                &self.control_semantics(),
                info.scope,
                representative_idx,
            );
            let label_meta = Self::scope_label_meta_at(
                &self.cursor,
                &self.control_semantics(),
                info.scope,
                loop_meta,
                representative_idx,
            );
            return Some(OfferEntryState {
                active_mask: self.offer_entry_active_mask_from_route_state(entry_idx),
                lane_idx: lane_idx as u8,
                parallel_root: info.parallel_root,
                frontier: info.frontier,
                scope_id: info.scope,
                selection_meta,
                label_meta,
                materialization_meta: self.compute_scope_arm_materialization_meta(info.scope),
                summary: self.compute_offer_entry_static_summary_from_route_state(entry_idx),
                observed: OfferEntryObservedState::EMPTY,
            });
        }
        None
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
        if !self.offer_entry_has_active_lanes(entry_idx)
            || self.offer_entry_scope_id(entry_idx, state) != scope_id
        {
            return None;
        }
        if let Some(info) = self.offer_entry_lane_state(scope_id, entry_idx) {
            return Some(self.compute_offer_entry_selection_meta(
                scope_id,
                info,
                !self.offer_lane_set_for_scope(scope_id).is_empty(),
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
        if !self.offer_entry_has_active_lanes(entry_idx)
            || self.offer_entry_scope_id(entry_idx, state) != scope_id
        {
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
        if !self.offer_entry_has_active_lanes(entry_idx)
            || self.offer_entry_scope_id(entry_idx, state) != scope_id
        {
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
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if !active_offer_lanes.contains(lane_idx) {
                lane_idx += 1;
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                return Some(info);
            }
            lane_idx += 1;
        }
        #[cfg(test)]
        {
            let lane_idx = entry_state.lane_idx as usize;
            if lane_idx < lane_limit {
                let mut flags = 0u8;
                if entry_state.summary.is_controller() || entry_state.selection_meta.is_controller()
                {
                    flags |= LaneOfferState::FLAG_CONTROLLER;
                }
                if entry_state.summary.is_dynamic() {
                    flags |= LaneOfferState::FLAG_DYNAMIC;
                }
                return Some(LaneOfferState {
                    scope: entry_state.scope_id,
                    entry: checked_state_index(entry_idx).unwrap_or(StateIndex::MAX),
                    parallel_root: entry_state.parallel_root,
                    frontier: entry_state.frontier,
                    static_ready: entry_state.summary.static_ready(),
                    flags,
                });
            }
        }
        #[cfg(not(test))]
        let _ = entry_state;
        None
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_entry_representative_lane_idx(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
    ) -> Option<usize> {
        let lane_limit = self.cursor.logical_lane_count();
        let active_offer_lanes = self.route_state.active_offer_lanes();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if !active_offer_lanes.contains(lane_idx) {
                lane_idx += 1;
                continue;
            }
            let info = self.route_state.lane_offer_state(lane_idx);
            if state_index_to_usize(info.entry) == entry_idx {
                return Some(lane_idx);
            }
            lane_idx += 1;
        }
        #[cfg(not(test))]
        let _ = entry_state;
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
        if !self.offer_entry_has_active_lanes(entry_idx) {
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
            scope_id,
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
                }
            }
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope_id, arm)
                && let Some(entry) = checked_state_index(entry)
            {
                meta.recv_entry[arm_idx] = entry;
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
        if let Some((dispatch, dispatch_len)) =
            self.cursor.route_scope_first_recv_dispatch_table(scope_id)
        {
            meta.first_recv_dispatch = dispatch;
            meta.first_recv_len = dispatch_len;
        }
        meta.first_recv_label_mask = self
            .cursor
            .route_scope_first_recv_dispatch_label_mask(scope_id);
        meta.first_recv_dispatch_arm_mask = self
            .cursor
            .route_scope_first_recv_dispatch_arm_mask(scope_id);
        meta
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn binding_demux_contains_lane(
        &self,
        scope_id: ScopeId,
        preferred_arm: Option<u8>,
        lane_idx: usize,
    ) -> bool {
        let lane = lane_idx as u8;
        preferred_arm
            .map(|arm| self.binding_demux_contains_lane_for_arm(scope_id, arm, lane))
            .unwrap_or_else(|| {
                self.binding_demux_contains_lane_for_arm(scope_id, 0, lane)
                    || self.binding_demux_contains_lane_for_arm(scope_id, 1, lane)
            })
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn binding_demux_contains_lane_for_label_mask(
        &self,
        scope_id: ScopeId,
        label_meta: ScopeLabelMeta,
        label_mask: u128,
        lane_idx: usize,
    ) -> bool {
        if label_mask == 0 {
            return false;
        }
        let mut matched_arm = false;
        let mut arm = 0u8;
        while arm <= 1 {
            if (label_meta.binding_demux_label_mask_for_arm(arm) & label_mask) != 0 {
                matched_arm = true;
                if self.binding_demux_contains_lane(scope_id, Some(arm), lane_idx) {
                    return true;
                }
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        !matched_arm && self.binding_demux_contains_lane(scope_id, None, lane_idx)
    }

    #[inline]
    fn binding_demux_contains_lane_for_arm(&self, scope_id: ScopeId, arm: u8, lane: u8) -> bool {
        if let Some((entry, _)) = self.cursor.controller_arm_entry_by_arm(scope_id, arm)
            && let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry))
            && recv_meta.lane == lane
        {
            return true;
        }
        if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope_id, arm)
            && let Some(recv_meta) = self.cursor.try_recv_meta_at(entry)
            && recv_meta.lane == lane
        {
            return true;
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
            && let Some(recv_meta) = self.cursor.try_recv_meta_at(state_index_to_usize(entry))
            && recv_meta.lane == lane
        {
            return true;
        }
        if let Some(target_idx) =
            self.preview_passive_materialization_index_for_selected_arm(scope_id, arm)
            && let Some(recv_meta) = self.cursor.try_recv_meta_at(target_idx)
            && recv_meta.lane == lane
        {
            return true;
        }
        let lane_bit = if lane < u8::BITS as u8 {
            1u8 << lane
        } else {
            0
        };
        if (self
            .cursor
            .route_scope_first_recv_dispatch_lane_mask(scope_id, arm)
            & lane_bit)
            != 0
        {
            return true;
        }
        false
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
            if self.offer_entry_has_active_lanes(entry_idx)
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
        let offer_lanes = if scope_id.is_none() {
            crate::global::role_program::LaneSetView::EMPTY
        } else {
            self.offer_lane_set_for_scope(scope_id)
        };
        let binding_ready = self
            .binding_inbox
            .has_buffered_for_lane_set(offer_lanes, self.cursor.logical_lane_count());
        let mut has_ack = !scope_id.is_none() && self.peek_scope_ack(scope_id).is_some();
        let pending_ack = if let Some(lane_idx) =
            self.offer_entry_representative_lane_idx(entry_idx, entry_state)
        {
            if scope_id.is_none() {
                false
            } else {
                self.preview_scope_ack_token_non_consuming(scope_id, lane_idx, offer_lanes)
                    .is_some()
            }
        } else {
            false
        };
        if !has_ack {
            has_ack = pending_ack;
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
        let summary = self.compute_offer_entry_static_summary(entry_idx);
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
        if !self.offer_entry_has_active_lanes(entry_idx) {
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
        if self.route_state.active_offer_lanes().is_empty() {
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
