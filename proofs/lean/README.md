# Lean verification

This host-only package verifies Hibana's normalized choreography, descriptor,
and execution models with Lean Core and Std. It is outside the Cargo workspace
and contributes no code, metadata, memory use, or flash cost to Pico builds.

The model specifies Hibana's finite-role, non-delegating protocol kernel. It is
not a source-level verification of arbitrary Rust code or a specification of a
concrete transport.

## Checked boundary

For an accepted choreography and its exact role descriptors, the Lean
development establishes:

- projection and projectability for `send`, `seq`, `par`, `route`, and `roll`;
- exact preservation of event identity, direction, peer, logical label,
  canonical wire schema, lane, route membership, and frame label;
- rejection of malformed descriptors, ambiguous inbound observations,
  mismatched operations, unresolved choices, duplicate resolution, and invalid
  compact values without committing protocol state;
- subject reduction, session fidelity, route agreement, and unique message
  consumption for the global asynchronous model;
- correspondence between distributed role-local execution and global
  transitions for accepted finite-state protocol artifacts;
- protocol progress for covered reachable states, subject to the scheduling and
  carrier premises below;
- affine cancellation, first-fault preservation, queue quarantine, waiter
  release, and finite retirement for covered finite transport states;
- guarded repeated-region reset, FIFO or causal re-entry freshness, and erasure
  of proof-only occurrence history from descriptors, frames, endpoint types,
  and runtime state;
- isolation, removal, and fresh re-attachment of independent sessions;
- soundness of compact layout, allocation, public-operation transition, codec,
  deployment, and production-kernel artifacts;
- composition of verified protocol artifacts with explicit deployment,
  carrier, codec, and production-kernel evidence.

The principal externally relevant claim types are listed in
[`ClaimSurface.lean`](ClaimSurface.lean). The complete elaborated theorem
inventories are machine-checked snapshots, not prose maintained in this file.

## Required evidence

The strongest end-to-end conclusions require all of the following:

1. Every role uses the exact accepted descriptor image for the same
   choreography.
2. Peers agree on each canonical wire schema and use conforming codecs.
3. The deployment supplies the carrier properties required by its selected
   profile: mediation, peer authenticity, FIFO and replay exclusion, observable
   close or abort, and fairness where liveness is claimed.
4. The executor eventually polls operations that remain enabled.
5. Production Rust supplies the prepare/commit refinement and ownership
   evidence required by `ProductionEndToEnd.lean`.

These are explicit premises. Lean does not infer remote installation agreement,
transport authentication, delivery, failure detection, or scheduler fairness
from the `Transport` trait.

Rust handle borrowing is also an implementation boundary, not a consequence of
the carrier model. `Transport` borrows each Tx only for a send/cancel call, and
each Rx for the returned frame's lifetime, independently of the handle's internal
borrowed storage. The safe trait must not promise a handle-lifetime mutable
borrow that the kernel then recreates on every poll. Compiler rejection tests
cover self-references retained from those calls and retained outgoing payloads;
`transport_borrow` drives legitimate borrowed handles through actual Endpoints
under Miri. These checks support this Rust boundary; the Lean model and Kani do
not by themselves prove Rust aliasing soundness for arbitrary carrier code.

`ActorAdmission.lean` checks read-only admission observations from the actual
shared actor cursor. For each active row it compares the recorded result with
the dependencies decoded independently from the source, including blocked and
already completed rows. Ready rows also require issued carrier and owned-payload
view headers. The Tokio fixture transfers an Endpoint between two runtimes and
executes its independent work with `join!`; its snapshots become kernel-checked
Lean inputs. Missing rows, premature return, lost parallel readiness, completed
step replay and a missing payload view are rejected. This checks the observed
selected visit. It does not derive actual Rust value availability from a header,
nor establish fresh rolled visits, carrier delivery or general distributed
progress. The public owned-projection guard remains closed.

`ResourceHeaderOrigin.lean` derives an issued tag's initial or latest actual
receive origin from the complete transfer/header history. Static source event
order cannot choose this origin: a closed earlier roll body can execute again
after a later source receipt. The real two-runtime `receipt_refresh` test does
this, retains the same completion bits, receives a new tag, and uses that
Endpoint for the pending continuation. Both wire receipts are checked against
their actual before/after snapshots. Source elaboration separately proves that
an accepted closed roll restores each resource's instantiated owner; it does
not preserve the old Endpoint tag. The generated application of the custody
theorem retains complete carrier/header and originating-call histories as
correspondence contracts. It does not derive these histories from readiness,
or establish general distributed progress across independent visits.

The public guard rejects owned choreography before returning its projected
image. Private qualification tests obtain the image from the same production
actor lowering and then attach and drive actual Endpoints, including owned
encoding, receive validation, view publication, and shared actor progress.
They do not replace the owned kernel with a mock or treat the guard's rejection
as a successful runtime test. This still leaves public-path qualification as a
separate release obligation.

`carrier_contract::replaying_an_owned_frame_breaks_uniqueness_without_affine_delivery`
is an explicit counterexample outside the carrier assumptions. It copies one
accepted loan frame without moving another Endpoint, preserves FIFO, and feeds
the duplicate to a subsequent roll visit. Two runtimes then hold live resource
views, and the genuine next loan still has the same envelope. The test must not
be counted as a conforming uniqueness run. It makes the load-bearing no-replay
assumption visible: the core's three-byte envelope alone is not replay
protection. A reusable binding or a `One` token name would not establish that
property without actual consumption/replay enforcement and physical fencing.

