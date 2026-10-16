# Ownership-aware endpoint projection

This proof package runs the ownership validator used by Hibana's typed source
boundary. The source tree and validator live in `src/g/source/`; the proof
harness includes that implementation and adds read-only observations. There
is no separate Rust source language or second implementation of its ownership
rules. The Lean checker remains independent.

Typed `g::Msg<L, Endpoint<'static, R>>` records an owned endpoint, not a wire
codec. Source projection checks its initial issuer, conflicting transfers,
parallel uses, and rolled ownership before lowering. Core source tests and
compiler rejection cases exercise the real `g` constructors.

Owned descriptor lowering and value-consuming `Endpoint::send` / `recv` now
execute in private actor-image tests. They cover return to the original actor,
distinct runtime slabs, branch operations and independent rolled resource
exchanges. Public owned projection remains closed while general continuation,
initial resource binding and the complete verification boundary are unfinished.
The public 0.9.9 release does not provide Endpoint delegation.

An accepted ownership plan is not an accepted Hibana protocol. This package
does not certify knowledge of choice, route authority, codecs, lane mapping,
remote delivery, or the implementation's operation commits. Its ownership
elaboration must also produce compact actor/facet facts and connect to those
checks and the operation kernel before any distributed progress claim is
justified. The harness can describe related components; production `g` source
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
choices to the existing compiled scope identities. Actor-local descriptor
emission still needs to consume them and preserve logical facets and operation
ordering before runtime integration is complete.
No runtime permission table, application flag, or second progress cursor is
produced. The plan is published only after validation;
failure cannot expose a previous accepted plan or a partial new one.

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

`Projection.lean` independently executes each complete source path with a
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

- Complete the actor-aware source checks and independently qualify accepted
  actor-local images, including branch knowledge and conditional holders.
- Qualify general continuation across route/roll/par combinations. Ordinary
  owned send/recv already shares one actor progress and serializes no pointer.
- Bind initial issuance to the unique resource owner, including related runs.
- Validate pending, commit, drop, failure, replay, and authenticated remote
  handoff against the same ownership and progress model.
- Prove the full projection/implementation correspondence and communication
  progress under explicit transport and executor assumptions.

Actual passing private executions qualify their exact fixtures. They do not
establish that these remaining requirements have been completed.
