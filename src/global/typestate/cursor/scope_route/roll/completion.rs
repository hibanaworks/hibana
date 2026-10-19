use super::super::super::{EventCursor, ScopeId};

impl EventCursor {
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
        let word = start / 32;
        let words = self.completed_event_words();
        words[word] & (u32::MAX << (start % 32)) != 0
            || words[word + 1..].iter().any(|&bits| bits != 0)
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
