use super::{
    CursorInvariantError, EVENT_CURSOR_STATE_NONE, EventCursor, RelocatableResidentLaneStep,
    ResidentLaneStep, ScopeId, StateIndex, state_index_to_usize,
};
impl EventCursor {
    pub(crate) fn next_offer_lane_alternative(
        &self,
        index: usize,
        preview: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        self.machine()
            .event_program()
            .next_offer_lane_alternative(index, preview)
    }

    /// Find a pending lane-head event with the given message contract.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn pending_step_for_contract(
        &self,
        role: u8,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
    ) -> Option<(usize, StateIndex)> {
        let target_code = Self::encode_current_step_label(target_label);
        let lane_limit = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if self.current_step_label_codes()[lane_idx] == target_code {
                let Some(state_idx) = self.step_state_index_at_lane(lane_idx) else {
                    crate::invariant();
                };
                let row = crate::invariant_some(self.event_row_at(state_index_to_usize(state_idx)));
                if row.logical_role() != role {
                    lane_idx += 1;
                    continue;
                }
                if row.atom().label != target_label {
                    crate::invariant();
                }
                if row.atom().payload != target_payload {
                    lane_idx += 1;
                    continue;
                }
                return Some((lane_idx, state_idx));
            }
            lane_idx += 1;
        }
        None
    }

    /// Get the step index at the current cursor position for a specific lane.
    pub(crate) fn step_index_at_lane(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        let end = self.local_steps_len();
        let step_idx = self.lane_cursors()[lane_idx] as usize;
        if step_idx == end {
            return None;
        }
        if step_idx > end
            || self.machine().event_program().local_step_lane(step_idx) != Some(lane_idx as u8)
        {
            crate::invariant();
        }
        Some(step_idx)
    }

    pub(crate) fn index_for_lane_step(&self, lane_idx: usize) -> Option<usize> {
        let state_idx = self.step_state_index_at_lane(lane_idx)?;
        Some(state_index_to_usize(state_idx))
    }

    pub(crate) fn first_pending_step_index(&self) -> Option<usize> {
        let lane_limit = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if let Some(idx) = self.index_for_lane_step(lane_idx) {
                return Some(idx);
            }
            lane_idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn step_state_index_at_lane(&self, lane_idx: usize) -> Option<StateIndex> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        let state_idx = self.machine().state_for_step_index(step_idx)?;
        if state_idx == EVENT_CURSOR_STATE_NONE {
            crate::invariant();
        }
        Some(state_idx)
    }

    // =========================================================================
    // =========================================================================

    fn event_lane_step_matches(&self, step_idx: usize, lane_idx: usize) -> bool {
        if lane_idx > u8::MAX as usize || step_idx >= self.local_steps_len() {
            return false;
        }
        if self
            .machine()
            .event_program()
            .local_step_lane(step_idx)
            .is_none_or(|lane| lane as usize != lane_idx)
        {
            return false;
        }
        match self.machine().state_for_step_index(step_idx) {
            Some(state_idx) => state_idx != EVENT_CURSOR_STATE_NONE,
            None => false,
        }
    }

    pub(crate) fn relocatable_resident_lane_step_at_index(
        &self,
        idx: usize,
        lane_idx: usize,
    ) -> Result<RelocatableResidentLaneStep, CursorInvariantError> {
        if lane_idx >= self.logical_lane_count()
            || lane_idx > u8::MAX as usize
            || !self.event_lane_step_matches(idx, lane_idx)
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        let step_idx = u16::try_from(idx).map_err(|_| CursorInvariantError::INVARIANT)?;
        Ok(RelocatableResidentLaneStep(ResidentLaneStep {
            step_idx,
            lane: lane_idx as u8,
        }))
    }

    // Seed each lane once across the whole image. Subsequent movement follows
    // the compiled same-lane successor and never reconstructs another lane.
    pub(super) fn seed_lane_heads(&mut self) {
        let end = self.local_steps_len();
        self.lane_cursors_mut().fill(Self::encode_index(end));
        for step in 0..end {
            let Some(lane) = self.machine().event_program().local_step_lane(step) else {
                crate::invariant();
            };
            let head = &mut self.lane_cursors_mut()[usize::from(lane)];
            if usize::from(*head) == end {
                *head = Self::encode_index(step);
            }
        }
    }

    /// Complete an admitted event and advance past this lane's completed prefix.
    pub(crate) fn advance_lane_to_relocatable_step(&mut self, target: RelocatableResidentLaneStep) {
        let target = target.0;
        let lane_idx = target.lane as usize;
        if lane_idx >= self.logical_lane_count()
            || !self.event_lane_step_matches(usize::from(target.step_idx), lane_idx)
        {
            crate::invariant();
        }
        self.mark_local_event_done(target.step_idx as usize);
        let previous = usize::from(self.lane_cursors()[lane_idx]);
        let mut next = previous;
        let end = self.local_steps_len();
        // Admission may complete a later independent event or an old receive
        // continuation. Neither consumes this visit's earlier unfinished head.
        while next < end && self.local_event_done(next) {
            next = self.machine().event_program().next_lane_step(next);
        }
        if next != previous {
            self.lane_cursors_mut()[lane_idx] = Self::encode_index(next);
            self.refresh_current_step_label_code(lane_idx);
        }
    }

    /// The view's first potentially live unfinished candidate on this lane.
    /// Other views can occupy an earlier alternative on the physical lane;
    /// source admission, rather than this lookup, decides their dependencies.
    /// Only completion advances the stored physical head.
    pub(crate) fn offer_lane_head(
        &self,
        view: u8,
        lane: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        self.potential_offer_lane_step(view, lane as u8, self.step_index_at_lane(lane)?, selected)
    }

    fn potential_offer_lane_step(
        &self,
        view: u8,
        lane: u8,
        mut start: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        use super::scope_route::ConflictAdmission;

        let program = self.machine().event_program();
        while let Some(step) = program.view_lane_step_at_or_after(view, lane, start) {
            if self.local_event_done(step) {
                start = step + 1;
                continue;
            }
            let conflict = self.machine().event_conflict_for_index(step);
            match self.event_conflict_row_admission(conflict, ScopeId::none(), None, selected) {
                ConflictAdmission::Allowed => {}
                ConflictAdmission::Excluded { scope, arm } => {
                    let slot = crate::invariant_some(program.route_scope_slot(scope));
                    // The committed choice excludes the complete arm. Resume
                    // this view's index after it, including across other views.
                    start =
                        crate::invariant_some(program.route_arm_event_row_by_slot(slot, arm)).end();
                    continue;
                }
                ConflictAdmission::Invalid => crate::invariant(),
            }
            if self.event_actor_is_local(step, selected) != Some(false) {
                return Some(step);
            }
            start = step + 1;
        }
        None
    }

    /// A retained visit can expose its first pending incoming continuation
    /// while this lane starts a fresh body visit. Each enclosing retained roll
    /// contributes at most one candidate; ordinary eligibility still checks it.
    pub(crate) fn retained_offer_lane_candidates<'a>(
        &'a self,
        head: usize,
        view: u8,
        lane: u8,
        selected: &'a mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> impl Iterator<Item = usize> + 'a {
        self.machine()
            .event_program()
            .roll_ancestors(head)
            .filter_map(move |scope| {
                let program = self.machine().event_program();
                let slot = crate::invariant_some(program.roll_scope_slot(scope));
                let bit = self.local_steps_len() + slot;
                if self.completed_event_words()[bit / 32] & (1 << (bit % 32)) == 0 {
                    return None;
                }
                let (_, range) = crate::invariant_some(program.roll_scope_row_by_slot(slot));
                let candidate =
                    self.potential_offer_lane_step(view, lane, range.end(), selected)?;
                (self.is_recv_at(candidate)
                    && self
                        .retained_roll_predecessor_end(head, candidate)
                        .is_some())
                .then_some(candidate)
            })
    }

    #[inline(always)]
    pub(crate) fn relocatable_step_done(&self, target: RelocatableResidentLaneStep) -> bool {
        let target = target.0;
        self.local_event_done(target.step_idx as usize)
    }

    pub(crate) fn node_index_for_relocatable_step(
        &self,
        target: RelocatableResidentLaneStep,
    ) -> Option<usize> {
        let target = target.0;
        if target.step_idx as usize >= self.local_steps_len() {
            return None;
        }
        if !self.event_lane_step_matches(target.step_idx as usize, target.lane as usize) {
            return None;
        }
        let state_idx = self
            .machine()
            .state_for_step_index(target.step_idx as usize)?;
        if state_idx == EVENT_CURSOR_STATE_NONE {
            return None;
        }
        Some(state_index_to_usize(state_idx))
    }

    /// Position one lane without changing any other lane or event completion.
    pub(crate) fn set_lane_cursor_to_relocatable_step(
        &mut self,
        target: RelocatableResidentLaneStep,
    ) {
        let target = target.0;
        let lane_idx = target.lane as usize;
        if lane_idx >= self.logical_lane_count()
            || !self.event_lane_step_matches(usize::from(target.step_idx), lane_idx)
        {
            crate::invariant();
        }
        self.lane_cursors_mut()[lane_idx] = target.step_idx;
        self.refresh_current_step_label_code(lane_idx);
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
