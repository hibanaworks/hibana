# Combined core followup qualification

The local tree based on `ff6d167ed23f2f566d16affdeedd3b2ca8569cc2` passed
combined core qualification after integrating the already qualified passive-child
window, projection-conflict reuse, and test-hygiene edits. No commit, publication,
QUIC vendor edit, compiler-limit change, or stack-limit override is part of this
qualification.

## Current validation

- Normal-stack internal tests: **469 passed, 0 failed, 8 ignored**. The command
  was `cargo test --locked --offline -p hibana --lib` with the existing
  `RUSTFLAGS='--cfg hibana_repo_tests'`. The run rejected `RUST_MIN_STACK` and
  compiler-wrapper overrides. The 65535/65536 boundary test passed after removing
  its explicit 4 MiB thread wrapper, retaining its assertions.
- Shared `rust-heavy-build.lock`, one job, no incremental compilation, debug
  info disabled, Rust 1.95.0, and the existing 270 s / 2.5 GiB guard were retained.
  Combined compile/test elapsed **22.080 s**, sampled process-group peak RSS
  **580,684 KiB**. These are qualification costs, not optimization savings.
- Fresh Lean 4.30.0 / Z3 5.1.0 replay passed the elastic color, no-roll,
  participant-mask, route-path, passive-child-window, and conflict-reuse models.
  Lean dependencies were compiled from checked source in a fresh directory.
- All 13 selected source/surface hygiene gates and `git diff --check` passed.
- The preserved original seven-source qualification manifest remains unchanged:
  SHA-256 `591d773fbb7192c6116c4f334077015b4fbfe20fd95821f557843f6f9347b2bc`.
  Each original source is archived and hash-checked. Four current sources still
  match directly; three test sources have explicit old/current hash mappings.
  The checker also verifies that these edits are exactly the oracle naming,
  whitespace/comment, and stack-wrapper changes described in the mapping.

## Exact current source identity

`current-source-manifest.json` covers all 360 `src/**/*.rs` files plus the root
and repo-test Cargo manifests and lockfile. Its combined SHA-256 is
`4068d89e28a18f555882cd4f7a713449516903fbe003a2e9e28409d5c1d2875b`.
The hash is over sorted `sha256 + two spaces + relative path + newline` entries.

The three changed production-owner files exactly match their separately
qualified candidates:

| Repository path | SHA-256 |
| --- | --- |
| `src/global/const_dsl/scope_ranges/route.rs` | `a85340702a5e0641fa764123bf4a8f1356c86c2957218e81a8fb4d20ac16f15c` |
| `src/global/role_program/image_impl/projection.rs` | `e9f4ad6dc81c25cf6c7cf53755c6e3368d3f9dc85fd2f0443ee2afe67f8be088` |
| `src/global/role_program/image_impl/blob_image.rs` | `8eb7b627e8d6b4ea602777db9d142f3adbc84740e0797bf1a57717ed9560ff1f` |

## Evidence and replay

Run from the repository root:

```sh
python proofs/core-followup/check_sources.py
bash proofs/core-followup/check_hygiene.sh
python proofs/elastic-roll-colors/check_all.py --lean /absolute/path/to/lean
```

The proof runner requires Python with `z3-solver`. Source verification includes
both copied proof packages, all original pre-edit proof/log hashes, exact current
candidate sources, and replay of the frozen reference-body verifier with only
its checkout path binding adapted. Historical scripts containing absolute paths
remain unmodified; they are records, not portable entry points.

`validation/` retains current raw logs and guard metadata, including the initial
468-test run before conflict reuse was applied. A first proof-run invocation
failed because its relative Lean executable path was invalid after changing
working directories; that failed log is retained. The subsequent absolute-path
invocation passed. No prior evidence or pre-edit gate was rewritten.

These are model proofs with documented Rust correspondence assumptions, exact
source hashes, and regression results. They do not prove all Rust semantics or
establish the separate full-owner QUIC build's time/RSS outcome. The elastic
allocation result remains conditional on `Covers` / `SameClassUnique`.
