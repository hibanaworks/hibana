# Projection diagnostic scope

The existing `projection_error_all_roles` acceptance gate is unchanged. The new
`projection_diagnostic` first calls that gate and returns `None` only when it
accepts. The diagnostic value has no constructor for RoleProgram, endpoint,
resolver decision, or projection proof. Projection still runs its original gate.

Rejected receive lanes are explained using the same CausalFlow must-analysis,
including roll reentry. The bounded Rust differential test enumerates 32,805
four-event sources (three-role domain; sequence, route, parallel, roll, and
nested route/roll) and checks witness presence against the original predicate.
Each witness names two different senders into the same actual receive lane.

Rejected routes report the source scope ordinal, both arm event ranges, unique
controller when known, affected passive role when found, and representative
first local source events. An absent arm event is `None`, not a fabricated
operation. Parallel/reentry selector and passive-child errors currently retain
their precise error category but can have no detailed witness; this is explicit.

`Diagnostics.lean` proves for any finite list of checked obligations that no
witness is equivalent to all obligations accepting, and every emitted witness
is in bounds and points at a failed obligation. `check_diagnostics.py` checks the
same report-preservation properties and first-failure property with Z3 for
1–32 symbolic obligations. These are scoped models, not proofs of all Rust
lowering, and do not prove QUIC or physical-device correctness.

Run:
- lean proofs/projection-diagnostics/Diagnostics.lean
- python3 proofs/projection-diagnostics/check_diagnostics.py
- RUSTFLAGS='--cfg hibana_repo_tests' cargo test --lib diagnostic_witness_matches
- cargo test --test projection_diagnostic
