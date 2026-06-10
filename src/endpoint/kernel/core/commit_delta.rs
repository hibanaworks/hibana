use super::{
    CommitDelta, CommitEventRow, CursorEndpoint, EpochTable, LabelUniverse, LoopCommitDisposition,
    LoopCommitRow, LoopDecision, MintConfigMarker, PreparedRouteCommitRows,
    RelocatableResidentLaneStep, ResidentLaneStepError, SelectedRouteCommitRow, Transport,
    state_index_to_usize,
};
use crate::global::typestate::StateIndex;

pub(crate) struct PreparedCommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: PreparedRouteCommitRows,
    loop_row: LoopCommitRow,
    lane_relocation: Option<RelocatableResidentLaneStep>,
    cursor_after: StateIndex,
}

impl PreparedCommitDelta {
    #[inline(always)]
    fn from_preflighted(delta: CommitDelta, selected_routes: PreparedRouteCommitRows) -> Self {
        Self {
            event: delta.event(),
            selected_routes,
            loop_row: delta.loop_row(),
            lane_relocation: delta.lane_relocation(),
            cursor_after: delta.cursor_after(),
        }
    }

    #[inline(always)]
    pub(crate) const fn event(&self) -> Option<CommitEventRow> {
        self.event
    }

    #[inline(always)]
    pub(crate) const fn selected_routes(&self) -> &PreparedRouteCommitRows {
        &self.selected_routes
    }

    #[inline(always)]
    pub(crate) const fn selected_route_lane(&self) -> Option<u8> {
        self.selected_routes.selected_lane()
    }

    #[inline(always)]
    pub(crate) const fn loop_row(&self) -> LoopCommitRow {
        self.loop_row
    }

    #[inline(always)]
    pub(crate) const fn lane_relocation(&self) -> Option<RelocatableResidentLaneStep> {
        self.lane_relocation
    }

    #[inline(always)]
    pub(crate) const fn cursor_after(&self) -> StateIndex {
        self.cursor_after
    }

    #[inline(always)]
    fn take_selected_routes(&mut self) -> PreparedRouteCommitRows {
        self.selected_routes.take()
    }
}

pub(crate) struct CommittedCommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: PreparedRouteCommitRows,
}

impl CommittedCommitDelta {
    #[inline(always)]
    pub(crate) const fn event(&self) -> Option<CommitEventRow> {
        self.event
    }

    #[inline(always)]
    pub(crate) const fn selected_routes(&self) -> &PreparedRouteCommitRows {
        &self.selected_routes
    }

