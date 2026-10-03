# Elastic Roll frame-color evidence

This package accompanies the compiler-only final color pass on baseline
`a6339772d4bf2c905f491e0284d3bef36e79bb6f`. It separates distinct occurrences
sharing `(from, to, lane)` when their frozen baseline colors or complete Roll
memberships differ. Runtime eligibility and `UniqueMatch` guards are unchanged.

The abstract allocation proof is **conditional on Covers / SameClassUnique**.
This is not a universal Rust selector-correctness or compiler-refinement theorem.
The finite source models cover the recorded fixtures and retain their original
source-transcription, lane, and ancestry assumptions. The detailed original
claim boundary is in `historical/SOURCE_BRIDGE.md`.

## Portable replay

Requirements: Lean 4.30.0 and Python with `z3-solver` (recorded version 5.1.0).
From this repository, run:

```sh
python proofs/elastic-roll-colors/check_all.py --lean /path/to/lean
```

`LEAN=/path/to/lean` is also supported. The runner verifies checksums and exact
pre-edit records, builds seven required Hibana Lean modules from checked-in
source in a fresh temporary directory, and checks all four color/trace Lean
files plus `NoRollFastPath.lean`. It then replays all 31 recorded Z3 checks,
the no-roll abstraction, and the qualified compiler-cost proofs in the sibling
directories. It does not reuse historical `.olean` caches or run Rust builds.
The original proof scripts that emit result files run only from temporary copies.

For only the color/no-roll proof layer, add `--skip-compiler-cost`. For only the
31-query Z3 replay, run:

```sh
python proofs/elastic-roll-colors/check_roll_membership_portable.py
```

The portable check function and complete query-generating core are byte-for-byte
identical to `historical/check_roll_membership_gate.py`; `query-equivalence.json`
records their hashes and the runner checks this identity. It verifies every
original graph-reconstruction assertion, every SAT premise and negative control,
and the exact ordered 31 names and expected/actual outcomes. The two capacity
JSON inputs are losslessly gzipped, with compressed and uncompressed checksums
in `preserved-artifacts.json`. Replay reconstructs from the accepted membership
model. The earlier lexical-lifetime model remains archived as superseded evidence.

Source correspondence is a separate check. By default, the runner validates
recorded evidence bytes and Lean dependency source identity. It does not claim
access to a fresh QUIC source tree. Add `--quic-source /path/to/hibana-quic` to
compare the seven historical external source input hashes with a local checkout.
`source-correspondence-final.json` remains an unchanged historical report,
including its explicit cache provenance limits. Fresh Lean dependency compilation
here does not retroactively change the provenance of the original recorded run.

`check_no_roll.py` is a fresh abstraction matching the two recorded no-roll Z3
outcomes; the original no-roll query script was not retained. Its exact Lean
source and original pre-edit record/logs are preserved. Total, read-only scratch
initialization is the assumption; the rewrite does not remove source validation.

## Chronology and scope

- `pre-edit-proof-gate.json`: original 2026-10-02 23:48 UTC four-file Lean/31-check
  gate and proof hashes, recorded before the allocator implementation
- `no-roll-pre-edit.json`: separate 2026-10-03 00:02 UTC Lean/Z3 gate before moving
  the no-roll early return
- `source-manifest.json`: packaging-time implementation source hashes and the
  exact Lean dependency sources; hashes establish identity, not semantics
- `../compiler-participant-mask/pre_edit_gate.json` and
  `../route-path-refinement/pre_edit_gate.json`: unchanged earlier qualification
  records; their original absolute command paths are historical, not portable
  commands to rerun
- `../route-path-refinement/qualified-source-manifest.json`: original exact
  seven-file compiler-cost qualification manifest, verified against this checkout

No original artifact was overwritten. `preserved-artifacts.json` records copies
and compression. Failed Lean attempts, the source-bridge first failure, and the
superseded model remain under `historical/`. They are not passing proof evidence.
The original checker/generator scripts are archived for inspection and depend on
the original workspace layout; use the new portable runner above.

## Runtime regressions and compiler validation

The unchanged-baseline log `evidence/security-before-expanded.log` records ten
passing tests and four new failures, including a queued current Open55 payload
accepted as the old Inspect52 logical type. The final source change is validated
by the 14 security regressions in `evidence/combined-workspace-tools.log`.
That default workspace run records 447 passed and three ignored doctests;
`evidence/combined-internal-tests.log` records 465 passed and eight ignored.
The internal run includes five new allocation tests for membership, palette,
exhaustion, no-roll reuse beyond 256 events, nested/coextensive Roll ancestry,
and preservation of source/receiver/lane/baseline-color distinctions.

One prior representation-only assertion expected old/current events to reuse the
same wire color. It now requires the same lane and distinct colors; the exact
change is preserved in `evidence/representation-assertion-change.patch`. The
behavioral security checks remain. Earlier `security-after.log` (13/14),
`security-and-domain-tests.log` (security passes, subsequent wrong package
selection fails), and `combined-workspace-reviewed.log` (missing rustup on PATH)
are retained as failed/intermediate runs, not final passing validation.

Relevant regression commands from repository root:

```sh
cargo test --workspace
cargo test -p hibana --test security_report_regressions
RUSTFLAGS='--cfg hibana_repo_tests' cargo test -p hibana --lib
RUSTFLAGS='--cfg hibana_repo_tests' cargo test -p hibana --lib reentry_domains
```

Use the repository's toolchain and installed targets. The proof runner itself
does not re-execute these Rust tests. Compiler-cost measurements in the sibling
directories are isolated historical benchmarks; they do not establish a full
TLS compile time, runtime speedup, or current consumer-build result.

`validation/portable-runner-first-failure.log` records a wrapper-only failure:
its first version wrongly applied a Lean `error:` log filter to passing Z3
check labels. The corrected runner applies that filter only to Lean output.
The final replay log is separate; original historical records remain intact.
