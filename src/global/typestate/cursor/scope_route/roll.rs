use super::super::super::facts::LocalAction;
use super::super::{EventCursor, RelocatableResidentLaneStep, ScopeId};

#[derive(Clone, Copy)]
enum ReentrantRouteCompletion {
    Outside,
    Incomplete,
    Complete(ScopeId),
}

impl EventCursor {
    #[inline(always)]
    fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left == right
    }

    #[inline(never)]
    fn roll_scope_contains_index(&self, scope: ScopeId, idx: usize) -> bool {
        if matches!(
            scope.kind(),
            Some(crate::global::const_dsl::ScopeKind::Roll)
        ) && let Some(row) = self.machine().roll_scope_row(scope)
        {
            return row.start() <= idx && idx < row.end();
        }
        if self.route_scope_reentry(scope) {
            return Self::same_scope(self.node_scope_id_at(idx), scope)
                || self
                    .preview_conflict_arm(self.machine().event_conflict_for_index(idx), scope)
                    .is_some();
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

    #[inline(always)]
    fn event_lane_at(&self, idx: usize) -> Option<u8> {
        match self.machine().node(idx).action() {
            LocalAction::Send { lane, .. }
            | LocalAction::Recv { lane, .. }
            | LocalAction::Local { lane, .. } => Some(lane),
            LocalAction::Terminate => None,
        }
    }

    #[inline(never)]
    fn roll_scope_events_complete(
        &self,
        scope: ScopeId,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
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
                if !self.event_conflict_row_allows_with_preview(
                    conflict,
                    conflict,
                    &mut *selected_arm_for_scope,
                ) {
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

    #[inline(never)]
    fn complete_roll_body_scope_for_index(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        let mut slot = 0usize;
        let mut best = ScopeId::none();
        let mut best_len = 0usize;
        while let Some((scope, row)) = self.machine().roll_scope_row_by_slot(slot) {
            if row.start() <= idx
                && idx < row.end()
                && row.len() >= best_len
                && self.roll_scope_events_complete(scope, &mut *selected_arm_for_scope)
            {
                best = scope;
                best_len = row.len();
            }
            slot += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    #[inline(never)]
    fn complete_roll_scope_for_index(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        match self.reentrant_route_completion_for_index(idx, &mut *selected_arm_for_scope) {
            ReentrantRouteCompletion::Complete(scope) => Some(scope),
            ReentrantRouteCompletion::Incomplete => None,
            ReentrantRouteCompletion::Outside => {
                self.complete_roll_body_scope_for_index(idx, &mut *selected_arm_for_scope)
            }
        }
    }

    #[inline(never)]
    fn reentrant_route_completion_for_index(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> ReentrantRouteCompletion {
        let mut contains_reentrant_route = false;
        let mut selected = ScopeId::none();
        let mut selected_len = usize::MAX;
        let mut slot = 0usize;
        while let Some(region) = self.machine().route_scope_rows_by_slot(slot) {
            let scope = region.scope();
            let len = region.end() - region.start();
            if region.start() <= idx
                && idx < region.end()
                && self.route_scope_reentry(scope)
                && self.roll_scope_contains_index(scope, idx)
            {
                contains_reentrant_route = true;
                if len < selected_len
                    && selected_arm_for_scope(scope).is_some_and(|arm| {
                        self.selected_route_arm_event_row_done(
                            scope,
                            arm,
                            &mut *selected_arm_for_scope,
                        )
                    })
                {
                    selected = scope;
                    selected_len = len;
                }
            }
            slot += 1;
        }
        if !selected.is_none() {
            ReentrantRouteCompletion::Complete(selected)
        } else if contains_reentrant_route {
            ReentrantRouteCompletion::Incomplete
        } else {
            ReentrantRouteCompletion::Outside
        }
    }

    #[inline(never)]
    fn roll_scope_lane_head_allows_index(&self, scope: ScopeId, idx: usize, lane: u8) -> bool {
        if !self.roll_scope_contains_index(scope, idx) {
            return false;
        }
        let Some(candidate_lane) = self.event_lane_at(idx) else {
            return false;
        };
        if candidate_lane != lane {
            return false;
        }
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        let (mut scan, end) = if let Some(row) = self.machine().roll_scope_row(scope) {
            (row.start(), row.end())
        } else {
            (0, self.local_steps_len())
        };
        while scan < idx && scan < end && scan < self.local_steps_len() {
            if self.roll_scope_contains_index(scope, scan)
                && let Some(prior_lane) = self.event_lane_at(scan)
                && prior_lane == lane
            {
                let prior_conflict = self.machine().event_conflict_for_index(scan);
                let mut selected_arm_for_scope = |_scope| None;
                if self.event_conflict_row_allows_with_preview(
                    prior_conflict,
                    preview_conflict,
                    &mut selected_arm_for_scope,
                ) {
                    return false;
                }
            }
            scan += 1;
        }
        true
    }

    #[inline(never)]
    fn roll_scope_lane_progress_allows_index(&self, scope: ScopeId, idx: usize, lane: u8) -> bool {
        if !self.roll_scope_contains_index(scope, idx) {
            return false;
        }
        let Some(candidate_lane) = self.event_lane_at(idx) else {
            return false;
        };
        if candidate_lane != lane {
            return false;
        }
        if self.index() == idx || self.index_for_lane_step(lane as usize) == Some(idx) {
            return true;
        }
        self.roll_scope_lane_head_allows_index(scope, idx, lane)
    }

    #[inline(never)]
    pub(crate) fn roll_reentry_index_for_label(
        &self,
        target_label: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        if !self.has_reentry_scopes() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.local_steps_len() {
            let Some((label, lane)) = self.event_label_lane_at(idx) else {
                idx += 1;
                continue;
            };
            let Some(scope) = self.complete_roll_scope_for_index(idx, &mut *selected_arm_for_scope)
            else {
                idx += 1;
                continue;
            };
            if label == target_label && self.roll_scope_lane_head_allows_index(scope, idx, lane) {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline(never)]
    pub(crate) fn roll_reentry_event_allows_index(
        &self,
        idx: usize,
        lane: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        if !self.has_reentry_scopes() {
            return false;
        }
        let Some(scope) = self.complete_roll_scope_for_index(idx, selected_arm_for_scope) else {
            return false;
        };
        self.roll_scope_lane_progress_allows_index(scope, idx, lane)
    }

    #[inline(never)]
    pub(crate) fn roll_reentry_scope_for_step(
        &self,
        target: RelocatableResidentLaneStep,
    ) -> Option<ScopeId> {
        if !self.has_reentry_scopes() {
            return None;
        }
        let idx = self.node_index_for_relocatable_step(target)?;
        let lane = self
            .machine()
            .event_program()
            .local_step_lane(target.0.step_idx as usize)?;
        let mut selected_arm_for_scope = |_| None;
        let scope =
            match self.reentrant_route_completion_for_index(idx, &mut selected_arm_for_scope) {
                ReentrantRouteCompletion::Complete(scope) => scope,
                ReentrantRouteCompletion::Incomplete => return None,
                ReentrantRouteCompletion::Outside => {
                    self.complete_roll_body_scope_for_index(idx, &mut selected_arm_for_scope)?
                }
            };
        self.roll_scope_lane_head_allows_index(scope, idx, lane)
            .then_some(scope)
    }

    #[inline(never)]
    pub(crate) fn clear_roll_scope_events_for_reentry(&mut self, scope: ScopeId) {
        if !self.has_reentry_scopes() {
            return;
        }
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
