use super::{
    CommitDelta, CursorEndpoint, EpochTable, LabelUniverse, LoopCommitDisposition, LoopCommitRow,
    LoopDecision, MintConfigMarker, PreparedCommitDelta, ResidentLaneStepError,
    SelectedRouteCommitRow, Transport, route_scope_materialization_index_from_cursor,
    state_index_to_usize,
};
use crate::global::typestate::StateIndex;

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CommitDeltaApplyPermit(());

impl CommitDeltaApplyPermit {
    #[inline(always)]
    const fn new() -> Self {
        Self(())
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[inline(always)]
pub(in crate::endpoint::kernel) const fn test_commit_delta_apply_permit() -> CommitDeltaApplyPermit
{
    CommitDeltaApplyPermit::new()
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn preflight_commit_delta(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        if let Some(event) = delta.event() {
            let idx = self
                .cursor
                .node_index_for_relocatable_step(event.progress_step())
                .ok_or(ResidentLaneStepError)?;
            let enabled = self.cursor.enabled_event_commit(
                idx,
                event.eff_index(),
                event.event_label(),
                event.event_control(),
                event.scope(),
                event.route_arm(),
                event.lane(),
                |scope| self.selected_arm_for_scope(scope),
            )?;
            if enabled.progress_step() != event.progress_step()
                || enabled.cursor_after() != delta.cursor_after()
            {
                return Err(ResidentLaneStepError);
            }
        } else if delta.selected_routes().is_empty() {
            self.preflight_cursor_realign_delta(delta)?;
        } else {
            self.preflight_route_only_cursor_after(delta)?;
        }
        self.preflight_selected_route_rows(delta)?;
        self.preflight_parent_route_evidence(delta)?;
        self.preflight_loop_row(delta.loop_row())?;
        self.preflight_lane_relocation(delta.lane_relocation())?;
        Ok(())
    }

    #[inline]
    fn preflight_cursor_realign_delta(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        let cursor_after = state_index_to_usize(delta.cursor_after());
        if cursor_after >= self.cursor.local_steps_len() {
            return Err(ResidentLaneStepError);
        }
        Ok(())
    }

    #[inline]
    fn preflight_route_only_cursor_after(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        let routes = delta.selected_routes();
        if routes.is_empty() {
            return Err(ResidentLaneStepError);
        }
        let expected_after = self
            .cursor
            .try_follow_jumps_from_index(StateIndex::from_usize(self.cursor.index()))
            .map_err(|_| ResidentLaneStepError)?;
        if expected_after == delta.cursor_after() {
            return Ok(());
        }
        let cursor_after = state_index_to_usize(delta.cursor_after());
        let mut idx = 0usize;
        while idx < routes.len() {
            let row = routes.get(idx).ok_or(ResidentLaneStepError)?;
            if route_scope_materialization_index_from_cursor(&self.cursor, row.scope())
                == Some(cursor_after)
            {
                return Ok(());
            }
            idx += 1;
        }
        Err(ResidentLaneStepError)
    }

    #[inline]
    fn preflight_selected_route_rows(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        let routes = delta.selected_routes();
        let mut idx = 0usize;
        while idx < routes.len() {
            let row = routes.get(idx).ok_or(ResidentLaneStepError)?;
            self.preflight_selected_route_row(row)?;
            let mut prev_idx = 0usize;
            while prev_idx < idx {
                let prev = routes.get(prev_idx).ok_or(ResidentLaneStepError)?;
                if prev.scope() == row.scope() && prev.selected_arm() != row.selected_arm() {
                    return Err(ResidentLaneStepError);
                }
                prev_idx += 1;
            }
            idx += 1;
        }
        Ok(())
    }

    #[inline]
    fn preflight_parent_route_evidence(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        let Some(parent) = delta.parent_route_evidence() else {
            return Ok(());
        };
        let child = delta
            .selected_routes()
            .get(0)
            .ok_or(ResidentLaneStepError)?;
        let expected = self
            .build_recvless_parent_route_decision_plan(child.scope())
            .ok_or(ResidentLaneStepError)?;
        if expected.scope == parent.scope()
            && expected.arm == parent.arm()
            && expected.lane == parent.lane()
        {
            Ok(())
        } else {
            Err(ResidentLaneStepError)
        }
    }

    #[inline]
    fn preflight_selected_route_row(
        &self,
        row: SelectedRouteCommitRow,
    ) -> Result<(), ResidentLaneStepError> {
        if row.is_empty() || row.selected_arm() > 1 {
            return Err(ResidentLaneStepError);
        }
        let lane_idx = row.lane() as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(ResidentLaneStepError);
        }
        let scope = row.scope();
        if scope.is_none()
            || self
                .cursor
                .route_scope_slot(scope)
                .is_none_or(|slot| slot != row.scope_slot() as usize)
        {
            return Err(ResidentLaneStepError);
        }
        if self
            .cursor
            .passive_observer_arm_entry_index(scope, row.selected_arm())
            .is_none()
            && self
                .cursor
                .controller_arm_entry_by_arm(scope, row.selected_arm())
                .is_none()
        {
            return Err(ResidentLaneStepError);
        }
        if let Some(selected) = self.selected_arm_for_scope(scope)
            && selected != row.selected_arm()
        {
            return Err(ResidentLaneStepError);
        }
        self.decision_state
            .preflight_selected_route_commit(
                lane_idx,
                scope,
                row.scope_slot() as usize,
                row.selected_arm(),
                row.is_linger(),
            )
            .ok_or(ResidentLaneStepError)?;
        Ok(())
    }

    #[inline]
    fn preflight_loop_row(&self, row: LoopCommitRow) -> Result<(), ResidentLaneStepError> {
        if row.is_empty() {
            return Ok(());
        }
        let lane_idx = row.lane() as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(ResidentLaneStepError);
        }
        if row.scope().is_none() || Self::loop_index(row.scope()) != Some(row.idx()) {
            return Err(ResidentLaneStepError);
        }
        let metadata = self
            .cursor
            .loop_metadata_inner()
            .ok_or(ResidentLaneStepError)?;
        if metadata.scope != row.scope() {
            return Err(ResidentLaneStepError);
        }
        match row.disposition().ok_or(ResidentLaneStepError)? {
            LoopCommitDisposition::Decision { .. } => {
                if metadata.role != super::LoopRole::Controller || metadata.controller != ROLE {
                    return Err(ResidentLaneStepError);
                }
            }
            LoopCommitDisposition::Ack { role, .. } => {
                if metadata.role != super::LoopRole::Target
                    || metadata.target != ROLE
                    || role != ROLE
                {
                    return Err(ResidentLaneStepError);
                }
            }
        }
        Ok(())
    }

