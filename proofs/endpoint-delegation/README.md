# Ownership-aware endpoint projection

This proof package runs the ownership validator used by Hibana's typed source
boundary. The source tree and validator live in `src/g/source/`; the proof
harness includes that implementation and adds read-only observations. There
is no separate Rust source language or second implementation of its ownership
rules. The Lean checker remains independent.
Its source ownership model lives in
[`../lean/Hibana/EndpointOwnership.lean`](../lean/Hibana/EndpointOwnership.lean)
and is shared with the exact descriptor checker. `Claims.lean` audits its
fourteen general laws; there is no second copy of those definitions.

Typed `g::Msg<L, Endpoint<'static, R>>` records an owned endpoint, not a wire
codec. Source projection checks its initial issuer, conflicting transfers,
parallel uses, and rolled ownership before lowering. Core source tests and
compiler rejection cases exercise the real `g` constructors.

Owned descriptor lowering and value-consuming `Endpoint::send` / `recv` now
execute in private actor-image tests. They cover return to the original actor,
distinct runtime slabs, branch operations and independent rolled resource
exchanges. Public owned projection remains closed while general continuation,
authenticated transport integration and the complete verification boundary are
unfinished. Initial attachment now reserves the physical resource's durable
`EndpointBinding`, independently of runtime and session identity. The integration
establishes physical identity at its explicit unsafe construction boundary.
The public 0.9.9 release does not provide Endpoint delegation.

An accepted ownership plan is not an accepted Hibana protocol. This package
also checks branch knowledge and the causal-path laws below, but does not
certify the complete projection boundary, route authority, codecs, lane mapping,
remote delivery or operation commits. Those boundaries and their implementation
correspondence remain necessary for distributed progress. The harness can describe related components; production `g` source
currently describes one composed choreography, not arbitrary unrelated sessions.

## What the compiler computes

`EndpointKey` identifies a logical endpoint by component and role. Component
identities distinguish related sessions in a statically composed family.
Nondelegated role identifiers name the same actor across the family.
Transferring an endpoint changes its holder, without changing its logical
identity, wire peer, message label, or payload schema.

The source is the sole input for initial-owner inference. A transfer requires
the current holder of the payload endpoint to own the sending endpoint.
Unification propagates those requirements to the initial issuers. Conflicting
or unrooted initial owners are rejected; an issuer is never guessed.

Projection assigns resource operations to their current holder's local order,
along with that holder's control endpoint operations. This addresses a real
gap in merely moving a unique value: the holder must not wait for a control
response before doing the resource work needed to produce that response.
The logical identity used on the wire remains unchanged.

Independent resources and operations remain unordered under `par`. A transfer
cannot race another branch's operation or transfer on the same endpoint.
After a route, the holder may depend on the selected arm, including in the
common continuation. The same decision determines ownership; there is no
second choice or guessed holder. Correlated resources may be passed through
their current carrier, and independent choices keep independent resources
parallel. Rolled fragments must restore their incoming ownership for each
legal path. An earlier choice may therefore remain visible at a loop header
without requiring the two alternative holders to be equal.

The compiler library is `no_std`, uses stable Rust1.95, and has no dependencies.
Scratch and output are caller-owned arrays sized from the input. The source
tree is borrowed. During compilation Rust queries ownership from source
prefixes, capturing the receiver's holder at the transfer event. Moving that
receiving endpoint later does not move other values along with it.
`ActorImage` records the resulting sender/receiver bindings as references into
one immutable decision DAG. Equal alternatives collapse, and equal decisions
share a node, including when several resources follow one choice. Direct actor
references retain the complete u8 domain; node references occupy a disjoint
domain. Nodes refer only to earlier nodes. Output capacity failure publishes
neither events nor decisions.

The read-only proof observations use this compiled image, rather than repeating
source-prefix queries for each binding. Missing choices return a holder only
when all remaining alternatives agree. This inspection is not runtime admission
or a route authority. No mutable current-holder map, undo trail, or map copy at a
fork is needed. The global image now retains these facts and maps source
choices to the existing compiled scope identities. Private runtime fixtures
consume actor-local descriptors, while complete public qualification remains
unfinished.
No runtime permission table, application flag, or second progress cursor is
produced. The plan is published only after validation;
failure cannot expose a previous accepted plan or a partial new one.

