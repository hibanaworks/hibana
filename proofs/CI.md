# Required verification contract

The `core-contract` CI result covers the checks below on one source revision.
It does not assert refinement of arbitrary Rust executions or exhaust every
possible protocol, payload, visit count or transport failure.

| Property | Required evidence |
| --- | --- |
| Public Rust API, borrowing and affine moves | All native targets, including existing compile-fail inputs and Endpoint regressions; strict Clippy and documentation checks |
| Projection and source rules | The complete checked Lean model inventory, standalone implementation theorems, theorem types and axiom boundaries; native projection and malformed-image tests |
| Finite public protocol conformance | Every constructor-budget-2 catalog shard, and every shared-message-contract catalog entry and variant |
| Missing or extra runtime permission | Every next-operation probe at every prefix of each catalog's finite scripts; complete probe and schedule inventories; no runtime-only or model-only discrepancy |
| Detection of known defects | Native Endpoint regressions and the resolver, parked preview, reentry, receive, prefix and shrinking sensitivity controls |
| Critical memory boundaries | Strict-provenance Miri for runtime ownership, borrowed Transport handles, initial issuance, real resource Endpoint joins and fault/cancellation ownership |
| Physical and portable cost constraints | Host and embedded no_std builds, forbidden long const evaluation, existing layout/allocation tests and portable work-count budgets on macOS and ARM Linux |

`protocol-shape-partitions ci-matrix` is the sole CI assignment source for
generated conformance. It assigns the complete budget-2 population across all
32 shards, one assignment for every shared-contract source and one set of sensitivity
controls. The contract sources are split without removing any variants or scripts.
Its tests reject missing or duplicate assignments. Catalog sizes are
shared with the generator rather than copied into workflow inputs.
`check_protocol_ci.sh ID [SEED]` obtains that ID's canonical assignment and runs
it unchanged locally and in CI. It rejects unknown IDs and refuses to overwrite
an existing assignment receipt. The workflow does not reimplement selection or
execution rules.
Generator, rejection-classifier and checker-control unit tests run once in the required
workspace test job. Individual catalog executions do not rerun the identical
unit suites; their source-specific compiler, Endpoint, oracle and completeness
checks still execute for every assignment.

The budget counts generated constructors, not syntax depth. The grammar includes
ordinary protocols and Endpoint loan/use/return/onward-transfer protocols,
`seq`, both children of `par`, both route arms, resolver and non-resolver
choices, and `roll`. Fixed outer loan/exchange structure and compound leaves
are described in [the generator's scope](README.md). The domain has three or
four roles and the generator's finite visit and scheduling inventories.
Successful CI claims completeness only for those emitted populations and
script ranks. It does not turn the budget into a bound on arbitrary source
depth or claim every parallel interleaving outside those scripts.

Every assigned population uses the same generator, compiler path, real Endpoint
executor, independent source oracle and inventory checker. Legal candidates are
actually executed; independently classified illegal candidates must fail
compilation. A compile-fail-only shard makes no runtime coverage claim. Missing
inputs, unclassified rejections, incomplete probes, timeouts and budget stops
fail CI. Each job saves its assignment, generated sources, hashes, observations
and verdicts. The final result requires the plan, every mandatory matrix assignment and every
other required job to succeed. Skipped, cancelled and missing jobs are failures.
The ordinary workflow has no optional verification jobs. Extended certification
invokes that same workflow plus Kani and the additional proof/memory checks; its
final result also rejects every skipped or unsuccessful dependency.

The former separate generated-shape and fuzz jobs are removed. Exhaustive
catalogs and seeded syntax sampling are selections of this one pipeline.
No second source model, admission rule or Endpoint execution path is introduced.

## Extended certification

The separate `extended-certification` workflow runs on `ci-certify-*` tags, and
nightly or manually once its definition is on the default branch. A certification
tag selects an immutable source revision; it is not a release or a passed result.
Branch pushes run the ordinary workflow. Certification tags invoke that same
workflow exactly once, rather than also triggering a duplicate ordinary run.
It adds the complete frontier catalog, the existing
deep interaction/spine regressions and seeded depth-4-through-8 syntax samples.
Sampling is not described as exhaustive. These use the same assignment and
execution path as mandatory conformance.

Extended certification also runs the full Kani inventory, full strict-provenance
Miri checks and all 16 delayed-parent reentry cases, generated internal-state
transition proofs, the remaining implementation proofs, capacity/compile-pressure
and package gates. Generated transition proofs connect concrete internal
snapshots to the observation model; they are distinct from the public-operation
conformance contract. Their successful finite examples are not a general Rust
refinement theorem.
The required Miri boundary checks and the additional Miri checks are disjoint
selections of the same script; its default still runs both. Likewise, model
proofs and generated observation proofs are complementary selections of one
Lean gate. Certification does not repeat the complete native/Clippy/documentation
job inside its additional proof job. They do not verify arbitrary user Transports,
OS driver FFI or every possible unsafe execution.

The full extended result remains required when fixing a publication/release
artifact. It is not placed on every development push's critical path. A required
CI success must never be reported as successful extended certification. After a
core change, run the required contract locally on fixed source as well; perform
extended checks for the final artifact and for changes to the properties they
specifically cover. Preserve failing inputs and verification stops.
