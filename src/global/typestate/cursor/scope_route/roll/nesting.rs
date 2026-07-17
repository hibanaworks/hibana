use super::super::super::{EventCursor, ScopeId};

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

    pub(super) fn outer_roll_scope(&self, current: ScopeId, candidate: ScopeId) -> ScopeId {
        if self.roll_scope_nested_within(current, candidate) {
            return candidate;
        }
        if self.roll_scope_nested_within(candidate, current) {
            return current;
        }
        crate::invariant();
    }
}
