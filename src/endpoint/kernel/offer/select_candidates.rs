use super::{
    FrontierVisitSet, OfferScopeSelection, OperationKernel, RecvError, RecvResult, ScopeId,
    Transport,
};
use crate::{
    endpoint::kernel::frontier::checked_state_index,
    global::{
        event_program::LocalEventRow,
        role_program::{LANE_SET_VIEW_WORDS, LaneWord, RoleLocalDirection, lane_word_index},
        typestate::EventArmView,
    },
};
use core::ops::ControlFlow;

pub(super) struct ProjectedOfferFrontier {
    pub(super) incoming: [LaneWord; LANE_SET_VIEW_WORDS],
    pub(super) candidate: Option<OfferScopeSelection>,
}

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    /// Read candidates and incoming directions from the same progress cut.
    /// The result contains locators, never admission that survives callbacks.
    pub(in crate::endpoint::kernel::offer) fn read_projected_offer_frontier(
        &self,
        visited: &FrontierVisitSet,
        only_lane: Option<u8>,
    ) -> RecvResult<ProjectedOfferFrontier> {
        let mut selected = None;
        let mut incoming = [0; LANE_SET_VIEW_WORDS];
        let mut outcome = Ok(());
        let mut previews = self.preview_queries();
        self.for_each_projected_offer_step(
            only_lane,
            None,
            &mut previews,
            |row, lane, previews| {
                if outcome.is_ok() {
                    let (word, bit) = lane_word_index(usize::from(lane));
                    // One enabled receive establishes this transport direction.
                    // A later candidate still needs its own admission if it wins.
                    let receive_enabled = (incoming[word] & bit == 0
                        && row.direction() == RoleLocalDirection::Recv)
                        .then(|| self.offer_row_is_enabled(row, previews));
                    if receive_enabled == Some(true) {
                        incoming[word] |= bit;
                    }
                    outcome = self.consider_offer_candidate(
                        row,
                        lane,
                        visited,
                        receive_enabled,
                        previews,
                        &mut selected,
                    );
                }
                ControlFlow::Continue(())
            },
        );
        outcome?;
        Ok(ProjectedOfferFrontier {
            incoming,
            candidate: selected.map(|(_, candidate)| candidate),
        })
    }

    /// Discover only receive directions while an already selected scope waits.
    /// The first enabled receive proves that its lane must remain polled.
    /// No branch is selected or admitted by this read.
    pub(super) fn read_projected_offer_incoming(
        &self,
        inspected_lanes: &[LaneWord; LANE_SET_VIEW_WORDS],
    ) -> [LaneWord; LANE_SET_VIEW_WORDS] {
        let mut incoming = [0; LANE_SET_VIEW_WORDS];
        let borrow_mask = (!self.region.is_whole()).then(|| self.region.mask());
        let lanes = self.progress.cursor.offer_candidate_lanes(borrow_mask);
        if (0..lanes.word_len()).all(|word| lanes.word_at(word) & !inspected_lanes[word] == 0) {
            return incoming;
        }
        let mut previews = self.preview_queries();
        self.for_each_projected_offer_step(
            None,
            Some(inspected_lanes),
            &mut previews,
            |row, lane, previews| {
                if row.direction() == RoleLocalDirection::Recv
                    && self.offer_row_is_enabled(row, previews)
                {
                    let (word, bit) = lane_word_index(usize::from(lane));
                    incoming[word] |= bit;
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        incoming
    }

    #[inline(never)]
    pub(super) fn recheck_projected_offer_candidate(
        &self,
        candidate: OfferScopeSelection,
        visited: &FrontierVisitSet,
    ) -> RecvResult<Option<OfferScopeSelection>> {
        let mut previews = self.preview_queries();
        let index = candidate.candidate_index.as_usize();
        let Some(row) = self.projected_offer_row(index, candidate.offer_lane) else {
            return Ok(None);
        };
        let mut selected = None;
        self.consider_offer_candidate(
            row,
            candidate.offer_lane,
            visited,
            None,
            &mut previews,
            &mut selected,
        )?;
        Ok(selected.map(|(_, candidate)| candidate))
    }

    fn projected_offer_row(&self, index: usize, lane: u8) -> Option<LocalEventRow> {
        let row = crate::invariant_some(self.progress.cursor.event_row_at(index));
        if row.logical_role() != self.public_header.role()
            || self.progress.cursor.event_lane_at(index) != Some(lane)
        {
            crate::invariant();
        }
        self.region
            .contains(row.effect().dense_ordinal())
            .then_some(row)
    }

    /// The common row visitor has checked the view, lane and borrow region.
    /// Only live progress and choices remain to be evaluated.
    fn offer_row_is_enabled(
        &self,
        row: LocalEventRow,
        previews: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let conflict = row.conflict();
        self.event_eligibility(row, &mut |scope, view| match view {
            EventArmView::Committed => self.selected_arm_for_scope(scope),
            EventArmView::Preview => {
                previews(scope).or_else(|| self.selected_arm_for_recv_event(conflict, scope, view))
            }
        })
        .is_ok()
    }

    fn for_each_projected_offer_step(
        &self,
        only_lane: Option<u8>,
        inspected_lanes: Option<&[LaneWord; LANE_SET_VIEW_WORDS]>,
        previews: &mut dyn FnMut(ScopeId) -> Option<u8>,
        mut visit: impl FnMut(
            LocalEventRow,
            u8,
            &mut dyn FnMut(ScopeId) -> Option<u8>,
        ) -> ControlFlow<()>,
    ) {
        let mut yield_entry = |mut index, lane, previews: &mut dyn FnMut(ScopeId) -> Option<u8>| {
            loop {
                if let Some(row) = self.projected_offer_row(index, lane) {
                    visit(row, lane, previews)?;
                }
                let Some(next) = self
                    .progress
                    .cursor
                    .next_offer_lane_alternative(index, previews)
                else {
                    return ControlFlow::Continue(());
                };
                index = next;
            }
        };
        let borrow_mask = (!self.region.is_whole()).then(|| self.region.mask());
        let lanes = self.progress.cursor.offer_candidate_lanes(borrow_mask);
        let lane_limit = self.progress.cursor.logical_lane_count();
        let mut start = only_lane.map_or(0, usize::from);
        'lanes: while let Some(lane) = lanes.next_set_from(start, lane_limit) {
            if only_lane.is_some_and(|target| usize::from(target) != lane) {
                break;
            }
            start = if only_lane.is_some() {
                lane_limit
            } else {
                lane + 1
            };
            let (word, bit) = lane_word_index(lane);
            if inspected_lanes.is_some_and(|lanes| lanes[word] & bit != 0) {
                continue;
            }
            if !self.owns_lane(lane) {
                continue;
            }
            let head = self.progress.cursor.index_for_lane_step(lane);
            if let Some(index) = head {
                if let Some(candidate) = self.progress.cursor.offer_lane_head(
                    self.public_header.role(),
                    lane,
                    &mut |scope| self.selected_arm_for_scope(scope),
                ) && yield_entry(candidate, lane as u8, previews).is_break()
                {
                    continue;
                }
                for index in self.progress.cursor.retained_offer_lane_candidates(
                    index,
                    self.public_header.role(),
                    lane as u8,
                    &mut |scope| self.selected_arm_for_scope(scope),
                ) {
                    if yield_entry(index, lane as u8, previews).is_break() {
                        continue 'lanes;
                    }
                }
            }
            if let Some((_, entry)) = self.active_reentry_offer_for_lane(lane) {
                let index = entry.as_usize();
                if self.progress.cursor.logical_role_at(index) == Some(self.public_header.role())
                    && self.progress.cursor.event_lane_at(index) == Some(lane as u8)
                {
                    let _ = yield_entry(index, lane as u8, previews);
                } else if let Some(index) =
                    self.foreign_reentry_offer_seed(index, lane as u8, previews)
                {
                    let _ = yield_entry(index, lane as u8, previews);
                }
            }
        }
    }

    // A reentry scope starts at an actor-wide row, which can belong to another
    // view or lane. Resolve this seed once; subsequent same-key alternatives
    // follow the compiled links exclusively.
    fn foreign_reentry_offer_seed(
        &self,
        index: usize,
        lane: u8,
        previews: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut next = None;
        self.progress
            .cursor
            .visit_route_arms_for_index(index, |scope, _| {
                if previews(scope).is_some() {
                    return;
                }
                for arm in 0..2 {
                    if let Some(entry) = self.progress.cursor.route_arm_view_lane_entry(
                        scope,
                        arm,
                        self.public_header.role(),
                        lane,
                    ) && entry > index
                        && next.is_none_or(|current| entry < current)
                    {
                        next = Some(entry);
                    }
                }
            });
        next
    }

    fn consider_offer_candidate(
        &self,
        row: LocalEventRow,
        lane: u8,
        visited: &FrontierVisitSet,
        receive_enabled: Option<bool>,
        previews: &mut dyn FnMut(ScopeId) -> Option<u8>,
        selected: &mut Option<(u8, OfferScopeSelection)>,
    ) -> RecvResult<()> {
        let index = row.index();
        let Some(scope) = self
            .progress
            .cursor
            .route_scope_for_offer_node(self.offer_scope_id_at(index, previews), index)
        else {
            return Ok(());
        };
        let live_arm = previews(scope);
        let entry = if live_arm.is_none() {
            match self.progress.cursor.route_scope_offer_entry(scope) {
                Some(entry) if !entry.is_absent() => entry.as_usize(),
                Some(_) => index,
                None => return Ok(()),
            }
        } else {
            index
        };
        if visited.contains(entry)
            || !self
                .progress
                .cursor
                .route_offer_entry_allows_current(scope, entry, live_arm)
            || !self
                .offer_lane_set_for_scope(scope)
                .contains(usize::from(lane))
        {
            return Ok(());
        }
        let controller = self.is_route_controller(scope);
        let dynamic = self.progress.cursor.route_scope_resolver(scope).is_some();
        let priority = if self.scope_has_ready_arm_evidence(scope) {
            4
        } else if controller && dynamic {
            3
        } else if controller {
            2
        } else {
            1
        };
        if selected.is_some_and(|(prior_priority, prior)| {
            priority < prior_priority
                || (priority == prior_priority && prior.entry_index.as_usize() <= entry)
        }) {
            return Ok(());
        }
        let candidate_row = if controller && dynamic && live_arm.is_none() {
            let Some(entry) = self.resolver_offer_entry(scope, previews) else {
                return Ok(());
            };
            entry
        } else {
            if !receive_enabled.unwrap_or_else(|| self.offer_row_is_enabled(row, previews)) {
                return Ok(());
            }
            row
        };
        let mut candidate = self.offer_scope_selection_for_scope_lane_with_selected_arm(
            scope,
            entry,
            candidate_row.atom().lane,
            live_arm,
        )?;
        candidate.candidate_index =
            checked_state_index(candidate_row.index()).ok_or(RecvError::PhaseInvariant)?;
        *selected = Some((priority, candidate));
        Ok(())
    }
}
