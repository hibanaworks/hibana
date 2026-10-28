use super::super::super::{EventArmView, EventCursor, ScopeId};

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[inline]
fn read_word_pair(words: &[u32; 2]) -> u64 {
    u64::from(words[0]) | (u64::from(words[1]) << 32)
}

#[inline]
fn write_word_pair(words: &mut [u32; 2], value: u64) {
    words[0] = value as u32;
    words[1] = (value >> 32) as u32;
}

impl EventCursor {
    fn roll_credit_word_start(&self) -> usize {
        self.roll_epoch_word_start()
            + 2 * self
                .machine()
                .event_program()
                .footprint()
                .borrow_epoch_count
    }

    fn roll_credit_words(&self, scope: ScopeId) -> usize {
        let slot = crate::invariant_some(self.machine().event_program().roll_scope_slot(scope));
        self.roll_credit_word_start() + 2 * slot
    }

    pub(super) fn completed_roll_credits(&self, scope: ScopeId) -> u64 {
        let index = self.roll_credit_words(scope);
        let words = self.completed_event_words();
        read_word_pair(crate::invariant_some(words[index..].first_chunk()))
    }

    fn set_completed_roll_credits(&mut self, scope: ScopeId, value: u64) {
        let index = self.roll_credit_words(scope);
        #[cfg(any(kani, all(test, hibana_repo_tests)))]
        self.record_completion_mutation();
        let words =
            crate::invariant_some(self.completed_event_words_mut()[index..].first_chunk_mut());
        write_word_pair(words, value);
    }

    /// The outermost repeated child with an unconsumed completed visit can
    /// contribute to this parent while its current visit keeps progressing.
    pub(crate) fn credited_roll_child(&self, parent: ScopeId, index: usize) -> Option<ScopeId> {
        if !matches!(
            parent.kind(),
            Some(crate::global::const_dsl::ScopeKind::Roll)
        ) || self
            .machine()
            .event_program()
            .footprint()
            .borrow_epoch_count
            == 0
        {
            return None;
        }
        let parent_row = crate::invariant_some(self.machine().roll_scope_row(parent));
        if index < parent_row.start() || parent_row.end() <= index {
            return None;
        }
        let credit_start = self.roll_credit_word_start();
        let credits = self.completed_event_words();
        let mut found = None;
        for scope in self.machine().event_program().roll_ancestors(index) {
            if scope.same(parent) {
                break;
            }
            let slot = crate::invariant_some(self.machine().event_program().roll_scope_slot(scope));
            if read_word_pair(crate::invariant_some(
                credits[credit_start + 2 * slot..].first_chunk(),
            )) != 0
            {
                found = Some(scope);
            }
        }
        found
    }

    pub(crate) fn roll_reentry_preserved_child(
        &self,
        parent: ScopeId,
        index: usize,
        entry: Option<usize>,
    ) -> Option<ScopeId> {
        let entry = entry?;
        let child = self.credited_roll_child(parent, index)?;
        if self.roll_scope_contains_index(child, entry) {
            return None;
        }
        self.roll_has_parallel_entry(child, entry).then_some(child)
    }

    fn roll_has_parallel_entry(&self, child: ScopeId, entry: usize) -> bool {
        let program = self.program_ref();
        let source = crate::invariant_some(self.machine().event_program().operation_at(entry))
            .0
            .dense_ordinal();
        program.source_roll_has_parallel_entry(child, source)
    }