`SourceTransferAvailability.lean` derives the causal order between a resource
use and any distinct transfer from accepted source elaboration and the exact
decoded actor rows. Consequently, a source-ready use excludes another handoff
of that resource. A checked drop of a still-needed view records a fault. These
results preserve the exact received value in application code or a private
future across any finite live source-use interval, while allowing independent
parallel work. The carrier composition binds this value to the checked receipt's
projected actor and view. The interval still needs boundary observations that
pass the independent checker; extracting these for every Rust execution,
realizing pending futures, and fresh rolled visits remain separate obligations.

The transferred value is identified by its actor and `(role, run, nonce)` header.
The temporary operation phase belongs to the actor's I/O state, rather than the
Endpoint value, and remains checked separately. The actual custody observations
show an unchanged value identity while its view changes to `SendPayload`.

`ActorReentryCommitObservation.ownedCheck` applies the owned wire check to the
same derived reset base as the first operation of a fresh visit. The general
composition preserves the pre-reset value tags and borrow phases, checks the
actual continuation envelope, and publishes at most one receiver authority
under the ordered carrier profile. The parallel reentry observations bind both
ends to the actual copied frame while preserving completed independent work.
They reject a different resource payload, sender or session. This envelope has
no visit tag: FIFO, replay exclusion and run isolation still come from the
carrier. General fresh-visit availability and distributed progress remain open.

`SourceHandoffAvailability.lean` covers a ready owned send without excluding
its own required acceptance. Checked boundaries either retain the exact value
in application code or a private future, accept that same source send action,
or report a checked retirement fault. Acceptance moves the sole held authority
to one queued frame; completed sender witnesses do not add an authority.
The result applies to arbitrary finite
suffixes and only asks for boundary observations while that exact value is
held. Its carrier composition starts from the actual checked receiver/view
tag, without requiring sender publication. This is a source-boundary and
value-location classification, not eventual admission, polling or delivery,
nor a proof of complete Rust correspondence or distributed progress.

`accepted_resource_use_derives_its_initial_or_last_receiver` derives the
required resource's origin for every instantiated binding of an accepted
source. It either retains the initial issuer or identifies the last earlier
handoff, proves that handoff causally precedes the use, and binds the use to
its receiver. The last handoff is derived from the immutable source prefix;
it is not an availability assumption or a runtime ownership table. This
source result still needs its runtime value and fresh-visit correspondence.

`SourceReceiptDependency.lean` derives the last handoff's receive operation,
binding lookup, actual actor and dependency from the accepted source. Exact
row coverage maps that source receive back to the decoded actor program.
Every source-ready required-resource row either has its derived initial
issuer or marks that receive complete among its actual source predecessors.
This result does not assume an issued header or a value-availability oracle.
The actual admission fixture applies it to every ready row of its generated
source and descriptor. This result alone does not establish that an Endpoint
value still exists in application code, a private future or an accepted frame.

`ActorCommitHistory.lean` connects completed source receipts to copied actual
commits and wire evidence. It derives the most recent commit of the same local
event, a checked suffix with no later commit of that event, and true completion
at every recorded suffix boundary. The recorded and current selected paths
may differ: the immutable source fixes the message at a shared occurrence.
Owned rows must pass the owned wire check; they cannot fall back to an ordinary
commit check. The two-runtime admission fixture records the actual pre/post
snapshots and wire bytes alongside its direct Endpoint operations, including
the observed order of both parallel sends. Its generated proofs reject missing
receipt records, unrecorded completion, corrupt owned bytes and reordered
observations. Reordering those pre/post snapshots is a trace inconsistency;
the protocol still permits either order of the independent parallel commits.
These records never control runtime execution.

Complete recording is an implementation contract, not something the Boolean
history checker can establish: it cannot detect an omitted reset/commit pair
with identical boundary snapshots. The current-receipt witness must still be
connected to its subsequent transfer history to establish value/future/frame
availability and general progress across independently advancing fresh visits.
Neither completion retention nor a valid header alone implies live ownership.
Comparison proofs use explicit decidable equalities without extending the
axiom policy.

The native remote-loss test executes the same owned global with a worker
process that exits before or after receiving its Endpoint. The survivor's
carrier deadline ends both pending roles, later operations retain the first
fault, and neither the same nor a different SessionId can reissue the binding.
The test observes one unread accepted frame before receipt and none after
receipt, then fences the old namespace. These are actual OS observations for
this carrier, not an automatic failure detector supplied by the core or a
general distributed-progress theorem.

Schema equality means equality of the canonical wire contract identified by
`SCHEMA_ID`; it does not prove cross-binary Rust nominal type equality.

## Deliberate non-claims

The proof package does not claim:

- verification of arbitrary Rust source or arbitrary `Transport`
  implementations;
- correctness of cryptography, authentication mechanisms, failure detectors,
  retries, deadlines, or application scheduling;
- safety or liveness of a distributed algorithm implemented above Hibana;
- termination of an intentionally infinite `roll`;
- channel delegation, unbounded role creation, or completeness for code that
  bypasses the endpoint kernel.

In particular, route agreement assumes a conforming controller execution.
Independently fabricated observations can select different arms at different
passive roles; authenticating the fabricating controller alone does not restore
this premise. There is no Byzantine agreement theorem for that setting.

Kani, Miri, Rust tests, and carrier conformance tests provide complementary
implementation evidence. Their success is not represented as a Lean theorem
about arbitrary Rust source.

## Proof structure

