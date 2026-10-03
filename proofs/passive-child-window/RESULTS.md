# Passive child scan window

The isolated candidate is based on `ff6d167ed23f2f566d16affdeedd3b2ca8569cc2`
and changes one production function: `passive_route_child_scope`. It starts the
existing ordered scan at `offset_lower_bound(arm_start, 0)` and stops at the
first greater offset. Its original domain checks, candidate predicate, range
checks, fold, and result remain unchanged. The main worktree and QUIC source
were not modified.

## Proof and validation

* Lean passed the unbounded nondecreasing-offset binary lower-bound theorem,
  preservation of all equal-offset markers, exclusion of later matches after
  the first greater offset, and exact stable-fold pruning for arbitrary state
  transitions. No `sorryAx` appears in the checked theorem dependencies.
* Z3 passed 7 unbounded loop/selection conditions and 13 symbolic ordered-fold
  equivalence checks (lengths 0–12), with 3 satisfiable negative controls for
  unsorted input, skipping ties, and reversing ties.
* The successful pre-edit gate was written at
  `2026-10-03T00:32:22.754817+00:00`, before changing production Rust. It records
  source/proof hashes and the successful proof commands/log hashes.
* All 9 scope-range tests passed, including 52,992 generated differential
  comparisons plus explicit late/equal-range ties, nonzero view base, empty
  source, and matching duplicate-candidate invariant failures.
* The full core unit suite passed: 468 passed, 0 failed, 8 ignored. Cold core
  test compilation took 41.650 s, with sampled process-group peak RSS 579,668
  KiB. The full cached suite then took 0.505 s including cargo startup.
* Rust 1.95.0, one build job, no incremental compilation, debug info disabled,
  and the existing 270 s / 2.5 GiB guard were retained. The repository's normal
  `--cfg hibana_repo_tests` enabled internal unit tests; no compiler limit was
  changed. The shared `rust-heavy-build.lock` serialized Rust work.

## Source-derived cost diagnosis

The unchanged original TLS graph has 333 events, 140 routes, and 5 rolls. Its
actual integrity-loan composition adds the 21-event/8-route/1-roll key graph
under one parallel scope: 354 events, 148 routes, 6 rolls, 607 markers, 4 roles.

Per role, the old passive-child inner loop traverses 179,672 markers. The
candidate instead performs 2,756 binary-search comparisons, 869 equal-offset
loop iterations, and 296 stopping-marker reads; the maximum tie group has 9
rows. This removes most of this loop's repeated work in both source validation
and role-image emission. These are source-model operation counts, not rustc
instruction counts or measured full-owner savings.

The larger full-owner HQ baseline was reported by the parent as reaching the
unchanged RSS guard at 200.407 s / 2,649,716 KiB, with warnings in this helper's
scan during source validation. Full-owner candidate compilation is a separate
measurement; the core test results do not claim it passes yet.

## Files

* `passive-child-window.patch`: one production-function change plus differential
  tests, relative to the base checkout
* `pre-edit-gate.json`, `candidate-manifest.json`: timing and source identity
* `PassiveChildWindow.lean`, `check_window.py`, `SOURCE_BRIDGE.md`: checked models
  and explicit Rust correspondence assumptions
* `lean.log`, `z3.log`, `scope-differential-tests.*`, `full-core-unit-tests.*`:
  raw proof/test and resource evidence

Candidate production file SHA-256:
`a85340702a5e0641fa764123bf4a8f1356c86c2957218e81a8fb4d20ac16f15c`.
