# Reuse the already computed event conflict

The production change is limited to returning the existing `PackedEventConflict`
with `PackedLocalEventRow` from `local_event_row_for_eff` and using that value in
`RoleImageBytes::emit` at the former second `route_conflict_for_eff` call. The
`has_route` branch remains: when false the serialized conflict remains `none()`.
There is no cache, table, allocation, runtime metadata, new limit or graph change.

## Exact source correspondence

Before the edit, `local_event_row_for_eff` calls `scope_at`, then
`route_scope_and_arm_at`, whose first operation is
`route_conflict_for_eff(eff_list.scope_markers(), eff_idx)`. The result is decoded,
used by `first_recv_eff_for_route_arm`, and then the event row is constructed.
The proposed code binds that same conflict at that same point, directly matches
its decoded value with the same cases, and returns `(event, conflict)`.

The caller then invokes `dependencies.next`, writes any dependency row, and
updates the event's dependency index. Only then does the original caller evaluate
`has_route` and recompute `route_conflict_for_eff(markers, eff_idx)`. The new caller
keeps all these operations in the same order and uses the returned conflict in
that true branch. Every subsequent conflict/event/lane write is unchanged.

* `markers` is exactly `eff_list.scope_markers()`, captured before the loop.
  `ScopeMarkerView` is Copy and contains only an immutable `&[SourceRow]` plus
  start/length. `EffList` owns a fixed array and integer partition fields;
  neither type has interior-mutability fields.
* The source reference is `&EffList<E>` in both functions. `eff_idx` is unchanged
  until the end of the iteration. Role and frame label do not enter the scan.
* `DependencyCursor` holds immutable references to `EffList` and `ScopeFacts`;
  `next(&mut self, ...)` changes only cursor-owned position, barrier and lane
  candidate fields. `out` is distinct local output storage. Neither can change
  the source, marker view or current event index.
* `route_conflict_for_eff` reads markers and uses stack-local selection state.
  Its helpers read immutable values and execute finite index scans. They have no
  I/O, global mutable state or allocator effects. A repeated successful scan
  therefore returns the same packed u16 and cannot newly panic.
* We do not assume scans are panic-free. If the first scan panics it still runs
  before determinant choice, row construction and `dependencies.next`. If choice,
  row construction, dependency processing or dependency output panics, that still
  wins at its original point. The omitted second scan is reached only after the
  identical first scan completed successfully.

## Model bridge and limits

Lean's `scan` is the unchanged pure route scan as an `Except` result. `prefix`
represents earlier checks and `scope_at`. `rowThenDependencies` represents decoded
choice, first-receive lookup, row construction, cursor advancement and dependency
writes, including their exact error payloads and output state. `serialize` is all
remaining conflict/event/lane writes and subsequent loop/suffix work. These
functions and state/output/error types are arbitrary: the theorem is not limited
to a finite marker corpus or particular u16 values. The retained Boolean branch
needs no lemma that no-route sources have no conflict.

Z3 independently checks packed 16-bit conflict equality, arbitrary dependency
state and serializer, and first-error precedence. Changed-source and reordered
failure negative controls are satisfiable. Both tools execute before production
source edits, with source/proof/log hashes recorded by the gate.

These are model proofs plus an audited Rust bridge, not a claim that Lean or Z3
parses/verifies Rust. Observable behavior here is completed bytes and failures;
compile-time resource use and scan counts intentionally differ. Differential
Rust checks compare full emitted bytes and exact panic text to the snapshotted
original emit/helper definitions over route/roll/parallel/passive/high-role and
malformed-plan cases. No evaluator threshold is changed.
