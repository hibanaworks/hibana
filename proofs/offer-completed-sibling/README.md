# Completed same-lane cursor hides a pending sibling offer

## Scope and provenance

Base: `102fc47e807d00318c75d7ad344fc99c2d5d3724` on
`development/dots-causality`. Only runtime change:
`src/endpoint/kernel/offer/select_observed.rs`. Regression additions are in
`tests/parallel_route_alternating.rs`; these supplemental proof artifacts have no
runtime dependencies and do not change the existing canonical proof inventory.

The original failure was isolated from a key-service client into a two-role,
two-service public-API test using independent `g::par` branches. Each branch is
`Installed; roll(route(Use; Done, Retire))`. Install both, optionally use/finish
one, retire it, then offer on the fresh sibling. The base rejects a valid frame
with `PhaseInvariant`; the exact typed receive control succeeds. The committed
regression exercises both fresh Use/Retire, prior use/no use, dropped-preview
restoration and further roll reentry, plus a missing-installation negative case.

A diagnostic-only copy of the unmodified base observed:

- ordinary cursor index 4, lane 1: already-consumed sibling installation
- `node_event_done_for_lane(4, 1) == true`
- observed wire lane 1, frame 0 (Use) or 1 (Retire)
- lane 1 pending head 5, inside the valid sibling route `[5, 8)`
- current-materialized, roll-reentry and active-reentry lookups all miss

The old fallback selects index 4 solely because its lane matches. The enclosing
route lookup then fails because that installation lies outside the route.
The correction selects the current same-lane index only while it is not done;
otherwise it uses the existing pending-lane-head lookup. The earlier selection
precedence is unchanged. There is no added runtime state or public API.

Lean and Z3 evidence was executed before the production change. Independent
pre-change verification completed at 03:39:33–36 UTC on 2026-10-02, a further
proof run at 03:40:35 UTC, and the initial production edit at 03:45 UTC. The final
form and proof rerun were verified after 04:01 UTC. Publication copies differ
from those proof sources only in comments clarifying that the correction is now
applied. `verification.txt` contains the original final proof output.

## Evidence and limits

- `TraceValidity.lean` imports this repository's actual `Hibana.GlobalSemantics`.
  Three kernel-decided theorems admit the original and minimal traces and reject
  the omitted-installation trace. Their reported axiom dependency is `propext`.
- `IngressSelection.lean` models the exact fallback and descriptor ranges above.
  Eleven theorems show the old failure, corrected selection, live/foreign-lane
  preservation, and preservation of an available pending scope. Seven have no
  axioms; four use `propext`. There are no custom axioms or admitted proofs.
- `ingress_selection.py` checks both concrete bug witnesses and nine negated
  properties. Each proof's premises are checked satisfiable before the property
  is checked UNSAT. There are two concrete SAT witnesses and nine UNSAT results.

The diagnostic-to-model mapping and Rust regression remain separate evidence.
These artifacts do **not** establish a universal Rust source refinement,
arbitrary choreography correctness, memory-safety proof, or whole-runtime proof.
They are supplemental, manually executed evidence, not newly registered CI jobs.

## Reproduce the scoped checks

Use the repository-pinned Lean 4.30.0 and Rust 1.95.0, plus Python `z3-solver`
5.1.0. From the repository root:

```sh
(cd proofs/lean && lake build Hibana)
LEAN_PATH="$PWD/proofs/lean/.lake/build/lib/lean" lean proofs/offer-completed-sibling/TraceValidity.lean
lean proofs/offer-completed-sibling/IngressSelection.lean
python3 proofs/offer-completed-sibling/ingress_selection.py
cargo +1.95.0 test --test parallel_route_alternating
```

Local corrected-core verification: 437 workspace tests passed across 41 result
targets, all 90 UI cases passed, and three doctests were explicitly ignored.
With `RUSTFLAGS='--cfg hibana_repo_tests'`, the library had 436 passed and eight
ignored exporter/measurement tests. Strict Clippy for the library and regression
target, and a no-default-features `thumbv6m-none-eabi` library check, passed.
The complete final-form measurement/Miri/Kani suite was not rerun locally;
remote CI is separate and must be evaluated for the published commit.

Corrected selector SHA-256:
`28ba7c8cba8734e96a94b64ec962987b8effbd4b0889fde440d5fad77a4faf74`.
Regression source SHA-256:
`d5913ea4d7482affeb4c9611cc050794caefcbb914b0a88fd95cc332cabcd409`.
The downstream vendored source can retain its base-plus-patch identity; adding
these supplemental proof files does not change either corrected Rust file.
