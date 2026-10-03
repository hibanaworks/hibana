# Exact route-path cache: proof and source correspondence

## Pre-edit gate

The cache replaces only the two `events_share_route_path` calls in the
baseline `color_roll_frame_labels` loop at a6339772. Its operation-key checks,
iteration order, greedy color selection, writes, local-event skip, and panic
messages stay unchanged. No bucket, compiler limit, wire limit, protocol
policy, runtime descriptor, or persisted layout is changed.

`pre_edit_gate.json` records source hashes before and after both Lean modules
and both Z3 programs ran successfully, before any cache production edit.
The Lean checks reject any `sorryAx` dependency. The record also hashes every
proof input and log. The earlier failed Lean attempt logs are retained as
failed attempts, not evidence of a successful proof.

## Relation and concrete loop bridge

1. `events_share_route_path` considers precisely first-enter Route markers
   for which `closed_route_arm_ranges_from_first_enter` succeeds. The new
   constructor uses the identical predicate and reads the same closed bounds.
2. Membership is left=0, right=1, outside=2. Both computations use the same
   half-open comparisons. The route accessor guarantees start < split < end;
   no nesting, disjointness, or source-tree assumption is needed.
3. `RoutePathRefinement.lean` proves arbitrary finite-feature refinement
   preserves exactly equality of the complete membership vector. Its uniform
   interval theorem justifies omitting a route only when the whole body is
   outside the route or wholly inside one arm. Equal endpoints alone are not
   used. Z3 includes a SAT counterexample for that incorrect shortcut.
4. `ProcessedCardinality.lean` proves, for arbitrary lists of ternary tags,
   the category-major traversal starts with zero processed slots, increments
   exactly at matching slots, preserves the count across category resets,
   and ends with every slot processed. A matching slot has a remaining
   allocation credit, so next_id cannot equal the u16 sentinel before a
   store and the following increment cannot overflow.
5. `prove_unbounded_loop.py` proves 25 symbolic, quantified inductive VCs for
   the concrete two-array update. Unprocessed slots retain their original
   class; processed IDs are bounded; equal new IDs mean exactly equal old
   classes and membership; each non-sentinel remap entry has a processed
   owner; reads use valid old-class indices. It proves initialization,
   matching and skipped steps, category reset, final exactness, and bounds.
   Event count is symbolic, not small-case unrolling. Cardinality premises
   are supplied by the Lean layer in item 4.
6. Rust resets remap entries 0..old_class_count, the only keys the invariant
   permits. Resetting other slots, as the mathematical constant-array model
   does, is unobservable. IDs are read only inside the membership match,
   before the event's sole update. next_id resets for each route, never
   between its categories. Its maximum final value is the event count;
   stored IDs are strictly smaller.
7. Classes start at zero. Every processed route refines these classes using
   the exact loop, so induction yields the baseline relation. Numeric IDs
   may differ from Lean's first-occurrence numbering; their equality and
   bounds are the proved observable properties. `consumer_congruence`
   then applies to the unchanged deterministic coloring loop, including
   its exact writes and failure result.

## Bounds, fallback, and honest scope

The existing roll-body validity check runs before cache construction and
implies 0 < end-start <= initialized length <= E. Compact caching is enabled
only for bodies of at most 65,535 events; the proof covers that whole domain.
All array indexes are relative to start, and old IDs are below body length.
The two scratch arrays use 4*E bytes, temporary to source lowering. They are
not runtime metadata. Existing published scope offsets are u16-bounded.
Private callers with a larger body retain the original predicate through an
explicit fallback, so the change adds no input ceiling or new rejection.

This is a source-correspondence refinement argument plus independent Lean
and Z3 proof layers, not a mechanically extracted proof of Rust semantics or
a proof of the entire compiler. Tests separately compare the concrete Rust
cache and complete coloring result with the original implementation.
