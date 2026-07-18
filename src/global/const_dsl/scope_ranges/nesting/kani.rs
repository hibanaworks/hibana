use super::{StructuredScopeRange, innermost_scope_range, outermost_scope_range};
use crate::global::const_dsl::ScopeId;

#[kani::proof]
fn equal_projected_scope_ranges_follow_source_preorder() {
    let candidate = (kani::any::<u16>(), kani::any::<u16>());
    let (outer_ordinal, inner_ordinal) =
        if candidate.0 < candidate.1 && candidate.1 <= ScopeId::MAX_LOCAL_ORDINAL {
            candidate
        } else {
            (0, 1)
        };

    let outer = StructuredScopeRange::new(ScopeId::roll_scope(outer_ordinal), 9, 13);
    let inner = StructuredScopeRange::new(ScopeId::roll_scope(inner_ordinal), 9, 13);

    assert!(
        innermost_scope_range(outer, inner)
            .scope()
            .same(inner.scope())
    );
    assert!(
        outermost_scope_range(outer, inner)
            .scope()
            .same(outer.scope())
    );
}

#[kani::proof]
fn strict_scope_containment_is_authoritative() {
    let candidate = (
        kani::any::<u16>(),
        kani::any::<u16>(),
        kani::any::<u16>(),
        kani::any::<u16>(),
    );
    let (outer_start, inner_start, inner_end, outer_end) = if candidate.0 <= candidate.1
        && candidate.1 < candidate.2
        && candidate.2 <= candidate.3
        && (candidate.0 < candidate.1 || candidate.2 < candidate.3)
    {
        candidate
    } else {
        (0, 0, 1, 2)
    };

    let outer = StructuredScopeRange::new(
        ScopeId::roll_scope(1),
        outer_start as usize,
        outer_end as usize,
    );
    let inner = StructuredScopeRange::new(
        ScopeId::roll_scope(0),
        inner_start as usize,
        inner_end as usize,
    );

    assert!(
        innermost_scope_range(outer, inner)
            .scope()
            .same(inner.scope())
    );
    assert!(
        outermost_scope_range(outer, inner)
            .scope()
            .same(outer.scope())
    );
}

#[kani::proof]
#[kani::should_panic]
fn crossing_scope_ranges_are_rejected() {
    let left = StructuredScopeRange::new(ScopeId::roll_scope(0), 0, 2);
    let right = StructuredScopeRange::new(ScopeId::roll_scope(1), 1, 3);
    let _ = innermost_scope_range(left, right);
}
