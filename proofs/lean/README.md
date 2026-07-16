# Hibana Lean proofs

This host-only package checks a normalized Hibana choreography model with Lean
Core and Std. It is outside the Cargo workspace and does not enter Pico builds,
runtime memory, or flash artifacts.

The checked object is Hibana's choreography-derived runtime enforcement kernel:
projection produces compact role descriptors, and `OperationAdmission` permits
only the exact enabled descriptor event to commit.

The model is normative for Hibana rather than a transcription of one paper
calculus. Paper results motivate the obligations; Hibana's production selector,
descriptor, affine ownership, and transport rules determine the checked
semantics when the designs differ.

The modeled language is the finite-role, non-delegating Hibana core. It does not
claim higher-order channel/session passing. Progress is per session under the
stated fairness and live-carrier premises; session-pool results prove isolation,
not deadlock freedom for arbitrary host code that interleaves multiple sessions.
The descriptor is a finite template; dynamic interaction instances and
coordination rounds are runtime `SessionId` values rather than new continuation
types.

The kernel-checked boundary covers:

- role projection for `send`, `seq`, `par`, `route`, and `roll`;
- send/receive duality, unit-only self-send locality, payload self-send
  rejection, uninvolved-role erasure, and exact
  `(label, payload schema)` preservation for the normalized send projection;
- structural validity of projected events, roll scopes, and route scopes;
- rejection of trace commits whose `(label, payload schema)` key is absent from
  the enabled frontier;
- operation-admission soundness for runtime-erased endpoints: accepted operations
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
- exact route-authority packing: the spare route-scope bit distinguishes
  intrinsic from dynamic authority, intrinsic rows require a canonical zero
  resolver field, and dynamic rows retain the complete `u16` resolver domain;
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
  checker filters the current inbound frontier by
  `(source, target, lane, frame-label)` and accepts exactly one candidate.
  Ordered occurrences on one route path may reuse a frame label. Distinct paths
  sharing an elastic roll, route alternatives, and parallel frontiers are proved
  to have different evidence before admission reconstructs logical label,
  schema, epoch, and global event ID;
- history-independent observation admission: carrier-private generation,
  sequence, and arrival history cannot change the descriptor occurrence selected
  by one fixed header observation. This exact local admission theorem has no FIFO
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
  a carrier instance, not an application-visible identifier; logical address
  migration and identifier rotation preserve it;
- a receipt-aware carrier boundary with explicit idle and outstanding states.
  Polling issues one receipt; commit and discard advance the affine cursor,
  requeue restores the exact pre-poll state, every resolution consumes the
  receipt, a second poll while borrowed is unavailable, close and requeue
  commute, and abort prevents receipt revival. A generic `CarrierSemantics`
  refines this model only when send, poll, close, abort, and all three receipt
  resolutions commute with one explicit abstraction;
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
- elastic rolled FIFO and proof-only route-choice history for legal output
  pipelining before remote
  receive. A proof-only mixed admission history derives each static
  occurrence's generation from its prior descriptor-admitted subsequence,
  preserves unrelated-event interleaving, and keeps route choices isolated by
  `(conflict, publication ordinal)`. Accepted descriptor cursor sends append the
  exact next occurrence while carrier send and history length/cursors remain
  aligned; no generation field enters the wire, resident endpoint, or Rust API;
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
  Canonical `(source, target, lane, frame-label)` evidence is complete and
  byte-bounded. The compiler colors exact route paths independently for each
  complete inbound key inside elastic rolls; ordered events on one path retain
  color reuse. The current-frontier checker rejects zero or multiple matches
  before an inbound global occurrence ID may be used as its abstract name.
  Every verified role descriptor carries the same canonical frame-label column.
  Per-role state learns intrinsic route selection only from an exact queued
  frame carrying that role's event identity and conflict membership; there is
  no arbitrary shared route-choice operation. Each operation is checked
  against its unique role owner, and each role-local queue entry must carry the
  canonical global occurrence lane recovered from the exact descriptor image.
  A wrong-lane frame invalidates the local state rather than being reinterpreted.
  Each visible distributed operation simulates exactly one global transition.
  Separate finite global and distributed closures establish semantic
  unstuckness for every accepted reachable live model state;
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
- one `ProtocolExecutionGuarantees` protocol-local proposition. An
  accepted verified protocol artifact simultaneously supplies exact all-role
  descriptor refinement including route participant lists, reachable-state subject
  reduction and session fidelity, separate global and distributed semantic
  unstuckness certificates, deterministic current-frontier transport
  admission, FIFO-or-causal `.roll` reentry, exact intersection of selected and
  locally attached participants, a unique dynamic-route controller, descriptor-
  admitted inbound choice knowledge for every other branch-sensitive role,
  sealed runtime-local membership before dynamic resolution, callback attach
  exclusion while endpoint storage is
  borrowed, and finite asynchronous cancellation retirement from a live fault
  and a covered well-formed transport snapshot. It does not claim that a
  concrete carrier or remote installation
  conforms;