| Area | Primary modules |
| --- | --- |
| Syntax and projection | `GlobalSyntax.lean`, `DescriptorTopology.lean`, `DescriptorRefinement.lean`, `StaticProjectability.lean` |
| Causal must analysis | `CausalFlow.lean`, `StaticProjectability.lean`, `IterationErasure.lean` |
| Admission and commit | `OperationAdmission.lean`, `Commit.lean`, `PublicOperationKernel.lean`, `PreparedKernelRefinement.lean` |
| Global and distributed semantics | `GlobalSemantics.lean`, `GlobalFidelity.lean`, `GlobalProgress.lean`, `DistributedSemantics.lean`, `DistributedProgress.lean` |
| Cancellation and carriers | `TransportContract.lean`, `CarrierRefinement.lean`, `CarrierProfile.lean`, `AsyncCancellationTermination.lean` |
| Repeated regions | `DistributedRollRefinement.lean`, `ElasticIterationQueue.lean`, `ElasticRouteHistory.lean`, `ElasticAdmissionHistory.lean`, `ElasticErasure.lean` |
| Sessions and deployment | `SessionComposition.lean`, `SessionLifecycle.lean`, `ProtocolArtifact.lean`, `Deployment.lean`, `EndToEndRefinement.lean`, `ProductionEndToEnd.lean` |
| Aggregate | `MainTheorems.lean`, `ClaimSurface.lean` |

The checked claim snapshots are:

- `all-claim-surface.txt` for static theorem types;
- `example-claim-surface.txt` for finite executable regressions;
- `generated-claim-surface.txt` for generated descriptor and protocol
  certificates;
- `runtime-generated-claim-surface.txt` for runtime layout and lifecycle
  certificates;
- `public-operation-generated-claim-surface.txt` for the public-operation
  transition table;
- `causal-generated-claim-surface.txt` for finite Rust/Lean causal decisions;
- `parallel-generated-claim-surface.txt` for ten exact parallel descriptors and
  their corruption rejections, all checked with kernel evaluation;
- `program-columns-generated-claim-surface.txt` for exact ordinary and owned
  program headers, actor bindings, conditional reference DAGs and initial
  issuer rows, including malformed-header and suffix rejections. It also binds
  holders to the independently interpreted typed global source and rejects
  incorrect actor bindings, issuers and swapped conditional holders.

The ordinary exact descriptor certificate now binds its claimed counts to the
actual eight-byte column header. The owned grammar decoder covers the header
and immutable actor suffix. `EndpointOwnership.lean` is the shared independent
source model; `OwnershipDescriptor.lean` connects its per-path holders and
initial issuers to the decoded bytes. Unknown route choices authorize no
holder. The descriptor-derived traversal bound is proved sufficient for every
valid backward reference under total choices. This correspondence does not establish
actor-local progress, ownership transfer commits, or continuation refinement.
The action decoder now derives the logical Endpoint role from each resident
row's direction and its global atom, keeping the physical actor separate. The
actor-row component checks all active operations against each independent source
path, including both wire facets when two logical roles share one actor.
It rejects missing operations and malformed directions. A general theorem binds
accepted rows to source operations; ordinary rows still belong to their single
image role. `ActorProgress.lean` compares the exact lane-prefix and dependency
guards with source order under a fixed complete path within one visit. On a
source-closed shared `CommitState`, both allow exactly the same next operation.
Initial progress and the commit done-bit delta preserve that property. Concrete
mutations with an extra parallel wait or a missing join are rejected even when
their operation rows and lane format still pass. This does not yet establish
the property for a prepared base before the explicit roll reset below. Route authority,
ownership transfer commits and continuation refinement remain open.
Typed owned projection also checks source order against every emitted local
image during const evaluation. It splits only the unknown choices needed for
an individual edge obligation, validates active actors against the source's
conditional references, and rejects either a missing source predecessor or a
guard that orders independent work. Immutable row decoding uses compiler
scratch and adds no runtime progress state. The pair-check equivalence and
source-prefix theorem in `ActorProgress.lean` state why direct predecessor
coverage preserves exactly the same ready operations. `SourceOrderGenerated`
checks the valid Rust image and two byte-level counterexamples with the
independent Lean decoder. This connects the dependency check at the typed
boundary; it does not by itself establish distributed execution refinement.
Blocked-operation classification also reads source order from the existing
immutable par/route/roll markers. The typed const check compares that query
with `Source.before` for every local operation pair. It must follow transitive
source ancestors rather than just the last compact join: a future holding an
ancestor's carrier or payload cannot wait for that same view to do the work.
The pending-ancestor theorem shows that source-closed progress cannot hide such
an unfinished prerequisite behind completed compact guards. Runtime waiting for
another independent view adds no ownership or progress authority.
The held-view lemmas in `MeasuredResourceAvailability.lean` recover an
unfinished source predecessor that requires an owned send's payload. They do
not infer availability from an idle header: the unpolled future already owns
the Rust value. `OwnedFuturePredecessorGenerated` applies them to a real
pre-poll actor snapshot. Its Tokio test checks that the first poll records a
fault before the future is dropped, with no frame or progress committed.
The source lemmas and this concrete execution are separate evidence; neither
establishes all future schedules or the full distributed progress result.
`ActorRouting.lean` binds event conflict paths and route-parent/reentry columns
to the source, including nested choices. The same committed history and
candidate preview therefore produce the same conflict filter on both paths.
A committed choice takes precedence over preview. This filter permits unknown
choices and is not a substitute for controller, ingress or reentry authority.
Forged single-pass routes marked as reenterable and self-parent cycles are
rejected even when their actor order and earlier route-column checks pass.
The ordinary certificate still rejects owned actor columns, and public owned
projection remains closed until those semantic connections are verified.

`ActorCommit.lean` checks observations copied from the existing Rust actor
cursor and view headers. The Tokio fixture lends and returns one Endpoint
between two runtimes and runs independent operations with `join!`. It observes
each successful send before the completed future is dropped. For each of four
commits, the generated kernel proof checks the exact source row, its causal
predecessors, one done-bit update, and the matching view retirement or issuance.
It rejects unchanged progress, unchanged view headers, and a different local
row. Each accepted operation also requires an issued carrier header with the
current session run and an odd live generation. Four copied observations lose
their carrier headers while retaining valid metadata and progress; the commit
checker rejects all four. The observations never control execution and add no production state.
This is translation validation of successful commits in one selected visit.
It does not establish ownership across in-flight transport frames, prepared
bases after reentry, resource binding, closure, or general distributed progress.