    /// A child restart retains a completed parent visit, including completion
    /// supplied by a parallel sibling that has already entered its next visit.
    /// The entering child's own current visit must still be complete.
    pub(super) fn roll_reset_retains_credit(
        &self,
        child: ScopeId,
        entry: Option<usize>,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        if self
            .machine()
            .event_program()
            .footprint()
            .borrow_epoch_count
            == 0
        {
            return false;
        }
        let Some(child_row) = self.machine().roll_scope_row(child) else {
            return false;
        };
        if self.completed_roll_credits(child) != 0 {
            return true;
        }
        let mut slot = 0;
        while let Some((parent, row)) = self.machine().roll_scope_row_by_slot(slot) {
            if !parent.same(child)
                && Self::roll_rows_nested_within((child, child_row), (parent, row))
                && self.roll_scope_events_complete_at(parent, entry, selected)
                && (row.start()..row.end()).any(|index| {
                    !self.roll_scope_contains_index(child, index)
                        && self.event_lane_at(index).is_some()
                        && self.event_actor_is_local(index, selected) != Some(false)
                        && self.event_conflict_row_allows(
                            self.machine().event_conflict_for_index(index),
                            ScopeId::none(),
                            None,
                            selected,
                        )
                        && self.roll_has_parallel_entry(child, index)
                })
            {
                return true;
            }
            slot += 1;
        }
        false
    }

    pub(crate) fn roll_reentry_preserves_route(
        &self,
        parent: ScopeId,
        route: ScopeId,
        entry: usize,
    ) -> bool {
        let Some(row) = self.route_scope_rows(route) else {
            return false;
        };
        self.roll_reentry_preserved_child(parent, row.start(), Some(entry))
            .is_some_and(|child| self.route_scope_contained_in_roll_scope(route, child))
    }

    pub(crate) fn roll_reentry_preserves_view(
        &self,
        parent: ScopeId,
        role: u8,
        entry: usize,
    ) -> bool {
        let Some(row) = self.machine().roll_scope_row(parent) else {
            return false;
        };
        let mut used = false;
        for index in row.start()..row.end() {
            let (event, _) =
                crate::invariant_some(self.machine().event_program().operation_at(index));
            let atom = self.program_ref().event_atom_at(event.dense_ordinal());
            if atom.from == role
                || atom.to == role
                || atom.payload == crate::eff::EventPayload::Endpoint(role)
            {
                if self
                    .roll_reentry_preserved_child(parent, index, Some(entry))
                    .is_none()
                {
                    return false;
                }
                used = true;
            }
        }
        used
    }

    fn roll_epoch_word_start(&self) -> usize {
        (self.local_steps_len() + self.machine().event_program().footprint().roll_scope_count)
            .div_ceil(32)
    }

    pub(crate) fn roll_reentry_epoch(&self, scope: ScopeId) -> u64 {
        let slot = crate::invariant_some(self.machine().event_program().roll_scope_slot(scope));
        let index = self.roll_epoch_word_start() + 2 * slot;
        let words = self.completed_event_words();
        read_word_pair(crate::invariant_some(words[index..].first_chunk()))
    }

