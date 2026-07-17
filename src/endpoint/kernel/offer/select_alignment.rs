use super::{CursorEndpoint, OfferEntryKey, RecvError, RecvResult, Transport};

mod candidates;
mod model;
use self::model::{
    CurrentOfferAuthority, CurrentOfferEntry, OfferAlignmentCandidateInput, ProgressSiblingPresence,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn align_cursor_to_selected_scope(
        &mut self,
    ) -> RecvResult<crate::global::const_dsl::ScopeId> {
        let node_scope = self.cursor.node_scope_id();
        if node_scope.is_none()
            && let Some(entry_idx) = self.cursor.first_pending_step_index(usize::MAX)
            && entry_idx != self.cursor.index()
        {
            self.commit_cursor_realign_index(entry_idx)
                .map_err(|_| RecvError::PhaseInvariant)?;
            self.sync_lane_offer_state();
            return self.align_cursor_to_selected_scope();
        }
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
        let node_scope = current_scope;
        let current_idx = self.cursor.index();
        let mut current_frontier_state =
            self.current_frontier_selection_state(node_scope, current_idx);
        let current_frontier = current_frontier_state.frontier;
        let current_parallel = current_frontier_state.parallel();
        let current_scope_meta =
            self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state);
        let current_scope_selected = self.selected_arm_for_scope(node_scope).is_some();
        if current_scope_selected && current_scope_meta.is_some_and(|meta| meta.is_route_entry()) {
            return Ok(node_scope);
        }
        let active_entries = self.active_frontier_entries(current_parallel);
        let current_key = OfferEntryKey::from_index(node_scope, current_idx);
        if current_key.is_some_and(|key| active_entries.contains_only(key)) {
            let Some(meta) = current_scope_meta else {
                return Ok(node_scope);
            };
            if meta.is_route_entry() && meta.has_offer_lanes() {
                return Ok(node_scope);
            }
        }
        let observed_entries = self.compose_frontier_observed_entries(active_entries);
        let reentry_ready_entry_idx =
            observed_entries.first_selectable_ready_entry_except(current_idx);
        let reentry_controller_evidence = current_frontier_state.reentry_controller_evidence();
        let progress_sibling_presence = ProgressSiblingPresence::from_observed_progress_sibling(
            self.observed_frontier_progress_sibling_exists(
                observed_entries,
                current_idx,
                current_frontier,
                reentry_controller_evidence,
            ),
        );
        let Some(current_scope_meta) = current_scope_meta else {
            return Ok(node_scope);
        };
        let current_entry = if current_scope_meta.is_route_entry() {
            if current_scope_meta.has_offer_lanes() {
                CurrentOfferEntry::RouteWithOfferLanes
            } else {
                CurrentOfferEntry::RouteWithoutOfferLanes
            }
        } else {
            CurrentOfferEntry::NonRoute
        };
        let current_authority = if current_scope_meta.is_controller() {
            CurrentOfferAuthority::Controller
        } else {
            CurrentOfferAuthority::Passive
        };
        let candidates = self.offer_alignment_candidates(
            observed_entries,
            OfferAlignmentCandidateInput {
                current_idx,
                current_entry,
                current_authority,
                progress_sibling_presence,
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
            return Ok(node_scope);
        }
        if self.current_route_arm_authorized()? {
            return Ok(node_scope);
        }
        if candidates.current_can_remain_after_alignment(current_frontier_state) {
            return Ok(node_scope);
        }
        if !current_entry.is_route_entry()
            && let Some(entry_idx) = reentry_ready_entry_idx
        {
            if entry_idx != self.cursor.index() {
                self.commit_cursor_realign_index(entry_idx)
                    .map_err(|_| RecvError::PhaseInvariant)?;
                self.sync_lane_offer_state();
                return self.align_cursor_to_selected_scope();
            }
            return Ok(node_scope);
        }
        Err(RecvError::PhaseInvariant)
    }
}
