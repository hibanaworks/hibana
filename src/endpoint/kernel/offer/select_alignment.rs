use super::{
    ControlFlow, FrontierScratchWorkspace, FrontierVisitSet, OfferEntryKey, OperationKernel,
    RecvError, RecvResult, Transport,
};

mod candidates;
mod model;
use self::model::{
    CurrentOfferAuthority, CurrentOfferEntry, CurrentOfferObservation,
    OfferAlignmentCandidateInput, OfferAlignmentDecision, ProgressSiblingPresence,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn align_cursor_to_selected_scope(
        &mut self,
        visited: &mut FrontierVisitSet,
        scratch: &mut FrontierScratchWorkspace<'_>,
    ) -> RecvResult<Option<crate::global::const_dsl::ScopeId>> {
        loop {
            let index = self.progress.cursor.index();
            let scope = self.offer_scope_id_at(index);
            let wrong_arm = self
                .preview_live_selected_arm_for_scope(scope)
                .is_some_and(|arm| {
                    !self
                        .progress
                        .cursor
                        .route_offer_entry_allows_current(scope, index, Some(arm))
                });
            // A completed parallel sibling can move the lookup position to a
            // blocked join. Select from the source-ready operations, rather
            // than treating that lookup position as the current frontier.
            let completed_choice = self.selected_arm_for_scope(scope).is_some()
                && self.selected_live_arm_for_scope(scope).is_none()
                && self.preview_live_selected_arm_for_scope(scope).is_none();
            if !self.permits_event(index)
                || wrong_arm
                || completed_choice
                || self.enabled_view_event(index).is_none()
            {
                let next = self.enabled_view_events().find_map(|(entry_idx, _)| {
                    let cursor = &self.progress.cursor;
                    let candidate = cursor
                        .route_scope_for_offer_node(self.offer_scope_id_at(entry_idx), entry_idx)?;
                    if candidate == scope && !completed_choice {
                        return None;
                    }
                    // Resolve the descendant while its ancestor's choice is live.
                    if self.selected_live_arm_for_scope(candidate).is_some()
                        && cursor
                            .route_scope_conflict_arm_for_scope(scope, candidate)
                            .is_some()
                    {
                        return None;
                    }
                    let entry = cursor.route_scope_offer_entry(candidate)?;
                    let entry_idx = if entry.is_absent() {
                        entry_idx
                    } else {
                        entry.as_usize()
                    };
                    (entry_idx != index
                        && cursor.route_offer_entry_allows_current(
                            candidate,
                            entry_idx,
                            self.preview_live_selected_arm_for_scope(candidate),
                        ))
                    .then_some((entry_idx, candidate))
                });
                if let Some((entry_idx, _)) = next {
                    if self
                        .commit_offer_alignment_realign(entry_idx, visited)?
                        .is_break()
                    {
                        return Ok(None);
                    }
                    continue;
                }
            }
            let node_scope = self.progress.cursor.node_scope_id();
            if node_scope.is_none()
                && let Some(entry_idx) = self.progress.cursor.first_pending_step_index()
                && entry_idx != self.progress.cursor.index()
            {
                if self
                    .commit_offer_alignment_realign(entry_idx, visited)?
                    .is_break()
                {
                    return Ok(None);
                }
                continue;
            }
            let current_scope = self.offer_scope_id_at(self.progress.cursor.index());
            if current_scope != node_scope
                && !self
                    .progress
                    .cursor
                    .route_offer_entry_cursor_position(
                        current_scope,
                        self.progress.cursor.index(),
                        self.preview_live_selected_arm_for_scope(current_scope),
                    )
                    .is_some_and(|position| position.is_at_entry())
                && let Some(entry_idx) = self.route_scope_offer_entry_index(current_scope)
                && entry_idx != self.progress.cursor.index()
            {
                if self
                    .commit_offer_alignment_realign(entry_idx, visited)?
                    .is_break()
                {
                    return Ok(None);
                }
                continue;
            }
            let node_scope = current_scope;
            let current_idx = self.progress.cursor.index();
            let mut current_frontier_state =
                self.current_frontier_selection_state(node_scope, current_idx);
            let current_frontier = current_frontier_state.frontier;
            let current_parallel = current_frontier_state.parallel();
            let current_scope_meta =
                self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state);
            // A send-selected controller retains its committed choice while
            // locating the following receive. Completing its first send is
            // not a new resolver visit. Completed reentry observations must
            // still yield to enabled parallel resolver work.
            let current_scope_selected = self.selected_live_arm_for_scope(node_scope).is_some()
                || (current_scope_meta.is_some_and(|meta| meta.is_controller())
                    && self
                        .progress
                        .cursor
                        .route_scope_resolver(node_scope)
                        .is_none()
                    && self.selected_arm_for_scope(node_scope).is_some());
            if current_scope_selected
                && current_scope_meta.is_some_and(|meta| meta.is_route_entry())
            {
                return Ok(Some(node_scope));
            }
            let active_entries =
                self.active_frontier_entries(current_parallel, &mut scratch.global_active_entries);
            let current_key = OfferEntryKey::from_index(node_scope, current_idx);
            if current_key.is_some_and(|key| active_entries.contains_only(key)) {
                let Some(meta) = current_scope_meta else {
                    return Ok(Some(node_scope));
                };
                if meta.is_route_entry() && meta.has_offer_lanes() {
                    return Ok(Some(node_scope));
                }
            }
            let (observed_entries, exact_current_observation) = self
                .compose_frontier_observed_entries(
                    active_entries,
                    current_key,
                    &mut scratch.observed_entries,
                );
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
                return Ok(Some(node_scope));
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
                if self
                    .progress
                    .cursor
                    .route_scope_resolver(node_scope)
                    .is_some()
                {
                    CurrentOfferAuthority::DynamicController
                } else {
                    CurrentOfferAuthority::Controller
                }
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
                    current_observation: CurrentOfferObservation::from_exact(
                        exact_current_observation,
                    ),
                },
            );
            current_frontier_state = candidates.merge_current_observation(current_frontier_state);
            let selection = candidates.select(current_frontier_state);
            let current_entry = candidates.current_entry();
            match selection {
                Some(OfferAlignmentDecision::KeepCurrent) => return Ok(Some(node_scope)),
                Some(OfferAlignmentDecision::Realign(entry_idx)) => {
                    if self
                        .commit_offer_alignment_realign(entry_idx, visited)?
                        .is_break()
                    {
                        return Ok(None);
                    }
                    continue;
                }
                None => {}
            }
            if self.current_route_arm_authorized()? {
                return Ok(Some(node_scope));
            }
            if candidates.current_can_remain_after_alignment(current_frontier_state) {
                return Ok(Some(node_scope));
            }
            if !current_entry.is_route_entry()
                && let Some(entry_idx) = reentry_ready_entry_idx
            {
                if self
                    .commit_offer_alignment_realign(entry_idx, visited)?
                    .is_break()
                {
                    return Ok(None);
                }
                continue;
            }
            return Err(RecvError::PhaseInvariant);
        }
    }

    fn commit_offer_alignment_realign(
        &mut self,
        entry_idx: usize,
        visited: &mut FrontierVisitSet,
    ) -> RecvResult<ControlFlow<()>> {
        let current_idx = self.progress.cursor.index();
        if entry_idx == current_idx {
            return Err(RecvError::PhaseInvariant);
        }
        if visited.contains(current_idx) {
            // Alignment changes only the lookup position. A cycle exhausts
            // this search; an incoming frame or actor progress can enable a
            // choice on a later poll.
            return Ok(ControlFlow::Break(()));
        }
        visited.record(current_idx);
        self.commit_cursor_realign_index(entry_idx)
            .map_err(|_| RecvError::PhaseInvariant)?;
        self.sync_lane_offer_state();
        Ok(ControlFlow::Continue(()))
    }
}