At runtime, an event's immutable actor reference is interpreted through the
same route history used by progress. A candidate row is excluded only after
that history selects another actor. An unresolved binding cannot authorize an
operation or erase its prerequisite. Per-view first operations and controller
authority remain distinct when several views reside at one actor. A receive
may await a different view's predecessor without polling its transport or
committing; missing actions of its own view still reject out-of-order use.
`conditional_controller` exercises one common rolled route after either holder
choice, including an observer polled before its co-resident controller. This
fixture bypasses only public qualification, not source validation or admission.
Five visits cover left-to-left, left-to-right, right-to-right and right-to-left
decisions. The returned-branch observation excludes another view's intervening
commit from the observer's own receive. The generated Lean checks bind all
twenty observed operations to source order and preserve the enclosing choice.
A separate regression exercises the same repeated route through ordinary
public projection, without a delegated Endpoint.

## Verification

Run from the Hibana root:

```sh
bash proofs/endpoint-delegation/check.sh
MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly-2026-05-28 miri test \
  --manifest-path proofs/endpoint-delegation/compiler/Cargo.toml --locked --lib
```

The check runs Rust tests, including actual const-evaluated plans, strict
Clippy, host and thumbv6m `no_std` builds, Lean, and Z3. Its evidence directory
contains the actual Rust results and generated Lean source.

`EndpointOwnership.lean` independently executes each complete source path with a
functional owner map, then solves the combined equations by graph closure.
All paths share one initial issuance; they may have different later holders.
It does not reuse Rust's source-prefix query or union/find implementation.
Fourteen general theorems cover independent moves, closure
connectivity, initial-owner inference, preservation of logical wire identity,
and structural local order. The inferred-owner theorem applies to every
assignment satisfying the emitted equations. It is not a full compiler
soundness or progress theorem.

The Rust exporter emits29 source terms and their actual results:20 accepted
ownership plans with38 complete source paths, and9 rejections.193 generated
Lean theorems check those results, every path's complete local-order relation,
and mutations of actors, wire identity, initial issuance, selected scopes,
and omitted order edges. Proofs use kernel
reduction; their reported axiom closures are checked against `propext` and
`Quot.sound` only. Z3 checks7 impossible violations,2 satisfiable witnesses
for incorrect rules, and1 positive conditional-loop witness. No source-string
matching substitutes for these checks.

The core receive-order validator uses logical peers and lanes to identify
competing receives, and this same actor image to propagate causal evidence.
It can follow a reply through a different endpoint view of its current holder.
Equal physical senders alone do not provide FIFO between different logical
directions. Branch-specific evidence remains correlated with the selected
holder; parallel siblings share only their input until their join. Inner
rolled choices vary between visits, while choices outside the repeated body
stay shared. No runtime ownership table or additional notification is added.

`Causality.lean` proves four general local-order/path composition laws over
the independent action model, including order through different views of one
holder and the absence of that edge across parallel siblings. They do not
prove the complete Rust causal validator sound. Its actual decisions are also
compared with a separate concrete send/receive DAG in core tests, enumerating
selected paths and two visits per repeated fragment. `Causality.smt2` checks
three impossible violations, three witnesses for incorrect shortcuts and a
legal change of choice between visits. Kani separately checks the actual
query implementation. These checks retain their stated finite boundaries.

Four bounded checks use the actual compiler's equation solver with arbitrary
u8 actors. They cover independent roots, conflicting merge atomicity, forced
ownership after merging, and merge orientation. Six additional checks cover
the exact disjoint actor/node reference domains, reduced nested choices with
arbitrary actors and selections, and atomic failure of an undersized actor
image rebuild. Nested-choice properties are separate proof obligations with
the same full actor/selection domains and enabled unwinding assertions:

```sh
bash .github/scripts/lib/resource_guard.sh \
  cargo kani --manifest-path proofs/endpoint-delegation/kani/Cargo.toml \
  --lib --jobs 1 --output-format terse
```

The separate verification manifest follows the repository's existing Kani
layout because Kani0.67.0 bundles a Rust1.93 nightly compiler. It does not lower
the production or reference compiler's Rust1.95 requirement. Unwinding checks
remain enabled. These harnesses do not establish arbitrary-size projection or
distributed runtime correctness.

## Typed source and compact payload identity

The typed `g::Msg<L, Endpoint<'r, R>>` source identifies an owned endpoint
without giving Endpoint an ordinary wire codec. Source ownership validation
rejects conflicting initial issuers, concurrent transfer/use, self-carriage and
ownership changes across roll reentry.

The internal global image now retains this payload kind and its full u8 role
in the existing nine-byte atom row. Wire payloads retain every u32 schema value;
no schema value aliases the owned kind. Decoder checks reject reserved bits,
out-of-range roles and truncated owned-role encodings. The source tests also
exercise independent rolled loans and a resolved route through compact image
construction. These are compiler/data checks, not an executing handoff.

