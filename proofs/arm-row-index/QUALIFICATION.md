# Arm-row prefix-elision qualification

## Source and proof gate

- Isolated checkout: `hibana-arm-row-index`, branch `perf/certified-arm-row`
- Original source: `3aef31ba015c75ea824b8b41f5603b03f5dd336b`
- Required stage 1 patch SHA256: `83693843e29ad7d1dad93d692a051affdd44a3ab69d0e590945d85bad324480d`
- Required stage 2 patch SHA256: `643399287acf95a98ef015060b894188c390d341d02b50db52436d58d3c19058`
- Stage 2 source manifest verified before any stage 3 source changes; additional preimplementation manifest retained
- Lean and Z3 gates passed before Rust edits; independently reviewed and rerun
- Lean: 12 named theorems, 28 kernel-decided witnesses; no holes/custom axioms, exact axiom closures in `lean.log`
- Z3: raw packed-word equivalence at 0, 1, 2, 3, 4, 8, 16, 34, 50, 68, 100 arm rows, 13 nonvacuous positive/negative cases, and 32-bit prefix bound
- Final source manifest: `source.sha256` (includes final stage 1+2+3 source; historical earlier manifests describe their earlier bases)
- `SOURCE_BRIDGE.md` explicitly identifies immutable lifetime, full constructor/column validity, selected-row decoding, original failure order, cumulative-prefix bounds and fixed-width arithmetic assumptions

## Final source change

Only two production files change (37 added / 3 removed lines). RoleImageRef uses its existing private parent-index certificate to select a direct packed-row accessor. RoleLaneImage retains its checked standalone APIs; the internal child helper preserves the same parent/child/owner checks. There are no fields, format or ABI changes. Actual lane-step location reads still compute their needed prefix. No candidate/frame reordering is included.

Certificate ownership is a reviewed internal invariant, not a type-level proof token. Both production callers of the faster internal helpers are immediately dominated by the private certificate check. A future caller must retain that precondition.

## Runtime and source qualification

- Complete `cargo test --offline --workspace`: **834 passed, 0 failed, 11 existing ignored**, including source authority checks and UI cases
- Four new tests compare certified/current/original arm and child accessors, invalid arms, usize overflow/out-of-range slots, byte mutations, malformed predecessor/later-row behavior and constructor-disabled fallback
- The malformed predecessor test explicitly shows an unguarded direct read would incorrectly succeed
- All eight source gates pass: source file size, maintainability, underscore discards, frozen image, exact layout, compiled descriptor authority, projection surface and route authority taxonomy
- Actual six-role production composition: all 50 route rows per role certify; 1,200 exact arm accessor comparisons and 366 parent lookup comparisons agree
- Header sizes stay RoleImageRef=96, RoleLaneImage=16, RoleImageColumns=60 bytes
- Initial-role blobs stay 3,188 bytes for roles 16/17/18/19; TLS-role blobs stay 4,280 bytes for roles 24/25
- Whole existing Lean gate: **passed**. Static 709-theorem and 36-example audits; 506 generated, 182 parallel, 36 causal, 16 runtime and 2 public-operation theorem audits all passed on the final source exports. Four generated files are byte-identical to stage 2; RuntimeGenerated differs only in its ASLR-dependent slab base address and passed its new audit.
- Normal runtime/Thumb footprint: **passed**. Core Thumb text is 75,164 bytes (stage 2: 72,570; original: 75,936), rodata 11,920 bytes (stage 2/original: 11,878); data and bss remain zero. The tradeoff is +2,594 text and +42 rodata versus stage 2, still 772 fewer text bytes than original. Pico projection example stays text 20 / rodata 319 bytes.
- Release host peak stacks: fanout 2,439, linear 1,879, route 2,487 bytes. Relative to stage 2, fanout/route are identical and linear decreases 16 bytes. Slab, endpoint, SessionKit and frontier/scratch measurements are unchanged. No additional metadata SRAM is introduced.

## Public-API performance

Three paired 300-exchange runs after a 100-exchange warm/control run, all serialized with other Rust workloads. Same unchanged production protocol/carrier/runtime inputs and exact release compiler/profile/features/rustflags fingerprints. Median microseconds per exchange:

| Workload | Stage 2 | Stage 3 | Reduction |
|---|---:|---:|---:|
| Tiny HeaderMask | 57.158 | 50.938 | 10.88% |
| Initial HeaderMask | 229.739 | 191.303 | 16.73% |
| TLS HeaderMask | 1,586.248 | 1,223.246 | 22.88% |
| TLS CryptoInput | 969.181 | 736.528 | 24.01% |
| TLS OpenEarly | 1,629.024 | 1,428.510 | 12.31% |

Every run completed the same exchanges/retirement and identical polls/wakes per workload (1,205 polls for 300 exchanges). This benchmark contains no crypto or network work. Short control timings are noisy; the reported values are repeated medians, not latency guarantees. Raw results and input/compiler hashes are in `artifacts/arm-row-index`.

The previous stage 2 real 90,112-byte-plus-empty transfer measured 9.427799 seconds user CPU versus original 58.824337 and stage 1 55.44428, with exact bytes, both Closed and unchanged 279/280 datagrams. That result belongs to stage 2. Subsequent stage 3 retiming passed at5.106626 seconds user CPU, exact payload, both Closed and the same279/280datagrams; see `../../artifacts/arm-row-index/real-owner-summary.json`. This is an observed composed-owner self-UDP fixture, not independent-peer interoperability.

## Qualification limits

No Kani or Miri interpreter run is claimed. Ordinary `miri_runtime_owner` cargo tests are not interpreter execution. No full final-form umbrella or complete QUIC interoperability claim is made. The existing blocked roll/liveness correctness work is outside this patch. Pico footprint measurement is not Pico application readiness. No commit, push, author-identity change or live-vendor integration was performed by this task.
