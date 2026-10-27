use super::{
    CursorInvariantError, CursorRefresh, EVENT_CURSOR_STATE_NONE, EventCursor,
    RelocatableResidentLaneStep, ResidentLaneStep, StateIndex, state_index_to_usize,
};
impl EventCursor {
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

    #[inline]
    pub(crate) fn lane_has_pending_step(&self, lane_idx: usize) -> bool {
        self.index_for_lane_step(lane_idx).is_some()
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

    /// Complete an admitted event and advance only its lane's lookup position.
    pub(crate) fn advance_lane_to_relocatable_step(
        &mut self,
        target: RelocatableResidentLaneStep,
    ) -> CursorRefresh {
        let target = target.0;
        let lane_idx = target.lane as usize;
        if lane_idx >= self.logical_lane_count()
            || !self.event_lane_step_matches(usize::from(target.step_idx), lane_idx)
        {
            crate::invariant();
        }
        self.mark_local_event_done(target.step_idx as usize);
        // An enclosing continuation can arrive after this lane has begun its
        // next body visit. Consuming it must not park the lane past that fresh
        // visit's unfinished head. Locators remain caches of current progress.
        if let Some(head) = self.step_index_at_lane(lane_idx)
            && head < usize::from(target.step_idx)
            && !self.local_event_done(head)
            && self.retained_roll_allows_predecessor(head, usize::from(target.step_idx))
        {
            self.set_index(head);
            return CursorRefresh::Lane(target.lane);
        }
        let next = self
            .machine()
            .event_program()
            .next_lane_step(usize::from(target.step_idx));
        if next > self.lane_cursors()[lane_idx] as usize {
            self.lane_cursors_mut()[lane_idx] = Self::encode_index(next);
            self.refresh_current_step_label_code(lane_idx);
        }
        CursorRefresh::Lane(target.lane)
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
    ) -> CursorRefresh {
        let target = target.0;
        let lane_idx = target.lane as usize;
        if lane_idx >= self.logical_lane_count()
            || !self.event_lane_step_matches(usize::from(target.step_idx), lane_idx)
        {
            crate::invariant();
        }
        self.lane_cursors_mut()[lane_idx] = target.step_idx;
        self.refresh_current_step_label_code(lane_idx);
        CursorRefresh::Lane(target.lane)
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
