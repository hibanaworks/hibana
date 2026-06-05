use super::{
    CursorRefresh, EffIndex, LocalAction, PHASE_CURSOR_NO_STATE, PhaseCursor, ResidentLaneStep,
    ResidentLaneStepError, StateIndex, state_index_to_usize,
};
impl PhaseCursor {
    /// Find the lane that has a pending step with the given label.
    ///
    /// This is the core of Phase-driven execution: we use the label→lane mask
    /// for the current phase to resolve the lane without scanning.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn find_step_for_label(&self, target_label: u8) -> Option<(usize, StateIndex)> {
        let target_code = Self::encode_current_step_label(target_label);
        let lane_set = self.current_phase_lane_set();
        let lane_limit = self.logical_lane_count();
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            if self.current_step_label_codes()[lane_idx] == target_code {
                let Some(state_idx) = self.step_state_index_at_lane(lane_idx) else {
                    debug_assert!(
                        false,
                        "current step label cache pointed at completed resident lane"
                    );
                    return None;
                };
                let node = self.machine().node(state_index_to_usize(state_idx));
                let Some(label) = (match node.action() {
                    LocalAction::Send { label, .. }
                    | LocalAction::Recv { label, .. }
                    | LocalAction::Local { label, .. } => Some(label),
                    LocalAction::Terminate => None,
                }) else {
                    debug_assert!(
                        false,
                        "current step label cache pointed at unlabeled resident step"
                    );
                    return None;
                };
                if label != target_label {
                    debug_assert!(false, "resident current step label cache out of sync");
                    return None;
                }
                return Some((lane_idx, state_idx));
            }
            next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        None
    }

    /// Get the step index at the current cursor position for a specific lane.
    pub(crate) fn step_index_at_lane(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }

        let lane_steps = self.current_phase_lane_steps(lane_idx)?;
        if !lane_steps.is_active() {
            return None;
        }

        let cursor_pos = self.lane_cursors()[lane_idx] as usize;
        let len = lane_steps.len as usize;
        if cursor_pos >= len {
            return None;
        }
        let step_idx = if lane_steps.is_contiguous() {
            (lane_steps.start as usize).saturating_add(cursor_pos)
        } else {
            self.current_phase_lane_step_at(lane_idx, cursor_pos)?
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
    pub(super) fn step_state_index_at_lane(&self, lane_idx: usize) -> Option<StateIndex> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        let state_idx = self.machine().state_for_step_index(step_idx)?;
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(
                false,
                "missing typestate index for lane step idx={}",
                step_idx
            );
            return None;
        }
        Some(state_idx)
    }

    // =========================================================================
    // =========================================================================

    fn phase_lane_eff_ordinal(
        &self,
        phase_idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        if step_idx >= self.local_steps_len() {
            return None;
        }
        let lane_steps = self.machine().phase_lane_steps(phase_idx, lane_idx)?;
        if !lane_steps.is_active() {
            return None;
        }
        if lane_steps.is_contiguous() {
            let start = lane_steps.start as usize;
            let end = start.saturating_add(lane_steps.len as usize);
            if step_idx >= start && step_idx < end {
                u16::try_from(step_idx.saturating_sub(start)).ok()
            } else {
                None
            }
        } else {
            self.machine()
                .phase_lane_step_ordinal(phase_idx, lane_idx, step_idx)
        }
    }

    pub(crate) fn resident_lane_step(
        &self,
        lane_idx: usize,
        eff_index: EffIndex,
    ) -> Result<ResidentLaneStep, ResidentLaneStepError> {
        if let Ok(step) = self.current_resident_lane_step(lane_idx, eff_index) {
            return Ok(step);
        }
        if lane_idx >= self.logical_lane_count() || lane_idx > u8::MAX as usize {
            return Err(ResidentLaneStepError);
        }
        let step_idx = self
            .machine()
            .step_for_eff_index(eff_index)
            .ok_or(ResidentLaneStepError)?;
        let mut phase_idx = 0usize;
        while self.machine().phase_min_start(phase_idx).is_some() {
            if let Some(ordinal) = self.phase_lane_eff_ordinal(phase_idx, lane_idx, step_idx) {
                let phase = u8::try_from(phase_idx).map_err(|_| ResidentLaneStepError)?;
                return Ok(ResidentLaneStep {
                    phase,
                    lane: lane_idx as u8,
                    ordinal,
                });
            }
            phase_idx = phase_idx.saturating_add(1);
        }
        Err(ResidentLaneStepError)
    }

    pub(crate) fn current_resident_lane_step(
        &self,
        lane_idx: usize,
        eff_index: EffIndex,
    ) -> Result<ResidentLaneStep, ResidentLaneStepError> {
        if lane_idx >= self.logical_lane_count() || lane_idx > u8::MAX as usize {
            return Err(ResidentLaneStepError);
        }
        let step_idx = self
            .machine()
            .step_for_eff_index(eff_index)
            .ok_or(ResidentLaneStepError)?;
        let ordinal = self
            .phase_lane_eff_ordinal(self.phase_index_usize(), lane_idx, step_idx)
            .ok_or(ResidentLaneStepError)?;
        Ok(ResidentLaneStep {
            phase: self.state().phase_index,
            lane: lane_idx as u8,
            ordinal,
        })
    }

    #[inline(always)]
    fn select_phase(&mut self, target: ResidentLaneStep) -> CursorRefresh {
        let phase_idx = target.phase as usize;
        if self.phase_index_usize() != phase_idx {
            self.state_mut().phase_index = target.phase;
            self.lane_cursors_mut().fill(0);
            self.rebuild_current_step_label_codes();
            CursorRefresh::Phase
        } else {
            CursorRefresh::Lane(target.lane)
        }
    }

    /// Position a lane at a resident step proved by `resident_lane_step`.
    pub(crate) fn set_lane_cursor_to_resident_step(
        &mut self,
        target: ResidentLaneStep,
    ) -> CursorRefresh {
        let refresh = self.select_phase(target);
        let lane_idx = target.lane as usize;
        self.lane_cursors_mut()[lane_idx] = Self::encode_index(target.ordinal as usize);
        self.refresh_current_step_label_code(lane_idx);
        refresh
    }

    /// Advance a lane past a resident step proved by `resident_lane_step`.
    pub(crate) fn advance_lane_to_resident_step(
        &mut self,
        target: ResidentLaneStep,
    ) -> CursorRefresh {
        let refresh = self.select_phase(target);
        let lane_idx = target.lane as usize;
        let next = usize::from(target.ordinal).saturating_add(1);
        if next > self.lane_cursors()[lane_idx] as usize {
            self.lane_cursors_mut()[lane_idx] = Self::encode_index(next);
            self.refresh_current_step_label_code(lane_idx);
        }
        refresh
    }

    pub(crate) fn complete_lane_phase(&mut self, lane_idx: usize) {
        if lane_idx >= self.logical_lane_count() {
            return;
        }
        let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
            return;
        };
        if !lane_steps.is_active() {
            return;
        }
        self.lane_cursors_mut()[lane_idx] = Self::encode_index(lane_steps.len as usize);
        self.refresh_current_step_label_code(lane_idx);
    }

    /// Advance to next phase without syncing the primary typestate index.
    #[inline]
    pub(crate) fn advance_phase_without_sync(&mut self) {
        let state = self.state_mut();
        state.phase_index = state.phase_index.saturating_add(1);
        self.lane_cursors_mut().fill(0);
        self.rebuild_current_step_label_codes();
    }

    pub(crate) fn sync_idx_to_phase_start(&mut self) {
        let phase_lane_set = self.current_phase_lane_set();
        if phase_lane_set.word_len() == 0 {
            return;
        }
        let Some(phase_min_start) = self.current_phase_min_start() else {
            return;
        };
        let step_idx = phase_min_start as usize;
        if step_idx >= self.local_steps_len() {
            debug_assert!(false, "phase start out of local steps range");
            return;
        }
        let Some(state_idx) = self.machine().state_for_step_index(step_idx) else {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        };
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        }
        self.state_mut().idx = state_idx.raw();
    }

    /// Check if all lanes in current phase are complete.
    pub(crate) fn is_phase_complete(&self) -> bool {
        let lane_set = self.current_phase_lane_set();
        if lane_set.is_empty() {
            return true;
        }
        let lane_limit = self.logical_lane_count();
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            let Some(lane_steps) = self.current_phase_lane_steps(lane_idx) else {
                debug_assert!(false, "resident phase lane mask missing lane steps");
                return false;
            };
            if (self.lane_cursors()[lane_idx] as usize) < lane_steps.len as usize {
                return false;
            }
            next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        true
    }
}
