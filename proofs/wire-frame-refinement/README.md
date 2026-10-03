# Canonical wire-frame refinement

The canonical source has one final wire-color refinement:
`canonicalProgramSource` applies `separateElasticFrameDomains` once to the
structurally compiled atoms. Both per-event and per-role canonical frame labels
read those final atoms. There is no second `canonicalWireAtoms` allocator.
`GlobalSyntax` remains the structural lowering source.

The selected external definitions are imported directly. The proofs name small
predicate and fold-step expressions only to reason about the production code;
`actual_head_uses_this_predicate` and `actual_owner_is_this_fold` bind those views
by definitional equality. No copied final allocator is used by the live proofs.
`canonical_source_applies_exactly_one_refinement` binds the entire final atom
list, and the nested source fixture checks both complete exact certificates.

The allocator freezes original frame colors and chooses the narrowest enclosing
Roll, using the later source ordinal for coextensive scopes. It scans prior
assignments in `(sender, receiver, lane)` and excludes assigned colors when the
frozen color or owner differs. Self sends retain their label, and no-Roll sources
remain unchanged. Exhaustion retains invalid byte value 256 in its event row.

## Replay

With Lean 4.30.0 and Python `z3-solver` installed, from the repository root:

```sh
python proofs/wire-frame-refinement/check_all.py --lean /path/to/lean
```

The runner verifies preserved historical artifacts and the selected production
source hashes, builds the actual Lean library, and audits all 54 named Lean
theorems. It runs the unchanged original 40-query Z3 generator, the eight option
match equivalence checks, and 29 external-specific Z3 obligations/controls.
Each finite correspondence layer checks 38,840 bounded cases; the external layer
adds ten fixtures, eleven historical models, five raw-marker controls and two
rejected source-placement mutants. The existing final-form invocation is kept.

The portable external checker changes only checkout identity/path handling:
exact source bytes remain pinned, the actual Git HEAD is recorded separately
from the qualified source commit, and the former source is read from a preserved
snapshot. All model/query function bodies remain byte-identical to qualification.

## Scope and history

Rust correspondence requires valid, laminar source intervals and unique Roll
exits after their enter markers, with offsets matching Rust's `segment_end`.
The external owner uses the first matching exit from the entire marker list;
the former helper searched a suffix and supplied a fallback for a missing exit.
The malformed-marker controls prove that arbitrary-list equivalence is false.
The positive bridge is qualified source-transcription and finite evidence, not
an unconditional Rust-to-Lean compiler refinement theorem.

Admission rejection for 256 is role-local: the role filter must retain the
invalid row. An unrelated role can omit it. Positive emitted-descriptor
correspondence assumes successful Rust lowering. Runtime `Covers` /
`SameClassUnique` remains explicit. Capacity failure concerns the fixed greedy
prefix, not optimal graph coloring. Lean proofs permit only `propext` and
`Quot.sound`, with no admitted or native-decision proofs in this package.

`pre-edit/`, `explicit-match/`, `validation/`, and `validation-explicit-match/`
remain immutable historical evidence for the earlier placement and its style
repair. `external-binding/before/` preserves the previous live proof package and
source snapshot. `external-binding/` records the proof-only qualification of the
single final-source phase before package integration. Its source-binding record
covers the changed final-color theorem statement; exact descriptor admission
conjuncts remain unchanged. The public theorem count remains 709; the external
proof changes one axiom classification from both to propext-only, without adding
an axiom. These public-surface changes remain subject to the full Lean audit.