`ActorReentryCommit.lean` derives a fresh base from the observed old state and
the exact source-validated body range. It checks completion of the old body,
the recorded choices against the old and new source paths, clearing of choices
inside the body, publication of the current operation's source route, and the
ordinary commit check on that fresh base. Its general theorem binds the reset
to a canonical source range, preserves every outside done bit, and reuses the
source-order and local view-publication theorem for the successful operation.
Before reset, the checker also resolves each source-derived entry-resource
reference against the existing view headers. The entry owner must have an
issued header; a different actor cannot retain an issued header for it. Five
general lemmas connect accepted commits and reentry to these header facts.
The local-return fixture retires the required DB header in both copied states,
preserving their validity and the progress delta, and the reentry checker rejects it.
Each view observation now also copies its existing public-operation phase under
the same actor lease. Reentry checks `checkSendPayloadBorrow` for every required
entry view. The prospective payload role comes from the immutable source atom.
A view in phase `sendPayload` can authorize that same payload's transfer, while
other resource-dependent reentry waits. Three general lemmas connect accepted
reentry observations to that borrow rule.

`PendingBorrowGenerated.lean` records a real pending owned send with an issued
DB header and phase `sendPayload`. Its resource-dependent restart is denied by
the borrow check, but its own send is allowed. Independent communication on the
same actor completes both an initial visit and a repeated visit. Eleven kernel
claims connect those observations to the source/commit checks. The runtime's
actual suspension and empty peer queue are also checked by the Rust test.

An operation lease may change during `offer` or drop while protocol progress
and header issuance stay unchanged. The two local-return preservation claims
therefore list every pre-existing observation field explicitly, including all
headers, metadata, steps and choices. They do not claim the new lease field is
unchanged. Header retirement/issuance checks likewise compare the same original
header fields separately from the transient operation phase.

Mapping all future-owned values, including unpolled futures, and in-flight
frames to source readiness remains part of the implementation correspondence.
These checks still do not establish general frame availability, transport
refinement, terminal behavior or distributed progress for every live execution.
No second runtime cursor or progress table is added.

One fixture emits six commits from both arms of an enclosing choice across
three runtimes. Another emits two commits after concurrent DB/AUTH operations,
with AUTH and a sequential continuation already complete. The generated checks
reject missing resets, erased outside progress or choices, a different body,
and an incorrect live-view update. The Rust snapshots read the existing cursor
and committed route history; they never schedule or authorize operations.
These checks cover successful body reentry observations. Passive route
materialization, interleaved distributed ownership, physical resource binding
and transport closure still need their complete correspondence.

The conditional-controller fixture also records five visits of an inner
choice, including both same-arm repetitions and both changes of arm. Its
controller runs either remotely or beside the observer in the same actor.
The latter observer is polled first with stale resolver input. Observations
at the returned branch distinguish its own receive from the controller's
intervening send. Twenty successful operations feed the ordinary or body
reentry checker, retaining the enclosing choice. Generated negative cases
remove the reset, erase choices, use an incomplete old visit, or forge the
live-view count. These are observations of execution, not another scheduler.

A completed visit must advertise its canonical offer entry even when the old
cursor points to an arm head of that same scope. Retaining that old head can
mistake a fresh controller decision for a receive continuation and wait for a
frame the protocol never sends. Repositioning the offer cache changes no done
bits, committed choices, view generations, or public operations. A separate
regression uses public projection without any owned payload.

Repetition authority belongs to an explicit `roll` body. A route's inherited
reentry metadata does not authorize repeating that child while its enclosing
body is unfinished. The runtime and `Commit.lean` both reject that implicit
loop. Old completion uses the committed choice; admission of a new head uses
the candidate choice. Rechecking old completion under the new choice would
incorrectly reject a legal change of arm.

`RollBoundary.lean` checks both projected roles of the same public Rust
regressions: an unrolled child cannot repeat before its parent's suffix, a
completed parent can restart at its prefix, and an explicitly rolled child
can change arms while preserving the parent prefix. The complete nested trace
runs twice. Before the parent suffix, both the completed arm and the unused
alternative are rejected. `PublicOperationKernel.lean` checks the owned-send payload borrow:
other resource-dependent work waits, that payload's own transfer may finish,
and an unborrowed view gains no extra dependency. The generated Rust table
checks all 180 phase/edge combinations. `PendingSend.smt2` checks the abstract
admission boundary; it does not establish transport refinement by itself.
The allowed axioms and inventory checks are unchanged. `RollBoundary.smt2`
checks all completion masks of this six-event shape, including the difference
between old committed and new preview choices and preservation outside the
explicit child. It is a shape-specific check, not a distributed progress proof.

`EndpointTransfer.lean` proves affine conservation for every finite transfer
execution from one initial binding. It distinguishes application values from
private send payloads and accepted frames. Acceptance can precede sender
publication, and receive can publish the new value while the old private
payload remains. Dropping that old payload cannot restore an application value.
The send and committed-receive theorems use the existing ordered carrier profile.
Receipt commit is a proof boundary, not another public Rust operation.

`FutureCustodyGenerated.lean` connects one actual two-runtime round trip to
that model, from construction of an unpolled public `send` future through its
final drop. The Tokio schedule copies real actor headers, operation phases,
progress, and accepted frame bytes. Constructing the future moves the Rust
value without changing runtime progress; its first pending poll borrows the
payload. Both completed send futures remain alive when the resource returns
to the original actor with a fresh tag. Dropping either old witness leaves
the issued view and progress unchanged, and that returned Endpoint performs
the next request and reply. Independent final transfers are driven with
`join!`; the observations do not schedule them.