- one `MediatedCarrierContract` proposition that assumes no ordering, delivery,
  replay, or authentication property. It proves only exclusive receipt
  ownership, exact requeue, affine resolution, and abort invalidation. A checked
  dropping-carrier witness satisfies this contract while admitting no closing
  affine refinement;
- one Lean-only `CarrierProfile` chain, `Mediated -> Authentic -> Ordered ->
  Closing -> Fair`. Each successor adds a contract, every stronger profile
  proves every weaker one, and `Fair` exposes rather than hides its deployment
  fairness premise. Instantiating it with one run's
  `GlobalFairnessAssumptions` now gives the recurrently-enabled scheduling
  result directly at the end-to-end boundary.
  `carrier_profile_hierarchy_is_strict` uses proof-only channel-erasing,
  dropping, close-ignoring, and false-fairness witnesses to refute every
  adjacent reverse implication. The profile index never enters a Rust type,
  endpoint image, or wire header;
- one `AssumptionIndexedDeploymentContract` that combines the selected profile,
  exact all-role images, and the message-erased endpoint runtime. Exact image evidence
  may be supplied by a static certificate, authenticated manifest, or separate
  verified bootstrap session; no mandatory handshake is added to the carrier;
- one checked `StaticDeploymentCertificate` whose entry identities cover the
  complete production protocol family in exact order and whose role images are
  byte-exact. The generated eight-entry artifact is mandatory in the Lean gate;
  proof metadata remains host-only and actual device flashing remains an
  external premise;
- `VerifiedCodec` evidence separating canonical wire schema identity from
  nominal Rust type identity. Equal IDs must resolve to one canonical byte
  language across the deployment registry. A full payload claim requires
  round-trip, canonical-byte, and implementation-refinement laws for every
  choreography schema; schema-ID matching alone does not acquire those laws;
- `PreparedKernelSemantics` and `RustKernelRefinement`, which state the pure
  prepare/commit, zero-or-one-transition production obligation explicitly.
  Kani and Miri discharge concrete Rust-side obligations; Lean does not recast
  their result as a source-level Lean proof;
- one checked `ProductionKernelArtifact` covering the seven normalized effect
  classes, all six protocol operation constructors, and the eight production
  ownership boundaries exactly once. The generated artifact constructs the
  canonical `RustKernelRefinement` for the normalized effect relation and for
  each independent operation case. `CrossToolProductionRefinement` retains the
  exact owner inventory instead of reducing it to an unnamed conjunction. This
  is the finite exported-kernel witness, not a proof of arbitrary Rust source;
- one eight-member accepted `VerifiedProtocolMember` family whose checked
  capability coverage contains communication, sequencing, parallel
  composition, intrinsic choice, resolved choice, and recursion. The witness
  is protocol-neutral, and every member carries canonical codec coverage for
  all event schemas. It does not import algorithm-specific correctness into the
  core;
- `ElasticErasureRefinement`, proving append, receive, transport send/receive,
  and proof-only route-choice history commute with erasure of iteration ordinals; and the
  composition theorem
  `assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement`, which
  joins exact translation validation, deployment agreement, verified codecs,
  Rust-kernel refinement, profile-indexed guarantees, and erased traces. These
  are machine-checkable conditional boundaries, not claims of historical
  priority;
