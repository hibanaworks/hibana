use super::super::super::facts::LocalAction;
use super::super::{EventCursor, ScopeId};

mod nesting;
mod reentry;

#[derive(Clone, Copy)]
enum ReentrantRouteCompletion {
    Outside,
    Incomplete,
    Complete(ScopeId),
}

#[derive(Clone, Copy)]
enum RollLaneAdmission {
    Head,
    Progress { current_step: Option<usize> },
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
                || self.route_scope_arm_rows_contain_index(scope, idx)
                || self
                    .preview_conflict_arm(self.machine().event_conflict_for_index(idx), scope)
                    .is_some();
        }
        false
    }

    #[inline(always)]
    fn route_scope_arm_rows_contain_index(&self, scope: ScopeId, idx: usize) -> bool {
        let Some(slot) = self.route_scope_slot_inner(scope) else {
            return false;
        };
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some(row) = self
                .machine()
                .event_program()
                .route_arm_event_row_by_slot(slot, arm)
                && row.start() <= idx
                && idx < row.end()
            {
                return true;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        false
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

    #[inline(always)]
    fn reentry_scope_event_bounds(&self, scope: ScopeId) -> Option<(usize, usize)> {
        match scope.kind() {
            Some(crate::global::const_dsl::ScopeKind::Roll) => self
                .machine()
                .roll_scope_row(scope)
                .map(|row| (row.start(), row.end())),
            Some(crate::global::const_dsl::ScopeKind::Route) if self.route_scope_reentry(scope) => {
                self.route_scope_rows(scope)
                    .map(|row| (row.start(), row.end()))
            }
            Some(crate::global::const_dsl::ScopeKind::Route)
            | Some(crate::global::const_dsl::ScopeKind::Parallel)
            | None => None,
        }
    }

    #[inline(never)]
    fn roll_scope_events_complete(
        &self,
        row: crate::global::event_program::LocalEventRowSet,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut found = false;
        let mut idx = row.start();
        while idx < row.end() {
            if let Some(lane) = self.event_lane_at(idx) {
                let conflict = self.machine().event_conflict_for_index(idx);
                if !self.event_conflict_row_allows(
                    conflict,
                    ScopeId::none(),
                    None,
                    selected_arm_for_scope,
                ) {
                    idx += 1;
                    continue;
                }
                found = true;
                if !self.node_event_done_for_lane(idx, lane) {
                    return false;
                }
            }
            idx += 1;
        }
        found
    }

    #[inline(never)]
    fn roll_scope_has_later_sequential_done_event(
        &self,
        scope: ScopeId,
        exclude_scope: ScopeId,
        idx: usize,
        lane: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        let Some(row) = self.machine().roll_scope_row(scope) else {
            return false;
        };
        let mut scan = idx + 1;
        while scan < row.end() && self.contains_node_index(scan) {
            if !exclude_scope.is_none() && self.roll_scope_contains_index(exclude_scope, scan) {
                scan += 1;
                continue;
            }
            if let Some(candidate_lane) = self.event_lane_at(scan)
                && candidate_lane == lane
                && self.node_event_done_for_lane(scan, candidate_lane)
            {
                if let Some(dependency) = self.dependency_for_index(scan)
                    && !Self::dependency_applies(dependency, &mut *selected_arm_for_scope)
                {
                    scan += 1;
                    continue;
                }
                let conflict = self.machine().event_conflict_for_index(scan);
                if self.event_conflict_row_allows_with_preview(
                    conflict,
                    preview_conflict,
                    &mut *selected_arm_for_scope,
                ) {
                    return true;
                }
            }
            scan += 1;
        }
        false
    }

    #[inline(never)]
    fn complete_roll_body_scope_for_index(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        let mut slot = 0usize;
        let mut best = None;
        let lane = self.event_lane_at(idx);
        while let Some((scope, row)) = self.machine().roll_scope_row_by_slot(slot) {
            if row.start() <= idx
                && idx < row.end()
                && self.roll_scope_events_complete(row, &mut *selected_arm_for_scope)
            {
                best = Some(match best {
                    Some(current) => self.deeper_roll_scope(current, scope),
                    None => scope,
                });
            }
            slot += 1;
        }
        let best = best?;
        let mut sequential_reentry = None;
        if let Some(lane) = lane {
            let mut slot = 0usize;
            while let Some((scope, row)) = self.machine().roll_scope_row_by_slot(slot) {
                if row.start() <= idx
                    && idx < row.end()
                    && !scope.same(best)
                    && self.roll_scope_nested_within(best, scope)
                    && self.roll_scope_events_complete(row, &mut *selected_arm_for_scope)
                    && self.roll_scope_has_later_sequential_done_event(
                        scope,
                        best,
                        idx,
                        lane,
                        &mut *selected_arm_for_scope,
                    )
                {
                    sequential_reentry = Some(match sequential_reentry {
                        Some(current) => self.outer_roll_scope(current, scope),
                        None => scope,
                    });
                }
                slot += 1;
            }
        }
        match sequential_reentry {
            Some(scope) => Some(scope),
            None => Some(best),
        }
    }

    #[inline(never)]
    fn reentrant_route_completion_for_index(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> ReentrantRouteCompletion {
        let mut contains_reentrant_route = false;
        let mut selected = None;
        let mut slot = 0usize;
        while slot < self.machine().route_scope_slot_count() {
            if let Some(region) = self.machine().route_scope_rows_by_slot(slot) {
                let scope = region.scope();
                if region.start() <= idx
                    && idx < region.end()
                    && self.route_scope_reentry(scope)
                    && self.roll_scope_contains_index(scope, idx)
                {
                    contains_reentrant_route = true;
                    if selected_arm_for_scope(scope).is_some_and(|arm| {
                        self.selected_route_arm_event_row_done(
                            scope,
                            arm,
                            &mut *selected_arm_for_scope,
                        )
                    }) {
                        selected = Some(match selected {
                            Some(current) => self.deeper_route_scope(current, scope),
                            None => scope,
                        });
                    }
                }
            }
            slot += 1;
        }
        match selected {
            Some(scope) => ReentrantRouteCompletion::Complete(scope),
            None if contains_reentrant_route => ReentrantRouteCompletion::Incomplete,
            None => ReentrantRouteCompletion::Outside,
        }
    }

    #[inline(never)]
    fn complete_roll_scope_for_index(
        &self,
        idx: usize,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<ScopeId> {
        if let Some(scope) =
            self.complete_roll_body_scope_for_index(idx, &mut *selected_arm_for_scope)
        {
            return Some(scope);
        }
        match self.reentrant_route_completion_for_index(idx, &mut *selected_arm_for_scope) {
            ReentrantRouteCompletion::Complete(scope) => Some(scope),
            ReentrantRouteCompletion::Incomplete => None,
            ReentrantRouteCompletion::Outside => None,
        }
    }

    #[inline(never)]
    fn roll_scope_lane_allows_index(
        &self,
        scope: ScopeId,
        idx: usize,
        lane: u8,
        admission: RollLaneAdmission,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
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
        let Some((mut scan, end)) = self.reentry_scope_event_bounds(scope) else {
            crate::invariant();
        };
        while scan < idx && scan < end && self.contains_node_index(scan) {
            if self.roll_scope_contains_index(scope, scan)
                && let Some(prior_lane) = self.event_lane_at(scan)
                && prior_lane == lane
            {
                if let RollLaneAdmission::Progress { current_step } = admission
                    && self.node_event_done_for_lane(scan, prior_lane)
                    && self.event_step_is_before(scan, prior_lane, current_step)
                {
                    scan += 1;
                    continue;
                }
                let prior_conflict = self.machine().event_conflict_for_index(scan);
                if self.event_conflict_row_allows_with_preview(
                    prior_conflict,
                    preview_conflict,
                    &mut *selected_arm_for_scope,
                ) {
                    return false;
                }
            }
            scan += 1;
        }
        true
    }

    #[inline]
    fn event_step_is_before(&self, idx: usize, lane: u8, current_step: Option<usize>) -> bool {
        let Ok(target) = self.relocatable_resident_lane_step_at_index(idx, lane as usize) else {
            crate::invariant();
        };
        let Some(current_step) = current_step else {
            return true;
        };
        (target.0.step_idx as usize) < current_step
    }

    #[inline]
    fn event_is_before_lane_cursor(&self, idx: usize, lane: u8) -> bool {
        self.event_step_is_before(idx, lane, self.step_index_at_lane(lane as usize))
    }

    #[inline(never)]
    fn roll_reentry_recv_index_for_frame_phase(
        &self,
        key: super::super::InboundFrameKey,
        phase: RollLaneAdmission,
        live_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while self.contains_node_index(idx) {
            let Some(meta) = self.try_recv_meta_at(idx) else {
                idx += 1;
                continue;
            };
            if !key.matches_recv(meta) {
                idx += 1;
                continue;
            }
            let before_cursor = self.event_is_before_lane_cursor(idx, key.lane);
            let phase_matches = match phase {
                RollLaneAdmission::Head => before_cursor,
                RollLaneAdmission::Progress { .. } => !before_cursor,
            };
            let mut arm_for_candidate = |scope| {
                let live = live_arm_for_scope(scope);
                let membership = self.route_arm_for_index(scope, idx);
                let committed = membership.and_then(|_| committed_arm_for_scope(scope));
                if live.is_some() { live } else { committed }
            };
            let allows = phase_matches
                && self.roll_reentry_event_allows_index(idx, key.lane, &mut arm_for_candidate);
            if allows {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline(never)]
    pub(crate) fn roll_reentry_recv_index_for_frame(
        &self,
        key: super::super::InboundFrameKey,
        live_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        if !self.has_reentry_scopes() {
            return None;
        }
        if let Some(current) = self.roll_reentry_recv_index_for_frame_phase(
            key,
            RollLaneAdmission::Progress {
                current_step: self.step_index_at_lane(key.lane as usize),
            },
            live_arm_for_scope,
            committed_arm_for_scope,
        ) {
            return Some(current);
        }
        self.roll_reentry_recv_index_for_frame_phase(
            key,
            RollLaneAdmission::Head,
            live_arm_for_scope,
            committed_arm_for_scope,
        )
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
        if self.event_is_before_lane_cursor(idx, lane) {
            return self.roll_scope_lane_allows_index(
                scope,
                idx,
                lane,
                RollLaneAdmission::Head,
                selected_arm_for_scope,
            );
        }
        self.roll_scope_lane_allows_index(
            scope,
            idx,
            lane,
            RollLaneAdmission::Progress {
                current_step: self.step_index_at_lane(lane as usize),
            },
            selected_arm_for_scope,
        )
    }
}
