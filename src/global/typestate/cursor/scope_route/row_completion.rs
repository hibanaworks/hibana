use super::super::{EventCursor, LocalDependency, ScopeId};
use super::ConflictAdmission;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
use crate::global::event_program::LocalEventRowSet;

impl EventCursor {
    pub(super) fn excluded_arm_end(&self, index: usize, scope: ScopeId, arm: u8) -> usize {
        let slot = crate::invariant_some(self.route_scope_slot_inner(scope));
        let row = crate::invariant_some(
            self.machine()
                .event_program()
                .route_arm_event_row_by_slot(slot, arm),
        );
        if index < row.start() || index >= row.end() {
            crate::invariant();
        }
        row.end()
    }

    pub(crate) fn is_view_complete(
        &self,
        role: u8,
        source: core::ops::Range<usize>,
        owns_endpoint: bool,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        use crate::{eff::EventPayload, global::role_program::RoleLocalDirection};
        for idx in self.pending_event_steps(0..self.local_steps_len()) {
            let (eff, direction) =
                crate::invariant_some(self.machine().event_program().operation_at(idx));
            let receiving = direction == RoleLocalDirection::Recv;
            if !source.contains(&eff.dense_ordinal()) {
                continue;
            }
            let atom = self.program_ref().event_atom_at(eff.dense_ordinal());
            let owns_step = (if receiving { atom.to } else { atom.from }) == role;
            // A borrow must end before its parent endpoint can be moved.
            // The owned endpoint still checks the pending transfer on Drop.
            let supplies_payload =
                owns_endpoint && !receiving && atom.payload == EventPayload::Endpoint(role);
            if (owns_step || supplies_payload)
                && self.event_actor_is_local(idx, selected_arm_for_scope) != Some(false)
                && let Some(row) = self.machine().event_program().event_row_at(idx)
                && self.event_conflict_row_allows(
                    row.conflict(),
                    ScopeId::none(),
                    None,
                    selected_arm_for_scope,
                )
            {
                return false;
            }
        }
        true
    }

    /// Query completion of every live event in this actor's committed image.
    /// Unlike a logical-view query, this needs no source ownership lookup.
    pub(crate) fn is_complete(
        &self,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.event_row_set_live_events_done(
            LocalEventRowSet::new(0, self.local_steps_len()),
            selected_arm_for_scope,
        )
    }