- `AssumptionIndexedProductionRefinement`, which composes the accepted primary
  member, complete static family, canonical codecs, cross-tool kernel evidence,
  and selected carrier profile into one production witness;
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
cursor-refinement result. The endpoint runtime additionally decides its mediated domain
exactly: a request is accepted iff that descriptor operation can commit, and
every other request is rejected. This is not completeness for arbitrary
black-box processes. The generated
corpus contains 14 traces with 66 frames, twenty-two exact-byte role projection
certificates, four focused local progress closures, eight all-role
projectability closures, eight distributed progress closures, and eight
verified protocol artifacts. It exercises
all roles of every generated choreography, both
intrinsic route arms, nonzero `u32`/`i32` schema separation, send/receive
projections, an augmenting-path lane reassignment that first-fit rejects,
nested and repeated roll
restart, resolved left/right arms, nested resolver sites, alternating resolved
roll reentry, resolver rejection, and a three-role cyclic sender handoff across
alternating roll arms. A separate production runtime export checks a four-region
live slab, poison retirement, lease-generation exhaustion, and four
allocation-failure atomicity certificates.

The static projectability boundary also matches production's single receive
FIFO per `(role, lane)`. A sender change is accepted only when the sender is
unchanged, the occurrences belong to opposite arms of one route, or a finite
send/receive causal closure proves that the earlier receiver precedes the later
send. Parallel-arm lanes are recolored only where endpoint-role sets conflict;
disjoint endpoint sets may reuse one physical lane. Arm membership prevents list
order across concurrent arms from masquerading as local order. Route-local
traffic can contribute to a causal closure only when
one endpoint fixes that arm, so an unrelated branch cannot silently authorize a
later sender. The production const checker and the Lean checker use the same
three cases and the same role-indexed, first-write-wins causal witness map.
Production scratch is fixed by the exact 256-role wire domain rather than by
source event capacity, and is erased after descriptor construction. For a
scope-free sequence, endpoint-independent propagation and exact fold-prefix
reuse justify one forward closure per earlier receive instead of recomputing
every prefix. Every roll
body is additionally checked through one explicit
unfolding with fresh per-iteration route and parallel identities. A sender
change from the body tail to the next head therefore requires a causal handoff;
route exclusion from the previous iteration cannot be reused as evidence. This
adds no runtime queues, endpoint types, descriptor rows, or wire fields.

This is not a source-to-source proof of arbitrary downstream Rust. The general
byte-decoder and transition theorems are quantified over accepted descriptor
images; the twenty-two generated role images are production witnesses used by the
gate and are grouped into eight complete protocol artifacts.
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
`GlobalConfig.step?` transition. A second finite certificate explores per-role
route-knowledge lag. It rejects repeated intrinsic frame evidence, admits a
dynamic resolver outcome at most once per role, rejects non-controller
resolution, and proves a local action or evidence transition exists in every
reachable unfinished live distributed model state. Distributed
liveness additionally requires the stated operation-scheduling fairness
assumption, and no theorem claims termination of an infinitely rolled
choreography.
Global and distributed closure exploration fuel is derived from choreography event, queue,
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
application-bootstrap premise, not a `Transport` handshake. Replayable early
traffic must be rejected, delayed, or admitted under an application-profile
replay policy before becoming Hibana protocol evidence.
Lean proves exact admission for every observation and the receipt-aware
reference carrier. It does not prove arbitrary Rust `Transport`
implementations. A deployment selects the weakest `CarrierProfile` supporting
its claim: mediation for exclusive receipts, authenticity for exact peer/frame
binding, ordering for FIFO/no replay, closing for close/abort observation, and
fair only with an explicit liveness premise. An unauthenticated, unordered, or
repeating carrier retains local protocol safety only at the mediated profile;
stronger conclusions require their corresponding evidence.
The standalone `proofs/unix-carrier` crate supplies one concrete connected Unix
datagram witness outside the Hibana package. Its official gate runs two
independent runtimes and checks FIFO delivery, fail-closed peer/header parsing,
affine consumption and requeue, normal logical-close wakeup, and fresh socket
generation isolation. The OS ordered-datagram contract is an explicit premise;
process crash detection, external authentication, and fairness are not inferred
from those tests.
Exact frame identity is part of that refinement: rewriting one valid branch
label into another is indistinguishable from an authentic alternate-branch
frame to a message-erased endpoint runtime unless additional wire identity is introduced.

This separation is what permits session families without contaminating the
core. A lossy or reordering carrier can expose raw message observations while
keeping authentication, sequence identity, retry, recovery, and ordering in
algorithm-owned state; authenticated ordered subchannels may use the strong
profile. A stateful distributed algorithm can repeat one finite interaction
template while keeping persistent state, membership, scheduling, and recovery
policy outside Hibana's protocol runtime. Session isolation is proved; algorithm-specific
safety and liveness invariants require their own proofs.

