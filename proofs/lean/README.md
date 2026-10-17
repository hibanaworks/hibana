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
alternative are rejected. These six kernel proofs replace seven lemmas about the removed
route-only reset and its unused selector. Including the transfer model below,
the static inventory is 848, with 465 using `propext` and `Quot.sound`,
277 using only `propext`, and 106 axiom-free. The three additional
`PublicOperationKernel.lean` claims cover the owned-send payload borrow:
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

Its 57 kernel claims check all eleven snapshots, four source-ordered owned
commits, exact issued and retired view tags, and the nine-step transfer
execution. The existing conservation
theorems then distinguish the two private sender witnesses from the single
application authority. Treating the unpolled payload as another application
value is inconsistent with that execution. Value placement is observed at
the Rust move/drop boundaries; this concrete correspondence is not an
extraction of every possible Rust execution or a proof of source readiness.

The twelve general transfer theorems assume execution follows the stated steps.
They do not establish that every Rust execution has that abstraction, that a
deployment supplies a valid physical binding, or that its carrier refines the
profile. Source/continuation correspondence and distributed progress remain
separate obligations; public owned projection is still closed.

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
56 checked claims, including rejection of a missing carrier, resource, effect index,
receiver, actor and session. Carrier generation and sequence are not invented
from the core header; the general theorem obtains them from the carrier profile.
These observations validate the exercised commits. Completeness of the
implementation abstraction, fresh visits and distributed progress remain
separate obligations.

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
surface, and audits the permitted proof dependencies. Its final status line is
the authoritative current inventory.
