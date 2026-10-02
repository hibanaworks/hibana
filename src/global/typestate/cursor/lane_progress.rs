use super::{
    CursorInvariantError, CursorRefresh, EVENT_CURSOR_STATE_NONE, EventCursor, LocalAction,
    RelocatableResidentLaneStep, ResidentLaneStep, StateIndex, state_index_to_usize,
};
impl EventCursor {
    /// Find a pending lane-head event with the given message contract.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn pending_step_for_contract(
        &self,
        target_label: u8,
        target_schema: u32,
    ) -> Option<(usize, StateIndex)> {
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
                let schema = if let Some(meta) =
                    self.try_send_meta_at(state_index_to_usize(state_idx))
                {
                    meta.payload_schema
                } else if let Some(meta) = self.try_recv_meta_at(state_index_to_usize(state_idx)) {
                    meta.payload_schema
                } else if let Some(meta) = self.try_local_meta_at(state_index_to_usize(state_idx)) {
                    meta.payload_schema
                } else {
                    crate::invariant()
                };
                if schema != target_schema {
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
        let (start, end) = self.resident_row_bounds(self.resident_row_index_usize())?;
        let step_idx = self.lane_cursors()[lane_idx] as usize;
        if step_idx == end {
            return None;
        }
        if step_idx < start
            || step_idx > end
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

    fn resident_row_bounds(&self, row_idx: usize) -> Option<(usize, usize)> {
        let start = usize::from(self.machine().resident_row_min_start(row_idx)?);
        let end = match self.machine().resident_row_min_start(row_idx + 1) {
            Some(next) => usize::from(next),
            None => self.local_steps_len(),
        };
        if start >= end || end > self.local_steps_len() {
            crate::invariant();
        }
        Some((start, end))
    }

    fn resident_lane_step_locator(
        &self,
        lane_idx: usize,
        step_idx: usize,
    ) -> Result<usize, CursorInvariantError> {
        if lane_idx >= self.logical_lane_count()
            || !self.event_lane_step_matches(step_idx, lane_idx)
        {
            return Err(CursorInvariantError::INVARIANT);
        }
        let mut row_idx = 0usize;
        while let Some((start, end)) = self.resident_row_bounds(row_idx) {
            if start <= step_idx && step_idx < end {
                return Ok(row_idx);
            }
            row_idx += 1;
        }
        Err(CursorInvariantError::INVARIANT)
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

    // Each lane stores its descriptor event index. The row end is its terminal
    // value; no ordinal/count/sparse-layout reconstruction runs on every poll.
    pub(super) fn seed_resident_lane_heads(&mut self) {
        let Some((start, end)) = self.resident_row_bounds(self.resident_row_index_usize()) else {
            if self.local_steps_len() != 0 {
                crate::invariant();
            }
            self.lane_cursors_mut().fill(0);
            return;
        };
        self.lane_cursors_mut().fill(Self::encode_index(end));
        for step in start..end {
            let Some(lane) = self.machine().event_program().local_step_lane(step) else {
                crate::invariant();
            };
            let head = &mut self.lane_cursors_mut()[usize::from(lane)];
            if usize::from(*head) == end {
                *head = Self::encode_index(step);
            }
        }
    }

    #[inline(always)]
    fn select_resident_row_for_lane(&mut self, row_idx: usize, lane: u8) -> CursorRefresh {
        if self.resident_row_index_usize() != row_idx {
            let Ok(row) = u16::try_from(row_idx) else {
                crate::invariant();
            };
            self.state_mut().resident_row_index = row;
            self.seed_resident_lane_heads();
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
        let Ok(row_idx) = self.resident_lane_step_locator(lane_idx, target.step_idx as usize)
        else {
            crate::invariant();
        };
        self.mark_local_event_done(target.step_idx as usize);
        let refresh = self.select_resident_row_for_lane(row_idx, target.lane);
        let Some((_, end)) = self.resident_row_bounds(row_idx) else {
            crate::invariant();
        };
        let mut next = usize::from(target.step_idx) + 1;
        while next < end
            && self.machine().event_program().local_step_lane(next) != Some(target.lane)
        {
            next += 1;
        }
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
        if state_idx == EVENT_CURSOR_STATE_NONE {
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
        let Ok(row_idx) = self.resident_lane_step_locator(lane_idx, target.step_idx as usize)
        else {
            crate::invariant();
        };
        let refresh = self.select_resident_row_for_lane(row_idx, target.lane);
        self.lane_cursors_mut()[lane_idx] = target.step_idx;
        self.refresh_current_step_label_code(lane_idx);
        refresh
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
