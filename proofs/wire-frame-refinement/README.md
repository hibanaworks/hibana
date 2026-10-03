# Canonical wire-frame refinement

The Rust compiler applies `separate_roll_frame_domains` after structural
lowering. The canonical Lean descriptor model now has the same final phase at
`canonicalWireAtoms`; the structural source and its existing theorems are
unchanged. Both canonical per-event and per-role wire labels read this phase.
The exact descriptor equality checks, claim snapshots, and theorem budgets are
unchanged.

The phase freezes each original frame color and chooses its innermost Roll
owner using the narrowest containing half-open span, then the later source
ordinal for coextensive scopes. It scans prior assigned colors in the exact
`(sender, receiver, lane)` partition, blocking a color if either frozen color
or owner differs. Self sends preserve their original labels, and programs
without Roll markers preserve the whole structural result.

Exhaustion retains the invalid value 256 in its original event row. The generic
`exhausted_row_rejects_exact_decoded_labels` theorem proves a list containing
that value cannot equal any successfully decoded event-label list. No failed
allocation turns into an empty list, wraps a byte, or weakens the exact check.
This admission theorem is role-local: it requires the role's filtered label
list to retain the invalid row. An unrelated role can omit it. Rust rejects
overflow globally; equivalence with emitted Rust descriptors assumes successful
Rust lowering. This package does not claim that existing per-role admission
globally rejects arbitrary synthetic certificates for an overflowing source.

## Replay

With Lean 4.30.0 and Python `z3-solver` installed:

```sh
python proofs/wire-frame-refinement/check_all.py --lean /path/to/lean
```

The runner checks the preserved pre-edit proof records and exact production
source bridge, builds the current Lean library, replays all regression proofs
against current production definitions, audits every named theorem, and runs
the Z3/finite correspondence checker into a temporary directory. The final-form
CI gate invokes it after the existing Lean and elastic proof gates.

The Lean proofs establish selected-color bounds and exclusion, preservation of
original inequalities and owner separation, safe same-class reuse at the graph
level, domain isolation, field/event preservation, self-send and no-Roll
behavior, owner tie-breaking, and exact exhausted-label rejection. Fixed
regressions cover ordinary, nested, reverse-nested, coextensive and sibling
Rolls, existing route inequalities, partitioned domains and all 256 byte colors.
`WireSourceBridge.lean` retains the exact nested fixture bytes freshly emitted
by Rust before the repair and proves both roles match the new canonical labels.

The Z3 checker has 40 symbolic obligations/controls, 38,840 exhaustive small
input cases, nine named fixtures, and eleven historical finite source models.
It compares independently written mask/imperative and list/argmin algorithms,
including rejection of the first invalid 256 row and concrete mutant controls.

## Claim boundary

Runtime `Covers` / `SameClassUnique` remains an explicit external premise.
Same-class reuse is not an unconditional concrete runtime uniqueness theorem.
The generic Lean theorems use only `propext` and `Quot.sound`; no admitted,
native-decision, or additional axioms are allowed by this package's audit.
The Z3 and Python models are source transcriptions, not extracted Rust or a
universal Rust-to-Lean refinement proof. Source intervals are valid, laminar,
and use unique preorder ordinals in the available scope-ID range. Capacity
failure describes the fixed greedy prefix, not optimal graph coloring.

`pre-edit/` preserves the passing candidate, generated source bridge, commands,
hashes, and Lean/Z3 logs recorded before editing production Lean definitions.
Those records describe the historical gate; the current regression proofs
deliberately import the production definitions rather than the copied candidate.
