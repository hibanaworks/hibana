use super::{
    CursorRefresh, EVENT_CURSOR_NO_STATE, EventCursor, LocalAction, RelocatableResidentLaneStep,
    ResidentLaneStep, ResidentLaneStepError, StateIndex, state_index_to_usize,
};
impl EventCursor {
    /// Find a pending lane-head event with the given label.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn pending_step_for_label(&self, target_label: u8) -> Option<(usize, StateIndex)> {
        let target_code = Self::encode_current_step_label(target_label);
        let lane_limit = self.logical_lane_count();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if self.current_step_label_codes()[lane_idx] == target_code {
                let Some(state_idx) = self.step_state_index_at_lane(lane_idx) else {
                    crate::invariant();
                };
                let node = self.machine().node(state_index_to_usize(state_idx));
                let Some(label) = (match node.action() {
                    LocalAction::Send { label, .. }
                    | LocalAction::Recv { label, .. }
                    | LocalAction::Local { label, .. } => Some(label),
                    LocalAction::Terminate => None,
                }) else {
                    crate::invariant();
                };
                if label != target_label {
                    crate::invariant();
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

        let lane_steps = self.current_resident_row_lane_steps(lane_idx)?;
        if !lane_steps.is_active() {
            return None;
        }

        let cursor_pos = self.lane_cursors()[lane_idx] as usize;
        let len = lane_steps.len as usize;
        if cursor_pos >= len {
            return None;
        }
        let step_idx = if lane_steps.is_contiguous() {
            (lane_steps.start as usize).checked_add(cursor_pos)?
        } else {
            self.current_resident_row_lane_step_at(lane_idx, cursor_pos)?
        };
        if step_idx >= self.local_steps_len() {
            return None;
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

    pub(crate) fn first_pending_step_index(&self, preferred_lane_idx: usize) -> Option<usize> {
        if let Some(idx) = self.index_for_lane_step(preferred_lane_idx) {
            return Some(idx);
        }
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
        if state_idx == EVENT_CURSOR_NO_STATE {
            crate::invariant();
        }
        Some(state_idx)
    }

    // =========================================================================
    // =========================================================================

    fn resident_row_lane_ordinal(
        &self,
        row_idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        if step_idx >= self.local_steps_len() {
            return None;
        }
        let lane_steps = self.machine().resident_row_lane_steps(row_idx, lane_idx)?;
        if !lane_steps.is_active() {
            return None;
        }
        if lane_steps.is_contiguous() {
            let start = lane_steps.start as usize;
            let end = start.checked_add(lane_steps.len as usize)?;
            if step_idx >= start && step_idx < end {
                u16::try_from(step_idx.checked_sub(start)?).ok()
            } else {
                None
            }
        } else {
            self.machine()
                .resident_row_lane_step_ordinal(row_idx, lane_idx, step_idx)
        }
    }

    fn resident_lane_step_locator(
        &self,
        lane_idx: usize,
        step_idx: usize,
    ) -> Result<(usize, u16), ResidentLaneStepError> {
        if lane_idx >= self.logical_lane_count() || step_idx >= self.local_steps_len() {
            return Err(ResidentLaneStepError);
        }
        let mut row_idx = 0usize;
        while self.machine().resident_row_min_start(row_idx).is_some() {
            if let Some(ordinal) = self.resident_row_lane_ordinal(row_idx, lane_idx, step_idx) {
                return Ok((row_idx, ordinal));
            }
            row_idx += 1;
        }
        Err(ResidentLaneStepError)
    }

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
            Some(state_idx) => state_idx != EVENT_CURSOR_NO_STATE,
            None => false,
        }
    }

    pub(crate) fn relocatable_resident_lane_step_at_index(
        &self,
        idx: usize,
        lane_idx: usize,
    ) -> Result<RelocatableResidentLaneStep, ResidentLaneStepError> {
        if lane_idx >= self.logical_lane_count() || lane_idx > u8::MAX as usize {
            return Err(ResidentLaneStepError);
        }
        let target_state = StateIndex::from_usize(idx);
        let mut step_idx = 0usize;
        while step_idx < self.local_steps_len() {
            if self.machine().state_for_step_index(step_idx) == Some(target_state) {
                if !self.event_lane_step_matches(step_idx, lane_idx) {
                    return Err(ResidentLaneStepError);
                }
                let step_idx_u16 = u16::try_from(step_idx).map_err(|_| ResidentLaneStepError)?;
                return Ok(RelocatableResidentLaneStep(ResidentLaneStep {
                    step_idx: step_idx_u16,
                    lane: lane_idx as u8,
                }));
            }
            step_idx += 1;
        }
        Err(ResidentLaneStepError)
    }

    #[inline(always)]
    fn select_resident_row_for_lane(&mut self, row_idx: usize, lane: u8) -> CursorRefresh {
        if self.resident_row_index_usize() != row_idx {
            let Ok(row) = u8::try_from(row_idx) else {
                crate::invariant();
            };
            self.state_mut().resident_row_index = row;
            self.lane_cursors_mut().fill(0);
            self.rebuild_current_step_label_codes();
            CursorRefresh::AllLanes
        } else {
            CursorRefresh::Lane(lane)
        }
    }

    /// Advance a lane past a resident step that may require resident-row relocation.
    pub(crate) fn advance_lane_to_relocatable_step(
        &mut self,
        target: RelocatableResidentLaneStep,
    ) -> CursorRefresh {
        let target = target.0;
        let lane_idx = target.lane as usize;
        let Ok((row_idx, ordinal)) =
            self.resident_lane_step_locator(lane_idx, target.step_idx as usize)
        else {
            crate::invariant();
        };
        self.mark_local_event_done(target.step_idx as usize);
        let refresh = self.select_resident_row_for_lane(row_idx, target.lane);
        let next = usize::from(ordinal) + 1;
        if next > self.lane_cursors()[lane_idx] as usize {
            self.lane_cursors_mut()[lane_idx] = Self::encode_index(next);
            self.refresh_current_step_label_code(lane_idx);
        }
        refresh
    }

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
        if state_idx == EVENT_CURSOR_NO_STATE {
            return None;
        }
        Some(state_index_to_usize(state_idx))
    }

    /// Position a lane at a resident step that may require resident-row relocation.
    pub(crate) fn set_lane_cursor_to_relocatable_step(
        &mut self,
        target: RelocatableResidentLaneStep,
    ) -> CursorRefresh {
        let target = target.0;
        let lane_idx = target.lane as usize;
        let Ok((row_idx, ordinal)) =
            self.resident_lane_step_locator(lane_idx, target.step_idx as usize)
        else {
            crate::invariant();
        };
        let refresh = self.select_resident_row_for_lane(row_idx, target.lane);
        self.lane_cursors_mut()[lane_idx] = Self::encode_index(ordinal as usize);
        self.refresh_current_step_label_code(lane_idx);
        refresh
    }
}
