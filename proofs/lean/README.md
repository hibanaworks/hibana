# Hibana Lean proofs

This host-only package checks a normalized Hibana choreography model with Lean
Core and Std. It is outside the Cargo workspace and does not enter Pico builds,
runtime memory, or flash artifacts.

The kernel-checked boundary covers:

- role projection for `send`, `seq`, `par`, `route`, and `roll`;
- send/receive duality, self-send locality, uninvolved-role erasure, and label
  preservation for the normalized send projection;
- structural validity of projected events, roll scopes, and route scopes;
- rejection of trace commits that are absent from the enabled frontier;
- exact dynamic `resolve` authority by route site and resolver id, including
  explicit left/right selection and terminal rejection;
- injective resident resolver identity over `(scope, resolver id)`, including
  separation of distinct resolver ids that share one local route ordinal;
- visibility of unresolved dynamic-route candidates without commit authority;
- single-use resolver authority until an enclosing `roll` reset;
- fail-closed rejection of direct unresolved commits, wrong resolver ids,
  resolver reuse without reset, and any continuation after resolver rejection;
- unique route-arm authority in every commit state;
- exact refinement of compact route-arm values into left/right authority,
  including rejection without a publication successor for every value above
  the binary domain, plus separation of valid non-unique ready masks from
  invalid masks carrying out-of-domain bits;
- exact optional descriptor-arm decoding with byte `255` as the sole absence
  representation and every other non-binary value rejected;
- exact commit and resolver successor deltas from one prepared base state;
- atomic roll and route-arm reentry resets, including inside-clear and
  outside-preservation properties.
- production descriptor topology refinement for every sealed choreography
  constructor: local actions, route authority, route-arm membership, roll
  membership, uninvolved-route erasure, and reentry mode;
- slab certificates proving absolute-address alignment, capacity bounds,
  resident/workspace/endpoint separation, and pairwise owner-region
  disjointness for production-generated layouts;
- finite-state closure certificates proving that every covered model-reachable
  state is complete, has a commit/resolve successor, or has an explicit
  resolver rejection transition;
- fail-closed endpoint lease generation exhaustion, strict successful
  generation increase, first-fault authority, poisoned-attach rejection, and
  absence of poison-to-live revival before generation retirement. Publication
  after transport or resolver callbacks is permitted only if the same
  generation remains live.
- two-phase endpoint lease allocation, including general failure-state identity,
  exact successful commit, nonshrinking table capacity, and production-exported
  initial/growth planning failures, exact post-plan aborts, and compaction-aware
  aborts that preserve endpoint and observed lane authority plus every resident
  owner capacity while allowing frontier shrink. The production snapshot binds
  active-association cardinality and explicit session/lane witnesses as well as
  allocator fields. A generation poisoned by external callback re-entry after
  planning is proved unable to publish and leaves allocator state unchanged.
  Physical root relocation is normalized out of failure-state equality and
  checked separately by slab certificates, Kani packing proofs, and Miri.

The gate exports real production-cursor frontiers and descriptor topology from
Rust and checks them as concrete Lean proofs. The generated corpus contains 13
traces with 55 frames, seven projection certificates, and four exhaustive
progress closures. It exercises all roles of the base choreography, both
intrinsic route arms, send/receive projections, nested and repeated roll
restart, resolved left/right arms, nested resolver sites, alternating resolved
roll reentry, and resolver rejection. A separate production runtime export
checks a five-region live slab, poison retirement, lease-generation exhaustion,
and four allocation-failure atomicity certificates.

This is not a source-to-source proof of arbitrary Rust. The topology certificate
intentionally compares virtual-lane-independent facts because production packs
mutually exclusive route arms into fewer physical lanes. Production-frontier
traces check concrete packed behavior; progress closures exhaust the normalized
Lean model. The slab theorem does not claim pointer provenance or typed
initialization; Rust unsafe contracts, Miri, and layout tests retain that
responsibility. Logical progress does not claim transport delivery, scheduler
fairness, distributed liveness, or termination of a rolled choreography.

The normalized model separates the production cursor's candidate frontier from
commit authority, so a dynamic route can expose candidate labels while remaining
uncommittable until its resolver transition succeeds. Aeneas, Verus, Mathlib,
custom axioms, `Classical.choice`, `sorry`, and `admit` are not part of this
boundary. The axiom audit permits only the `propext` and `Quot.sound`
dependencies introduced by the checked Core/Std proofs.

Run the same fail-closed gate used by CI:

```sh
bash .github/scripts/check_lean_proofs.sh
```