Dynamic route choice is controller-local and affine. Its participant list is
descriptor-derived, and runtime-local membership is sealed before external
resolver code, so a late attach cannot obtain a second evaluation of the same
occurrence. Resolved and intrinsic routes both require one first-visible
controller; the static checker rejects competing branch initiators rather than
treating a resolver as a distributed agreement oracle. Every non-controller
role must receive distinct descriptor-admitted branch evidence before its first
branch-sensitive operation. Each endpoint records its selected arm locally and
commits at most once; no shared route-decision sidecar is a second authority.
Rolled routes may choose differently after reentry without adding an iteration
field to the wire header, descriptor row, or resident endpoint.

The remaining trust boundaries are explicit. `RuntimeRefinement.lean`
classifies normalized runtime effects, while `RustKernelRefinement.lean` states
the pure prepare/commit simulation that production Kani/Miri evidence must
discharge; Lean does not extract or verify arbitrary Rust source.
`ProductionKernelArtifact.lean` removes that premise from the generated
reference composition: its checker requires all seven effect classes and all
six protocol operation constructors plus all eight production owners, proves
every accepted row, and constructs the same canonical `RustKernelRefinement`
for the effect relation and each operation relation.
`ProductionEndToEnd.lean` joins that cross-tool evidence to the exact static
deployment family and assumption-indexed end-to-end theorem. The Rust exporter,
Kani owner harnesses, and
strict-provenance Miri exporter case remain explicit members of the cross-tool
TCB; no source-level Rust theorem is inferred from their exit status.
`PublicOperationKernel.lean` independently specifies the nine public endpoint
operation phases and their lease classifier. The host-only Rust exporter emits
all 81 production outcomes; the Lean gate accepts them only when the generated
table is byte-for-byte equal to the independently computed table. Kani also
checks the same finite product symbolically, while Miri owns the executable and
ignored-exporter boundary.
`ElasticErasure.lean` does not use length agreement as a surrogate for trace
agreement: its transport relation requires well-formed histories and equality
between every carrier label and the descriptor-label image of the erased
occurrence trace. Send preservation therefore requires the exact descriptor
label for the appended global occurrence.
`DistributedRollRefinement.lean` now proves that independent endpoint reset
after one drained global `.roll` linearization is affine, order-independent,
strictly decreasing, and an abstract stutter. `ElasticIterationQueue.lean`,
`ElasticRouteHistory.lean`, and `ElasticAdmissionHistory.lean`
model the independent pre-receive case instead of forcing a false global
barrier. One mixed descriptor-admitted FIFO history per authenticated carrier
direction derives the next event-local ordinal across arbitrary nested rolls,
permits unrelated occurrences to interleave,
and composes every accepted descriptor cursor send with one exact append.
`global_atomic_roll_cannot_model_pipelined_single_send_reentry` remains as a
checked separation theorem: it prevents the atomic one-iteration view from
silently replacing the elastic normative view. The paired two-send production
cursor test is an official Miri case. `ElasticErasure.lean` then proves that
iteration ordinals disappear while static descriptor identity and affine
carrier cursors are preserved. Carrier FIFO,
anti-replay, peer binding, close/abort wakeup,
and link-loss notification are proved only for the reference model and must be
established by each concrete carrier. Any higher-level distributed algorithm
can be implemented above the protocol-neutral primitives, but its own safety,
recovery, timing, and fairness invariants are not Hibana theorems.