Its kernel claims check all eleven snapshots, four source-ordered owned
commits, exact issued and retired view tags, and the nine-step transfer
execution. They also apply the general value/frame suffix theorems to the
received actor tags. The conservation theorems distinguish the two private sender witnesses from the single
application authority. Treating the unpolled payload as another application
value is inconsistent with that execution. Value placement is observed at
the Rust move/drop boundaries; this concrete correspondence is not an
extraction of every possible Rust execution or a proof of source readiness.

`SourceResourceAvailability.lean` preserves the exact received value across
arbitrary finite suffixes of the existing affine transfer model, including
unpolled/pending private futures and completed sender witnesses. It either
locates that value in application code/private future, or exhibits a concrete
later handoff or retirement while that value was still present. An accepted
frame likewise remains the exact receipt until consumption or carrier fencing.
The source-checked send/receive theorems apply these results to the projected
physical actor and the ordered carrier. The send theorem explicitly binds the
observation's inserted frame to the carrier's accepted frame. The actual
two-runtime fixture applies both general suffix results to its loan and return
and checks both receiver actor identities. These results do not yet establish
all source-ready admission, fresh visits, observable terminal faults or general
distributed progress. No predicate here is a runtime ownership/progress store.

The general transfer theorems assume execution follows the stated steps.
They do not establish that every Rust execution has that abstraction, that a
deployment supplies a valid physical binding, or that its carrier refines the
profile. Source/continuation correspondence and distributed progress remain
separate obligations; public owned projection is still closed.

`ResourceHeaderTransfer.lean` derives the exact raw header changes from checked
owned commits and retirement. It also shows that the actual prepared reentry
base preserves these tags. Tags include the actor and existing role/run/nonce
header. They exclude temporary operation phases: an unpolled send future
already owns its payload while that phase can still be idle.

`ResourceValueCorrespondence.lean` connects complete header/transfer histories
to actual custody. An issued raw header can name an application value, a
private preparing value, or an accepted sender witness. That last case has
zero operation authority. Transport acceptance and sender local publication
are separate boundaries. The receiver can use, return and retire its future
before the sender publishes its commit; the old sender header remains live
while the returned frame carries the sole authority. A general counterexample
theorem rejects the stronger claim that every live header names local authority.

Source-ready admission supplies the exact observed tag and this custody
alternative. At a cut with no accepted witness for that tag, the complete
history derives its actual value. Witness absence is obtained from the
execution, not added to runtime admission or assumed for all schedules. The
two-runtime admission artifact checks both transfers, their local publication
and the resulting carrier/payload values. The existing native preemption test
checks the legal receiver-first schedule with a separate process. A second
native test uses OS queues between two local runtimes: a completed resource
retires, independent communication succeeds, and a peer's next rolled receive
ends at the carrier deadline with the first fault preserved. That fixture
qualifies a deadline profile, not instantaneous peer closure.

These proofs still require complete implementation observations, affine
carrier delivery and the physical binding contract. They do not turn local
header existence into general source progress, or complete the distributed
delegation qualification. Public owned projection remains closed.

`SendCompletion.lean` checks copied observations from the actual non-generic
`kernel_send` loop at its prepared-I/O boundary. A successful checked plan is
published exactly once and returns success in that call; a failure finishes
without publication; pending retains the plan without publishing. The complete
three-case table and corrupted-result/publication cases are kernel-checked.
The source-checked owned publication supplies the actual retired resource tag.

The same production loop executes under ordinary tests, Miri and a symbolic
Kani harness. Its boundary double supplies an opaque test plan and records
calls; it does not establish source admission, the real publication body or
physical transport correctness. The native receiver-first test separately
checks that, once OS acceptance has allowed the child to complete its return,
the sender completes publication in that same poll. It may still wait before
acceptance. Arbitrary callback termination and executor fairness are execution
assumptions; a completed observation is not a liveness proof for a stuck callback.

`PublicAdmissionCustody.lean` separates a live public admission from an internal
transport callback. Every retained sender witness has an acceptance origin in
the same complete transfer history. At a later cut, its originating public call
is either still protected by its mutation lease, has published its checked
source commit, or has faulted the run. Publication retires the exact old tag;
the subsequent nonce history prevents that tag from reappearing when the
resource returns, even if the old completed future remains alive. Other views
may advance the actor's nonce counter independently.

Under that public-call correspondence, a live actor with an available lease
cannot use a retained accepted witness to justify an issued header. Composing
this exclusion with the complete header/transfer history gives actual custody
in application code or a preparing future. The call correspondence must cover
each acceptance origin; the proof does not assume source-ready value
availability, add an admission predicate, or require sender publication before
the remote receiver proceeds. Establishing readiness of the intended operation
from that custody remains part of the distributed progress obligation.

Five additional claims in `ActorCommitGenerated.lean` apply the publication
connection to both actual sender commits and the owner's returned continuation.
The native receiver-first fixture checks the real lease inside the accepted
send callback and its release after publication. An uncertain-delivery fixture
has the OS accept a frame and the transport then report an error: the sender
releases only a faulted actor, while the other runtime can receive, use and
return the sole resource. The carrier reentry test checks the same observation
boundary under Miri; Kani checks the existing lease's exclusion and release.
These tests qualify their concrete schedules. General originating-call and
complete-history correspondence are still explicit implementation contracts,
not a claim of Rust source extraction or completed global progress.

`ActorVisitProgress.lean` derives the finite point domain from the selected
source bindings, with the path itself checked against the normalized global.
It contains acceptance and both local halves of each wire
message, or one local step for a self message. Domain closure follows from
those bindings, so a caller cannot omit a predecessor to manufacture progress.
The existing ready-point theorem then applies without another rank argument.

