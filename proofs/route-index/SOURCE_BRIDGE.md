# Immutable sorted route index: proof and source bridge

Base core: 3aef31ba015c75ea824b8b41f5603b03f5dd336b.

## Current claim

For every finite raw route table and every raw u16 scope query, certified binary lookup has exactly the original scan's outcome: a unique matching slot, no match, or invariant failure. The constructor selects the fast path only if every complete raw row is a valid Route ID (0..8191) and adjacent rows are strictly increasing. Any failed certificate retains the original scan unchanged, including duplicate/malformed rejection and unordered-table behavior.

SortedRouteIndex.lean proves adjacent-checker soundness, the old scan's specification, result uniqueness, the half-open binary loop's range preservation and strict termination, and unconditional indexed/scan equivalence. It has no sorry, admitted statement, or custom axiom; reported foundations are propext, Classical.choice, and Quot.sound.

check_sorted_route_index.py independently checks symbolic loop invariants and arbitrary-u16 tables of every length0..34 plus42,64,79. Nonvacuity cases cover shifted/gapped layouts, successful lookup, miss, wrong kind, absent sentinel, duplicate matches, invalid rows and permutation fallback. The deliberately unguarded binary-search mutant has a SAT counterexample. Both formal gates passed before the sorted Rust patch.

## Explicit source bridge

1. ScopeId is a complete encoded u16. Route IDs are exactly0..8191. The query comparison uses raw(), not a masked local ordinal; other kinds and the absent sentinel cannot alias a route.
2. The old lane_image.rs::route_scope_slot validates every visited row via decode_resident_route_scope, retains a first match and fails on a second. The formal scan models the same left-to-right operation with failure absorbing. That Rust fallback file remains byte-identical to base.
3. metadata/route_index.rs reads the constructor's exact little-endian column bytes. It rejects truncated columns, raw IDs>=8192 and non-increasing adjacent rows. Returning false neither admits nor rejects a descriptor; it only retains the old lookup.
4. RoleImageRef::new computes the private boolean from its own columns and immutable static byte array. The bytes and columns remain paired for the image's lifetime. There is no public certificate setter or constructor argument. The four intentionally forged negative fixtures explicitly set false. Trust in the crate's immutable descriptor ownership is an explicit source assumption, not a theorem about arbitrary memory mutation.
5. RoleImageRef::route_scope_slot retains the footprint/column-count guard. Certified search reads each candidate through the existing route decoder, compares complete raw IDs, and maintains[low,high). Every iteration shrinks width. The Lean fuel count+1 only expresses termination; Rust uses a while loop. Midpoint arithmetic is bounded by the compact column count and cannot overflow on the qualified host64/Thumb32 targets.
6. No event-enabled, frame, lane, roll, route authority, liveness, endpoint state, or wire-protocol logic changes. The certificate only optimizes immutable structural lookup. The stored byte fits existing RoleImageRef padding; no index array, blob bytes, or endpoint scratch is added.
7. Rust/source correspondence remains a reviewed bridge. The proofs are not a proof of all Rust execution, compiler correctness, or QUIC interoperability. QUALIFICATION.md and source.sha256 bind the reviewed implementation to constructor differential tests, every decodable ScopeId, exact production graph qualification, full workspace tests, existing full Lean gate and measured footprint/performance.

## Qualification decisions

The final lookup uses inline(never), a nonsemantic code-generation choice that satisfies the large-function source guard and avoids flash duplication. Host metadata/blob sizes and all measured slab/scratch sizes remain unchanged; Thumb core archive text decreases. Full QUIC integration, network retiming and the separate rolled-offer correctness issue remain outside this patch.

An initial dense-identity proposal was proved first (RouteIndex.lean/check_route_index.py and their logs). A new production-shape test correctly found route IDs[1,2,3], because SourceBuilder shares ordinals across Route/Roll/Parallel. That proposal was superseded by the sorted proof before the revised implementation. Historical preimplementation hashes preserve the gate chronology; they are not the final source manifest.

## Reproduce the final local proof gate

Run proofs/route-index/check.sh with LEAN_BIN pointing to Lean4.30.0 and PYTHON pointing to a Python interpreter with z3-solver installed. The recorded Z3 version is5.1.0. No downloads or source changes are performed by the gate.
