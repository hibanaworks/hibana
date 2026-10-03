# Exact route-path cache results

Production changes are isolated to `event_relations.rs` and the two relation
calls in `allocation/frame_labels/roll.rs`. `route-path-cache.patch` contains
those changes plus the new differential tests. The earlier participant mask
work remains a separate change in this worktree. No main vendor or remote
was modified.

## Proof before implementation

At 2026-10-02 23:45:34 UTC, the recorded source-hash gate passed both Lean
modules, 25 unbounded Z3 loop-invariant obligations, and 18 additional
concrete/machine-domain/SAT-witness checks. No Lean sorry/custom axiom was
used. See `pre_edit_gate.json` and `SOURCE_BRIDGE.md` for scope and limits.

## Concrete validation

- 460 library tests passed; 8 pre-existing export tests remained ignored
- 83 integration tests passed in seven relevant route/roll/security binaries
- thumbv6m-none-eabi no_std library check passed
- Source file size, maintainability, source lowering, lowering, descriptor
  authority, no-nightly, no-generic-const-expressions, no-underscore-discard,
  and git whitespace checks passed
- The four new tests cover every pair of small route intervals and every
  body subrange (including overlapping, nested, and disjoint intervals),
  256 generated full-label differential cases, the exact baseline wire
  exhaustion panic plus partially written state, and 65,535/65,536-event
  cache/fallback behavior

## Cold rustc CTFE measurements

The standalone benchmark forces source construction and roll coloring. It
compares the original roll loop with the cached loop on otherwise identical
source snapshots, records all hashes, alternates order, and takes three
successful runs per variant. All compiler defaults and limits are unchanged.
This is an isolated source-coloring benchmark, not a full TLS build and not
a runtime benchmark.

| Routes / events | Original | Cache | Result |
| --- | --- | --- | --- |
| 16 / 32 | 3.053 s median | 1.453 s median | 52.4% lower wall time |
| 32 / 64 | Rejected by existing long_running_const_eval | 1.568 s median | Succeeds at unchanged ceiling |
| 64 / 128 | Rejected by existing long_running_const_eval | 1.916 s median | Succeeds at unchanged ceiling |

At 16 routes, peak RSS was 271,368 versus 269,716 KiB, a 0.6% difference;
there is no material RSS win established. Failure durations at larger sizes
are not comparable to successful compilation durations. The benchmark's
larger original runs stop after the first observed compiler-budget failure;
no limit was raised to obtain a result. Full TLS compile cost remains for
the parent integration task to establish.
