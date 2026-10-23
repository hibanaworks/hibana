use super::super::{EventCursor, ScopeId};

mod completion;
mod nesting;
mod reentry;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

impl EventCursor {
    /// A completed repeat may restart instead of continuing its current body.
    /// Its next visit may also choose another arm. Neither alternative belongs
    /// to a borrow of the current arm. Unfinished prefixes and parallel siblings
    /// remain required members of the frontier.
    pub(crate) fn completed_roll_alternative_to(
        &self,
        index: usize,
        continuation: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let program = self.program_ref();
        let Some((event, _)) = self.machine().event_program().operation_at(index) else {
            return false;
        };
        let source = event.dense_ordinal();
        if source == continuation {
            return false;
        }
        for marker in 0..program.source_scope_marker_count() {
            let Some((scope, start, end)) = program.source_roll_at(marker) else {
                continue;
            };
            let Some(row) = self.machine().roll_scope_row(scope) else {
                continue;
            };
            if !(row.start() <= index && index < row.end()) {
                continue;
            }
            let prefix = end <= continuation && program.source_precedes(end - 1, continuation);
            let enclosing = start <= continuation
                && continuation < end
                && program.source_precedes(source, continuation);
            let other_arm = || {
                start <= source
                    && source < end
                    && start <= continuation
                    && continuation < end
                    && (0..self.machine().route_scope_slot_count()).any(|slot| {
                        let Some(route) = self.machine().route_scope_rows_by_slot(slot) else {
                            return false;
                        };
                        program.source_route_separates(route.scope(), source, continuation)
                    })
            };
            if (prefix || enclosing || other_arm())
                && self.roll_scope_events_complete(scope, selected)
            {
                return true;
            }
        }
        false
    }

