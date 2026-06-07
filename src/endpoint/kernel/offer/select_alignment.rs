use super::{
    Clock, CursorEndpoint, EpochTable, FrontierObservationDomain, LabelUniverse, MintConfigMarker,
    RecvError, RecvResult, Transport,
};

mod candidates;
mod model;
use self::model::{CurrentOfferAuthority, CurrentOfferEntry, OfferAlignmentCandidateInput};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(in crate::endpoint::kernel) fn align_cursor_to_selected_scope(&mut self) -> RecvResult<()> {
        let node_scope = self.cursor.node_scope_id();
        let current_scope = self.current_offer_scope_id();
        if current_scope != node_scope
            && let Some(entry_idx) = self.route_scope_offer_entry_index(current_scope)
            && entry_idx != self.cursor.index()
        {
            self.commit_cursor_realign_index(entry_idx)
                .map_err(|_| RecvError::PhaseInvariant)?;
            self.sync_lane_offer_state();
            return self.align_cursor_to_selected_scope();
        }
        let node_scope = self.current_offer_scope_id();
        let current_idx = self.cursor.index();
        let mut current_frontier_state =
            self.current_frontier_selection_state(node_scope, current_idx);
        let current_frontier = current_frontier_state.frontier;
        let current_parallel = current_frontier_state.parallel();
        let current_scope_selected = self.selected_arm_for_scope(node_scope).is_some();
        if current_scope_selected
            && self
                .current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
                .map(|meta| meta.is_route_entry())
                .unwrap_or(false)
        {
            return Ok(());
        }
        let observation_domain = FrontierObservationDomain::from_parallel(current_parallel);
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
        let observation_key = Self::frontier_observation_key(self, observation_domain);
        let mut observed_entries = self.frontier_observed_entries(observation_domain);
        let cached_entries =
            self.cached_frontier_observed_entries(observation_domain, observation_key);
        if cached_entries.is_none() && observed_entries.len() != 0 {
            Self::refresh_frontier_observation_cache(self, observation_domain);
            observed_entries = self.frontier_observed_entries(observation_domain);
        }
        let reentry_ready_entry_idx =
            self.observed_reentry_entry_idx(observed_entries, current_idx, true);
        let loop_controller_without_evidence =
            current_frontier_state.loop_controller_without_evidence();
        let progress_sibling_exists = if !observation_domain.uses_root_entries() {
            self.global_frontier_progress_sibling_exists(
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        } else {
            self.root_frontier_progress_sibling_exists(
                observation_domain.root_scope(),
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
        let current_entry = CurrentOfferEntry::from_meta(
            current_scope_meta.is_route_entry(),
            current_scope_meta.has_offer_lanes(),
        );
        let current_authority =
            CurrentOfferAuthority::from_meta(current_scope_meta.is_controller());
        let candidates = self.offer_alignment_candidates(
            observed_entries,
            OfferAlignmentCandidateInput {
                current_idx,
                current_entry,
                current_authority,
                progress_sibling_exists,
            },
        );
        current_frontier_state = candidates.merge_current_observation(current_frontier_state);
        let selection = candidates.select(current_frontier_state);
        let current_entry = candidates.current_entry();
        if let Some((_priority, entry_idx)) = selection {
            if entry_idx != self.cursor.index() {
                self.commit_cursor_realign_index(entry_idx)
                    .map_err(|_| RecvError::PhaseInvariant)?;
                self.sync_lane_offer_state();
                return self.align_cursor_to_selected_scope();
            }
            return Ok(());
        }
        if self.current_route_arm_authorized()?.is_some() {
            return Ok(());
        }
        if candidates.current_can_remain_after_alignment(current_frontier_state) {
            return Ok(());
        }
        if !current_entry.is_route_entry() {
            if let Some(entry_idx) = reentry_ready_entry_idx {
                if entry_idx != self.cursor.index() {
                    self.commit_cursor_realign_index(entry_idx)
                        .map_err(|_| RecvError::PhaseInvariant)?;
                    self.sync_lane_offer_state();
                    return self.align_cursor_to_selected_scope();
                }
                return Ok(());
            }
        }
        Err(RecvError::PhaseInvariant)
    }
}
