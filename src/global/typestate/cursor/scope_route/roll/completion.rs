use super::super::super::{EventArmView, EventCursor, ScopeId};

impl EventCursor {
    fn roll_epoch_word_start(&self) -> usize {
        (self.local_steps_len() + self.machine().event_program().footprint().roll_scope_count)
            .div_ceil(32)
    }

    pub(crate) fn roll_reentry_epoch(&self, scope: ScopeId) -> u64 {
        let mut slot = 0;
        while let Some((candidate, _)) = self.machine().roll_scope_row_by_slot(slot) {
            if candidate.same(scope) {
                let index = self.roll_epoch_word_start() + 2 * slot;
                let words = self.completed_event_words();
                return u64::from(words[index]) | (u64::from(words[index + 1]) << 32);
            }
            slot += 1;
        }
        crate::invariant()
    }

    pub(crate) fn completed_parallel_reentry(
        &self,
        start: usize,
        end: usize,
        selected: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> Option<(ScopeId, u64)> {
        if self
            .machine()
            .event_program()
            .footprint()
            .borrow_epoch_count
            == 0
        {
            return None;
        }
        let mut selected_scope = None;
        for marker in 0..self.program_ref().source_scope_marker_count() {
            let Some((scope, body_start, body_end)) = self.program_ref().source_roll_at(marker)
            else {
                continue;
            };
            if body_start > start || end > body_end {
                continue;
            }
            let Some(row) = self.machine().roll_scope_row(scope) else {
                continue;
            };
            let mut committed = |scope| selected(scope, EventArmView::Committed);
            if !self.roll_scope_events_complete(row, &mut committed) {
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
                start <= event.dense_ordinal() && event.dense_ordinal() < end
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
            && self
                .machine()
                .roll_scope_row(scope)
                .is_some_and(|row| self.roll_scope_events_complete(row, selected))
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

    /// A restart replaces the current visit, but cannot revoke prerequisites
    /// already established for the enclosing continuation. These bits belong
    /// to that enclosing visit: restarting an ancestor clears its descendants.
    pub(super) fn retain_reentry_completion(&mut self, restarting: ScopeId) {
        let Some((start, end)) = self.reentry_scope_event_bounds(restarting) else {
            crate::invariant();
        };
        let mut slot = 0;
        while let Some((scope, range)) = self.machine().roll_scope_row_by_slot(slot) {
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
                    self.completed_event_words_mut().get_mut(index..index + 2),
                );
                words[0] = epoch as u32;
                words[1] = (epoch >> 32) as u32;
            }
            if scope.same(restarting) {
                self.write_completion_bit(self.local_steps_len() + slot, true);
            } else if start <= range.start()
                && range.end() <= end
                && (range.start() != start
                    || range.end() != end
                    || restarting.local_ordinal() < scope.local_ordinal())
            {
                self.write_completion_bit(self.local_steps_len() + slot, false);
            }
            slot += 1;
        }
    }

    /// Only a prerequisite inside a completed visit can serve an operation
    /// outside it. A fresh operation inside that visit still needs its current
    /// predecessors. No old completion can authorize another body operation.
    #[inline]
    pub(crate) fn retained_roll_allows_predecessor(
        &self,
        predecessor: usize,
        target: usize,
    ) -> bool {
        // A new outgoing acceptance belongs to the current local visit. Only
        // a peer's already accepted incoming continuation can arrive late.
        if self.machine().event_program().footprint().roll_scope_count == 0
            || !self.is_recv_at(target)
        {
            return false;
        }
        let mut slot = 0;
        while let Some((_, range)) = self.machine().roll_scope_row_by_slot(slot) {
            if range.start() <= predecessor
                && predecessor < range.end()
                && !(range.start() <= target && target < range.end())
            {
                let index = self.local_steps_len() + slot;
                if self.completed_event_words()[index / 32] & (1 << (index % 32)) != 0 {
                    return true;
                }
            }
            slot += 1;
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

    /// Forward continuation candidates begin after a retained body containing
    /// this head. A roll with no suffix needs no candidate scan.
    pub(crate) fn retained_continuation_start(&self, head: usize) -> Option<usize> {
        let mut first = None;
        let mut slot = 0;
        while let Some((_, range)) = self.machine().roll_scope_row_by_slot(slot) {
            if range.start() <= head && head < range.end() && range.end() < self.local_steps_len() {
                let index = self.local_steps_len() + slot;
                if self.completed_event_words()[index / 32] & (1 << (index % 32)) != 0 {
                    first = Some(first.map_or(range.end(), |end: usize| end.min(range.end())));
                }
            }
            slot += 1;
        }
        first
    }
}