    pub(crate) fn completed_parallel_reentry(
        &self,
        start: usize,
        end: usize,
        selected: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Option<(ScopeId, u64)> {
        if start >= end
            || self
                .machine()
                .event_program()
                .footprint()
                .borrow_epoch_count
                == 0
        {
            return None;
        }
        let program = self.program_ref();
        let mut selected_scope = None;
        for marker in 0..program.source_scope_marker_count() {
            let Some((scope, body_start, body_end)) = program.source_roll_at(marker) else {
                continue;
            };
            if body_start > start || end > body_end {
                continue;
            }
            let Some(row) = self.machine().roll_scope_row(scope) else {
                continue;
            };
            let mut committed = |scope| selected(scope, EventArmView::Committed);
            if !(row.start()..row.end())
                .any(|index| self.roll_scope_complete_for_entry(scope, index, &mut committed))
            {
                continue;
            }
            let mut entry = |scope| selected(scope, EventArmView::Preview);
            let covered = (row.start()..row.end()).all(|index| {
                if self.event_actor_is_local(index, &mut entry) == Some(false)
                    || !self.event_conflict_row_allows(
                        self.machine().event_conflict_for_index(index),
                        ScopeId::none(),
                        None,
                        &mut entry,
                    )
                {
                    return true;
                }
                let (event, _) =
                    crate::invariant_some(self.machine().event_program().operation_at(index));
                let source = event.dense_ordinal();
                // The borrow starts the new visit. Its causal continuation
                // remains outside the borrow and runs after the parallel join.
                (start <= source && source < end) || program.source_precedes(end - 1, source)
            });
            if covered {
                selected_scope = Some(
                    selected_scope.map_or(scope, |prior| self.deeper_roll_scope(prior, scope)),
                );
            }
        }
        selected_scope.map(|scope| (scope, self.roll_reentry_epoch(scope)))
    }

    pub(crate) fn borrowed_roll_reentry_is_current(
        &self,
        scope: ScopeId,
        epoch: u64,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.roll_reentry_epoch(scope) == epoch
            && self.machine().roll_scope_row(scope).is_some_and(|row| {
                (row.start()..row.end())
                    .any(|index| self.roll_scope_complete_for_entry(scope, index, selected))
            })
    }

    pub(crate) fn roll_reentry_epochs_available(&self, scope: ScopeId) -> bool {
        if self
            .machine()
            .event_program()
            .footprint()
            .borrow_epoch_count
            == 0
        {
            return true;
        }
        let Some((start, end)) = self.reentry_scope_event_bounds(scope) else {
            return false;
        };
        let mut slot = 0;
        while let Some((candidate, row)) = self.machine().roll_scope_row_by_slot(slot) {
            if start <= row.start()
                && row.end() <= end
                && (candidate.same(scope)
                    || row.start() != start
                    || row.end() != end
                    || scope.local_ordinal() < candidate.local_ordinal())
                && self.roll_reentry_epoch(candidate) == u64::MAX
            {
                return false;
            }
            slot += 1;
        }
        true
    }

    /// Copy existing scope completions for qualification without treating a
    /// raw admission result as the evidence for its own prerequisites.
    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn retained_roll_scopes(&self) -> std::vec::Vec<u16> {
        let mut scopes = std::vec::Vec::new();
        let mut slot = 0;
        while let Some((scope, _)) = self.machine().roll_scope_row_by_slot(slot) {
            let index = self.local_steps_len() + slot;
            if self.completed_event_words()[index / 32] & (1 << (index % 32)) != 0 {
                scopes.push(scope.local_ordinal());
            }
            slot += 1;
        }
        scopes
    }

    /// A parent restart consumes one completed visit of each independent child
    /// whose current progress is retained. Its sequential descendants restart
    /// with the new parent visit.
    pub(super) fn retain_reentry_completion(
        &mut self,
        restarting: ScopeId,
        entry: Option<usize>,
        retain_credit: bool,
    ) {
        let Some((start, end)) = self.reentry_scope_event_bounds(restarting) else {
            crate::invariant();
        };
        let credits = self
            .machine()
            .event_program()
            .footprint()
            .borrow_epoch_count
            != 0;
        let count = self.machine().event_program().footprint().roll_scope_count;
        for slot in (0..count).rev() {
            let (scope, range) = crate::invariant_some(self.machine().roll_scope_row_by_slot(slot));
            let preserved = if credits && !scope.same(restarting) {
                self.roll_reentry_preserved_child(restarting, range.start(), entry)
                    .filter(|ancestor| self.roll_scope_nested_within(scope, *ancestor))
            } else {
                None
            };
            if let Some(ancestor) = preserved {
                if ancestor.same(scope) {
                    self.set_completed_roll_credits(scope, 0);
                }
                continue;
            }
            let contained = start <= range.start()
                && range.end() <= end
                && (scope.same(restarting)
                    || range.start() != start
                    || range.end() != end
                    || restarting.local_ordinal() < scope.local_ordinal());
            if contained
                && self
                    .machine()
                    .event_program()
                    .footprint()
                    .borrow_epoch_count
                    != 0
            {
                let index = self.roll_epoch_word_start() + 2 * slot;
                let epoch = crate::invariant_some(self.roll_reentry_epoch(scope).checked_add(1));
                #[cfg(any(kani, all(test, hibana_repo_tests)))]
                self.record_completion_mutation();
                let words = crate::invariant_some(
                    self.completed_event_words_mut()[index..].first_chunk_mut(),
                );
                write_word_pair(words, epoch);
            }
            if scope.same(restarting) {
                if credits {
                    self.set_completed_roll_credits(scope, u64::from(retain_credit));
                }
                self.write_completion_bit(self.local_steps_len() + slot, true);
            } else if start <= range.start()
                && range.end() <= end
                && (range.start() != start
                    || range.end() != end
                    || restarting.local_ordinal() < scope.local_ordinal())
            {
                if credits {
                    self.set_completed_roll_credits(scope, 0);
                }
                self.write_completion_bit(self.local_steps_len() + slot, false);
            }
        }
    }

    /// End of the outermost retained visit that covers this prerequisite but
    /// not the incoming target. Every row from the prerequisite to this end
    /// has the same retained-completion justification and can be skipped.
    #[inline]
    pub(crate) fn retained_roll_predecessor_end(
        &self,
        predecessor: usize,
        target: usize,
    ) -> Option<usize> {
        // A new outgoing acceptance belongs to the current local visit. Only
        // a peer's already accepted incoming continuation can arrive late.
        if self.machine().event_program().footprint().roll_scope_count == 0
            || !self.is_recv_at(target)
        {
            return None;
        }
        let mut retained_end = None;
        for scope in self.machine().event_program().roll_ancestors(predecessor) {
            let slot = crate::invariant_some(self.machine().event_program().roll_scope_slot(scope));
            let (_, range) = crate::invariant_some(self.machine().roll_scope_row_by_slot(slot));
            if !(range.start() <= target && target < range.end()) {
                let index = self.local_steps_len() + slot;
                if self.completed_event_words()[index / 32] & (1 << (index % 32)) != 0 {
                    retained_end =
                        Some(retained_end.map_or(range.end(), |end: usize| end.max(range.end())));
                }
            }
        }
        retained_end
    }

    /// An earlier completed visit can keep a later receive enabled while an
    /// inner parallel region runs in the current visit. The region must be
    /// wholly inside that retained roll; independent siblings remain required.
    pub(crate) fn retained_roll_continuation_after(
        &self,
        index: usize,
        source: core::ops::Range<usize>,
    ) -> bool {
        if source.start >= source.end
            || !self.is_recv_at(index)
            || !self.has_retained_roll_completion()
        {
            return false;
        }
        let Some((event, _)) = self.machine().event_program().operation_at(index) else {
            return false;
        };
        let program = self.program_ref();
        for marker in 0..program.source_scope_marker_count() {
            let Some((scope, start, end)) = program.source_roll_at(marker) else {
                continue;
            };
            if start > source.start
                || source.end > end
                || !program.source_precedes(end - 1, event.dense_ordinal())
            {
                continue;
            }
            let mut slot = 0;
            while let Some((candidate, _)) = self.machine().roll_scope_row_by_slot(slot) {
                if candidate.same(scope) {
                    let bit = self.local_steps_len() + slot;
                    if self.completed_event_words()[bit / 32] & (1 << (bit % 32)) != 0 {
                        return true;
                    }
                    break;
                }
                slot += 1;
            }
        }
        false
    }

    pub(crate) fn has_retained_roll_completion(&self) -> bool {
        let count = self.machine().event_program().footprint().roll_scope_count;
        if count == 0 {
            return false;
        }
        let start = self.local_steps_len();
        let words = self.completed_event_words();
        (start..start + count).any(|index| words[index / 32] & (1 << (index % 32)) != 0)
    }
}
