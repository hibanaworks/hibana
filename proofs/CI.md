# Required verification contract

The `core-contract` CI result covers the checks below on one source revision.
It does not assert refinement of arbitrary Rust executions or exhaust every
possible protocol, payload, visit count or transport failure.

| Property | Required evidence |
| --- | --- |
| Public Rust API, borrowing and affine moves | All native targets, including existing compile-fail inputs and Endpoint regressions; doctests, strict Clippy and documentation checks |
| Projection and source rules | The complete checked Lean model inventory, standalone implementation theorems, theorem types and axiom boundaries; native projection and malformed-image tests |
| Finite public protocol conformance | Every constructor-budget-2 catalog shard, and every shared-message-contract catalog entry and variant |
| Missing or extra runtime permission | Every next-operation probe at every prefix of each catalog's finite scripts; complete probe and schedule inventories; no runtime-only or model-only discrepancy |
| Detection of known defects | Native Endpoint regressions and the resolver, parked preview, reentry, receive, prefix and shrinking sensitivity controls |
| Critical memory boundaries | Strict-provenance Miri for runtime ownership, borrowed Transport handles, initial issuance, real resource Endpoint joins and fault/cancellation ownership |
| Physical and portable cost constraints | Host and embedded no_std builds, forbidden long const evaluation, existing layout/allocation tests and portable work-count budgets on macOS and ARM Linux |

`protocol-shape-partitions ci-matrix` is the sole CI assignment source for
generated conformance. It assigns the complete budget-2 population across all
32 shards and every shared-contract source. Five full contract populations also
serve as the positive side of the required sensitivity controls; the controls
assignment owns these populations instead of duplicating their execution in
separate jobs. This gives 43 ordinary assignments without removing any contract
source, variant or script.
Its tests reject missing or duplicate assignments. Catalog sizes are
shared with the generator rather than copied into workflow inputs.
`check_protocol_ci.sh ID [SEED]` obtains that ID's canonical assignment and runs
it unchanged locally and in CI. It rejects unknown IDs and refuses to overwrite
an existing assignment receipt. The workflow does not reimplement selection or
execution rules.
Formatting and all verification-tool tests (including the generator, rejection
classifier, checker controls and assignment planner) run once in a small preflight
job. Kernel tests and Clippy exclude that already-checked tool package. Every
expensive verification job depends on a successful preflight; a broken tool does
not start the full population, Kani or the additional proofs. The planner emits
the complete assignment matrix without repeating its unit tests. The sampling
seed is chosen once for the whole run, including runs that cross UTC midnight.
The work-count reader's unit tests also run only in preflight. Both host jobs
still compile their own reader and run every instrumented Endpoint workload.
Embedded profiling builds run once in the no_std gate for both supported ARM
targets, rather than being repeated on both host jobs.

Each CI run compiles the shared public conformance oracle once for its selected
source revision and host platform. Every assignment verifies the oracle's binary hash, source-input
hashes, source revision and platform before using it. Missing or mismatched
artifacts fail the job. The controls assignment also receives the compiled Lean
imports and installs Lean to execute its model mutations. Other assignments do
not reinstall Lean or rebuild the same oracle. Each source still gets its own
compiler diagnostics, real Endpoint execution, oracle comparison and structural
observation controls. The four synthetic accepted-bit mutations (initial and
continuation, false admission and false rejection) run once in the required
controls assignment, on its resolver-occurrence catalog. They use the same
compiled checker and complete probe inventory. Repeating those comparisons for
every population only recomputed unchanged source witnesses; it did not exercise
a different comparison rule. Source-dependent schedule and history controls
remain attached to every population.
The source-model theorems and their axiom checks run in this same oracle job,
reusing its Lean imports. The Rust job does not install Lean and rebuild those
same modules. Local `run_ci_gates.sh` still checks both parts; `--rust` names
only the Rust part that CI assigns separately.
The build cache is an optimization of the same checker, not another oracle.

