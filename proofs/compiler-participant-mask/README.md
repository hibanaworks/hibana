# Qualified participant-mask evidence

Historical pre-edit Lean/Z3 gate, exact proof sources, source bridge, benchmark
fixture, and raw results copied byte-for-byte from the qualified compiler-only
change. `preserved-artifacts.json` pins all original bytes. No runtime guard is
removed. See `SOURCE_BRIDGE.md` for assumptions and proof limits.

Run the portable shared proof check from the repository root:

```sh
python proofs/elastic-roll-colors/check_all.py --lean /path/to/lean
```

This checks `ParticipantMask.lean` and all 44 Z3 claims/witnesses. It preserves
the original pre-edit records and logs. `run_pre_edit_gate.py` is a historical
pre-edit-only entry point with original workspace tool paths; do not rerun it
on the implemented checkout. `measure_compile_cost.py` is the original isolated
measurement driver and may create snapshots/build outputs; it is not run by
the portable proof check.

The measurement evidence includes the initial missing-time-tool failure under
`validation/compile-cost-initial-missing-time.log`; it is not a successful run.
All other original empty logs/time samples remain to preserve the measurement
record. Binary/compiler caches were omitted. Isolated benchmark results are not
full TLS compile measurements or runtime performance claims.
