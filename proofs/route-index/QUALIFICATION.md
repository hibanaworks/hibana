# Immutable route lookup qualification

Base core commit: 3aef31ba015c75ea824b8b41f5603b03f5dd336b.
Isolated worktree: hibana-route-index, branch perf/certified-route-index.

## Scope

The constructor certifies immutable route rows as valid, strictly increasing complete raw ScopeIds. Certified RoleImageRef lookup uses binary search. Noncertified descriptors retain the original full uniqueness/decode scan unchanged. No event-enabled, frame, lane, roll, route authority, liveness, or wire-protocol logic changed.

The initial dense-ordinal experiment correctly failed certification on [1,2,3]; SourceBuilder shares ordinals across Route/Roll/Parallel. It was superseded by strictly sorted lookup only after the revised Lean and Z3 proofs passed. Earlier diagnostic/proof logs remain for chronology and are not the final performance claim.

## Formal gate

proofs/route-index/SortedRouteIndex.lean proves adjacent-checker soundness, scan specification, result uniqueness, binary-search range preservation and termination, and unconditional indexed/scan equivalence for arbitrary finite tables. No sorry, custom axiom, or admitted theorem is used; reported foundations are propext, Classical.choice, Quot.sound.

check_sorted_route_index.py independently verifies inductive loop conditions and arbitrary-u16 tables of lengths0..34,42,64,79. Positive/negative witnesses cover shifted and gapped layouts, misses, wrong kinds, absent sentinel, duplicate authorities, invalid rows and permutation fallback; an unguarded binary-search mutant has a counterexample. SOURCE_BRIDGE.md states the immutable-data/decoder/constructor correspondence and proof limits.

## Existing full Lean gate

The repository's check_lean_proofs.sh passed on the final source:709 static theorems,506 generated theorems,182 parallel and36 causal correspondence theorems,16 runtime and2 public-operation certificates, including claim and axiom audits. The existing production proof still states its external kernel-refinement and owner-evidence premises. This does not turn the lookup proof into a full QUIC correctness proof.

## Runtime and source qualification

- Final regression source uses inline(never) for the lookup, avoiding forced large-function inlining and code duplication
- final-regression.log: cargo test --offline --workspace,823 passed,0 failed,11 existing ignored (including3 doctests)
- New tests include every decodable u16 ScopeId on a projected Seq+Roll graph,49 two-row raw-table constructor cases times7 queries comparing outcomes including panic, malformed constructor fallback, complete raw kinds, bounds and8192-ID boundary
- source-gates.log: source file and maintainability budgets, underscore-discard, frozen-image, exact-layout, compiled-descriptor, projection-surface and route-authority gates
- qualification/result.log: exact current six-role g::par(g::par(key16/17,key18/19),TLS24/25) graph copied for diagnostic access only, with protocol imports changed from hibana to crate; all50 route rows certify on all6 roles and every decodable ScopeId equals the old scan
- Main vendor, QUIC sources, and the independent correctness branch were not patched

## Public API benchmark

The unchanged projected-api-microbench/bench.rs imports production protocol, carrier and runtime sources. Baseline is its original unmodified reference-TLS release rlib; optimized is this worktree’s release core. Both benchmark programs use rustc edition2024,opt-level3,codegen-units1,panic abort. Build and timing are serialized with the shared Rust lock. Both complete installation, repeated16-byte transactions and retirement; all poll/wake counts match.

Final paired300-transaction runs,3 repetitions,median microseconds/request:

| Case | Baseline | Indexed | Reduction |
|---|---:|---:|---:|
| Tiny HeaderMask |52.262|54.273|−3.85%|
| Initial HeaderMask |283.561|259.245|8.58%|
| TLS HeaderMask |6433.479|5480.455|14.81%|
| TLS CryptoInput |4996.698|4568.390|8.57%|
| TLS OpenEarly |5215.946|4349.968|16.60%|

The tiny result is a small regression in this run and timings are noisy; no universal speedup is claimed. This isolates a useful structural improvement but leaves the ancestor/parent scan hotspot. No real-network throughput improvement has been measured with the patch.


A separate small-topology follow-up retained all earlier results and used5 paired runs of10000 transactions: tiny medians 54.579→52.058µs (4.62% reduction), with identical40005 polls/40004 wakes. The differing short-run sign shows noise; neither a universal improvement nor a systematic tiny regression is claimed. See small-topology.log.

## Footprint

Host exact-graph diagnostics: RoleImageRef96B,RoleLaneImage16B,RoleImageColumns60B unchanged. All four Initial-role blobs remain3188B and both TLS-role blobs4280B. The bool fits existing padding; no blob/index array or endpoint scratch was added.

Thumbv6m release archive totals (qualification/footprint.log and final-footprint.log): core text75936→72258B (−3678B,−4.84%),rodata11878B unchanged,data/bss0. Pico projection example remains text20B,rodata319B,data/bss0. These are archive section totals, not final linked firmware sizes.

Existing release-mode stack-canary measurements (qualification/stack-metrics.log): fanout2519→2439B,route-heavy2567→2487B,linear1895B unchanged. Slab/live endpoint/frontier/scratch sizes are identical for all3 measured shapes. These are host-stack measurements, not Thumb hardware stack measurements.

## Limits

This is an immutable lookup equivalence/performance change, not a correction for the separate rolled-offer bug. Full QUIC regression/interoperability against the chosen integrated core and actual network performance remain integration responsibilities. Ordinary miri_runtime_owner tests do not mean the Miri interpreter was run. Kani,Miri,and the complete final-form aggregate were not run for this change.