Public projection still rejects owned sources. The new source knowledge checker
tracks communication through the holder's actor, including owned receives. It
does not learn from an independent parallel sibling. It retains correlations
between prior ownership choices when comparing the actual participants in route
arms. Candidate actor unions serve storage and lane coloring, not acceptance
authority. `Knowledge.lean` interprets every complete normalized path and checks
each sender's controller or causally preceding receive, together with actual
participation in the opposite arm under the same other choices. It does not
reuse Rust's actor DAG or query contexts. All20 ownership-accepted sources are
compared, including four branch-knowledge regressions. Three general theorems
cover knowledge shared by an actor's views, exclusion of receives outside a
choice, and exclusion of parallel siblings. This correspondence for the exported
cases is not an arbitrary-source projection/progress theorem.

The normal source-to-global-image path emits owned bindings, reduced decisions
and initial issuers into an owned-only tail. The ordinary program's eight-byte
column record and encoded bytes are unchanged. A reserved atom-count bit tags
the owned grammar; its final count word stores the complete blob length. A
four-byte header stores scope/decision counts, and the remaining bytes determine
the exact initial-issuer count. Atom rows stay nine bytes, binding rows four,
decision rows six and initial-issuer rows two. All bytes share the existing
65,535-byte descriptor limit. Compiler decision scratch is bounded by that byte
limit rather than assuming there can be at most one decision per source event.

Construction rejects absent ownership facts, cyclic/out-of-range references,
foreign choice scopes, malformed count headers, duplicate initial issuers and
an initial actor inconsistent with the first transfer. It does not recompute
source ownership or authorize an operation at runtime. Header-aware global
translation checks and actor-local admission must be connected before an owned
RoleProgram may be accepted. Existing count-only acceptance deliberately does
not certify an owned image.

## Remaining integration

An additional private execution selects either of two holders, receives DB and
AUTH views, runs the DB fragment twice with `roll`, and returns DB through the
owned AUTH carrier. The initial holder then continues DB. All ordinary source
validation constants are evaluated; only the public qualification guard is
bypassed to obtain test images. Both choices pass native and strict Miri with
four separate runtime/slab owners connected by the shared-memory test carrier.
This is not a separate-process transport or a general initial-binding proof.

## Initial physical binding

`Transport::initial_endpoint_binding` supplies the unique resource owner's
authority. Initial attachment of an owned logical role fails if no authority
is available. Ordinary wire roles and checked owned receives do not mint an
initial right. A reservation excludes callback reentry; failed unpublished
attachment returns it. Successful publication permanently consumes it. Endpoint
and runtime Drop do not make another initial right available.

The one-byte binding contains no holder table, branch history, or second cursor.
All aliases of one physical resource must return the same authority. Establishing
that identity, keeping it alive across remote loans, and fencing older rights
before replacing it belong to the physical integration's unsafe constructor
contract. Core cannot infer that two arbitrary OS or service handles alias.

Actual Endpoint tests reject duplicate issuance in another runtime and another
session, retain rejection after destroying the original runtime, and permit
retry after an undersized, unpublished attachment. `Issuance.lean` proves four
facts about the irreversible issuance boundary. `Issuance.smt2` checks three
invariants, exposes the erroneous destruction/revival rule, and keeps a legal
publication witness. These models do not establish physical handle identity or
authenticated distributed handoff.

## Remaining qualification

Repetition uses only the source's explicit roll boundaries. An inherited
reentrant route mark is not an independent loop. The public Rust regressions
and `Hibana.RollBoundary` cover rejection of an implicit child restart and
acceptance of an explicit child's repeated, alternating arms under a retained
parent prefix. An unused arm's clear completion bit does not bypass the
committed choice before an explicit roll completes. `RollBoundary.smt2`
checks six invariants and retains three
witnesses for this projected six-event shape. Old completion is evaluated
against committed choices, before the new preview chooses its arm.

- Complete the actor-aware source checks and independently qualify accepted
  actor-local images, including branch knowledge and conditional holders.
- Qualify general continuation across route/roll/par combinations. Ordinary
  owned send/recv already shares one actor progress and serializes no pointer.
- Qualify the initial binding against a real physical operation boundary,
  including related runs and native fencing.
- Validate pending, commit, drop, failure, replay, and authenticated remote
  handoff against the same ownership and progress model.
- Prove the full projection/implementation correspondence and communication
  progress under explicit transport and executor assumptions.

Actual passing private executions qualify their exact fixtures. They do not
establish that these remaining requirements have been completed.