    #[inline]
    fn preflight_lane_relocation(
        &self,
        step: Option<super::RelocatableResidentLaneStep>,
    ) -> Result<(), ResidentLaneStepError> {
        let Some(step) = step else {
            return Ok(());
        };
        self.cursor
            .node_index_for_relocatable_step(step)
            .ok_or(ResidentLaneStepError)?;
        Ok(())
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn prepare_commit_delta(
        &self,
        delta: CommitDelta,
    ) -> Result<PreparedCommitDelta, ResidentLaneStepError> {
        self.preflight_commit_delta(&delta)?;
        Ok(PreparedCommitDelta::from_preflighted(delta))
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn commit_cursor_realign_index(
        &mut self,
        idx: usize,
    ) -> Result<(), ResidentLaneStepError> {
        if idx > u16::MAX as usize {
            return Err(ResidentLaneStepError);
        }
        let delta = CommitDelta::cursor_only(StateIndex::from_usize(idx));
        let delta = self.prepare_commit_delta(delta)?;
        self.commit_prepared_delta(delta);
        Ok(())
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn commit_prepared_delta(
        &mut self,
        delta: PreparedCommitDelta,
    ) {
        let delta = delta.delta();
        let routes = delta.selected_routes();
        let mut idx = 0usize;
        while idx < routes.len() {
            if let Some(row) = routes.get(idx) {
                self.apply_prepared_selected_route_commit_row(row);
            }
            idx += 1;
        }
        self.apply_loop_commit_row(delta.loop_row());
        self.apply_prepared_cursor_index(state_index_to_usize(delta.cursor_after()));
        if let Some(event) = delta.event() {
            self.apply_prepared_lane_advance(event.progress_step());
        }
        if let Some(step) = delta.lane_relocation() {
            self.apply_prepared_lane_relocation(step);
        }
    }

    #[inline(always)]
    fn apply_prepared_cursor_index(&mut self, idx: usize) {
        self.cursor.set_index(idx);
    }

    #[inline(always)]
    fn apply_prepared_lane_advance(&mut self, target: super::RelocatableResidentLaneStep) {
        let refresh = self.cursor.advance_lane_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn apply_prepared_lane_relocation(&mut self, target: super::RelocatableResidentLaneStep) {
        let refresh = self.cursor.set_lane_cursor_to_relocatable_step(target);
        self.refresh_after_cursor_move(refresh);
    }

    #[inline(always)]
    fn apply_prepared_selected_route_commit_row(&mut self, row: SelectedRouteCommitRow) {
        let lane_idx = row.lane() as usize;
        let _ = self
            .decision_state
            .apply_prepared_route_selection(row, CommitDeltaApplyPermit::new());
        self.refresh_lane_offer_state(lane_idx);
    }

    #[inline(always)]
    fn apply_loop_commit_row(&mut self, row: LoopCommitRow) {
        if row.is_empty() {
            return;
        }
        match row
            .disposition()
            .expect("non-empty loop commit row must carry a disposition")
        {
            LoopCommitDisposition::Decision { decision } => {
                self.apply_loop_decision_commit_row(row, decision);
            }
            LoopCommitDisposition::Ack {
                role,
                local_decision,
            } => {
                let port = self.port_for_lane(row.lane() as usize);
                let lane = port.lane();
                port.loop_table().acknowledge(lane, role, row.idx());
                if local_decision {
                    port.ack_loop_decision(row.idx(), role);
                }
            }
        }
    }

    #[inline(never)]
    fn apply_loop_decision_commit_row(&mut self, row: LoopCommitRow, decision: LoopDecision) {
        let port = self.port_for_lane(row.lane() as usize);
        let disposition = match decision {
            LoopDecision::Continue => crate::rendezvous::tables::LoopDisposition::Continue,
            LoopDecision::Break => crate::rendezvous::tables::LoopDisposition::Break,
        };
        let arm = match decision {
            LoopDecision::Continue => 0,
            LoopDecision::Break => 1,
        };
        let epoch = port.record_loop_decision(row.idx(), disposition);
        let ts = port.now32();
        let causal = crate::observe::core::TapEvent::make_causal_key(ROLE, row.idx());
        let arg1 = match decision {
            LoopDecision::Continue => ((row.idx() as u32) << 16) | epoch as u32,
            LoopDecision::Break => ((row.idx() as u32) << 16) | (epoch as u32) | 0x1,
        };
        let event = crate::observe::events::LoopDecision::with_causal_and_scope(
            ts,
            causal,
            self.sid.raw(),
            arg1,
            self.scope_trace(row.scope()).map(|t| t.pack()).unwrap_or(0),
        );
        crate::observe::core::emit(port.tap(), event);
        if self.cursor.route_scope_slot(row.scope()).is_some() {
            self.record_route_decision_for_scope_lanes(row.scope(), arm, row.lane());
            self.emit_route_decision(
                row.scope(),
                arm,
                super::RouteDecisionSource::Ack,
                row.lane(),
            );
        }
    }
}
