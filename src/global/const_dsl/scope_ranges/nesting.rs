use crate::global::const_dsl::ScopeId;

#[derive(Clone, Copy)]
pub(crate) struct StructuredScopeRange {
    scope: ScopeId,
    start: usize,
    end: usize,
}

impl StructuredScopeRange {
    pub(crate) const fn new(scope: ScopeId, start: usize, end: usize) -> Self {
        if scope.is_none() || start >= end {
            panic!("structured scope range must be present and nonempty");
        }
        Self { scope, start, end }
    }

    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    const fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

const fn equal_range_inner(left: StructuredScopeRange, right: StructuredScopeRange) -> bool {
    let left_ordinal = left.scope.local_ordinal();
    let right_ordinal = right.scope.local_ordinal();
    if left_ordinal == right_ordinal {
        panic!("distinct structured scopes must have distinct preorder ordinals");
    }
    right_ordinal > left_ordinal
}

/// Selects the structurally innermost of two nested source ranges. Strict
/// containment is authoritative. Equal projected ranges use the source
/// preorder ordinal, where an enclosing scope is allocated before its child.
pub(crate) const fn innermost_scope_range(
    left: StructuredScopeRange,
    right: StructuredScopeRange,
) -> StructuredScopeRange {
    if left.scope.same(right.scope) {
        panic!("scope nesting comparison requires distinct scopes");
    }
    let left_contains_right = left.contains(right);
    let right_contains_left = right.contains(left);
    match (left_contains_right, right_contains_left) {
        (true, false) => right,
        (false, true) => left,
        (true, true) => {
            if equal_range_inner(left, right) {
                right
            } else {
                left
            }
        }
        (false, false) => panic!("structured scope ranges cross without nesting"),
    }
}

pub(crate) const fn outermost_scope_range(
    left: StructuredScopeRange,
    right: StructuredScopeRange,
) -> StructuredScopeRange {
    let inner = innermost_scope_range(left, right);
    if inner.scope.same(left.scope) {
        right
    } else {
        left
    }
}

#[cfg(kani)]
mod kani;

#[cfg(test)]
mod tests {
    use super::{StructuredScopeRange, innermost_scope_range, outermost_scope_range};
    use crate::global::const_dsl::ScopeId;

    #[test]
    fn equal_projected_ranges_follow_source_preorder_nesting() {
        let outer = StructuredScopeRange::new(ScopeId::roll_scope(3), 7, 11);
        let middle = StructuredScopeRange::new(ScopeId::roll_scope(4), 7, 11);
        let inner = StructuredScopeRange::new(ScopeId::roll_scope(5), 7, 11);

        let selected = innermost_scope_range(innermost_scope_range(outer, middle), inner);
        assert!(selected.scope().same(inner.scope()));
        assert!(
            outermost_scope_range(inner, outer)
                .scope()
                .same(outer.scope())
        );
    }

    #[test]
    #[should_panic(expected = "cross without nesting")]
    fn crossing_scope_ranges_fail_closed() {
        let left = StructuredScopeRange::new(ScopeId::roll_scope(3), 0, 2);
        let right = StructuredScopeRange::new(ScopeId::roll_scope(4), 1, 3);
        let _ = innermost_scope_range(left, right);
    }
}
