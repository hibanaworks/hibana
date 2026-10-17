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
row. The observations never control execution and add no production state.
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
the static inventory is 789, with 420 using `propext` and `Quot.sound`,
265 using only `propext`, and 104 axiom-free.
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

These twelve theorems assume the execution follows the stated transfer steps.
They do not establish that every Rust execution has that abstraction, that a
deployment supplies a valid physical binding, or that its carrier refines the
profile. Source/continuation correspondence and distributed progress remain
separate obligations; public owned projection is still closed.

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
