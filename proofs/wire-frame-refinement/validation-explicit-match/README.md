# Explicit option-match validation

The following passed against the current source after the separate Lean/Z3
equivalence gate and the two explicit option-match replacements:

```sh
cargo +1.95.0 test -p hibana-repo-tests --test semantic_surface
bash .github/scripts/check_lean_proofs.sh
python proofs/wire-frame-refinement/check_all.py --lean /path/to/lean
python proofs/elastic-roll-colors/check_all.py --lean /path/to/lean
bash .github/scripts/check_text_integrity.sh
bash .github/scripts/check_source_file_budget.sh
git diff --check
```

- Semantic surface: 124 passed, including the unchanged hidden-default guard
- Full Lean gate: unchanged 709 static and 506 generated theorems, 22 exact
  descriptors, parallel 182, causal 36, runtime 16 and public-operation 2
- New package: 44 Lean theorems; original 40 Z3 checks and 38,840 finite
  correspondence cases; eight additional option-equivalence checks
- Existing complete elastic/compiler proof replay passed

Official Lean 4.30.0, Rust 1.95.0 and Z3 5.1.0 were used. All five official Rust
exporters passed and the main generated artifact still has SHA256
`e6c2fa7b24ced5fe6ce17a70868e69d117f1f78aca481c9f183cb9fe0d380af7`.
Cargo calls used the shared Rust build lock and one build job.

`current-source-validation.json` binds the exact source, unchanged test/checker
and claim snapshots, generated artifacts, and logs. Original pre-edit history
and earlier validation files remain unchanged. This records the requested proof
and semantic-surface checks, not an entire final-form integration run.

The package-selection failure log preserves an initial command targeting
`hibana`; the semantic-surface test belongs to `hibana-repo-tests`, and the
correct package run subsequently passed all 124 cases.

Capacity and runtime premises are unchanged: invalid-row admission rejection
is role-local under retained-row membership, positive correspondence assumes
successful Rust lowering, and `Covers` / `SameClassUnique` remains explicit.
