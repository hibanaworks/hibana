use super::{
    Clock, CursorEndpoint, EndpointSlot, EpochTable, FrontierObservationDomain,
    FrontierObservationKey, LabelUniverse, MintConfigMarker, ScopeId, Transport,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_scope(
        &mut self,
        scope_id: ScopeId,
    ) {
        let global_active_entries = self.global_active_entries();
        let mut active_entries = global_active_entries.occupancy_mask();
        let mut frontier_scratch = self.frontier_scratch_view();
        let roots = frontier_scratch.root_scopes_mut();
        roots.fill(ScopeId::none());
        let mut root_len = 0usize;
        let mut matches_scope = false;
        while let Some(slot_idx) =
            CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::next_slot_in_mask(
                &mut active_entries,
            )
        {
            let Some(entry_idx) = global_active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state_snapshot(entry_idx) else {
                continue;
            };
            if !self.offer_entry_has_active_lanes(entry_idx)
                || self.offer_entry_scope_id(entry_idx, entry_state) != scope_id
            {
                continue;
            }
            matches_scope = true;
            let Some(parallel_root) =
                self.offer_entry_parallel_root_from_state(entry_idx, entry_state)
            else {
                continue;
            };
            let mut seen_root = false;
            let mut idx = 0usize;
            while idx < root_len {
                if roots[idx] == parallel_root {
                    seen_root = true;
                    break;
                }
                idx += 1;
            }
            if !seen_root && root_len < roots.len() {
                roots[root_len] = parallel_root;
                root_len += 1;
            }
        }
        if !matches_scope {
            return;
        }
        self.refresh_cached_frontier_observation_scope_entries(
            FrontierObservationDomain::global(),
            scope_id,
        );
        let mut idx = 0usize;
        while idx < root_len {
            self.refresh_cached_frontier_observation_scope_entries(
                FrontierObservationDomain::root(roots[idx]),
                scope_id,
            );
            idx += 1;
        }
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        self.refresh_cached_frontier_observation_binding_lane_entries(
            FrontierObservationDomain::global(),
            lane_idx,
            previous_nonempty,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.frontier_state.root_frontier_len() {
            let root = self.frontier_state.root_frontier_state[slot_idx].root;
            if Self::frontier_observation_offer_lane_entry_slot_masks(
                self,
                FrontierObservationDomain::root(root),
            )[lane_idx]
                != 0
            {
                self.refresh_cached_frontier_observation_binding_lane_entries(
                    FrontierObservationDomain::root(root),
                    lane_idx,
                    previous_nonempty,
                );
            }
            slot_idx += 1;
        }
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        if lane_idx >= self.cursor.logical_lane_count() {
            return;
        }
        self.refresh_cached_frontier_observation_route_lane_entries(
            FrontierObservationDomain::global(),
            lane_idx,
            previous_change_epoch,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.frontier_state.root_frontier_len() {
            let root = self.frontier_state.root_frontier_state[slot_idx].root;
            self.refresh_cached_frontier_observation_route_lane_entries(
                FrontierObservationDomain::root(root),
                lane_idx,
                previous_change_epoch,
            );
            slot_idx += 1;
        }
    }

    pub(in crate::endpoint::kernel) fn cached_frontier_changed_entry_slot_mask(
        &mut self,
        domain: FrontierObservationDomain,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        if cached_key == FrontierObservationKey::EMPTY
            || !cached_key.entries_equal(&observation_key)
        {
            return None;
        }
        let mut changed_slot_mask = 0u8;
        let slot_len = observation_key.len();
        let mut slot_idx = 0usize;
        while slot_idx < slot_len {
            if cached_key.slot(slot_idx) != observation_key.slot(slot_idx) {
                changed_slot_mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        let slot_masks = Self::frontier_observation_offer_lane_entry_slot_masks(self, domain);
        let lane_limit = self.cursor.logical_lane_count();
        let mut mark_changed_lane = |lane_idx: usize| {
            if cached_key.offer_lanes().contains(lane_idx)
                != observation_key.offer_lanes().contains(lane_idx)
                || cached_key.binding_nonempty_lanes().contains(lane_idx)
                    != observation_key.binding_nonempty_lanes().contains(lane_idx)
            {
                changed_slot_mask |= slot_masks[lane_idx];
            }
        };
        Self::for_each_set_lane(cached_key.offer_lanes(), lane_limit, &mut mark_changed_lane);
        Self::for_each_set_lane(
            observation_key.offer_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Self::for_each_set_lane(
            cached_key.binding_nonempty_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Self::for_each_set_lane(
            observation_key.binding_nonempty_lanes(),
            lane_limit,
            &mut mark_changed_lane,
        );
        Some(changed_slot_mask)
    }
}