The literature boundary is deliberate. [Multiparty Asynchronous Session
Types](https://www.doc.ic.ac.uk/~yoshida/multiparty/multiparty.pdf) motivates
global types, projection, FIFO queues, fidelity, and progress; [Stay Safe Under
Panic](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2022.4)
motivates at-most-once ownership and cancellation; the [mechanised 2025
correction](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2025.31)
motivates Hibana's stricter explicit causality and projectability checker.
[Asynchronous distributed
monitoring](https://mrg.cs.ox.ac.uk/publications/asynchronous-distributed-monitoring-for-multiparty-session-enforcement/TGC_2011.pdf)
motivates local enforcement and global fidelity, but Hibana uses trusted
endpoint kernels rather than wrappers around arbitrary untrusted
processes.
[Session-type monitorability](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2021.20)
separates monitor soundness from completeness over monitored processes.
Hibana proves exact acceptance and rejection only for operations mediated by
its endpoint kernel; it does not claim completeness for a process or carrier
that bypasses that kernel.
[Crash-stop
MPST](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.CONCUR.2022.35)
and [timed affine
MPST](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2024.19)
make reliability and timeout assumptions part of their protocol theories;
Hibana instead keeps deadline, retry, membership, and failure-detector policy in
algorithm-owned state and maps terminal observations into one protocol-neutral
cancellation boundary.
[Communicating-session-automata bounded
compatibility](https://mrg.cs.ox.ac.uk/publications/verifying-asynchronous-interactions-via-communicating-session-automata/cav19.pdf)
and [decidable sender-driven
implementability](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2023.32),
[precise asynchronous
subtyping](https://arxiv.org/abs/2010.13925), and [denotational asynchronous
reasoning](https://arxiv.org/abs/2604.10646) are useful adjacent models. Hibana
does not claim their unbounded automata, substitution, or message-reordering
optimization theorems. [Asynchronous mixed
choice](https://arxiv.org/abs/2602.23927) permits transiently inconsistent
distributed choices that Hibana deliberately rejects, while [network-enforced
session types](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2026.17)
move loss/reordering-aware monitors into a programmable data plane. Hibana
claims neither mixed choice nor network enforcement. Its normative claim is
exactly the checked finite-role endpoint execution and deployment criteria
above.

"Choreography-derived runtime enforcement kernel" names this checked
implementation boundary; it is not presented as a new session-type calculus or
as an independent novelty claim for projection or runtime enforcement.

A defensible research-novelty candidate is the combination, not any component
in isolation: byte-exact proof-carrying role descriptors, exhaustive global and
distributed finite closures, a strict assumption-indexed carrier hierarchy,
affine receive receipts, in-band knowledge for both intrinsic and dynamic
choices, order-independent affine `.roll` materialization, epoch-erased elastic
roll pipelining, explicit cross-tool prepare/commit refinement, and one
message-erased resource-bounded `no_std` runtime representation. The targeted
related-work review
above did not identify that exact combination, but this repository does not
claim "first" or "world-first". Establishing priority requires a systematic
literature review, comparison artifacts, and external peer review. Recent work
on [mechanised synchronous
liveness](https://arxiv.org/abs/2605.23633) and [denotational asynchronous
reasoning](https://arxiv.org/abs/2604.10646) also prevents a novelty claim based
merely on using a proof assistant or proving model-level progress.

The normalized model separates the production cursor's candidate frontier from
commit authority, so a dynamic route can expose candidate labels while remaining
uncommittable until its resolver transition succeeds. Aeneas, Verus, Mathlib,
custom axioms, `Classical.choice`, `sorry`, and `admit` are not part of this
boundary. The axiom audit permits only the `propext` and `Quot.sound`
dependencies introduced by the checked Core/Std proofs, and the gate requires
every exported theorem in the package to appear in that audit.

Frontier scratch capacity is derived from the projected active-lane count. Lean
proves that every active offer entry has an owning active lane and that the
complete 256-lane wire domain bounds the resulting allocation; the production
selection kernel streams those entries instead of representing them in a fixed
candidate bit mask. Visit tracking uses exact local-entry identity rather than
route-scope identity, and Lean proves that the same allocation covers every
visited entry. Capacity exhaustion is an invariant failure, never silent
truncation.

All other lane-indexed endpoint storage follows the exact projected lane span.
Lean and Kani prove that this span covers every active lane and remains inside
the 256-lane wire domain; the removed two-lane binding reserve had no descriptor
or runtime authority.

Selected route history uses the same descriptor-first rule. One sparse runtime
row is admitted only for an emitted `(lane, route)` relation, and Lean proves
that relation membership is exactly the set of lanes used by the arm, duplicate
events do not allocate another relation, the 256-lane domain bounds the relation
count, and the emitted relation count covers every live row. Production lowering
computes the bitmap, local row, and last step in one event pass; Kani verifies
the full-domain accumulator transition. The endpoint therefore does not
allocate the Cartesian product of active lanes and maximum route depth.

Canonical program atom rows are strictly ordered by compact event identity.
Lean proves this for every normalized choreography, the compact Rust image
rejects a noncanonical order at construction, and Kani checks every order class
used by the production binary search. Runtime event lookup is therefore
logarithmic without adding an event-index column to the descriptor or endpoint.

Run the same fail-closed gate used by CI:

```sh
bash .github/scripts/check_lean_proofs.sh
```