    #[inline(always)]
    fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left == right
    }

    #[inline(never)]
    pub(super) fn roll_scope_contains_index(&self, scope: ScopeId, idx: usize) -> bool {
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
    pub(crate) fn event_lane_at(&self, idx: usize) -> Option<u8> {
        if idx > self.local_steps_len() {
            crate::invariant();
        }
        self.machine().event_program().local_step_lane(idx)
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
        scope: ScopeId,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let row = crate::invariant_some(self.machine().roll_scope_row(scope));
        let mut participates = |idx| {
            self.event_lane_at(idx).is_some()
                && self.event_actor_is_local(idx, &mut *selected_arm_for_scope) != Some(false)
                && self.event_conflict_row_allows(
                    self.machine().event_conflict_for_index(idx),
                    ScopeId::none(),
                    None,
                    &mut *selected_arm_for_scope,
                )
        };
        // Completion is the existing bitmap's authority. Only its clear bits
        // can block this scope; do not decode its completed prefix repeatedly.
        if self
            .pending_event_steps(row.start()..row.end())
            .any(|index| {
                participates(index)
                    && (self
                        .machine()
                        .event_program()
                        .footprint()
                        .borrow_epoch_count
                        == 0
                        || self.credited_roll_child(scope, index).is_none())
            })
        {
            return false;
        }
        // A scope with no participating event is absent, rather than complete.
        (row.start()..row.end()).any(participates)
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
                // A conditional dependency guards its prerequisite, not the
                // event itself. An inactive prerequisite from another route
                // arm does not erase this committed continuation.
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
    fn credited_roll_path_complete(
        &self,
        mut scope: ScopeId,
        index: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        while let Some(child) = self.credited_roll_child(scope, index) {
            if !self.roll_scope_events_complete(child, selected) {
                return false;
            }
            scope = child;
        }
        true
    }

    #[inline(never)]
    pub(super) fn complete_roll_body_scope_for_index(
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
                && (self
                    .machine()
                    .event_program()
                    .footprint()
                    .borrow_epoch_count
                    == 0
                    || self.credited_roll_path_complete(scope, idx, &mut *selected_arm_for_scope))
                && self.roll_scope_events_complete(scope, &mut *selected_arm_for_scope)
            {
                best = Some(match best {
                    Some(current) => self.deeper_roll_scope(current, scope),
                    None => scope,
                });
            }
            slot += 1;
        }
        let best = best?;
        // With one roll row, the second scan cannot find an ancestor.
        if slot == 1 {
            return Some(best);
        }
        let mut sequential_reentry = None;
        if let Some(lane) = lane {
            let mut slot = 0usize;
            while let Some((scope, row)) = self.machine().roll_scope_row_by_slot(slot) {
                if row.start() <= idx
                    && idx < row.end()
                    && !scope.same(best)
                    && self.roll_scope_nested_within(best, scope)
                    && self.roll_scope_events_complete(scope, &mut *selected_arm_for_scope)
                    && self.roll_scope_has_later_sequential_done_event(
                        scope,
                        best,
                        idx,
                        lane,
                        &mut *selected_arm_for_scope,
                    )
                    // A later completed step does not let this event skip an
                    // enclosing prefix. Promote only to an ancestor whose
                    // next visit can actually begin with this event.
                    && self.roll_scope_head_allows_index(
                        scope,
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
    pub(super) fn roll_scope_head_allows_index(
        &self,
        scope: ScopeId,
        idx: usize,
        lane: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let roll_row = self.machine().roll_scope_row(scope);
        let contains = |index| match roll_row {
            Some(row) => row.start() <= index && index < row.end(),
            None => self.roll_scope_contains_index(scope, index),
        };
        if !contains(idx) {
            return false;
        }
        let Some(candidate_lane) = self.event_lane_at(idx) else {
            return false;
        };
        if candidate_lane != lane {
            return false;
        }
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        let Some((mut scan, end)) = roll_row
            .map(|row| (row.start(), row.end()))
            .or_else(|| self.reentry_scope_event_bounds(scope))
        else {
            crate::invariant();
        };
        while scan < idx && scan < end && self.contains_node_index(scan) {
            if contains(scan)
                && self.event_actor_is_local(scan, selected_arm_for_scope) != Some(false)
                && let Some(prior_lane) = self.event_lane_at(scan)
                && prior_lane == lane
            {
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
        // A predecessor on another lane is cleared by the same reset. The
        // candidate must be a source head, as well as a head of its own lane.
        if idx >= self.local_steps_len() {
            return false;
        }
        if let Some(dependency) = self.machine().event_program().dependency_for_index(idx)
            && Self::dependency_applies(dependency, &mut *selected_arm_for_scope)
        {
            let rows = self
                .machine()
                .event_program()
                .dependency_row_set(dependency);
            for prior in rows.start()..rows.end() {
                if contains(prior)
                    && self.event_actor_is_local(prior, &mut *selected_arm_for_scope) != Some(false)
                    && prior < self.local_steps_len()
                    && self.event_conflict_row_allows_with_preview(
                        self.machine()
                            .event_program()
                            .event_conflict_for_index(prior),
                        preview_conflict,
                        &mut *selected_arm_for_scope,
                    )
                {
                    return false;
                }
            }
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
    pub(super) fn event_is_before_lane_cursor(&self, idx: usize, lane: u8) -> bool {
        self.event_step_is_before(idx, lane, self.step_index_at_lane(lane as usize))
    }

    #[inline(never)]
    pub(crate) fn roll_reentry_event_allows_index(
        &self,
        idx: usize,
        lane: u8,
        arm_for_scope: &mut dyn FnMut(ScopeId, super::EventArmView) -> Option<u8>,
    ) -> bool {
        if !self.has_reentry_scopes() {
            return false;
        }
        let mut committed = |scope| arm_for_scope(scope, super::EventArmView::Committed);
        let Some(scope) = self.complete_roll_body_scope_for_index(idx, &mut committed) else {
            return false;
        };
        let mut selected_arm_for_scope = |scope| arm_for_scope(scope, super::EventArmView::Preview);
        // Only an explicit completed roll admits a new visit, at its head.
        // An inherited route mark cannot create an independent loop.
        self.roll_scope_head_allows_index(scope, idx, lane, &mut selected_arm_for_scope)
    }
}
