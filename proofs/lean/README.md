# Hibana Lean proofs

This host-only package checks a normalized Hibana choreography model with Lean
Core and Std. It is outside the Cargo workspace and does not enter Pico builds,
runtime memory, or flash artifacts.

The model is normative for Hibana rather than a transcription of one paper
calculus. Paper results motivate the obligations; Hibana's production selector,
descriptor, affine ownership, and transport rules determine the checked
semantics when the designs differ.

The modeled language is the finite-role, non-delegating Hibana core. It does not
claim higher-order channel/session passing. Progress is per session under the
stated fairness and live-carrier premises; session-pool results prove isolation,
not deadlock freedom for arbitrary host code that interleaves multiple sessions.
The descriptor is a finite template; dynamic streams and RPC rounds are runtime
`SessionId` instances rather than new continuation types.

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
- one all-role `GlobalConfig` with event-indexed descriptor-admitted queues,
  exact session/epoch/event/peer/lane/label/schema facts, route-arm agreement,
  and a `SessionFidelity` invariant. These semantic facts are reconstructed
  after carrier evidence matches the resident descriptor; they are not
  misrepresented as raw wire fields. Send, receive, unit-local, and resolver
  steps preserve choreography identity, queue fidelity, and subject reduction;
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
  global message preorder, its canonical physical lane assignment, and every
  route authority decoded from the shared program blob even when the projected
  role is uninvolved, and take no independently supplied topology input;
- unified asynchronous cancellation states separating protocol-admitted queues
  from transport-accepted FIFO frames. Faults clear protocol queues, while
  accepted frames can only drain into quarantine; peer close and drain commute,
  no post-fault step can re-enable protocol progress, and a constructive
  decreasing scheduler reaches finite transport and participant retirement
  without `Classical.choice`. Every accepted FIFO is keyed by the exact
  `(session, generation, lane, sender, receiver)` channel, channel ownership is
  unique, and every queued raw frame carries only that channel, its affine
  sequence, and a byte-bounded frame label. A fail-closed descriptor admission
  checker maps `(source, target, lane, frame-label)` to one global occurrence;
  evidence injectivity proves that the same header cannot name two events, and
  only then reconstructs logical label, schema, epoch, and global event ID;
- history-independent observation admission: carrier-private generation,
  sequence, and arrival history cannot change the descriptor occurrence selected
  by one fixed header observation. This exact local monitor theorem has no FIFO
  premise;
- one choreography-derived route-membership row per global event. Global route
  selection is synchronized into every declared role state, non-selected arm
  events retire only with an empty queue, and resolver rejection is an explicit
  protocol-fault transition into distributed cancellation;
- a strong affine-delivery reference profile with authenticated peer/direction
  binding, append-only live sequence identity, carrier-instance generation,
  affine receive cursors, no unsolicited
  replay, consecutive exactly-once delivery, post-drain graceful close, immediate
  abort retirement, and rejection of sends after close. A generation identifies
  a carrier instance, not a QUIC connection ID; migration and connection-ID
  rotation preserve it;
- guarded nested or top-level roll reset through one global transition, with
  atomic owned-queue, local-cursor, and route-authority reset. The logical epoch
  remains unchanged. An end-to-end composition theorem joins roll queue clearing
  to a drained transport cursor: the next accepted frame has the next sequence,
  every prior-iteration frame has a smaller sequence, and a reused `SessionId`
  cannot alias an old frame when the carrier allocates a new carrier
  generation. A finite
  scheduler scans every send, receive, local action, resolver selection,
  resolver rejection, and roll operation; one exhaustive strong-fairness
  premise ranges over recurrently enabled operations of that type instead of
  imposing an impossible obligation on a losing one-shot route alternative;
- slab certificates proving absolute-address alignment, capacity bounds,
  resident/workspace/endpoint separation, and pairwise owner-region
  disjointness for production-generated layouts;
- finite-state projectability certificates covering every role-local projection
  and the all-role asynchronous `GlobalConfig`. Every covered reachable state
  is complete, has a real global successor, or is cancelling/retired; this
  removes the former hand-written `GlobalExecutionCoherent` premise for accepted
  artifacts. An independent static checker rejects parallel endpoint-selector
  races, route-controller/observer knowledge failures, and ambiguous roll
  reentry using the same message-erased selector rules as production lowering.
  Empty-versus-visible observer paths are rejected because absence is not
  asynchronous branch evidence, and roll reentry never substitutes for route
  authority.
  Canonical `(source, target, lane, frame-label)` evidence is complete,
  byte-bounded, and injective before an inbound global occurrence ID may be
  used as its abstract name; every verified role descriptor carries the same
  canonical frame-label column.
  Per-role state learns intrinsic route selection only from an exact queued
  frame carrying that role's event identity and conflict membership; there is
  no arbitrary shared route-publication operation. Each operation is checked
  against its unique role owner, and each role-local queue entry must carry the
  canonical global occurrence lane recovered from the exact descriptor image.
  A wrong-lane frame invalidates the local state rather than being reinterpreted.
  Each visible distributed operation simulates exactly one global transition.
  The finite projectability closure establishes semantic unstuckness for every
  accepted reachable live state;
- session-family lifecycle proofs: fresh attach cannot overwrite either the
  finite authority domain or its backing lookup, removal requires a retired
  global status, removal preserves every other session, and re-attach creates a
  fresh initial configuration. All protocol, first-fault, and cancellation-
  observation operations on distinct `SessionId` values commute, so
  multiplexing does not require protocol-specific Rust types;
