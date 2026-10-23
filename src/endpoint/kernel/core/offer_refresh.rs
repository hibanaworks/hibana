use super::{
    ActiveEntrySet, ActiveEntrySlot, CursorInvariantError, CursorRefresh,
    FrontierScratchSectionLease, LaneOfferState, OfferEntryKey, OperationKernel,
    ReentryScopeLiveness, ScopeId, StateIndex, Transport, frontier_global_active_entries_view,
    state_index_to_usize,
};
impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn root_frontier_has_active_entries(
        &self,
        root: ScopeId,
    ) -> bool {
        self.progress
            .frontier_state
            .root_frontier_has_active_entries(root)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn root_frontier_active_entries(
        &self,
        root: ScopeId,
    ) -> ActiveEntrySet<'_> {
        self.progress
            .frontier_state
            .root_frontier_active_entries(root)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn active_frontier_entries<'a>(
        &self,
        current_parallel: Option<ScopeId>,
        scratch: &'a mut FrontierScratchSectionLease<'_, ActiveEntrySlot>,
    ) -> ActiveEntrySet<'a> {
        match current_parallel {
            Some(root) => {
                let source = self.root_frontier_active_entries(root);
                let mut active_entries = frontier_global_active_entries_view(scratch);
                active_entries.clear();
                let mut slot_idx = 0usize;
                while slot_idx < source.len() {
                    let slot = crate::invariant_some(source.slot_at(slot_idx));
                    if self.permits_event(state_index_to_usize(slot.key.entry())) {
                        active_entries.insert_key(slot.key, slot.lane_idx);
                    }
                    slot_idx += 1;
                }
                active_entries.seal()
            }
            None => self.global_active_entries(scratch),
        }
    }

    pub(in crate::endpoint::kernel) fn detach_lane_from_root_frontier(
        &mut self,
        info: LaneOfferState,
    ) {
        self.progress
            .frontier_state
            .detach_lane_from_root_frontier(info);
    }

    pub(in crate::endpoint::kernel) fn attach_lane_to_root_frontier(
        &mut self,
        info: LaneOfferState,
    ) {
        self.progress
            .frontier_state
            .attach_lane_to_root_frontier(info);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn attach_offer_entry_to_root_frontier(
        &mut self,
        key: OfferEntryKey,
        root: ScopeId,
        lane_idx: u8,
    ) {
        self.progress
            .frontier_state
            .attach_offer_entry_to_root_frontier(key, root, lane_idx);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn detach_offer_entry_from_root_frontier(
        &mut self,
        key: OfferEntryKey,
        root: ScopeId,
    ) {
        self.progress
            .frontier_state
            .detach_offer_entry_from_root_frontier(key, root);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn refresh_after_cursor_move(
        &mut self,
        refresh: CursorRefresh,
    ) {
        match refresh {
            CursorRefresh::Lane(lane) => self.refresh_lane_offer_state(lane as usize),
            CursorRefresh::AllLanes => self.sync_lane_offer_state(),
        }
    }

    pub(in crate::endpoint::kernel) fn compute_lane_offer_state(
        &self,
        lane_idx: usize,
    ) -> Option<LaneOfferState> {
        if lane_idx >= self.progress.cursor.logical_lane_count() {
            return None;
        }
        let reentry_offer = self.active_reentry_offer_for_lane(lane_idx);
        let (entry_idx, scope_id) =
            if let Some(idx) = self.progress.cursor.index_for_lane_step(lane_idx) {
                let scope_id = self.progress.cursor.node_scope_id_at(idx);
                // A completed visit restarts at its canonical entry. An old
                // arm head, in this scope or a child, is not a fresh decision.
                if let Some((reentry_scope, entry)) = reentry_offer
                    && self.selected_live_arm_for_scope(reentry_scope).is_none()
                    && (scope_id == reentry_scope
                        || self
                            .progress
                            .cursor
                            .route_scope_conflict_arm_for_scope(scope_id, reentry_scope)
                            .is_some())
                {
                    (state_index_to_usize(entry), reentry_scope)
                } else if let Some(scope_id) = self
                    .progress
                    .cursor
                    .route_scope_for_offer_node(scope_id, idx)
                {
                    // Completed choices share one fresh entry across all lanes.
                    let entry = self
                        .selected_arm_for_scope(scope_id)
                        .filter(|_| self.selected_live_arm_for_scope(scope_id).is_none())
                        .and_then(|_| self.progress.cursor.route_scope_offer_entry(scope_id))
                        .filter(|entry| !entry.is_absent());
                    (entry.map_or(idx, state_index_to_usize), scope_id)
                } else {
                    let (scope_id, entry) = reentry_offer
                        .or_else(|| self.retained_continuation_offer_for_lane(lane_idx))?;
                    (state_index_to_usize(entry), scope_id)
                }
            } else {
                let (scope_id, entry) = reentry_offer
                    .or_else(|| self.retained_continuation_offer_for_lane(lane_idx))?;
                (state_index_to_usize(entry), scope_id)
            };
        // This is a readiness cache only. Exact frame identity and current
        // source admission still select the branch in select_observed_ingress.
        let normalized = self.progress.cursor.normalize_lane_offer_entry(
            scope_id,
            entry_idx,
            |scope| self.selected_live_arm_for_scope(scope),
            || self.active_reentry_offer_for_lane(lane_idx),
        )?;
        let scope_id = normalized.scope_id();
        let entry_idx = normalized.entry_idx();
        let is_controller = self.progress.cursor.contains_route_controller(scope_id);
        let is_dynamic = self
            .progress
            .cursor
            .route_scope_resolver(scope_id)
            .is_some();
        let frontier_facts = Self::frontier_facts_at(
            &self.progress.cursor,
            scope_id,
            is_controller,
            is_dynamic,
            entry_idx,
        );
        let mut flags = 0u8;
        if is_controller {
            flags |= LaneOfferState::FLAG_CONTROLLER;
        }
        if is_dynamic {
            flags |= LaneOfferState::FLAG_DYNAMIC;
        }
        if frontier_facts.ready() {
            flags |= LaneOfferState::FLAG_INTRINSIC_READY;
        }
        let parallel_root = match Self::parallel_scope_root(&self.progress.cursor, scope_id) {
            Some(root) => root,
            None => ScopeId::none(),
        };
        Some(LaneOfferState {
            scope: scope_id,
            entry: StateIndex::from_usize(entry_idx),
            parallel_root,
            frontier: frontier_facts.frontier,
            flags,
        })
    }

    fn retained_continuation_offer_for_lane(&self, lane: usize) -> Option<(ScopeId, StateIndex)> {
        let cursor = &self.progress.cursor;
        if cursor.route_chain_bound() == 1 {
            return None;
        }
        let head = cursor.index_for_lane_step(lane)?;
        let start = cursor.retained_continuation_start(head)?;
        for index in start..cursor.local_steps_len() {
            let Some(meta) = cursor.try_recv_meta_at(index) else {
                continue;
            };
            if usize::from(meta.lane) != lane
                || !cursor.retained_roll_allows_predecessor(head, index)
            {
                continue;
            }
            let Some(scope) =
                cursor.route_scope_for_offer_node(cursor.node_scope_id_at(index), index)
            else {
                continue;
            };
            let Some(arm) = cursor.route_arm_for_index(scope, index) else {
                continue;
            };
            let role = crate::invariant_some(cursor.logical_role_at(index));
            if cursor.route_arm_view_lane_entry(scope, arm, role, meta.lane) != Some(index) {
                continue;
            }
            let conflict = cursor.event_conflict_for_index(index);
            if self
                .event_enabled(index, meta.into(), &mut |scope, view| {
                    self.selected_arm_for_recv_event(conflict, scope, view)
                })
                .is_ok()
            {
                return Some((scope, StateIndex::from_usize(index)));
            }
        }
        None
    }

    pub(super) fn active_reentry_offer_for_lane(
        &self,
        lane_idx: usize,
    ) -> Option<(ScopeId, StateIndex)> {
        if lane_idx >= self.progress.cursor.logical_lane_count() {
            return None;
        }
        let scope =
            match self
                .progress
                .decision_state
                .active_reentry_scope_for_lane(lane_idx, |scope| {
                    if !self.is_reentry_route(scope) {
                        return Ok(ReentryScopeLiveness::NotReentry);
                    }
                    let Some(arm) = self.selected_arm_for_scope(scope) else {
                        return Ok(ReentryScopeLiveness::Incomplete);
                    };
                    Ok(if self.reentrant_selected_arm_complete(scope, arm) {
                        ReentryScopeLiveness::Complete
                    } else {
                        ReentryScopeLiveness::Incomplete
                    })
                }) {
                Ok(scope) => scope?,
                Err(CursorInvariantError::INVARIANT) => crate::invariant(),
            };
        self.progress.cursor.active_reentry_offer_entry(scope)
    }
}
