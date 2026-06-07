use super::super::{EventCursor, LocalDependency, ScopeId};
use crate::global::event_program::LocalEventRowSet;

impl EventCursor {
    pub(super) fn selected_route_arm_event_row_done(
        &self,
        scope_id: ScopeId,
        arm: u8,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
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
        self.event_row_set_live_events_done(row_set, |scope| selected_arm_for_scope(scope))
    }

    pub(super) fn dependency_row_live_events_done(
        &self,
        dependency: LocalDependency,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let row_set = self
            .machine()
            .event_program()
            .dependency_row_set(dependency);
        self.event_row_set_live_events_done(row_set, |scope| selected_arm_for_scope(scope))
    }

    fn event_row_set_live_events_done(
        &self,
        row_set: LocalEventRowSet,
        mut selected_arm_for_scope: impl FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let start = row_set.start().min(self.local_steps_len());
        let end = row_set.end().min(self.local_steps_len());
        if start >= end {
            return true;
        }

        let mut word_idx = start / u32::BITS as usize;
        let end_word = (end - 1) / u32::BITS as usize;
        while word_idx <= end_word {
            let word_start = word_idx.saturating_mul(u32::BITS as usize);
            let low = start.saturating_sub(word_start).min(u32::BITS as usize);
            let high = end.saturating_sub(word_start).min(u32::BITS as usize);
            let low_mask = u32::MAX << low;
            let high_mask = if high >= u32::BITS as usize {
                u32::MAX
            } else {
                (1u32 << high) - 1
            };
            let row_mask = low_mask & high_mask;
            let completed = self
                .completed_event_words()
                .get(word_idx)
                .copied()
                .unwrap_or(0);
            let mut pending = (!completed) & row_mask;
            while pending != 0 {
                let bit = pending.trailing_zeros() as usize;
                let idx = word_start.saturating_add(bit);
                if let Some(row) = self.machine().event_program().event_row_at(idx)
                    && self.event_conflict_row_allows(
                        row.conflict(),
                        ScopeId::none(),
                        None,
                        |scope| selected_arm_for_scope(scope),
                    )
                {
                    return false;
                }
                pending &= pending - 1;
            }
            word_idx = word_idx.saturating_add(1);
        }
        true
    }
}
