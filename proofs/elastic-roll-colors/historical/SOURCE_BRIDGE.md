# Elastic Roll frame-color proposal gate

The frozen compiler/runtime baseline is `a6339772d4bf2c905f491e0284d3bef36e79bb6f`, integrating the requested external rolled-route correction with the qualified route-index performance change. This gate supports a compiler color-allocation proposal. It does not authorize weakening the unchanged `UniqueMatch` guard, modifying the old blocked runtime patch, or claiming a universal Rust correctness proof.

## Established defect

The diagnostic Rust copy adds prints and leaves control flow unchanged. `../run-inventory.log` records all ten effective descriptors. After the legal prefix Install40, Installed41, PeerReady42, Ready78, ResultTaken71, both Inspect52 (occurrence 2) and Open55 (occurrence 6) have source 0, receiver 1, lane 0, wire frame color 0. Both pass the unchanged `event_enabled` and offer-entry qualification in `../run.log`. `UniqueMatch` correctly rejects the two different `(scope, occurrence)` candidates. The current typed Open55 receive succeeds in the control.

The later durable regression in `../../elastic-roll-color-fix/security-before-expanded.log` additionally establishes a safety failure: a queued Open55 payload is accepted by a typed Inspect52 receive. The unchanged baseline has 10 existing tests passing and four new failures: legal current offer, Pending/dropped preview, genuine old reentry then current continuation, and rejecting the wrong typed receive. `LeanMinimalTrace.lean` proves that the exact global trace cannot consume queued event 6 as event 2, including after an explicit old reset. The new Z3 key checks expose that baseline alias and reject it under the proposed colors. Z3 is checking the wire-key abstraction; the durable Rust regression is the actual execution evidence.

## Final proposed graph

Every vertex is an effective event occurrence ID. Its immutable domain is `(from, to, lane)`, not its logical message label. Snapshot all baseline frame colors before recoloring. Also compute the full vector/set of containing elastic Roll scope identities. For distinct non-self-send occurrences `a,b`, add an undirected edge precisely when:

1. their immutable domains are equal; and
2. their frozen baseline colors differ **or** their full Roll memberships differ.

Run one final allocation pass after source emission has completed. In occurrence order, choose the first available color among all 256 byte values. Preserve every vertex; do not truncate the event count, wrap a counter/color, drop an edge, or silently reuse a blocked color. Report exhaustion if no color is available. The generic Lean theorem proves prefix exhaustion, not optimal graph colorability. This particular proposed graph is complete multipartite per domain, with classes `(baseline color, Roll membership)`, so its first-fit behavior is class coloring; this is still not a claim of minimal semantic interference or a minimally necessary number of wire colors.

The earlier `color_capacity_model.py` and its outputs are preserved as superseded evidence. Their lexical `continuation_end` cutoff is **not** accepted as a lifetime proof. `scope_ranges.rs:120–142` serves compile-time selector checks; it does not fence runtime reentry. A nested Roll inside an outer route arm may remain eligible after execution continues outside that arm. Moreover, separate candidate previews can represent an outer Roll prefix and a completed inner Roll. Symmetric full membership handles both directions without relying on such a cutoff.

## Innermost identity bridge, including coextensive scopes

In `hibana-rolled-route-perf-integration`:

- `src/g/source.rs:75–81`, `89–94`, `109–114`, and `126–132` recursively emit each subtree into one contiguous interval
- `src/global/const_dsl/eff_list.rs:95–103` appends events; `216–233` records nonempty Roll intervals
- Thus Roll intervals emitted by this AST are nested or disjoint; scope ordinals are unique and allocated in preorder (`src/g/source.rs:64–70`)
- Nested scopes can have equal interval bounds. The greater preorder ordinal is the deeper identity, consistent with `scope_ranges/nesting.rs:27–58` and `src/global/typestate/cursor/scope_route/roll/nesting.rs:4–19`

For a vertex, select the containing Roll of minimum interval width, breaking equal widths by greater preorder ordinal; use a separate None/sentinel for no containing Roll. All Roll memberships are exactly this scope's ancestors, including itself. Therefore equal full memberships are equivalent to equal innermost **identities**. Equal interval bounds alone do not identify a scope. The planned Rust encoding `ordinal + 1`, with 0 for None, must retain that distinction and must fit the source scope ordinal domain.

`LeanRollMembership.innermost_identity_iff_full_membership` proves this abstract characterization under reflexive/antisymmetric ancestry and the exact membership-as-ancestors premise. The emitter correspondence above is source inspection, not a mechanized refinement of the Rust emitter. The membership capacity model verifies interval laminarity. Its independent Z3 checker reconstructs full memberships from the emitted interval inventory and compares the full vector with the canonical innermost identity for every pair in every retained fixture.

## Exact source map

All paths below are relative to `hibana-rolled-route-perf-integration`; the companion checker compares the frozen files against the pinned Git commit.

