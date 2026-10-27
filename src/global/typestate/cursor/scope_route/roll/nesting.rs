use super::super::super::{EventCursor, ScopeId};
use crate::global::event_program::LocalEventRowSet;

impl EventCursor {
    pub(super) fn roll_scope_nested_within(&self, inner: ScopeId, outer: ScopeId) -> bool {
        if inner.same(outer) {
            return true;
        }
        let Some(inner_row) = self.machine().roll_scope_row(inner) else {
            crate::invariant();
        };
        let Some(outer_row) = self.machine().roll_scope_row(outer) else {
            crate::invariant();
        };
        Self::roll_rows_nested_within((inner, inner_row), (outer, outer_row))
    }

    pub(super) fn roll_rows_nested_within(
        (inner, inner_row): (ScopeId, LocalEventRowSet),
        (outer, outer_row): (ScopeId, LocalEventRowSet),
    ) -> bool {
        if inner.same(outer) {
            return true;
        }
        let contained =
            outer_row.start() <= inner_row.start() && inner_row.end() <= outer_row.end();
        if !contained {
            return false;
        }
        outer_row != inner_row || outer.local_ordinal() < inner.local_ordinal()
    }

    pub(super) fn deeper_roll_scope(&self, current: ScopeId, candidate: ScopeId) -> ScopeId {
        if self.roll_scope_nested_within(current, candidate) {
            return current;
        }
        if self.roll_scope_nested_within(candidate, current) {
            return candidate;
        }
        crate::invariant();
    }
}
