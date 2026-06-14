use super::super::super::facts::{LocalAction, LocalConflict};
use super::super::{EventCursor, RelocatableResidentLaneStep, ScopeId};

impl EventCursor {
    #[inline(always)]
    fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left.canonical_raw() == right.canonical_raw()
    }

    #[inline(always)]
    fn route_reentry_scope_for_index(&self, idx: usize) -> Option<ScopeId> {
        let scope = self.node_scope_id_at(idx);
        if self.route_scope_reentry(scope) {
            Some(scope)
        } else if let Some(LocalConflict::RouteArm { scope, .. }) =
            self.machine().event_conflict_for_index(idx).to_conflict()
            && self.route_scope_reentry(scope)
        {
            Some(scope)
        } else {
            None
        }
    }

    fn roll_scope_contains_index(&self, scope: ScopeId, idx: usize) -> bool {
        if matches!(scope.kind(), crate::global::const_dsl::ScopeKind::Roll)
            && let Some(row) = self.machine().roll_scope_row(scope)
        {
            return row.start() <= idx && idx < row.end();
        }
        if self.route_scope_reentry(scope) {
            return self
                .route_reentry_scope_for_index(idx)
                .is_some_and(|candidate| Self::same_scope(candidate, scope));
        }
        false
    }

    #[inline(always)]
    fn event_label_lane_at(&self, idx: usize) -> Option<(u8, u8)> {
        match self.machine().node(idx).action() {
            LocalAction::Send { label, lane, .. }
            | LocalAction::Recv { label, lane, .. }
            | LocalAction::Local { label, lane, .. } => Some((label, lane)),
            LocalAction::Terminate => None,
        }
    }

    fn first_roll_lane_event_index(&self, scope: ScopeId, lane: u8) -> Option<usize> {
        if let Some(row) = self.machine().roll_scope_row(scope) {
            let mut idx = row.start();
            while idx < row.end() && idx < self.local_steps_len() {
                if let Some((_event, candidate_lane)) = self.event_label_lane_at(idx)
                    && candidate_lane == lane
                {
                    return Some(idx);
                }
                idx += 1;
            }
            return None;
        }
        let mut idx = 0usize;
        while idx < self.local_steps_len() {
            if self.roll_scope_contains_index(scope, idx)
                && let Some((_event, candidate_lane)) = self.event_label_lane_at(idx)
                && candidate_lane == lane
            {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    fn roll_scope_events_complete(
        &self,
        scope: ScopeId,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut found = false;
        let (mut idx, end) = if let Some(row) = self.machine().roll_scope_row(scope) {
            (row.start(), row.end())
        } else {
            (0, self.local_steps_len())
        };
        while idx < end && idx < self.local_steps_len() {
            if self.roll_scope_contains_index(scope, idx) && self.event_label_lane_at(idx).is_some()
            {
                let conflict = self.machine().event_conflict_for_index(idx);
                if !self.event_conflict_row_allows_with_preview(conflict, conflict, |scope| {
                    selected_arm_for_scope(scope)
                }) {
                    idx += 1;
                    continue;
                }
                found = true;
                if !self.local_event_done(idx) {
                    return false;
                }
            }
            idx += 1;
        }
        found
    }

    fn complete_roll_body_scope_for_index(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        let mut slot = 0usize;
        let mut best = ScopeId::none();
        let mut best_len = 0usize;
        while let Some((scope, row)) = self.machine().roll_scope_row_by_slot(slot) {
            if row.start() <= idx
                && idx < row.end()
                && row.len() >= best_len
                && self.roll_scope_events_complete(scope, &mut selected_arm_for_scope)
            {
                best = scope;
                best_len = row.len();
            }
            slot += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    fn complete_roll_scope_for_index(
        &self,
        idx: usize,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        self.complete_roll_body_scope_for_index(idx, &mut selected_arm_for_scope)
            .or_else(|| {
                let scope = self.route_reentry_scope_for_index(idx)?;
                self.roll_scope_events_complete(scope, selected_arm_for_scope)
                    .then_some(scope)
            })
    }

    pub(crate) fn roll_reentry_index_for_label(
        &self,
        target_label: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.local_steps_len() {
            let Some((label, lane)) = self.event_label_lane_at(idx) else {
                idx += 1;
                continue;
            };
            let Some(scope) = self.complete_roll_scope_for_index(idx, &mut selected_arm_for_scope)
            else {
                idx += 1;
                continue;
            };
            if label == target_label && self.first_roll_lane_event_index(scope, lane) == Some(idx) {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    pub(crate) fn roll_reentry_event_allows_index(
        &self,
        idx: usize,
        lane: u8,
        selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let Some(scope) = self.complete_roll_scope_for_index(idx, selected_arm_for_scope) else {
            return false;
        };
        self.first_roll_lane_event_index(scope, lane) == Some(idx)
    }

    pub(crate) fn roll_reentry_scope_for_step(
        &self,
        target: RelocatableResidentLaneStep,
    ) -> Option<ScopeId> {
        let idx = self.node_index_for_relocatable_step(target)?;
        let lane = self
            .machine()
            .event_program()
            .local_step_lane(target.0.step_idx as usize)?;
        let scope = self
            .complete_roll_body_scope_for_index(idx, |_| None)
            .or_else(|| self.route_reentry_scope_for_index(idx))?;
        (self.first_roll_lane_event_index(scope, lane) == Some(idx)).then_some(scope)
    }

    pub(crate) fn clear_roll_scope_events_for_reentry(&mut self, scope: ScopeId) {
        let (mut idx, end) = if let Some(row) = self.machine().roll_scope_row(scope) {
            (row.start(), row.end())
        } else {
            (0, self.local_steps_len())
        };
        while idx < end && idx < self.local_steps_len() {
            if self.roll_scope_contains_index(scope, idx) && self.event_label_lane_at(idx).is_some()
            {
                self.clear_local_event_done(idx);
            }
            idx += 1;
        }
    }
}
