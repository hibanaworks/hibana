# Lean models

This package models Hibana protocols and their execution. The definitions in
`Hibana/` are shared by the general theorems, the independent descriptor
checkers and the generated-input checks.

## Protocol and projection

`Syntax`, `GlobalSyntax`, `StaticProjectability` and `DescriptorImage` define
the protocol language, local operations and compiled protocol data. The
descriptor checkers validate the exact data exported by Rust tests. Their
results apply to those accepted inputs, rather than assuming that every Rust
projection has been proved correct.

`EndpointOwnership` models initial ownership, transfers and source order.
The [ownership harness](../checks/projection/endpoint-delegation/README.md)
executes the production Rust validator and checks its output independently.
`SourceOccurrenceProjection` and `SourceOccurrenceProgress` describe finite
unfoldings in which each send, publication and receive has its own occurrence.
Independent roles and parallel fragments need not advance repetitions together.

`GlobalSemantics` and `DistributedProgress` also contain a model with a common
repetition boundary. Results using that boundary have that explicit scope.
The occurrence model is used for asynchronous, pipelined repetition.

## Ownership and transport

`OwnedCallHistory`, `SourceCallHistory` and `DistributedCallExecution` distinguish
transport acceptance from sender publication. Independent sends by one actor
can overlap and publish in a different order from their acceptance.
`DistributedPhysicalExecution` relates those calls to the same FIFO receipts,
ordinary frames, close, abort and local faults.

`OccurrenceResourceOrigin`, `ResourceOccurrenceCausality` and
`OccurrenceResourceCustody` relate that history to the resource authority held
in application values, pending futures or accepted frames. Authority is affine:
at most one usable right exists. A sender's record of an accepted transfer is
not another right to operate the resource.

The theorem
[`a_joint_occurrence_execution_has_retained_resource_authority_and_source_progress`](Hibana/OccurrenceResourceCustody.lean)
combines source progress, FIFO correspondence and retained resource authority
over one qualified execution of an accepted finite unfolding. Its retention
condition excludes retiring the resource needed for the next operation.
The version allowing retirement reports an actual completion or fault boundary.
A transport error alone is not evidence that ownership has been released.

Carrier assumptions include peer and direction binding, FIFO delivery,
nonduplication, replay exclusion, session-run isolation and receive continuity
when the holder changes. Normal close preserves accepted frames until they
are received. Abort and fencing have separate rules. Neither a deadline nor
`Offline` proves non-delivery or authorizes ownership recovery.

## Correspondence with Rust

The generated-input checks read real projection data and operation observations
from production tests. They check selection, dependencies, cursor changes,
operation slots, pending sends, ownership and physical receipts against the
models. Accepted observations are evidence for those executions. Completeness
of arbitrary Rust instrumentation and all reachable Rust states is not a theorem
of this package.

The compiled `endpointConformance` executable compares Rust and model decisions
over bounded schedules. Input comes from public `project` and actual Endpoint
operations. It checks both kinds of disagreement, stalled cuts, split transport
acceptance and completion, and close/abort schedules. Mutation controls check
that lost ownership, lost dependencies and unnecessary serialization are detected.
Schedules are JSON input, rather than generated Lean declarations.

Production Rust ownership and borrowing, native tests, Miri and bounded Kani
checks complement these models. The implementation remains responsible for
making a legitimately held Endpoint callable and preserving accepted frames
across holder changes. The model does not assume success of the Rust call that
the implementation is meant to justify.

Participants must invoke the permitted continuation, retain authority-bearing
values and futures until completion or ordinary drop, and drive independent
fragments when required. The executor must poll those futures. These are
application and executor assumptions. A theorem that a model transition exists
is not a theorem that arbitrary Rust code eventually executes it.

## Run and audit

Use the toolchain in [`lean-toolchain`](lean-toolchain). From the repository root:

```sh
TOOLCHAIN=stable bash .github/scripts/lib/resource_guard.sh \
  bash .github/scripts/check_lean_proofs.sh
```

The gate builds the common package serially, checks theorem types and permitted
axioms against [`claims/`](claims), regenerates Rust inputs, and checks them with
the Lean kernel or compiled conformance executable. Missing or extra claims,
failed generators, malformed inputs and forbidden axioms are errors.
The permitted axioms are `propext` and `Quot.sound`. `sorry`, additional axioms
and `native_decide` are not permitted.

Generated input and logs belong to build or temporary directories. They are not
protocol state, and are never used to authorize application operations.