    #[inline(always)]
    pub(crate) const fn selected_route_lane(&self) -> Option<u8> {
        self.selected_routes.selected_lane()
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct CommitDeltaApplyPermit(());

impl CommitDeltaApplyPermit {
    #[inline(always)]
    const fn new() -> Self {
        Self(())
    }
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
            let enabled = self.cursor.event_enabled(
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
            self.preflight_event_selected_route_chain(idx, event.route_arm(), delta)?;
        } else if delta.selected_routes().is_empty() {
            self.preflight_cursor_realign_delta(delta)?;
        } else {
            self.preflight_route_only_cursor_after(delta)?;
        }
        self.preflight_selected_route_rows(delta)?;
        self.preflight_loop_row(delta.loop_row())?;
        self.preflight_lane_relocation(delta.lane_relocation())?;
        Ok(())
    }

    #[inline]
    fn preflight_event_selected_route_chain(
        &self,
        event_idx: usize,
        event_arm: Option<u8>,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        let routes = delta.selected_routes();
        let conflict = self.cursor.event_conflict_for_index(event_idx);
        let Some(range) = self
            .cursor
            .route_commit_range_for_conflict(conflict, event_arm)
        else {
            return if event_arm.is_none() && routes.is_empty() {
                Ok(())
            } else {
                Err(ResidentLaneStepError)
            };
        };
        if routes.len() != range.len() {
            return Err(ResidentLaneStepError);
        }
        let mut idx = 0usize;
        while idx < range.len() {
            let Some(expected) = self
                .cursor
                .route_commit_row_at(range, idx)
                .and_then(SelectedRouteCommitRow::from_resident_conflict)
            else {
                return Err(ResidentLaneStepError);
            };
            let Some(row) = routes.get(&self.cursor, idx) else {
                return Err(ResidentLaneStepError);
            };
            if row.scope() != expected.scope() || row.selected_arm() != expected.selected_arm() {
                return Err(ResidentLaneStepError);
            }
            idx += 1;
        }
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
        if delta.selected_route_lane().is_none() {
            return Err(ResidentLaneStepError);
        }
        let expected_after = StateIndex::from_usize(self.cursor.index());
        (expected_after == delta.cursor_after())
            .then_some(())
            .ok_or(ResidentLaneStepError)
    }

    #[inline]
    fn preflight_selected_route_rows(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), ResidentLaneStepError> {
        let routes = delta.selected_routes();
        let route_lane = if routes.is_empty() {
            None
        } else {
            delta.selected_route_lane()
        };
        let mut idx = 0usize;
        while idx < routes.len() {
            let row = routes.get(&self.cursor, idx).ok_or(ResidentLaneStepError)?;
            let lane = route_lane.ok_or(ResidentLaneStepError)?;
            self.preflight_selected_route_row(row, lane)?;
            let mut prev_idx = 0usize;
            while prev_idx < idx {
                let prev = routes
                    .get(&self.cursor, prev_idx)
                    .ok_or(ResidentLaneStepError)?;
                if prev.scope() == row.scope() {
                    return Err(ResidentLaneStepError);
                }
                prev_idx += 1;
            }
            idx += 1;
        }
        Ok(())
    }

    #[inline]
    fn preflight_selected_route_row(
        &self,
        row: SelectedRouteCommitRow,
        lane: u8,
    ) -> Result<(), ResidentLaneStepError> {
        if row.is_empty() || row.selected_arm() > 1 {
            return Err(ResidentLaneStepError);
        }
        let lane_idx = lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(ResidentLaneStepError);
        }
        let scope = row.scope();
        if scope.is_none() || self.cursor.route_scope_slot(scope).is_none() {
            return Err(ResidentLaneStepError);
        }
        let scope_slot = self
            .cursor
            .route_scope_slot(scope)
            .ok_or(ResidentLaneStepError)?;
        let is_linger = self.cursor.route_scope_linger(scope);
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
                scope_slot,
                row.selected_arm(),
                is_linger,
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
        &mut self,
        delta: CommitDelta,
    ) -> Result<PreparedCommitDelta, ResidentLaneStepError> {
        self.preflight_commit_delta(&delta)?;
        let selected_routes = self
            .route_commit_rows
            .seal(delta.selected_route_rows_ref())
            .map_err(|_| ResidentLaneStepError)?;
        Ok(PreparedCommitDelta::from_preflighted(
            delta,
            selected_routes,
        ))
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
        mut delta: PreparedCommitDelta,
    ) -> CommittedCommitDelta {
        let route_lane = delta.selected_route_lane();
        let mut idx = 0usize;
        while idx < delta.selected_routes().len() {
            let Some(row) = delta.selected_routes().get(&self.cursor, idx) else {
                crate::invariant();
            };
            let Some(lane) = route_lane else {
                crate::invariant();
            };
            self.apply_prepared_selected_route_commit_row(row, lane);
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
        CommittedCommitDelta {
            event: delta.event(),
            selected_routes: delta.take_selected_routes(),
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
    fn apply_prepared_selected_route_commit_row(&mut self, row: SelectedRouteCommitRow, lane: u8) {
        let lane_idx = lane as usize;
        let Some(scope_slot) = self.cursor.route_scope_slot(row.scope()) else {
            crate::invariant();
        };
        let is_linger = self.cursor.route_scope_linger(row.scope());
        self.decision_state.apply_prepared_route_selection(
            lane_idx,
            scope_slot,
            is_linger,
            row,
            CommitDeltaApplyPermit::new(),
        );
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
            self.record_route_arm_selection_for_scope_lanes(row.scope(), arm, row.lane());
            let Some(arm) = super::Arm::new(arm) else {
                crate::invariant();
            };
            self.emit_route_arm_selection(
                row.scope(),
                super::RouteArmToken::from_ack(arm),
                row.lane(),
            );
        }
    }
}
