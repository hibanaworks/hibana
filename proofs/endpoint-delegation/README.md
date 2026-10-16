# Ownership-aware endpoint projection

This is an executable reference model and a proof package for the delegation
extension. It is not the Hibana runtime, a public API, or a second application
language. The source constructors mirror `send`, `seq`, `par`, binary
`route`/resolver, and `roll` so that ownership rules can be checked before the
owned-message runtime is introduced.

An accepted ownership plan is not an accepted Hibana protocol. This package
does not yet check knowledge of choice, route authority, codecs, lane mapping,
remote delivery, or the implementation's operation commits. Its ownership
elaboration must be incorporated into typed projection, with those checks,
before any distributed progress claim is justified.

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
tree is borrowed. Rust queries ownership from source prefixes, capturing the
receiver's holder at the transfer event. Moving that receiving endpoint later
does not move other values along with it. No mutable current-holder map, undo
trail, or map copy at a fork is needed. These queries belong to projection and
reference checking; they must be lowered to runtime facts before integration.
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

The check runs28 Rust tests, including actual const-evaluated plans, strict
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

The Rust exporter emits25 source terms and their actual results:16 accepted
ownership plans with26 complete source paths, and9 rejections.129 generated
Lean theorems check those results, every path's complete local-order relation,
and mutations of actors, wire identity, initial issuance, selected scopes,
and omitted order edges. Proofs use kernel
reduction; their reported axiom closures are checked against `propext` and
`Quot.sound` only. Z3 checks7 impossible violations,2 satisfiable witnesses
for incorrect rules, and1 positive conditional-loop witness. No source-string
matching substitutes for these checks.

Four bounded checks use the actual compiler's equation solver with arbitrary
u8 actors. They cover independent roots, conflicting merge atomicity, forced
ownership after merging, and merge orientation:

```sh
cargo kani --manifest-path proofs/endpoint-delegation/kani/Cargo.toml \
  --lib --output-format terse
```

The separate verification manifest follows the repository's existing Kani
layout because Kani0.67.0 bundles a Rust1.93 nightly compiler. It does not lower
the production or reference compiler's Rust1.95 requirement. Unwinding checks
remain enabled. These harnesses do not establish arbitrary-size projection or
distributed runtime correctness.

## Remaining integration

- Connect ownership elaboration to the typed global and accepted local images.
- Lower conditional ownership and cross-facet ordering to compact local facts,
  preserving independent parallel flows without source scans in the hot path.
- Send an owned Endpoint and receive its continuation through ordinary
  `send`/`recv`, with one resident owner and no pointer serialization.
- Bind initial issuance to the unique resource owner, including related runs.
- Validate pending, commit, drop, failure, replay, and authenticated remote
  handoff against the same ownership and progress model.
- Prove the full projection/implementation correspondence and communication
  progress under explicit transport and executor assumptions.

The public0.9.9 release does not implement Endpoint delegation. The separate
resident-owner foundation is an implementation prerequisite, not evidence
that these remaining requirements have been completed.
