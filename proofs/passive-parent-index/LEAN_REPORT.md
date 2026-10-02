# Passive-parent lookup: Lean qualification

## Result

`PassiveParentIndex.lean` passes Lean 4.30.0. No Rust files were changed for this proof. The model is an arbitrary finite, ordered relation, not a tree. Queries and scopes are unbounded naturals; no small-model bound, dense numbering, connectedness, or whole-cursor claim is used.

Main theorems:

- `certified_direct_equals_scan`: unique parent/arm keys, every legacy fact valid, and every present child agreeing with its immutable owner imply owner lookup followed by the exact passive-edge check equals the legacy first-match scan
- `hybrid_equals_scan` and `source_hybrid_equals_scan`: full equivalence for all inputs, including malformed facts and arbitrary failed certificates; fallback preserves the original error/first-match order
- `checked_hybrid_equals_scan`: additionally models actual owner decoding as success or invariant failure. The full-owner validation guard must imply every owner read is safe, including unused conflict rows. There is no safety assumption on the false/fallback branch
- `scopeKeys_nodup`: unique scope IDs imply unique flattened `(scope, 0)` / `(scope, 1)` keys
- `backward_invalid` and `cycle_not_all_forward`: backwards edges fail the modeled legacy source validation; any consistently slotted finite cycle contains a non-forward edge

The proof prints dependencies for the principal results. Only Lean's standard `propext`, `Classical.choice`, and `Quot.sound` appear; there are no custom axioms or proof holes.

## Non-vacuity and negative witnesses

Kernel-evaluated examples cover present/absent queries, empty/missing facts, an owner with no passive edge, duplicate parent/arm keys, duplicate child edges with differing parents, duplicate scope IDs, wrong owners, invalid unused owner reads, a two-edge cycle, harmless unused cyclic owner metadata, and malformed facts both before and after a matching row.

`dropping_owner_guard_changes_harmless_miss` explicitly proves a guard-dropping mutant changes a harmless legacy miss to an invariant failure. Another witness has duplicate keys where unguarded direct lookup misses but fallback correctly returns the later matching legacy row. A later malformed fact disables indexing but does not invalidate an earlier successful first match.

## Source bridge boundary

The Lean file documents the bridge obligations in detail. These remain Rust review/testing obligations, not asserted Lean axioms:

1. Flatten exactly the legacy `(slot ascending, arm 0, arm 1)` getter results, preserving missing/invalid facts and returned keys/children
2. Establish all legacy getter validations and unique scopes, including byte/column/row bounds, scope decoding, arm/event/lane-step checks, strict child-slot ordering, and ownership agreement
3. Validate all owner/conflict rows, including unused rows newly read by the fast path. A rejected descriptor must store a false certificate without eagerly raising the legacy error
4. Bridge semantic `parentRow` to the already-qualified unique raw-ID route lookup plus binary arm indexing, and retain the exact child comparison
5. Keep certificate and lookups on the same immutable bytes. Leave the bounded ancestor walk and its exhaustion behavior unchanged

The abstract full-owner guard has an explicit soundness hypothesis. This file does not prove the Rust const checker, byte decoder, pointer safety, stage-1 binary search, or performance.

## Reproduction

From `hibana-passive-parent-index`:

```sh
../tools/lean/lean-4.30.0-linux/bin/lean proofs/passive-parent-index/PassiveParentIndex.lean
```

Verified successfully with Lean `4.30.0`, commit `d024af099ca4bf2c86f649261ebf59565dc8c622`.