A checked actor family covers every source action with exactly one actual
descriptor row and its copied cursor-admission observation. Missing or duplicate
rows reject the cut. Completed wire operations require their recorded accepted
send, and unknown or repeated acceptance IDs reject the cut. These records are
qualification data and never authorize a localside operation.

The general selected-visit theorem derives an admitted actual row, or an
accepted send whose local publication remains unfinished. It connects source
dependency edges to each actor's existing done bits and admission query. A
publication candidate is the existing call, not permission to send again.
The result does not establish available carrier bytes, a successful arbitrary
application payload, or refinement of independently advancing fresh visits.
Per-cut translation validation remains distinct from universal Rust refinement.
The observations must come from the same run and complete, authentic acceptance
history. The cursor observation is distinct from the public kernel's first-fault
check, so it is not a claim that a faulted run can resume.

`ActorVisitProgressGenerated.lean` checks seven real cuts from four actors in
two runtimes. Both independent resource/worker sends are admitted before either
runs, and the direct Tokio localside drives them in parallel. Six unfinished
cuts use the general progress theorem, and the final cut has no unfinished
point. Omitted/duplicated actors and missing/unknown/repeated acceptance records
are rejected, as is a fabricated empty source domain. Three explicitly model-only
claims admit acceptance and receipt before sender publication using copied
progress. They are not presented as
additional observations from a real callback cut.

`SourceReadyReceipt.lean` connects readiness to the current actual receipt for
the source-derived resource. It starts from the accepted source and complete
commit history, without assuming an admission result, an issued-header bit or
an available value. It derives the initial source issuer or the most recent
wire-checked resource receipt, its exact resource and receive side, published
tag, and a checked suffix in which that receipt remains complete. Identity
comes from the immutable source occurrence even if recorded route selections
differ. The result retains the source prefix: either no prior handoff of this
resource exists, or the actual last handoff is followed by no further handoff
before the target. It does not discard that witness after recovering the
receipt. Its baseline concerns receipts for this resource only; it does not
reset independent parallel work or outside continuations. The actual admission
fixture applies the general theorem to both the read and the later return.
This establishes the receipt boundary, not possession at the final cut; that
still requires its subsequent transfer/fault history and visit correspondence.

`SourceHandoffInterval.lean` connects that static interval to measured progress.
For an accepted source, every competing handoff of the same resource is before
the last receipt, that receipt itself, the intended target, or causally after
the target. The occurrence coordinates orient a causal relation already
established for the same resource; they do not impose an order on unrelated
parallel work. A zero-write measured interval preserves completion before the
receipt and pending progress before the final target. Consequently a distinct
handoff cannot become ready at an intermediate cut. The target need not have
been ready at earlier cuts. Its own handoff is not excluded by this result.

The last-receipt result requires prefix closure at that receipt for the current
selected source path. The new two-runtime Tokio fixture derives this from its
wire-checked receipt. It performs a loan, return and second loan, holds one
parallel send pending in the carrier while another actually commits, then
executes a post-join resource operation and returns the resource. Its generated
proof applies the interval theorem to both the already completed old return
and the future return. The post-join target is not ready when its resource
arrives, and becomes ready only after the parallel work. Another application
covers actual initial issuance and a later loan.

This proves source-admission exclusion over the qualified interval, not yet
current value availability or general distributed progress. A receipt checked
under another full route path must still be connected to the current causal
prefix without requiring unrelated future choices to be known. Immutable
message identity alone does not supply that prefix closure. Actual transfer,
future custody, retirement/fault and independently fresh visits still need
their corresponding execution boundaries. The SMT countermodels retain the
need for receipt prefix closure and an unreset measured suffix, rather than
hiding those premises behind a value-availability assumption.

`MeasuredResourceAvailability.lean` connects this interval to a finite transfer
execution. A source-checked ordered receipt derives the actual first value.
Subsequent matching source observations and measured histories then establish
that the exact value remains in application code or a private future, or that
the required global message is accepted, or that retirement reports a checked
fault. An ordinary resource use cannot accept an unrelated loan. For a sending
target, acceptance identifies its exact source handoff. Intermediate cuts need
not have the target ready, and acceptance is separate from sender publication.

The same two-runtime fixture supplies both read and return interval origins
from its actual second receipt. Its generated receipt application universally
quantifies the carrier and matching transfer history; it does not fabricate
that history from a header or assume an available value at the final cut.
Correspondence to the same physical binding and run remains an implementation
contract. Custody in a private future is not yet callable availability:
eventual polling, causal prefix correspondence across independent route/roll
visits, and their composition into distributed progress remain open.

`ActorMutationCoverage.lean` strengthens selected-visit history observations
with measurements at the actual completion-bit writes. Qualification builds
increment one checked counter in the existing mark/clear functions, including
when the written bit already has that value. Production builds contain neither
the field nor the observation code. The counter never authorizes an operation,
chooses a branch, or changes a cursor's semantic progress. Exhaustion stops
qualification before a write rather than wrapping into apparently fresh data.

Each recorded source commit includes the measured counters at its real before
and after cuts. The checker requires a single mark for that commit and no
unrecorded completion write in a gap. It recovers the existing checked source
history and accounts for every measured write. A zero-write gap must preserve
the observed progress exactly; unchanged counters alone do not validate a
snapshot that silently clears a bit. Deliberately inconsistent observations
are rejected both at the final gap and before another checked commit.

The checked commit and measured-history theorems derive that completed events
remain complete throughout such an interval. Conversely, an event pending at
the final cut was pending at every measured commit boundary. These are facts
about actual source progress within the qualified interval, not assumptions
that a future target was ready throughout earlier independent parallel work.
They do not establish that its Endpoint value is still available: the exact
resource receipt and subsequent transfer/fault boundaries must still connect
to that progress history.