    pub(super) fn selected_route_arm_event_row_done(
        &self,
        scope_id: ScopeId,
        arm: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let Some(slot) = self.route_scope_slot_inner(scope_id) else {
            return false;
        };
        let Some(row_set) = self
            .machine()
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)
        else {
            return false;
        };
        self.event_row_set_live_events_done(row_set, selected_arm_for_scope)
    }

    pub(crate) fn reentrant_route_arm_event_row_done(
        &self,
        scope_id: ScopeId,
        arm: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.route_scope_reentry(scope_id)
            && self.selected_route_arm_event_row_done(scope_id, arm, selected_arm_for_scope)
    }

    pub(super) fn selected_route_arm_completes_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        if self.route_scope_reentry(scope_id) {
            return false;
        }
        self.selected_route_arm_event_row_done(scope_id, arm, selected_arm_for_scope)
    }

    pub(crate) fn selected_arm_for_reentry_preview_conflict(
        &self,
        scope_id: ScopeId,
        preview_conflict: crate::global::typestate::PackedEventConflict,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<u8> {
        let selected = selected_arm_for_scope(scope_id)?;
        if self
            .preview_conflict_arm(preview_conflict, scope_id)
            .is_some_and(|preview| preview != selected)
            && self.route_scope_reentry(scope_id)
            && self.selected_route_arm_event_row_done(scope_id, selected, selected_arm_for_scope)
        {
            return None;
        }
        Some(selected)
    }

    #[inline(never)]
    pub(super) fn dependency_row_live_events_done(
        &self,
        target: usize,
        dependency: LocalDependency,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let row_set = self
            .machine()
            .event_program()
            .dependency_row_set(dependency);
        self.row_set_live_events_done(row_set, Some(target), selected_arm_for_scope)
    }

    pub(super) fn completed_retained_route_end(
        &self,
        predecessor: usize,
        target: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        if self.machine().event_program().footprint().roll_scope_count == 0 {
            return None;
        }
        let chain =
            self.route_commit_range_for_conflict(self.event_conflict_for_index(predecessor))?;
        for row in self.route_commit_rows(chain) {
            let Some(super::LocalConflict::RouteArm { scope, .. }) = row.to_conflict() else {
                crate::invariant();
            };
            if selected(scope).is_some() {
                continue;
            }
            let range = crate::invariant_some(self.route_scope_rows(scope));
            if target < range.end()
                || !self
                    .machine()
                    .event_program()
                    .roll_ancestors(predecessor)
                    .any(|roll| {
                        let body = crate::invariant_some(self.machine().roll_scope_row(roll));
                        body.start() <= range.start()
                            && range.end() <= body.end()
                            && !(body.start() <= target && target < body.end())
                            && self.roll_has_retained_completion(roll)
                    })
            {
                return None;
            }
            let slot = crate::invariant_some(self.route_scope_slot_inner(scope));
            for arm in 0..=1 {
                let rows = crate::invariant_some(
                    self.machine()
                        .event_program()
                        .route_arm_event_row_by_slot(slot, arm),
                );
                // This discharges only a completed arm in a retained visit.
                // It does not publish that historical choice as this visit's
                // decision, or use old completion to skip unfinished work.
                if rows.start() < rows.end() && self.current_route_arm_done(scope, rows, selected) {
                    return Some(range.end());
                }
            }
            return None;
        }
        None
    }

    /// Search the finite projected choice tree without recursive calls or a
    /// stack proportional to source depth. Completed rows justify a historical
    /// path; no trial choice is stored in the actor's decision state.
    fn current_route_arm_done(
        &self,
        root: ScopeId,
        rows: LocalEventRowSet,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let mut index = rows.start();
        while index < rows.end() {
            let conflict = self.event_conflict_for_index(index);
            match self.event_conflict_row_admission(conflict, ScopeId::none(), None, selected) {
                ConflictAdmission::Excluded { scope, arm } => {
                    index = self.excluded_arm_end(index, scope, arm).min(rows.end());
                    continue;
                }
                ConflictAdmission::Allowed | ConflictAdmission::Invalid => {}
            }
            let chain = crate::invariant_some(self.route_commit_range_for_conflict(conflict));
            let mut next = index + 1;
            let mut completed_scope = None;
            if !self.local_event_done(index)
                && self.event_actor_is_local(index, &mut |scope| {
                    selected(scope).or_else(|| self.preview_conflict_arm(conflict, scope))
                }) != Some(false)
            {
                let mut retry = None;
                for row in self.route_commit_rows(chain).rev() {
                    let Some(super::LocalConflict::RouteArm { scope, arm }) = row.to_conflict()
                    else {
                        crate::invariant();
                    };
                    if scope.same(root) {
                        break;
                    }
                    if arm == 0 && selected(scope).is_none() {
                        retry = Some(scope);
                        break;
                    }
                }
                let Some(scope) = retry else {
                    return false;
                };
                let slot = crate::invariant_some(self.route_scope_slot_inner(scope));
                let alternative = crate::invariant_some(
                    self.machine()
                        .event_program()
                        .route_arm_event_row_by_slot(slot, 1),
                );
                if alternative.start() < alternative.end() {
                    index = alternative.start();
                    continue;
                }
                next = crate::invariant_some(self.route_scope_rows(scope)).end();
                completed_scope = Some(scope);
            }
            // A successful arm skips its alternative. An empty alternative
            // completes its route before continuing through enclosing arms.
            for row in self.route_commit_rows(chain).rev() {
                let Some(super::LocalConflict::RouteArm { scope, arm }) = row.to_conflict() else {
                    crate::invariant();
                };
                if let Some(completed) = completed_scope {
                    if completed == scope {
                        completed_scope = None;
                    }
                    continue;
                }
                if scope.same(root) {
                    break;
                }
                let slot = crate::invariant_some(self.route_scope_slot_inner(scope));
                let arm_rows = crate::invariant_some(
                    self.machine()
                        .event_program()
                        .route_arm_event_row_by_slot(slot, arm),
                );
                if next != arm_rows.end() {
                    break;
                }
                next = crate::invariant_some(self.route_scope_rows(scope)).end();
            }
            index = next;
        }
        true
    }

    fn row_set_live_events_done(
        &self,
        row_set: LocalEventRowSet,
        target: Option<usize>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let local_len = self.local_steps_len();
        if row_set.start() > local_len || row_set.end() > local_len {
            crate::invariant();
        }
        let retained_completion = target.is_some() && self.has_retained_roll_completion();
        let retained = target.is_some_and(|target| self.is_recv_at(target)) && retained_completion;
        let mut pending = self.pending_event_steps(row_set.start()..row_set.end());
        while let Some(idx) = pending.next() {
            if retained
                && let Some(end) =
                    self.retained_roll_predecessor_end(idx, crate::invariant_some(target))
            {
                pending = self.pending_event_steps(end.min(row_set.end())..row_set.end());
                continue;
            }
            match self.event_conflict_row_admission(
                self.event_conflict_for_index(idx),
                ScopeId::none(),
                None,
                selected_arm_for_scope,
            ) {
                ConflictAdmission::Allowed | ConflictAdmission::Invalid => {
                    if retained_completion
                        && let Some(target) = target
                        && let Some(end) =
                            self.completed_retained_route_end(idx, target, selected_arm_for_scope)
                    {
                        pending = self.pending_event_steps(end.min(row_set.end())..row_set.end());
                        continue;
                    }
                    if self.event_actor_is_local(idx, selected_arm_for_scope) != Some(false) {
                        return false;
                    }
                }
                ConflictAdmission::Excluded { scope, arm } => {
                    let end = self.excluded_arm_end(idx, scope, arm).min(row_set.end());
                    pending = self.pending_event_steps(end..row_set.end());
                }
            }
        }
        true
    }

    #[inline(never)]
    pub(super) fn event_row_set_live_events_done(
        &self,
        row_set: LocalEventRowSet,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.row_set_live_events_done(row_set, None, selected_arm_for_scope)
    }
}
