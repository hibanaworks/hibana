# Qualified participant-mask evidence

`ParticipantMask.lean` and `ParticipantMask.smt2` check the participant-mask
model, including all 44 symbolic claims and non-vacuity/negative witnesses.
No runtime guard is removed. See `SOURCE_BRIDGE.md` for assumptions and limits.

Run the shared proof check with Rust 1.95.0, Lean 4.30.0 and Z3:

```sh
bash proofs/elastic-roll-colors/check_all.sh --lean /path/to/lean
```

Historical pre-edit reports, benchmark fixtures and raw results are retained.
`preserved-artifacts.json` describes the original artifacts, rather than the
current replay implementation. Historical source hashes do not establish
correctness of a subsequently edited checkout.

The measurement evidence includes the initial missing-time-tool failure under
`validation/compile-cost-initial-missing-time.log`; it is not a successful run.
Isolated benchmark results are not full TLS compile measurements or runtime
performance claims.