The real Tokio reset/recommit
fixture has equal ordinary endpoint snapshots after successive visits, but
counts1 and3. The old gap check accepts that omitted pair; the measured check
rejects it. The actual loan/par/return fixture also supplies the measurements
for the receipt and return prefixes used by the source-ready receipt theorem.

The new Kani harness executes the actual cursor initializer and mark/clear
functions on a concrete two-event image with symbolic target and mutation kind
for each of two writes. It checks both the real bitset and measurements, without
a publication double. The same bit state with different write history is
reachable. This is bounded verification of those real functions, alongside the
existing full-domain bit-location checks; it is not every Rust execution.

This checker deliberately qualifies histories without an unrecorded reset. It
does not redefine fresh reentry as an error or impose that restriction on the
runtime. Reentry uses its separate prepared-base/reset evidence. Authenticity
and same-run correspondence of the observations remain required. The counter
measures completion writes, not every field mutation, carrier delivery,
ownership transfer or arbitrary callback. These measurements must not be used
as a runtime owner table or as evidence of completed distributed progress.

`actorReentryMutationCheck` joins that prepared-base evidence to the actual
before/after mutation counts. The same source-selected ordinary or owned
commit validates its payload; the measured poll accounts for one clear per
projected body row and one successful mark. The prepared event and post-poll
snapshot are the actual operation's, and every progress bit outside that
source-validated body is preserved, whether complete or pending. It does not
reset other actors or independent parallel bodies. The existing Tokio fixture
checks owner counts5→8 and receiver counts9→14, including exact owned wire
validation. Omitting the clears is rejected.

`ActorMeasuredExecution.lean` composes those fresh boundaries, ordinary
commits and zero-write gaps into finite measured histories. Every write is
accounted for by a source-derived body clear or checked mark. A required row's
completion survives if no actual reset contains it; unrelated bodies may
restart any number of times. The real owner and receiver boundaries instantiate
this history and its extension theorem for arbitrary checked independent
suffixes. These are erased proof observations. They neither schedule work nor
add runtime progress state. Applying this history to the source resource
intervals and transfer/fault correspondence remains part of the distributed
progress composition.

The prepared-send Kani harness calls the production non-generic send loop with
a boundary double. Its publication count proves that successful loop completion
invokes publication exactly once. The double does not execute the real commit
body or record every reset. It cannot discharge complete observation of all
runtime histories merely by incrementing a harness-side counter. Those claims
need actual mutation-boundary correspondence, rather than a successful table
audit or matching endpoint snapshots alone.

`OwnedRetirement.lean` checks retirement against the global source for the
current selected visit. It considers every path compatible with the committed
choices. A view is needed for its own send/receive or as the owned payload of
a later send; a missing source binding or row fails closed. Eighteen general
theorems connect this check to the existing first-fault/generation model,
prove that a nonfaulting retirement removes no pending required source action,
and show that transfer authority decreases only at explicit retirement steps.
This is not a proof about all future rolled visits or distributed termination.
For an arbitrary finite transfer execution, the authority is in one application
value, one private send future, or one accepted frame, unless its history has
an explicit retirement boundary. The proof supplies a prefix and suffix for
that boundary. Completed send witnesses cannot stand in for a missing value.
Source correspondence and observable-fault obligations are still necessary to
apply this model result to arbitrary Rust executions.

The real Tokio tests cover an owned future dropped before polling, while
awaiting another view, and while transport readiness is paused. All three
preserve progress, retire the payload, reject reattachment, and retain the
first EndpointDropped fault after another failed operation. A separate test
drops a completed resource view, then runs independent co-resident work with
one send deliberately delayed. Its delay is a test stimulus, not measured
benchmark overlap. The two generated artifacts have 54 and 18 kernel claims,
including source completion, generation transitions, explicit authority loss,
and rejection of missing faults, spurious faults, source rows and header deltas.
The observations never authorize execution and add no production runtime state.

`OwnedCommit.lean` connects the actor commit checker to that transfer model.
It decodes the same source path and image as the observed actor, then checks
the logical sender/receiver, physical actor, lane, wire label, session and exact
three-byte resource/effect envelope. Its receive theorem issues the observed
receiver view while preserving the ordered carrier's unique authority. It does
not require the sender to have published its local completion first.

The real two-runtime exchange copies each accepted frame before its receive,
without consuming it or changing readiness. Its four observed commits generate
64 checked claims, including rejection of a missing carrier, resource, effect index,
receiver, actor and session. Carrier generation and sequence are not invented
from the core header; the general theorem obtains them from the carrier profile.
These observations validate the exercised commits. Completeness of the
implementation abstraction, fresh visits and distributed progress remain
separate obligations.

`SourceResourceContinuation.lean` derives a resource continuation's physical
actor from the last handoff in an arbitrary accepted selected source path.
Unrelated communication between them cannot change that owner expression.
The earlier handoff is causally before the use or next loan, and its receive
is an edge in the same SourcePoint graph used by the progress model. Actor
equality is derived from solved source equations rather than assumed.
Successful normalization and the decoded actor image supply that source
derivation; `OwnedCommit.lean` connects it to a checked owned wire observation.
The four real handoff commits apply this derivation, and two further kernel
claims check the worker's resource use and return depend on its loan receipt.
These are single-visit source/receipt dependencies. Availability across all
future phases, rolled visits and terminal outcomes remains a separate obligation.
The shared three-byte owned-payload codec preserves the resource role and full
u16 source identifier. Its Lean envelope is injective, and a checked send and
receive with the same actual payload name the same resource and source event.
The real loan and return observations each apply that result. Carrier ordering,
run isolation and preservation of the accepted bytes remain explicit transport
contracts; this does not identify rolled frame instances from the payload alone.

