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
Rolled fragments must preserve ownership across visits. A branch join needs
common ownership only for endpoints used by a following fragment or visit.
An endpoint with no later use may remain in different hands in the two arms;
no redundant return message is required.

Live path-dependent ownership at a branch join is not represented yet. The
prototype rejects it explicitly rather than choosing one arm's owner. This is
an outstanding expressiveness limit, not a claim that every such source is
unsafe. A general extension must retain safe uses of choices and repetition.

The compiler library is `no_std`, uses stable Rust1.95, and has no dependencies.
Scratch and output are caller-owned arrays sized from the input. The source
tree is borrowed. Branch rollback uses the existing event trail, without
copying a complete ownership map at each fork. Continuation liveness borrows
source fragments. No runtime permission table, application flag, or second
progress cursor is produced. The plan is published only after validation;
failure cannot expose a previous accepted plan or a partial new one.

## Verification

Run from the Hibana root:

```sh
bash proofs/endpoint-delegation/check.sh
MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly-2026-05-28 miri test \
  --manifest-path proofs/endpoint-delegation/compiler/Cargo.toml --locked --lib
```

The check runs23 Rust tests, including an actual const-evaluated plan, strict
Clippy, host and thumbv6m `no_std` builds, Lean, and Z3. Its evidence directory
contains the actual Rust results and generated Lean source.

`Projection.lean` elaborates ownership functionally and infers initial owners
through graph closure. It does not reuse Rust's union/find implementation or
undo trail. Fourteen general theorems cover independent moves, closure
connectivity, initial-owner inference, preservation of logical wire identity,
and structural local order. The inferred-owner theorem applies to every
assignment satisfying the emitted equations. It is not a full compiler
soundness or progress theorem.

The Rust exporter emits21 source terms and their actual results:11 accepted
ownership plans and10 rejections. Seventy-three generated Lean theorems check
those results, the complete local-order relations, and mutations of actors,
wire identity, initial issuance, and omitted order edges. Proofs use kernel
reduction; their reported axiom closures are checked against `propext` and
`Quot.sound` only. Z3 checks7 impossible violations and2 satisfiable witnesses
for incorrect rules. No source-string matching substitutes for these checks.

Optional bounded checks use exactly the same compiler source:

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
- Represent safe live path-dependent ownership without global serialization.
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