- `src/global/const_dsl/eff_list.rs:95`: baseline frame color begins at 0
- `src/g/source.rs:79`: Seq preserves source order without recoloring
- `src/g/source.rs:89–102`: Route publishes bounds and merges colors
- `src/global/const_dsl/allocation/frame_labels/route.rs:35–70`: per-domain right-arm classes are injectively remapped away from left-arm colors
- `src/g/source.rs:126–132`: Roll marks descendants Reentrant and colors its body
- `src/global/const_dsl/allocation/frame_labels/roll.rs:24–61`: per-domain exact-route-path classes get separate colors
- `src/global/const_dsl/event_relations.rs:22–44`: exact route-path equality includes inside/outside distinctions
- `src/global/const_dsl/allocation.rs:11` and `allocation/frame_labels.rs:19–28`: palette size is 256 and first-free searches every byte value
- `src/global/typestate/facts/inbound_key.rs:7–34`: inbound key is source/lane/frame color; receiver is fixed by the endpoint
- `src/endpoint/kernel/offer/select_observed.rs:27–88`: all descriptors are scanned and key, lane, event-enabled, and offer-entry conditions are checked before unique selection
- `src/runtime_core/unique_match.rs:21–25,44–49`: different candidates become Ambiguous and ambiguity is rejected

## Proof boundary

`LeanColorGate.lean` proves successful greedy allocation preserves palette bounds, every interference edge, and the exact vertex inventory. It proves preservation of baseline inequalities plus additional edges. Its eligible-wire uniqueness theorem explicitly requires `Covers`.

`LeanRollMembership.lean` specializes the graph to baseline color plus innermost identity, proves symmetry and both separation properties, and states the exact remaining obligation `SameClassUnique`: in every **reachable, well-formed** runtime state, two completely qualified eligible occurrences with the same domain, baseline color, and membership must be the same occurrence. This obligation implies `Covers`, but it is not proved for arbitrary Rust states here. Eligibility must use all unchanged guards under the proposed wire colors, including key-dependent passive offer qualification. Freezing the baseline eligible set is insufficient.

Same membership ensures every Roll reset affects both events together; baseline Roll allocation distinguishes route paths. Same-path ordered occurrences then rely on completion, dependency, and lane-head invariants. A universal mechanized proof of those runtime invariants remains absent. This package is a bounded/source-linked proposal gate, not such a proof.

`LeanNestedReverse.lean` checks the exact global shape `Roll(Seq(A, Roll(B)))`. After A,B complete, either inner reset→B or outer reset→A is legal. An actual outer reset clears inner completion: outer reset→B without fresh A and outer reset→inner reset are rejected. These are alternative legal reset suffixes, not an assertion of simultaneous Rust offer eligibility; the fixture uses ordinary messages.

## Results and limitations

- Existing `LeanMinimalTrace.lean` passes, including queued-Open/wrong-Inspect rejection and legal old reentry preserving current Open
- `LeanColorGate-recheck.log`, `LeanRollMembership.log`, and `LeanNestedReverse.log` pass with no `sorryAx`; intermediate failed proof attempts remain in separately named logs
- `roll-membership-z3.log` and JSON contain 31 expected SAT/UNSAT results: baseline collision and wrong typed-key match; new separation; SAT premises; removed-edge/coverage and safe-reuse negative controls; full 0–255 palette; honest generic overflow; and all 11 concrete source-derived graph assignments
- `roll-membership-capacity-model.json` models TLS 333 events, full base 765, and early-enabled 830. All use at most 150 colors in any domain; edge counts are 27,485 / 41,985 / 43,027. The ten-event diagnostic uses six colors with assignment `[0,0,1,2,0,0,3,4,1,5]`
- The Python source transcription supports the supplied AST subset and asserts disjoint-endpoint parallel components, hence lane 0. It is not a general Rust lane allocator or a compiler-equivalence proof. Self-sends are excluded from the new graph in correspondence with the proposed Rust pass
- Source hashes establish identity, not semantics. Matching upstream Lean source to cached `.olean` donors permits documented cache reuse but cannot independently attest how the old cache was built

The production allocator and all 14 durable tests must still be validated after implementation. This package makes no postimplementation runtime-pass claim.

## Reproduction

From the workspace root:

```
python artifacts/current-offer-source-diagnostic/proofs/roll_membership_capacity_model.py hibana-quic
/tmp/hibana-quic-proof-venv/bin/python artifacts/current-offer-source-diagnostic/proofs/check_roll_membership_gate.py
LEAN_PATH=artifacts/current-offer-source-diagnostic/proofs tools/lean/lean-4.30.0-linux/bin/lean -o artifacts/current-offer-source-diagnostic/proofs/LeanColorGate.olean artifacts/current-offer-source-diagnostic/proofs/LeanColorGate.lean
LEAN_PATH=artifacts/current-offer-source-diagnostic/proofs tools/lean/lean-4.30.0-linux/bin/lean artifacts/current-offer-source-diagnostic/proofs/LeanRollMembership.lean
LEAN_PATH=hibana/proofs/lean/.lake/build/lib/lean tools/lean/lean-4.30.0-linux/bin/lean artifacts/current-offer-source-diagnostic/proofs/LeanNestedReverse.lean
python artifacts/current-offer-source-diagnostic/proofs/check_source_bridge.py
```

Retain stdout/stderr in the corresponding logs when refreshing the evidence. The source checker records immutable identities and validates all ten diagnostic descriptors; it must not be described as proving Rust refinement.