The ordinary actor commit checker also binds both copied choice histories to
the selected source path and checks their exact canonical publication. The
same definitions are used for the prepared base of a rolled commit. Generated
regressions reverse or erase a known outer choice before and after a commit;
they must be rejected without changing the observed progress or view records.

`SourceResourceOrder.lean` proves five source properties for arbitrary accepted
elaborations. Compiled occurrences retain their exact source message and index;
transferred resources and endpoint uses belong to that source. In independent
parallel branches, an actual transfer cannot alias a use in the other branch.
This includes use as a peer or as another loan's carrier. The occurrence list
is static enumeration, not a schedule for parallel execution. Availability of
the corresponding live view/receipt and distributed progress remain to be
connected to these properties.

`SourceResourceCausality.lean` establishes that an emitted resource transfer and
a use of that same endpoint are either the same occurrence or ordered by the
source's `before` relation. This covers arbitrary accepted compositions and
selected route arms within one visit. Numeric positions only orient that
already-established causal relation; they add no dependency between independent
resources. `SymbolicResourceFlow.lean` proves that the normalizer's owner
expression is exactly the fold of the declared transfers for that resource.
Untouched resources retain their owner expression. This is compile-time
symbolic flow, not a runtime ownership ledger or a schedule for parallel work.
Actual view/receipt availability and the complete distributed progress argument
remain separate obligations.

`SourceResourcePrefix.lean` checks every emitted binding against its exact
source prefix. Sender and receiver references read the corresponding owner
expressions. A loan emits the equation that equates its resource's previous
owner to its sender. If issuance solves those equations, the instantiated
sender is that resource's owner before the transfer and the receiver is its
owner afterward. Parallel composition uses the established separation law to
preserve these references without adding execution order. This is a general
normalizer result; it does not by itself establish that every Rust execution
supplies the corresponding live view/receipt or has distributed progress.

`ReentryKnowledge.lean` proves that the causal origins of a resource-dependent
fresh visit trace back to its entry owner when the needed-action set is closed
under wire predecessors and each origin follows an incoming message or belongs
to that owner. Strict `Source.before` supplies well-foundedness; a parallel
sibling supplies no permission edge. The Rust projection check constructs this
closure for resources that the continuation does not restore to their entry
owner. An initial prefix outside the roll cannot authorize another visit.
These three general lemmas specify that construction obligation; they do not
complete the owned runtime's pending-I/O or distributed progress refinement.

`OwnedRecovery.lean` independently derives incoming owned-receive rows from the
serialized directions and global payload kinds. It checks the complete receipt
catalog and the exact receipt slice of every declared roll-entry resource. Its
two general lemmas establish catalog equality and completeness of those slices.
The kernel checks one real generated image and four malformed versions: an
out-of-bounds row, a wire-only receive, a missing entry slice, and a missing
catalog. These receipt-slice claims alone do not establish that the set of
entry resources is complete or prove all distributed waiting paths.

`OwnedEntry.lean` rebuilds local roll ranges from global source markers and
checked local directions, then rebuilds each range's resource and entry-owner
references from global atoms and actor bindings. The checker requires exact
agreement with the serialized local tables, including when those tables claim
to be empty. Two general lemmas bind accepted tables to these reconstructed
obligations and preserve every required entry. Five actual-byte claims cover
a valid image, a missing resource table, a changed owner, an unrelated resource,
and a missing roll table. The Rust constructor enforces the same comparisons
without adding runtime state. The full source-normalization ownership argument
and refinement of arbitrary distributed executions remain separate obligations.

Runtime admission uses this immutable index with existing affine view headers,
completion, and route choices. A temporarily absent resource may return through
a different view while a rolled send, receive, or offered local or wire receive waits.
No owner ledger or application progress flag is added. Real Tokio tests retain
independent parallel work and reject waits that borrow their own prerequisite.

`LocalReturnGenerated.lean` records an actual local branch's pending, drop,
receipt, and resumed commit. Its eight kernel-checked claims preserve affine
progress while waiting and bind the resumed commit to the source's next visit.
The fixture switches from the left arm to the right arm, changes the resolver's
input while pending, and keeps an independent rolled view progressing. Missing
reset and changed live-view count are rejected. This correspondence covers the
observed execution, not every local operation or distributed waiting path.

The compiler scope memo, fork-input restoration, independent-sibling exclusion,
and retained sequential joins have general model lemmas. Ingress-head
preservation is conditional on progress outside the observed lane. Their
connection to Rust includes finite descriptor and decision correspondence;
these artifacts do not establish refinement of every Rust execution.

Sequential inputs to a fork are included in the canonical dependency columns,
including the right arm's newly introduced lane. The additional fixtures cover
`seq(send, par(send, send))` and nested sequential inputs in both role images.
The interval lemma establishes coverage of both arms; the generated exact-byte
checks compare the Rust dependency rows with this canonical model. They do not
constitute a universal proof of the Rust dependency compiler.

The gate discovers declarations from Lean source, compares their elaborated
types with these snapshots, and audits their axiom closures. Generated Rust
artifacts are accepted only when their complete theorem inventory and exact
claim types match.

Static proofs use the pinned Lean Core and Std environment. The gate rejects
custom axioms, `sorry`, and `admit`. Finite executable regressions and concrete
finite-closure artifacts that use the pinned native evaluator remain isolated
and explicitly audited; general soundness theorems do not depend on those
evaluator decisions.

## Run

From the repository root:

```sh
bash .github/scripts/check_lean_proofs.sh
```

The gate pins the Lean toolchain, builds the static package, exports the
production descriptor and kernel artifacts from Rust, checks every claim
surface, and audits the permitted proof dependencies. Each artifact audit prints
its checked inventory. The final status is emitted only after all checks pass.
