# Certified passive-parent lookup qualification

## Patch basis and scope

Base core: 3aef31ba015c75ea824b8b41f5603b03f5dd336b plus qualified stage 1 patch SHA256 83693843e29ad7d1dad93d692a051affdd44a3ab69d0e590945d85bad324480d. This is a separate stage 2 patch against that exact staged base. No author identity, commit, push, or live QUIC vendor modification is included.

Stage 2 uses two certificate bits in stage 1's existing one-byte header field. When every old fact read and every newly read owner is certified, passive-parent lookup uses the child's recorded owner and verifies that exact parent/arm's child. Otherwise it executes the original scan. The original scan body and route-count-bounded ancestor walk are text-identical. The existing arm metadata decoder moved unchanged into its own module to meet the 600-line source budget. Its visibility and the total conflict decoder's visibility are only widened within their existing internal module hierarchy.

No event-enabled, frame, lane, roll, route authorization, selected-arm state, or wire-protocol checks changed. Owning a conflict row alone does not create a passive edge.

## Formal gate before Rust edits

- Lean 4.30.0: arbitrary finite relation/query equivalence; explicit malformed facts, first-match early returns, failing owner reads and guarded fallback; unique scope-to-arm keys and strict forward-edge cycle exclusion
- Main theorem foundations: propext, Classical.choice, Quot.sound; no sorry, custom axioms or admitted statements
- The owner-guard-dropping counterexample is kernel proved without axioms
- Z3 5.1.0: arbitrary raw relations with 0..8, 16, 34 and 50 routes; positive and negative witnesses plus mutations showing both full-owner validation and the exact-edge check are necessary
- Source bridge is explicit in SOURCE_BRIDGE.md; preimplementation-source.sha256 records the unchanged stage 1 source at gate time
- Rust correspondence remains a reviewed bridge, not an extracted whole-program proof

## Runtime/source qualification

- Final cargo test --offline --workspace: 830 passed, 0 failed, 11 existing ignored, including 3 doctests
- New tests compare every decodable ScopeId with the original scan and cover unused invalid owners, malformed later edges, duplicate targets, wrong arms, passive cycles, and unused cyclic owners that grant no edge
- 360 primitive byte-mutation cases establish that every certified variant allows every skipped original fact and owner getter to execute; both accepted and rejected mutations are required
- Source-size, maintainability, discard, frozen-image, exact-layout, compiled-descriptor, projection-surface and route-authority guards pass
- Exact production g::par(g::par(Initial16/17, Initial18/19), TLS24/25) graph: both certificate bits true for all six roles' 50 routes; 366 comparisons with the original parent lookup pass, including gaps, missing IDs, other scope kinds and absent sentinel
- Existing full Lean aggregate PASSED: 709 static, 506 generated, 182 parallel, 36 causal, 16 runtime and 2 public-operation theorems, with claim/axiom audits

## Public API benchmark

The unchanged projected-api-microbench/bench.rs drives installation, repeated 16-byte endpoint transactions and retirement through production protocols, carrier and TaskSet. It has no crypto, sockets, provider snapshots or mailbox exchange. Build/timing use the shared lock. Compiler, profile, rustflags and target-kind fingerprints match between the original and both stages. Isolated builds enable the empty default=[] feature; production sources have no cfg(feature="default") use. Every measured poll and wake count matches.

Three paired runs of 300 transactions, median microseconds/request:

| Case | Original | Stage 1 sorted | Stage 2 parent | Reduction vs original |
|---|---:|---:|---:|---:|
| Tiny HeaderMask | 56.678 | 54.325 | 57.070 | −0.69% |
| Initial HeaderMask | 307.846 | 269.377 | 223.046 | 27.55% |
| TLS HeaderMask | 6771.252 | 5715.696 | 1689.435 | 75.05% |
| TLS CryptoInput | 5317.052 | 4488.454 | 1056.612 | 80.13% |
| TLS OpenEarly | 5520.104 | 4265.425 | 1705.794 | 69.10% |

Stage 2 reductions relative to stage 1 are 70.44% / 76.46% / 60.01% for the three TLS cases. Tiny is effectively flat relative to the original within these noisy small-duration measurements. These are isolated public-API observations, not real-network throughput claims.

## Footprint

- Host headers unchanged: RoleImageRef 96 B, RoleLaneImage 16 B, RoleImageColumns 60 B
- Actual four Initial-role blobs remain 3188 B; both TLS-role blobs remain 4280 B
- No new blob/index array, heap state or endpoint scratch
- Thumbv6m core archive: text 72,570 B, versus stage 1 72,258 B and original 75,936 B; +312 B vs stage 1, −3366 B vs original
- Thumb core rodata remains 11,878 B; data/bss remain zero
- Pico projection archive remains text 20 B / rodata 319 B / data+bss zero
- Existing host release stack metrics match stage 1 exactly: fanout 2439 B, linear 1895 B, route-heavy 2487 B; fanout/route-heavy are 80 B below original
- All measured slab, endpoint, frontier, sidecar and SessionKit sizes remain unchanged

Archive section totals are not final linked firmware sizes, and host stack-canary results are not hardware Thumb stack measurements.

## Limits

The separate rolled-loop boundary failures are not repaired by this equivalent lookup change. Full QUIC interoperability and controlled real 90 KiB retiming belong to integration qualification. The Miri interpreter, Kani and complete final-form umbrella script were not run. Stage 1 manifests document the prior qualified source; after applying stage 2, use this directory's source.sha256 for the current source.
