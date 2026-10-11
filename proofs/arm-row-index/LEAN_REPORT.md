# Arm-row prefix-elision Lean gate

## Result

PASS with Lean 4.30.0, commit d024af099ca4bf2c86f649261ebf59565dc8c622.

Command, from the isolated hibana-arm-row-index worktree:

```sh
/workspace/scratch/0915e8fbff81/tools/lean/lean-4.30.0-linux/bin/lean \
  proofs/arm-row-index/ArmRowIndex.lean
```

Exit status: 0. Output is recorded in `lean.log`. This gate ran before any stage 3 Rust edits. This preimplementation gate changed only its proof file/report/log; no Rust implementation or vendor source was changed at that point.

## Formal scope

The model is an arbitrary finite list of raw packed route-arm rows. Each eight-byte row is represented by exactly its 16-bit event-start, 16-bit event-length, 16-bit child-slot, 8-bit encoded lane-step-length and 8-bit reserved fields. Explicit missing and invalid entries model failed row reads. No generated-protocol, tree, contiguity, sorted-event or nonempty-row assumption is required.

The original accessor reads the selected packed row first, then predecessor packed rows and lane-step lengths in ascending order, then the selected lane-step length and final event/coherence/cumulative bounds. The direct accessor returns the same selected packed row. The distinction between packed-row decoding and lane-step-length validation is explicit: a zero-event row with a nonzero encoded length passes packed decoding but fails length decoding.

The executable Boolean certificate validates every row and every cumulative prefix. Its accepted input set is at least as broad as the proposed source gate; the existing passive-parent certificate has additional scope, owner, count and forward-edge conditions. These additional conditions can only restrict when direct lookup is used.

Proved results include:

- Every successfully decoded lane-step length is at most 256 and coherent with zero-event status
- Out-of-range selected reads fail identically
- Certificate acceptance establishes every queried row, all preceding length reads, and the precise final bounds
- Certified direct lookup equals original lookup for every natural-number query, including out-of-range queries
- Guarded hybrid equals original for every raw table, query, event bound and lane-step-row bound
- Failed certification retains the original result, including earlier success before a malformed later row
- Shared binary-arm and checked index-arithmetic guards preserve every result
- Each decoded prefix is bounded by its row count times 256
- With at most 65535 arm rows, the total is at most 16,776,960, strictly below 2^32

The key results are `certified_direct_equals_original`, `guarded_hybrid_equals_original`, `query_guards_preserve_every_outcome`, and `source_prefix_no_u32_overflow`.

## Nonvacuous kernel-checked witnesses

The examples include a valid three-row table with a canonical zero row and gapped nonempty event ranges, empty tables, out-of-range queries, malformed reserved bits, explicit missing/unreadable predecessors, malformed zero-event encoding, malformed later rows that preserve earlier success through fallback, cumulative prefix overflow although individual rows fit, event-bound failure, invalid binary arms and checked index multiplication overflow.

The malformed-predecessor and prefix-overflow witnesses explicitly show that an unguarded direct accessor would change an invariant failure into success. The malformed-later-row witness shows why failed global certification must fall back rather than reject the descriptor.

## Axiom audit

Lean's printed axiom closures are:

- `certified_direct_equals_original`: propext, Quot.sound
- `guarded_hybrid_equals_original`: propext, Quot.sound
- `query_guards_preserve_every_outcome`: propext, Quot.sound
- `source_prefix_no_u32_overflow`: propext, Classical.choice, Quot.sound

No `sorry`, `admit`, custom axiom declarations, native-decide trust shortcut or `sorryAx` occurs in the accepted proof. Concrete examples use kernel-checked `decide`.

## Source bridge and limits

1. `metadata/route_arm.rs` splits the same packed fields and rejects the all-ones event sentinel, noncanonical zero event ranges, nonzero reserved byte and zero-event/nonzero encoded-length mismatch. These conditions match `PackedRouteArmRow::from_packed_parts` and `lane_step_len` exactly.
2. `metadata/passive_parent.rs` checks event bounds and every cumulative lane-step prefix against the same columns used by `lane_image.rs::route_arm_row`. Its arm count matches the scope count and footprint on the published parent-index path.
3. Certificate reads use the backing array bound, whereas runtime reads use `columns.blob_len()`. This difference is closed by `RoleImageRef::new` first validating every column span and constructing `BlobPtr` with blob_len no greater than the immutable array length. A naked `RoleLaneImage` is not automatically certified.
4. The existing parent-index bit is valid only for its associated immutable bytes and columns. The fast path must remain gated by that bit. Existing query/arm guards and the selected packed decoder must remain in place, and the false-bit path must retain the old checked accessor.
5. The proof returns the entire same row, including child-slot metadata. It does not authorize changing receive scanning, callback order, event/frame validation, route/roll/liveness authority, allocations, resident state or ABI.
6. Source correspondence is a reviewed bridge. This is not extracted Rust, a full byte-pointer safety proof, whole-cursor verification or a QUIC correctness/performance claim. Independent Z3 validation and final Rust/source/runtime qualification remain separate obligations.
