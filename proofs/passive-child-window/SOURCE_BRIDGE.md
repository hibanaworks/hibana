# Passive child equal-offset window

Base: `ff6d167ed23f2f566d16affdeedd3b2ca8569cc2` of Hibana. Production mutation
is restricted to `passive_route_child_scope` in the isolated snapshot.

The proposed change replaces `idx = 0` with
`idx = scope_markers.offset_lower_bound(arm_start, 0)` and breaks immediately
after reading the first marker with `marker.offset() > arm_start`. The existing
invalid-arm/kind guard, parent lookup, range validation, full candidate predicate,
candidate range decoding, outermost fold, tie ordering, and result remain exact.

## Source-to-model correspondence

* `ScopeMarkerView::offset_lower_bound` is the half-open search in
  `PassiveChildWindow.search`, with midpoint `low + (high-low)/2`, `< offset`
  selecting the right half, and otherwise selecting the left half. It returns
  the first equal marker, not an arbitrary equal marker. The Lean model uses
  fuel `n+1`; the strictly decreasing interval theorem proves that fuel cannot
  be exhausted. Rust needs no new loop bound or evaluator limit.
* Source marker offsets are nondecreasing. The sole production construction
  of `ScopeMarkerView` is `EffList::scope_markers`; its private marker partition
  is populated by `insert_scope_marker_mut`. That insertion shifts every larger
  offset right and changes equal-offset order only within the equal group.
  Route, roll, and parallel insertion all go through that function. Direct
  struct construction elsewhere is confined to test/Kani fixtures.
* Every view row is an in-bounds `SourceRow::Scope`. The constructor and private
  arena partition establish this; the rewrite is not a decoder for untrusted
  runtime image bytes. Consequently omitted noncandidate `at` calls cannot hide
  a reachable invalid-row check. The existing lower-bound helper already relies
  on this same sorted, valid-partition contract in route/parallel scope search.
* The old fold changes state or checks candidate range/nesting only when the
  marker offset equals `arm_start`. All skipped rows therefore act as identity.
  Lean's generic `visit` includes the unchanged primary/kind/scope predicate,
  `closed_route_arm_ranges_from_first_enter`, containment, outermost selection,
  and their error outcomes. It does not assume commutativity, unique offsets,
  dense ordinals, a tree, or panic-free candidate decoding.
* `no_equal_marker_skipped` establishes that the lower bound skips no candidate.
  `first_greater_ends_equal_group` establishes that the new break omits no later
  candidate. `stable_window_exact` preserves the exact ordered fold over all
  candidates, including every equal-offset tie and error outcome.
* Caller-visible domain checks execute before the changed scan as before.
  No changes are proposed to source validation, runtime guards, descriptor
  layout, ownership, roles, choreography, or evaluator thresholds.

## Evidence boundary

Lean checks the unbounded search and fold arguments; Z3 independently checks the
loop preservation conditions and symbolic ordered folds of lengths 0 through 12.
Negative controls confirm that unsorted input, skipping equal ties, or reversing
ties can change the result. Rust source correspondence is an explicit audited
bridge, not a claim that Lean/Z3 parsed or verified the Rust compiler.

Runtime differential tests must compare the optimized source helper against the
original full-scan definition, including empty views, absent scope, invalid arm,
wrong scope kind, late groups, multiple equal-offset candidates, gaps, and
malformed candidate outcomes. Compile-cost measurements use unchanged defaults.
