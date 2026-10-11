# Projection conflict reuse

Qualified in the isolated `hibana-projection-conflict-reuse` checkout, based on
`ff6d167ed23f2f566d16affdeedd3b2ca8569cc2` plus the parent's already qualified
passive-child window and test-hygiene changes. The main checkout, QUIC sources,
vendors, public API, graph, roles and compiler limits were not changed.

## Production change

`local_event_row_for_eff` returns its already computed `PackedEventConflict`
alongside the event. The emitter uses that exact packed value at the original
second scan's position, after dependency processing and writes. The `has_route`
branch and its `none()` result remain. The helper that immediately discarded the
conflict is inlined into its sole caller. This adds no cache, allocation,
persistent metadata/state, new flag or limit.

`projection-conflict-reuse.patch` contains only those two production files.
`differential-harness.patch` is separate and places its original-source oracle
and checks under the repository's excluded `tests` subtree. Both patches pass
`git apply --check` against the parent's main checkout.

## Proof before edit

The pre-edit gate completed at `2026-10-03T00:51:52.718412+00:00` (the JSON record
is authoritative). Lean 4.30 proved exact conditional common-subexpression reuse
with arbitrary source/index/conflict/state/output/error types, plus an arbitrary
failing prefix. No `sorryAx` occurs. Z3 5.1.0 independently proved 16-bit conflict,
serializer and earliest-error equality; changed-source and reordered-dependency
negative controls were satisfiable. Source/proof/log hashes were recorded before
production Rust changed. `SOURCE_BRIDGE.md` states the immutable-input and exact
control-flow correspondence; `REVIEW.md` records independent qualified approval.

These are model proofs and an audited Rust bridge, not machine verification of
Rust. Error preservation means payload and evaluation ordering, not unchanged
source line/backtrace text or compiler resource use.

## Final validation

* Differential corpus: 15 shapes covering no route, sequences, nested routes,
  roll, parallel, passive roles and endpoint IDs 254/255, with every role in each
  source's declared role domain. 292 ordinary full projections plus 8 accepted
  mutated-layout projections produced identical complete bytes and columns;
  828 emission/index failures produced identical panic text. 901 individual row
  and packed-conflict comparisons passed.
* An additional 24 crossing-marker cases matched the original row helper,
  including 12 identical early invariant failures.
* All core unit tests passed: **469 passed, 0 failed, 8 ignored**.
* `cargo check -p hibana --lib --no-default-features` passed.
* Surface/alias/endpoint hygiene, source file budget, underscore-discard checks
  and `git diff --check` passed on the final files.
* The frozen reference emitter and all three row-helper bodies were verified
  byte-for-byte against pre-edit snapshots after only documented naming,
  visibility and module-path adaptations (`verify_reference.py`).

Rust 1.95.0, one build job, no incremental compilation, disabled debug info,
unique target directory and shared heavy-build lock were retained. The successful
final differential compile/run took 21.162 s with 585,180 KiB sampled peak process
RSS; cached full-unit run took 1.008 s. The 270 s / 2.5 GiB observation guard was
unchanged. These are qualification costs, not measured optimization savings.

Earlier failed attempts were limited to harness imports/module paths and an
incorrect expectation that every mutated layout must be rejected. The original
emitter accepts eight such corpus cases; the corrected oracle requires the
optimized emitter to preserve those exact bytes. All attempt logs are retained,
and the production patch remained byte-identical after its initial edit.

## Source-derived savings

The unchanged 333-event TLS graph removes 666 duplicate scans and 190,362 outer
marker-loop reads across its two roles. The actual TLS+key composition contains
354 events, 148 routes, 6 rolls and 607 markers; across four roles it removes
**708 duplicate scans and 215,752 outer marker-loop reads**. The count model
includes the first greater-offset stopping read. It does not count additional
reads inside unchanged scan helpers, rustc instructions or wall-clock savings.
See `removed-scans.json` and `count_removed_scans.py` for source hashes/model.

Full-owner compile-time/RSS qualification after integrating this optimization
remains the parent's next measurement. No claim that this alone fixes full-owner
compilation is made here.
