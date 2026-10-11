# Qualified route-path refinement evidence

The Lean cardinality proof and 25 unbounded verification conditions do not
bound the event-loop length in the solver. The additional 18 Z3 checks cover
concrete loops, machine bounds and negative controls. See `SOURCE_BRIDGE.md`
and `RESULTS.md` for assumptions and limits.

Run the shared proof check with Rust 1.95.0, Lean 4.30.0 and Z3:

```sh
bash proofs/elastic-roll-colors/check_all.sh --lean /path/to/lean
```

It checks both Lean files, `UnboundedLoop.smt2` and
`RoutePathRefinement.smt2`. Historical pre-edit reports, qualification manifests,
benchmark fixtures and raw results are retained. `preserved-artifacts.json`
describes the original artifacts, rather than the current replay implementation.

`lean-cardinality-attempt1.log` and `lean-cardinality-attempt2.log` record failed
attempts; the successful evidence is `lean-cardinality.log`. Larger baseline
compile-cost samples rejected by the unchanged const-eval ceiling remain
failures, not successful timings. Isolated benchmark results do not establish
full TLS compile cost or runtime speedups.
