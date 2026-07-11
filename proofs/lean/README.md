# Hibana Lean proofs

This host-only package checks a normalized Hibana choreography model with Lean
Core and Std. It is outside the Cargo workspace and does not enter Pico builds,
runtime memory, or flash artifacts.

The kernel-checked boundary covers:

- role projection for `send`, `seq`, `par`, `route`, and `roll`;
- send/receive duality, unit-only self-send locality, payload self-send
  rejection, uninvolved-role erasure, and exact
  `(label, payload schema)` preservation for the normalized send projection;
- structural validity of projected events, roll scopes, and route scopes;
- rejection of trace commits whose `(label, payload schema)` key is absent from
  the enabled frontier;
- runtime-monitor soundness for runtime-erased endpoints: accepted operations
  name the exact descriptor event ID and match its direction, peer, label, and
  payload schema before the exact commit; stale IDs and action mismatches are
  rejected without changing the commit state;
- one all-role `GlobalConfig` with event-indexed wire queues, exact
  session/epoch/event/peer/label/schema messages, route-arm agreement, and a
  `SessionFidelity` invariant. Send, receive, unit-local, and resolver steps
  preserve choreography identity, queue fidelity, and subject reduction;
- one exact rendezvous owner and structural program image per session
  generation; mixed-image and cross-rendezvous role attachment have no
  successor binding;
- exact dynamic `resolve` authority by route site and resolver id, including
  explicit left/right selection and terminal rejection;
- injective descriptor resolver-site identity over `(program image, scope,
  resolver id)`, where the image explicitly contains role facts, packed column
  counts for atoms, resolver rows, and every scope marker, plus exact blob bytes;
  callback registration is keyed exactly by `(program image, resolver id)` so
  topology-distinct programs cannot overwrite each other and same-id sites share
  one typed callback only inside one exact image; differing scope-marker counts
  or exact marker bytes directly imply distinct registration keys;
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
  membership, uninvolved-route erasure, and reentry mode. Exact certificates
  accept only the canonical program bytes, every role event/dependency/conflict,
  route arm, resident boundary, lane bitmap/range/step, commit-chain, and roll
  row, plus the blob-external role/lane/depth metadata. They match the complete
  global message preorder and every route authority decoded from the shared
  program blob even when the projected role is uninvolved, and take no
  independently supplied topology input;
- distributed cancellation states with first-fault preservation, queue
  quarantine, affine role observation, and explicit transport, waiter, and
  lease owner sets. Each observation consumes that role's three owner entries,
  and finite or fairly scheduled observation reaches a state with all queues and
  all resource owners retired;
- one choreography-derived route-membership row per global event. Global route
  selection is synchronized into every declared role state, non-selected arm
  events retire only with an empty queue, and resolver rejection is an explicit
  protocol-fault transition into distributed cancellation;
- a FIFO transport reference contract with append-only sequence identity,
  affine receive cursors, no replay, consecutive exactly-once delivery,
  post-drain peer-close observation, and rejection of sends after close;
- guarded nested or top-level roll reset through one global transition, with
  atomic owned-queue, local-cursor, and route-authority reset. The logical epoch
  remains unchanged; stale-frame exclusion outside the modeled queues depends
  on roll readiness plus the FIFO/exactly-once/no-replay transport contract and
  still requires an end-to-end transport-composition refinement. A finite
  scheduler scans every send, receive, local action, resolver selection,
  resolver rejection, and roll operation; one exhaustive strong-fairness
  premise ranges over recurrently enabled operations of that type instead of
  imposing an impossible obligation on a losing one-shot route alternative;
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

The gate exports real production-cursor frontiers, payload schemas, canonical
resident column counts, exact program/role bytes, and an independent diagnostic
topology witness from Rust. The exact certificate derives its topology from the
bytes rather than trusting that witness. General Lean theorems cover every
accepted production-layout byte image, equate its action column with
`projectGraph`, and join byte decoding, exact operation keys, and commit in one
cursor-refinement result. The generated
corpus contains 13 traces with 55 frames, seven exact-byte projection
certificates, and four exhaustive
progress closures. It exercises all roles of the base choreography, both
intrinsic route arms, nonzero `u32`/`i32` schema separation, send/receive
projections, nested and repeated roll
restart, resolved left/right arms, nested resolver sites, alternating resolved
roll reentry, and resolver rejection. A separate production runtime export
checks a five-region live slab, poison retirement, lease-generation exhaustion,
and four allocation-failure atomicity certificates.

This is not a source-to-source proof of arbitrary downstream Rust. The general
byte-decoder and transition theorems are quantified over accepted descriptor
images; the seven generated images are production witnesses used by the gate.
Every generated exact image also carries rejecting mutations for its first
global message contract, local action lane, resident metadata and lane bitmap,
plus dependency, route resolver, route child/lane-step/commit, local route, and
roll rows whenever those columns exist. The normalized projection shape still
omits physical packing as protocol semantics, but the exact descriptor
certificate separately reconstructs and compares that packing byte for byte.
Pointer provenance and typed storage operations remain checked by production
tests, Kani, and Miri rather than modeled as source-level Lean memory. Production
frontier traces check concrete packed behavior; progress closures exhaust the
normalized Lean model. The slab theorem does not claim pointer provenance or typed
initialization; Rust unsafe contracts, Miri, and layout tests retain that
responsibility. Global logical progress uses a constructive finite scan of the
descriptor-derived operation universe. `GlobalExecutionCoherent` now states
only that this executable scan is nonempty for a live unfinished state; it no
longer incorrectly requires every causally later `seq` event or non-selected
route arm to be immediately executable. A separate theorem proves that every
returned operation is a real `GlobalConfig.step?` successor. Distributed
liveness additionally requires the stated operation-scheduling fairness
assumption, and no theorem claims termination of an infinitely rolled
choreography.
The message-contract proofs treat equal `SCHEMA_ID` values as equal canonical
wire schemas, never as proof of Rust nominal type equality. Schema zero denotes
the canonical zero-byte unit contract used by `()`; local runtime actions reject
a nonempty encoding that claims this identity. Incompatible downstream
encodings or validators require distinct nonzero ids. Lean and Kani preserve every schema bit
and compare the descriptor identity; they cannot derive uniqueness across
arbitrary downstream trait implementations. Exact program-image binding closes
this boundary for roles attached to one local session generation. A remote
transport must bind its `SessionId` to the same protocol image during connection
setup.

The normalized model separates the production cursor's candidate frontier from
commit authority, so a dynamic route can expose candidate labels while remaining
uncommittable until its resolver transition succeeds. Aeneas, Verus, Mathlib,
custom axioms, `Classical.choice`, `sorry`, and `admit` are not part of this
boundary. The axiom audit permits only the `propext` and `Quot.sound`
dependencies introduced by the checked Core/Std proofs, and the gate requires
every exported theorem in the package to appear in that audit.

Run the same fail-closed gate used by CI:

```sh
bash .github/scripts/check_lean_proofs.sh
```