The replay checker decodes observations incrementally and shares immutable
source preparation and repeated prefixes. Each cached replay carries a Lean
equality to fresh replay; general theorems preserve every query and the whole
candidate family. Query inputs are prepared once, and an empty candidate state
reuses the previous answer row instead of constructing another all-false row.
Neither optimization removes source candidates, scripts or admission probes.
Source-occurrence lookup is prepared once per witness, and the resumption
table receives shared source-domain and unfolding-scope values explicitly.
Answer rows use arrays, so reusing an empty candidate's row does not traverse
every probe merely to check its length. These representations remain equal to
the original source-readiness and list-query specifications.
Each prefix computes its enabled calls once for the whole group of questions.
The grouped path and individual calls use the same binding step; general Lean
theorems equate their selections and every resulting query answer. A positive
existential answer remains true for the whole family once it has a witness;
the proven row fold reuses those answers. Prefix-only questions do not build
an operation frontier. A periodic counter reports considered witnesses and
does not change the result or candidate population.
The oracle build executes its fifteen fixed source-cut regressions once and
archives their log with the binary hash. Normal population invocations do not
repeat them. Their mutation controls retain the explicit regression entry.
Sensitivity captures stay in their own baseline or mutant source root; an
outer population-output override cannot redirect both to the same directory.
The controls assignment prepares its passing populations once. Every subsequent
production mutation verifies that baseline's completed-run and verdict hashes,
source content, revision and generated inputs before using it. A missing or
mismatched baseline fails instead of selecting another checkout or rebuilding a
different positive case. An existing population directory cannot be overwritten.
Generated tests use the executable inventory from their initial Cargo build,
with binary hashes checked before and after execution. A second Cargo build
only to enumerate the same executables is removed. Successful populations
discard their private build targets and consumed temporary admission arrays.
Their sources, inputs, binary hashes, complete successful execution traces,
checker verdicts, negative-control rejection logs and hashes of retired arrays
remain. Partition copies are removed after their contents have been combined
and checked. Failed populations retain their debugging evidence.


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
fail CI. Each job saves its assignment, generated sources, hashes, complete executions
and verdicts. The final result requires preflight, the shared oracle build, every mandatory
matrix assignment and every other required job to succeed. Skipped, cancelled and missing jobs are failures.
The ordinary workflow has no optional verification jobs. A failed population
cancels the remaining matrix work because that source cannot qualify; cancelled
assignments still make the final contract fail. A fixed revision must run the
complete matrix successfully. Extended certification
invokes that same workflow plus Kani and the additional proof/memory checks; its
final result also rejects every skipped or unsuccessful dependency.

The former separate generated-shape and fuzz jobs are removed. Exhaustive
catalogs and seeded syntax sampling are selections of this one pipeline.
No second source model, admission rule or Endpoint execution path is introduced.

## Extended certification

The separate `extended-certification` workflow runs on `ci-certify-*` tags, and
explicit manual requests. It does not rerun unchanged Kani/proof populations
on a nightly timer; seeded syntax samples remain part of every certification
and can be requested with a new seed. A certification
tag selects an immutable source revision; it is not a release or a passed result.
Branch pushes run the ordinary workflow. Certification tags invoke that same
workflow exactly once, rather than also triggering a duplicate ordinary run.
A final candidate is published through its certification tag, then advanced
to main after qualification. Root runs share one concurrency group across
branches and tags; a newer run cancels the previous source run. The reusable
core-checks child has a run-specific group so that it cannot cancel its own caller.
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
The additional proof job runs on macOS, including the actual TCP publication
and Unix-carrier fixtures that use hibana-runtime's macOS host I/O. Portable
work-count checks and generated public conformance also run on ARM Linux.
The four native layout assertions remain in the required Rust suite; the
measurement job collects their numerical report and does not rerun those tests.
The runtime-performance job likewise leaves the four warm-target regression
tests in the native suite. It retains the independent cold-target compile budget
and the runtime measurements. Packaged examples are executed, and the delegated
example's tests are run; a separate compile-only pass of those same examples is
unnecessary.
The 16 delayed-parent schedules execute once in `check_miri.sh`, including
each scenario's completion record and unique source-specific evidence. The
extended entry point calls that gate and does not repeat the schedules.
The required Miri boundary checks and the additional Miri checks are disjoint
selections of the same script; its default still runs both. Overlapping broad
and individual test filters have one execution site, including the parked-view,
actor-progress, image, tap and delegated-offer regressions. Native and Miri
executions remain distinct evidence.

The source-order equivalence catalogs (all 19,765 depth-three trees and the same
1,000 seeded depth-eight trees) remain mandatory native tests. They are not
rerun as semantic catalog enumerations inside Miri. Miri runs the packed image,
malformed ownership/offset, selected-preview and temporary-storage lifetime
fixtures, including the production comparison path's allocation and unwinding
boundary. This finite memory check does not claim Miri interpreted every sampled
tree. The former broad module filter accidentally included both native catalog
enumerators; its 180-second stops are incomplete runs, not passing evidence.
The independent grammar oracle now caches its relations and ancestor paths;
all original trees, production queries, boundary values and native assertions
remain present. Likewise, model
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