- verified protocol certificates binding the projectability closure and every
  role's exact descriptor bytes to one choreography, plus runtime transition
  certificates classifying protocol success as one transition,
  preview/drop/requeue as zero transitions, and mismatch/fault as cancellation.
  Compact states decode only after every finite column and local snapshot is
  canonical and the choreography passes the role/self-send well-formedness
  checker; malformed or partial states reject every effect, including preview
  and fault, instead of acquiring executable semantic state. Starting from the
  full global invariant, accepted protocol, fault, ambiguous-receive, and
  cancellation-observation transitions preserve it, while preview/drop/requeue
  are proved stutters;
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
corpus contains 13 traces with 55 frames, sixteen exact-byte role projection
certificates, four focused local progress closures, seven all-role
projectability closures, and seven verified protocol artifacts. It exercises
all roles of every generated choreography, both
intrinsic route arms, nonzero `u32`/`i32` schema separation, send/receive
projections, nested and repeated roll
restart, resolved left/right arms, nested resolver sites, alternating resolved
roll reentry, and resolver rejection. A separate production runtime export
checks a five-region live slab, poison retirement, lease-generation exhaustion,
and four allocation-failure atomicity certificates.

The static projectability boundary also matches production's single receive
FIFO per `(role, lane)`. A sender change is accepted only when the sender is
unchanged, the occurrences belong to opposite arms of one route, or a finite
send/receive causal closure proves that the earlier receiver precedes the later
send. Canonical parallel-arm intervals are proved physically lane-disjoint, and
their memberships prevent list order across concurrent arms from masquerading
as local order. Route-local traffic can contribute to a causal closure only when
one endpoint fixes that arm, so an unrelated branch cannot silently authorize a
later sender. The production const checker and the Lean checker use the same
three cases without adding runtime queues, endpoint types, or wire fields.

This is not a source-to-source proof of arbitrary downstream Rust. The general
byte-decoder and transition theorems are quantified over accepted descriptor
images; the sixteen generated role images are production witnesses used by the
gate and are grouped into seven complete protocol artifacts.
Every generated exact image also carries rejecting mutations for its first
global message contract, local action lane, resident metadata and lane bitmap,
plus dependency, route resolver, route child/lane-step/commit, local route, and
roll rows whenever those columns exist. The normalized projection shape still
omits physical packing as protocol semantics, but the exact descriptor
certificate separately reconstructs and compares that packing byte for byte.
Pointer provenance and typed storage operations remain checked by production
tests, Kani, and Miri rather than modeled as source-level Lean memory. Production
frontier traces check concrete packed behavior; projectability closures exhaust
the normalized Lean model for the generated protocols. The slab theorem does
not claim pointer provenance or typed initialization; Rust unsafe contracts,
Miri, and layout tests retain that responsibility. Global logical progress uses
a constructive finite scan of the descriptor-derived operation universe.
`GlobalExecutionCoherent` remains a low-level proposition, while accepted
projectability certificates establish its needed progress conclusion for every
certificate-reachable state. Every compact successor is proved to be a real
`GlobalConfig.step?` transition. Distributed
liveness additionally requires the stated operation-scheduling fairness
assumption, and no theorem claims termination of an infinitely rolled
choreography.
Global closure exploration fuel is derived from choreography event, queue,
route, and all-role local-state dimensions; acceptance still requires checking
the complete successor closure, so an insufficient search cannot pass.
The message-contract proofs treat equal `SCHEMA_ID` values as equal canonical
wire schemas, never as proof of Rust nominal type equality. Schema zero denotes
the canonical zero-byte unit contract used by `()`; local runtime actions reject
a nonempty encoding that claims this identity. Incompatible downstream
encodings or validators require distinct nonzero ids. Lean and Kani preserve
every schema bit and compare the descriptor identity; they cannot derive
uniqueness across arbitrary downstream trait implementations. Exact
program-image binding closes this boundary for roles attached to one local
session generation. A verified protocol artifact proves that every exported
role descriptor refines the same choreography; it does not prove that remote
machines exchanged that artifact. Initial all-role agreement is a deployment or
application-bootstrap premise, not a `Transport` handshake. A QUIC carrier needs
no custom transport parameter, TLS extension, or additional round trip. Its
replayable 0-RTT data must be rejected, delayed, or admitted under an
application-profile replay policy before becoming Hibana protocol evidence.
Lean proves exact admission for every observation and separately proves the
strong affine-delivery profile; it does not prove arbitrary Rust `Transport`
implementations. A carrier claiming global fidelity retains conformance proof
for peer binding, FIFO framing, requeue, cancellation, wakeup, generation
isolation, and anti-replay. A protocol that exposes unauthenticated, unordered,
or repeated observations retains local monitor safety, but its authentication,
loss, retry, and freshness logic is outside the affine-delivery conclusion.

This separation is what permits protocol families without contaminating the
core. A QUIC implementation can treat raw datagrams as generic packet events and
keep packet numbers, stream state, loss recovery, and congestion control in
protocol-owned data; established ordered streams may use the strong profile. A
Raft implementation can use one repeated request/reply template per peer and
attempt, keeping terms, logs, quorum, persistence, and timers in application
state. Session isolation is proved, but QUIC cryptographic/transport correctness
and Raft consensus invariants require their own protocol-specific proofs.

Dynamic resolver publication is atomic inside one rendezvous generation. Across
independent runtimes, the resolver input or carrier must provide one agreed
decision to roles that can act before receiving branch evidence; the model does
not claim to synthesize distributed consensus. Intrinsic routes keep this
choice in band. Production shared route-table authority is reserved for these
explicit resolver scopes; non-controller intrinsic route commits require
endpoint-local frame evidence, and intrinsic-only descriptors allocate no
shared route table.

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
