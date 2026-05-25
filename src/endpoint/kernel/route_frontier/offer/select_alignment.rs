use super::*;

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(in crate::endpoint::kernel) fn align_cursor_to_selected_scope(&mut self) -> RecvResult<()> {
        let node_scope = self.endpoint.cursor.node_scope_id();
        let current_scope = self.endpoint.current_offer_scope_id();
        if current_scope != node_scope
            && let Some(entry_idx) = self.endpoint.route_scope_offer_entry_index(current_scope)
            && entry_idx != self.endpoint.cursor.index()
        {
            self.endpoint.set_cursor_index(entry_idx);
            self.endpoint.sync_lane_offer_state();
            return self.align_cursor_to_selected_scope();
        }
        let node_scope = self.endpoint.current_offer_scope_id();
        let current_idx = self.endpoint.cursor.index();
        let mut current_frontier_state =
            self.current_frontier_selection_state(node_scope, current_idx);
        let current_frontier = current_frontier_state.frontier;
        let current_parallel = current_frontier_state.parallel();
        let current_parallel_root = current_frontier_state.parallel_root;
        let current_scope_selected = self.endpoint.selected_arm_for_scope(node_scope).is_some();
        if current_scope_selected
            && self
                .current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
                .map(|meta| meta.is_route_entry())
                .unwrap_or(false)
        {
            return Ok(());
        }
        let use_root_observed_entries = current_parallel.is_some();
        let active_entries = self.endpoint.active_frontier_entries(current_parallel);
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
        let observation_key = RouteFrontierMachine::frontier_observation_key(
            self.endpoint,
            current_parallel_root,
            use_root_observed_entries,
        );
        let mut observed_entries = if use_root_observed_entries {
            self.endpoint
                .root_frontier_observed_entries(current_parallel_root)
        } else {
            self.endpoint.global_frontier_observed_entries()
        };
        let cached_entries = self.endpoint.cached_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
        );
        if cached_entries.is_none() && observed_entries.len() != 0 {
            RouteFrontierMachine::refresh_frontier_observation_cache(
                self.endpoint,
                current_parallel_root,
                use_root_observed_entries,
            );
            observed_entries = if use_root_observed_entries {
                self.endpoint
                    .root_frontier_observed_entries(current_parallel_root)
            } else {
                self.endpoint.global_frontier_observed_entries()
            };
        }
        let reentry_ready_entry_idx =
            self.endpoint
                .observed_reentry_entry_idx(observed_entries, current_idx, true);
        let loop_controller_without_evidence =
            current_frontier_state.loop_controller_without_evidence();
        let progress_sibling_exists = if current_parallel_root.is_none() {
            self.endpoint.global_frontier_progress_sibling_exists(
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        } else {
            self.endpoint.root_frontier_progress_sibling_exists(
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
        let mut selectable_mask = 0u8;
        let mut slot_idx = 0usize;
        let observed_len = observed_entries.len();
        while slot_idx < observed_len {
            let slot_bit = 1u8 << slot_idx;
            if let Some(entry_idx) = observed_entries.first_entry_idx(slot_bit)
                && self.entry_has_route_scope(entry_idx)
            {
                selectable_mask |= slot_bit;
            }
            slot_idx += 1;
        }
        let observed_mask = observed_entries.occupancy_mask() & selectable_mask;
        let ready_mask = observed_entries.ready_mask & observed_mask;
        let ready_arm_mask = observed_entries.ready_arm_mask & observed_mask;
        let controller_mask = observed_entries.controller_mask & observed_mask;
        let dynamic_controller_mask = observed_entries.dynamic_controller_mask & observed_mask;
        let current_entry_bit = observed_entries.entry_bit(current_idx);
        let progress_mask = if current_is_route_entry {
            observed_entries.progress_mask & observed_mask
        } else {
            (observed_entries.progress_mask & observed_mask) & !current_entry_bit
        };
        if current_entry_bit != 0 {
            current_frontier_state.ready |= (current_entry_bit & ready_mask) != 0;
            current_frontier_state.has_progress_evidence |=
                (current_entry_bit & progress_mask) != 0;
        }
        let current_matches_candidate = current_entry_bit != 0 && current_is_route_entry;
        let mut current_has_evidence = (current_entry_bit & observed_entries.progress_mask) != 0;
        let suppress_current_controller_without_evidence = current_is_controller
            && current_matches_candidate
            && (current_entry_bit & observed_entries.ready_arm_mask) == 0
            && (current_entry_bit & observed_entries.progress_mask) == 0
            && progress_sibling_exists;
        let controller_progress_sibling_exists =
            (progress_mask & controller_mask & !current_entry_bit) != 0;
        let mut static_controller_ready_mask = observed_mask & !controller_mask;
        static_controller_ready_mask |= current_entry_bit & controller_mask;
        static_controller_ready_mask |= progress_mask & controller_mask;
        if suppress_current_controller_without_evidence {
            static_controller_ready_mask &= !current_entry_bit;
        }
        let current_entry_unrunnable = current_is_route_entry && !current_has_offer_lanes;
        let mut candidate_mask = progress_mask;
        if current_matches_candidate {
            candidate_mask |= current_entry_bit;
        }
        if current_entry_unrunnable {
            candidate_mask |= observed_mask & !current_entry_bit;
        }
        candidate_mask &= static_controller_ready_mask;
        let hinted_mask = candidate_mask & ready_arm_mask;
        let hinted_count = hinted_mask.count_ones() as usize;
        let hint_filter_mask = if hinted_count == 1 { hinted_mask } else { 0 };
        let hint_filter = observed_entries.first_entry_idx(hint_filter_mask);
        let candidate_mask = if hint_filter_mask != 0 {
            hinted_mask
        } else {
            candidate_mask
        };
        let candidate_controller_mask = candidate_mask & controller_mask;
        let candidate_dynamic_controller_mask = candidate_controller_mask & dynamic_controller_mask;
        let candidate_count = candidate_mask.count_ones() as usize;
        let controller_count = candidate_controller_mask.count_ones() as usize;
        let dynamic_controller_count = candidate_dynamic_controller_mask.count_ones() as usize;
        let candidate_idx = observed_entries.first_entry_idx(candidate_mask);
        let controller_idx = observed_entries.first_entry_idx(candidate_controller_mask);
        let dynamic_controller_idx =
            observed_entries.first_entry_idx(candidate_dynamic_controller_mask);
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
            if entry_idx != self.endpoint.cursor.index() {
                self.endpoint.set_cursor_index(entry_idx);
                self.endpoint.sync_lane_offer_state();
                return self.align_cursor_to_selected_scope();
            }
            return Ok(());
        }
        if self.endpoint.current_route_arm_authorized()?.is_some() {
            return Ok(());
        }
        if current_has_offer_lanes
            && (current_is_route_entry
                || current_frontier_state.ready
                || current_frontier_state.has_progress_evidence)
        {
            return Ok(());
        }
        if !current_is_route_entry {
            if let Some(entry_idx) = reentry_ready_entry_idx {
                if entry_idx != self.endpoint.cursor.index() {
                    self.endpoint.set_cursor_index(entry_idx);
                    self.endpoint.sync_lane_offer_state();
                    return self.align_cursor_to_selected_scope();
                }
                return Ok(());
            }
        }
        Err(RecvError::PhaseInvariant)
    }
}
