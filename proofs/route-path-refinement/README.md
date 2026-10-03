# Qualified route-path refinement evidence

Historical pre-edit proof gate, exact seven-source qualification manifest,
Lean/Z3 sources, benchmark fixture, patches, raw measurement results, and test
logs are copied byte-for-byte. `preserved-artifacts.json` pins original bytes.
See `SOURCE_BRIDGE.md` and `RESULTS.md` for the qualified transformation and its
limits. The Lean cardinality proof and 25 unbounded verification conditions do
not depend on bounding the event-loop length in the solver. The additional 18
Z3 checks cover concrete loops, machine bounds, and negative controls.

Run the portable shared proof check from repository root:

```sh
python proofs/elastic-roll-colors/check_all.py --lean /path/to/lean
```

It checks both Lean files, the 25 unbounded conditions, all 18 concrete checks,
and all seven qualified implementation hashes. Original scripts that emit JSON
run from temporary copies. `run_pre_edit_gate.py` remains a historical pre-edit
entry point with original workspace paths; do not rerun it on edited source.
The measurement driver creates snapshots/build outputs and is not automatically
rerun. Exact benchmark fixtures retain their original `.rs` filenames.

`lean-cardinality-attempt1.log` and `lean-cardinality-attempt2.log` are preserved
failed proof attempts. The final successful evidence is `lean-cardinality.log`
and the immutable `pre_edit_gate.json`. Larger baseline compile-cost samples
rejected by the unchanged const-eval ceiling remain explicit failures; their
failure durations are not successful compile timings. Isolated benchmark wins
do not establish full TLS compile cost or runtime speedup. Only generated Python
and compiler caches are omitted.
