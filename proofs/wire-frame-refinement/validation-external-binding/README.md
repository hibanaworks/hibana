# Integration package replay

The following command passed in the integration checkout after the separate
proof-only qualification and package-only adaptation:

```sh
python proofs/wire-frame-refinement/check_all.py --lean /path/to/lean
```

The actual Lean library was built from source. The replay audited 54 package
theorems, the unchanged original 40 Z3 queries and eight option checks, and 29
external-specific Z3 obligations/controls. Both main finite layers covered
38,840 cases; the external layer includes ten fixtures, eleven historical
models, five raw-marker controls and two rejected source-placement mutants.

`current-source-validation.json` binds all selected production sources, live
package files and this log. `DescriptorImage.lean` and
`DescriptorRefinement.lean` match the qualified external source exactly.
Original history files are unchanged. The descriptor-refinement code tokens
also match the preceding source; its only change is explanatory prose.

This is a package replay, not the full integrated static/generated audit or
complete integration test result. Those remain separate gates. Marker
well-formedness, successful-lowering and role-local capacity premises are
explicit; no unconditional Rust compiler refinement or runtime coverage is
claimed.
