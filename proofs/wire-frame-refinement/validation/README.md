# Current-source validation

All runs used the reviewed production allocator block and official Lean 4.30.0.
The existing Lean library was rebuilt from this checkout into a fresh build
directory. The generated Rust artifact was freshly exported and still has
SHA256 `e6c2fa7b24ced5fe6ce17a70868e69d117f1f78aca481c9f183cb9fe0d380af7`.

Passed commands from the repository root:

```sh
bash .github/scripts/check_lean_proofs.sh
python proofs/elastic-roll-colors/check_all.py --lean /path/to/lean
python proofs/wire-frame-refinement/check_all.py --lean /path/to/lean
bash .github/scripts/check_text_integrity.sh
bash .github/scripts/check_source_file_budget.sh
bash -n .github/scripts/run_final_form_gates.sh
git diff --check
```

The full existing Lean gate retains 709 static theorems and all 506 generated
theorems (466 kernel, 40 native), including 22 exact descriptors, 182 parallel
correspondence claims, 36 causal claims, 16 runtime claims and two public
operation claims. Its five official Rust exporters all passed. The original
70-predicate diagnostic now passes every column for both nested-roll roles;
the actual and canonical labels both equal `[0, 0, 1]`.

The new regression package passes 40 Lean theorems, 40 Z3 obligations/controls,
38,840 finite small-input equivalence cases, nine named fixtures and eleven
historical finite models. The existing complete elastic/compiler proof replay
passes unchanged. The replay used Z3 5.1.0; CI's pinned solver remains unchanged.

`current-source-validation.json` binds source files, unchanged certificate
checks and claim snapshots, generated artifacts, and logs by SHA256. The
commands ran with the repository's Rust 1.95.0 toolchain, one build job and the
shared Rust build lock. This is validation of the requested Lean/reference
repair; it is not a claim that the entire final-form integration script ran.

`runner-first-failure.log` preserves a wrapper-only first failure: Lean's
compiled output required the input module's own working directory. The runner
now invokes each module there and uses absolute verified dependency paths.

Capacity scope remains explicit: Rust rejects overflow globally; a per-role
exact certificate rejects it when its label filter retains the invalid row.
An unrelated role can omit that row. Positive Rust correspondence assumes
successful source lowering. Concrete runtime `Covers` / `SameClassUnique`
remains an external premise.
