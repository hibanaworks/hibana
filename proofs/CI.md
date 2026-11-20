# Required verification contract

The `core-contract` CI result covers the checks below on one source revision.
It does not assert refinement of arbitrary Rust executions or exhaust every
possible protocol, payload, visit count or transport failure.

Development and CI both use the adjacent `hibana-runtime` source directory.
CI obtains its exact public revision from `.github/runtime-revision`; packaging
checks require that same clean executor before testing the shipped examples.
An unversioned path dev-dependency is excluded from the published crate, whose
production core remains independent of the executor.

| Property | Required evidence |
| --- | --- |
| Public Rust API, borrowing and affine moves | All native targets, including existing compile-fail inputs and Endpoint regressions; doctests, strict Clippy and documentation checks |
| Projection and source rules | The complete checked Lean model inventory, standalone implementation theorems, theorem types and axiom boundaries; native projection and malformed-image tests |
| Finite public protocol conformance | Every constructor-budget-2 catalog shard, and every shared-message-contract catalog entry and variant |
| Missing or extra runtime permission | Every next-operation probe at every prefix of each catalog's finite scripts; complete probe and schedule inventories; no runtime-only or model-only discrepancy |
| Detection of known defects | Native Endpoint regressions, complete generated operation matrices, malformed-record controls and failure-preserving shrinking |
| Critical memory boundaries | Strict-provenance Miri for runtime ownership, borrowed Transport handles, initial issuance, real resource Endpoint joins and fault/cancellation ownership |
| Physical and portable cost constraints | Host and embedded no_std builds, forbidden long const evaluation, existing layout/allocation tests and portable work-count budgets on macOS and ARM Linux |
| Actual long consumer programs | Required macOS ChoreoFix integration: build the pinned 6E, codebase and 16E real globals and localsides, including all projections of the 1,586-event review source; drive 1H concurrent delegated commits and 16E two-resource processing with the real WASI guest. Every compiled host selects this candidate core and keeps forbidden long const evaluation. |

`check_choreofix.sh` preserves immutable public inputs for the original
1,586-event review projection and one 1H three-commit schedule. Its candidate-core
override is confined to that test fixture. CI/CD executes the current direct-file
implementation from a pinned adjacent ChoreoFix checkout, with the recorded
WASI/EPF sources and the executor selected by `.github/runtime-revision`.
Development consumers use the actual canonical adjacent worktrees, with their
revision and dirty source recorded; the public pins initialize missing CI
inputs and never rewind an existing development tree. Cargo metadata must
resolve core, executor, EPF and WASI to those canonical directories.
`--historical` and `--current` select the two complementary consumer checks for
local resumption. Ordinary CI invokes both through the default command; neither
part alone satisfies the consumer contract. Published
experiment artifacts retain the exact source they actually qualified.
The consumer's production global/localside Rust is not rewritten. The current
qualifier records source hashes, dependency resolution, build receipts and actual
results. Its four CI/CD conditions cover original failure depths 0/1/2, invalid
input/retry and early termination without an effect. Together with 1H these are
five scripted mechanism cases,
not live AI, all 48 task ports or scored
comparison data. An absent test, failed compile, missing receipt, source change
or unsuccessful dependency fails the required aggregate. Full consumer
qualification remains a separate obligation. The complete 6E execution,
dynamic EPF reconstruction and its record mutations belong to consumer/EPF
qualification; their unbounded search is not duplicated in the ordinary core
capacity regression. Stopped 6E audit attempts remain unqualified evidence.

`protocol-shape-partitions ci-matrix` is the sole CI assignment source for
generated conformance. It assigns the complete budget-2 population across all
32 shards and every shared-contract source. Every shared-contract population has
its own assignment, alongside the 32 constructor shards and the common controls
assignment, with every source, variant and script executed once. There is no
old-implementation rebuild job.
At a fixed prefix and resolver input the whole-Endpoint driver runs each
distinct public selector once directly and once after an offer. Source aliases
of that selector reference those observations. Every source question and its
resolver history are retained. Lean proves that equivalent public selectors
match the same candidate occurrences and checks the full answer matrix.
The delegated-file catalog includes a repeated request with parallel replies to
the file view and its physical holder, both reply orders and two visits. The
real Endpoint regression additionally retains the next send Future before the
holder receives: it must stay Pending without sending or resetting progress,
then resume. An unread caller reply is still rejected. Acceptance-only probes
do not by themselves distinguish that suspension from a recoverable rejection.
The three `PendingAcceptance` lemmas check those concrete source cuts; they do
not prove the Rust wait classifier for arbitrary protocols.
The next catalog entry covers a rolled choice with a co-resident file reply
and a parallel retirement suffix. Its native/Miri case borrows that suffix
before the file receive, requires its send to remain Pending without any tap
or transport progress, then consumes the actual file receipt and completes
the real Endpoint return. The local-fork theorem preserves every unfinished
row when completed prefix rows are ignored during borrow selection; it is not
a general Rust refinement theorem.
The same catalog exercises repeated left and right choices before retirement.
A group belonging to another arm of a completed roll is classified as reentry,
while choices outside that roll retain their committed context. A materialized
preview constrains candidate groups before the current/reentry preference, so
forward selection cannot strand an accepted input. The public Endpoint/Miri
regression covers both choices and one to three visits across separate runtimes.
The local-fork proofs establish whole-range arm containment and preservation of
the materialized-input filter; they do not prove the complete Rust selector.

The 6M-derived receive regression observes a nonrepeating endpoint handoff
through a repository relay. It checks an empty wait before that knowledge,
recoverable refusal after it, an arrived but unconsumed notice, a borrowed view,
and the same run's valid continuation. A second real Endpoint case sends two
visits ahead of the file and delivers the retirement notice from another peer;
the earlier accepted file message must still be received. Both are ordinary
Rust and critical Miri obligations. Image validation checks the stored handoff
and witness against source-only causal knowledge; return paths, independent
parallel notices and repeated notices prevent that proof. This is an empty-wait
classification, not a new owner table or a claim of arbitrary AI termination.
The notice must use the requested receive's own peer/view/lane FIFO direction:
a control return on another direction can overtake legitimate work still in
flight. The existing typed control-return regression remains required.
The 7H-derived case combines a completed rewrite visit with a new two-edit
visit on a delegated document. The native Endpoint test checks the early
result receive after the first edit and then completes both edits and the
real result. It also checks that the peer cannot send the result yet and
that its first edit reply and final result use the same FIFO direction.
The generated catalog includes both visits and their legal continuation.
The native test checks the distinction between an empty wait and rejection.
A separate returned-loan case probes a borrowed send at the head of a roll
after its holder's only handoff. Each public-contract assignment exercises
the same source. The generated driver checks the error category and the
unchanged actor, custody and tap history. The native 7H regression then runs
the next holder's review through its final reply. The receipt-wait lemma
characterizes when an independent possible receipt justifies waiting. The
public return value and continuation are checked on the actual Endpoint.
The shared-contract catalog also covers a view-local parallel prefix followed
by a foreign sequential suffix, both with ordinary messages and after an
actual resource loan, request and reply. Generated localsides retain the join
for the actor that performs the suffix. Strict-provenance Miri includes those
positive/negative local suffix boundaries and a pre-loan receive that must
remain unavailable without faulting the run, then completes the real loan and
reply. These three tests belong to the ordinary memory contract and are not
repeated by the extended selection. The standalone local-fork lemmas establish
the filtered-row equivalence for a foreign suffix and an empty range; they do
not establish a refinement theorem for arbitrary Rust executions.
Repeated passive choices also have a negative mixed-peer source and positive
causal-ACK, FIFO-fence and delegated/parallel/nested-roll sources. The compiler
checks current and next actor paths separately; separating wire lanes alone
does not distinguish those visits. The exact delayed relay has a source-model
obstruction proof in `SourceOccurrenceProgress.lean`, not a general Rust
validator soundness proof. The cross-lane rework catalog contains an inner
request loop, an outer review loop and an independent sender. Its ordinary
two-visit scripts complement the unchanged native 17-event regression, which
uses one independent input before three outer reviews. Completed continuation
on another lane is checked using source precedence; parallel completion alone
does not authorize a parent restart.
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
source and host platform. Every assignment verifies its binary and complete
source-input hashes before use. Missing or mismatched artifacts fail the job.
Every new binary runs the fifteen guarded source cases. The normal Lean audit
checks the source-model theorems and their proof boundaries.
Each source keeps its compiler diagnostics, real Endpoint executions, model
comparison and source-dependent record, schedule and history controls. Four
accepted-bit mutations (initial and continuation, false admission and false
rejection) run once in contracts-2. They alter test observations, not production
code, and use the same compiled checker and complete query inventory.
The source-model theorems and their axiom checks run in this same oracle job,
reusing its Lean imports. The Rust job does not install Lean and rebuild those
same modules. Local `run_ci_gates.sh` still checks both parts; `--rust` names
only the Rust part that CI assigns separately.
The build cache is an optimization of the same checker, not another oracle.

Admission observations retain every query at every independently derived script prefix and recorded resolver history. Missing, duplicate, malformed, foreign or unfinished inventories fail closed. A word proves that its prefix has a finite source execution; it does not identify the next roll visit. Both whole Endpoint calls and actual parallel-borrow calls therefore use the same PublicCandidateAdmission source predicate at their measured call cut. A whole-handle cut is taken after a successful parked offer, before its typed call. The evidence keeps the exact resolver inputs, preview source identity, borrow region, retained completion, actor progress and immutable resident image. The source checker derives matching public contracts and prerequisites; it never consumes a Rust readiness verdict. The shared Lean fixture FIFO reconstructs accepted packets from actual commits. Completion eligibility is compared for every measured call; Pending versus an error is a separate runtime obligation. An absent affine handle is recorded as unavailable, rather than counted as a rejected operation.

The successful preview is observed before its borrow is dropped, separately
from that post-Drop typed-call cut. The checker verifies that Drop releases
only the target operation lease, preserving all progress, custody, route choices
and repetition credits. An ordinary input clears its preview row and requeues
the frame; an actual choice may retain its knowledge. Its exact pre-Drop source
row is checked against the same source predicate, without treating the absence
of a retained ordinary preview as an absent successful offer.

For an actual call returning `EndpointError::is_unavailable()`, both generated
drivers also require the post-call operation cut to equal the pre-call cut and
the capture to remain unchanged. This checks progress, custody, fault and parked
previews on the actual target, including parallel borrows. It is a finite Rust
state comparison, separate from the Lean permission predicate. Native and Miri
regressions additionally continue through the next valid communication with a
queued frame, a parked preview and a returned owned Endpoint. Whole receive
probes on the valid generated fixtures require recoverable rejection, rather
than counting a faulting rejection as equivalent. Native and Miri tests also
start a receive before the peer chooses a different arm, retain that legal
frame and then consume it with the matching typed receive. This recoverable
pre-I/O rejection covers declared receives and completed borrowed-payload sends
outside repeating source bodies. A wrong fragment or a failed owned send remains
terminal. This is distinct from an absent affine handle and from a terminal
error after communication starts. The generated trial still ends after
its probe; it does not execute the application's shell-request dispatch.

Actual localsides execute every finite source script in both poll orders, including real parallel borrows and owned Endpoint moves. Their completed-call records are checked against SourceUnfolding and source execution. Scoped questions are taken on those actual handles, with the same preceding calls, borrows and capture. The common source-cut checker compares all measured positive and negative outcomes. Its fixture-head theorem preserves the earliest matching lane packet and the full wire label. This is finite conformance and checked metadata/cut correspondence, not a refinement proof for arbitrary Rust histories.

A public message label can occur in both source arms. The generated driver
requires the observed preview to permit its planned word; it does not infer a
unique source arm from that label. Actual occurrences, resolver histories and
every measured permission remain independently checked from the captures.
The contract catalog includes nested choices with different first peers and
reused ordinary or affine payloads. Send preparation follows the sealed route
ancestry from outer to inner, without invoking an inactive child's resolver.
The nested-roll population also switches a child into a previously unused arm
after its parent body completed. This must retain the parent's selected arm
until an actual enclosing restart; an unused row's completion bit cannot decide
which roll is being reentered. Native stateful callbacks additionally reject a
second invocation of the same fresh decision during one send preparation.

The whole-call checker shares decoded passive states using the same complete
content key already used by query preparation. Original context IDs remain
consecutive and individually consumed; prefix histories, input, payload
availability and FIFO evidence are retained for every context. Only the
inventory ID is absent from the state key. A wrong-ID record is an executable
rejection control. This storage sharing does not reduce the input population.

The passive cut distinguishes retained historical completion from an unconsumed
child-completion credit. Both copy existing kernel data; neither authorizes an
operation. A credited child can retain current work only when the entering
operation is outside that child and source order makes it independent of every
participating child row. Its internal choices may remain. A still-running
child also retains its known enclosing choice; a completed child permits a
fresh enclosing decision. Neither can supply an unknown parent choice.
A preview of a prospective visit does not overwrite
the current visit's choice for an independent continuation. The complete cut,
including these credits, is part of the cache key; every original state and
query is still checked. Old observations without the credit field are rejected,
rather than guessed or silently upgraded.

A typed send can select an intrinsic route that `offer()` alone cannot choose.
An ancestor resolver does not determine a nested intrinsic branch. Required
offer-availability assertions therefore cover resolver-controlled paths without
such a message-selected ancestor. This does not omit any typed-operation query;
all successful parked previews still receive the same source-cut check.
The previewed source row itself must be a source candidate, independently of
whether the following typed call succeeds. Assignment 18 keeps the original
carrier live after a failed-result roll transfers its entry resource onward;
an obsolete result must not be published by `offer()`. The source model already
requires the entry resource's issued header when preparing another visit.
Assignment 19 retains an unconsumed return in the continuation. Its preview
can name the next choice before that receipt; its typed call must still wait
for, or first perform, the receipt. Preview eligibility and call completion
are distinct source questions. A completed old return cannot stand in for an
outstanding one. Existing native regressions also keep the independent-view
wait and the same-view rejection cases.
The direct file-view regressions also distinguish a permanently transferred
entry resource from a temporary loan. An immutable receipt row may name a
mandatory, non-repeated receive from a terminal owner, causally after its
handoff. Existing completion of that notice rules out waiting for an old
receipt; it does not close a roll or add a holder table. Direct and borrowed
calls reject before transport and leave an independent continuation usable.
Temporary departure remains Pending and resumes after the actual return.
The symbolic ownership-fold lemma proves the sink property under explicit
source-conformance assumptions; it is not a complete Rust refinement proof.
The ordinary Miri contract includes both actual Endpoint regressions.
Assignment 20 combines a resolver at a repeated entry, actual resource return,
and an incoming continuation after that repeated prefix. Its generated
localsides start the continuation's `offer()` before the peer sends and retain
that same Future across Pending until the input arrives. A cancelled one-poll
probe or a successful preview with an already queued frame does not cover this
schedule. Four contract variants and eight source scripts run in both poll
orders. The separate two-resource Endpoint regression also tests one, two and
four failed visits, with input queued before the offer and after it is Pending.
This extra availability assertion identifies the successful send through its
actual 16-bit committed source record, rather than the alias used to name the
public call. That result metadata is not supplied to the independent permission
checker.
When a typed choice entry precedes a parallel borrow, its cut belongs to that
actual entry handle. Its occurrence comes from the scheduled message leaf,
not the parallel group's canonical first row. Owned-payload queries use the
entry's current source scope rather than the later borrowed child.

The cut also records the materialized preview's actual scope and arm. An active
`RouteBranch` retains its exact source entry. An idle typed send reuses a parked
preview only while that entry remains enabled; otherwise it searches matching
operations with the held choice and its source ancestors. Other calls share
those choices without inventing knowledge of deeper choices. A nested send
cannot replace an uncommitted held parent decision by calling its resolver
again. Native regressions and the generated catalog compare these source rules
with actual Endpoint completions.
A receive preview does not imply that its packet arrived: it can name a
prospective repeated operation. Receive availability is checked against the
commit-derived fixture FIFO, whose earliest matching packet cannot be skipped.
Source candidate membership does not infer a buffered packet from preview
metadata. The recorded source 64 return-Endpoint continuation exercises this
distinction after two inner visits and a retained receive preview.

Offer candidate discovery includes a sealed forward index of repetition entries
for each logical view and lane. Entries remain discoverable after a containing
choice and its ordinary tail have completed. The index adds two flash bytes per
local row only in images with both routes and repetitions; it adds no mutable
progress or caller storage. Serialization uses cached innermost source spans.
The image constructor independently derives head membership from the compiled
source and checks every forward link. Native tests compare enumeration with a
raw all-scope/all-prior-row scan and reject every bit mutation of the indexed
topology. Ordinary operation admission still decides whether each candidate is
currently legal. `DescriptorTopology.lean` proves the innermost-scope reduction,
the immediate-predecessor interval test and forward traversal bounds; it does
not claim refinement of the complete Rust discovery loop.
The ordinary shared-contract assignment 17 requires independently validated
delayed-parent schedule witnesses in both poll orders. A header-only coverage
report cannot qualify that assignment.

Typed receive lookup and framed source lookup use the same binary partition loop.
For any sorted sealed column, `sorted_column_queries_are_prefixes` proves that
both key comparisons are prefix predicates and that their two cuts delimit
exactly the equal rows. Kani checks the actual Rust loop for every prefix and
every supported 16-bit row count, including bounds on each probe. The two
inbound-key harnesses compare production matching to the independently chosen
input tuples, including all wire labels and both payload variants.
For every row of a decoded image, Lean proves that a typed receive lookup
includes each matching alias in the borrowed view, source range and lanes.
Once a row is admitted by the source guards, its exact queued FIFO head
prevents the executable Lean oracle from returning an empty ready set.
`source_ready_survives_completed_row_filter` also shows that a source-ready
current or fresh row is unfinished or its checked image contains a roll. Thus
the prefilter that drops completed rows only when reentry is absent can retain
legitimate revisits without adding a special case for each nested roll.
Completeness of live Rust candidate admission through route choice, roll reentry,
and delegation remains a separate obligation.

To check public-call readiness and dynamic execution on the same observations, run
`check_protocol_shapes.sh --epf /path/to/hibana-epf --contracts FIRST COUNT`.
The runner executes the Endpoint population once. Lean checks the measured calls,
including refusals. EPF checks the recorded prefix and probe events for
completed, pending, and rejected calls. Each query names its certificate.
Identical histories within the same baseline share that certificate after
their individual checkpoints are checked. EPF certifies the observed history.
Lean checks whether the call could complete at that point with the fixture FIFO.
It returns every ready row with its source event and lane, independently of the
actual outcome. A successful call must commit one of those precise operations.
A refusal or a pending poll requires an empty ready set when the required
payload is available. The same ready-row check applies to the ordinary
whole-Endpoint calls, including calls made after dropping an offer. Each trial
records the actual source and lane beside its passive cut identifier, so
sharing one cut between different calls never shares their outcomes.
An unresolved witness fails qualification. The receipt records the EPF binary
digest, witnesses, captures, and certificates.
`--check --epf` verifies that evidence against the supplied executable.
The ordinary `contracts-39` assignment builds EPF against the same core. It
requires a matched receive in both physical schedules. At the same local call
and with the same nonempty Lean candidate set, one poll waits for its FIFO
head and the other commits the arrived frame. The other assignments use the
Lean cut check.
Extending the source-witness
proposals does not change which Endpoint scripts run.

Prefix existence is an existential search over the complete original witness
family. The driver uses the proved `shapeGroupAnswers_as_candidate_queries`
equality to stop at the first checked witness. The family, scripts, questions
and input histories are unchanged.

The oracle is compiled from its complete Lean/build inputs and platform, with
binary checksums verified on reuse. A source or codec change rechecks its new
binary and actual observations; previous population success is not promoted to
a changed checker without rechecking.
Population receipts compare their implementation, generator, model and tool
inputs, compiler and platform. They preserve the producer's revision instead of
invalidating identical content after staging or committing it. Documentation
and generated claim reports are not executable population inputs. Both input
manifests sort tracked and untracked paths together before hashing them.

The shrinker uses the same external-process result contract as syntax fuzz.
Preflight runs the actual reduction loop through that process boundary,
requiring the original failure class, strict source-size reduction and evidence
preservation; a compiler/process failure aborts rather than becoming a witness.
The real Endpoint/backend classification is checked by the four admitted-bit
controls and native regression expectations. The former whole-pipeline shrink
demo repeatedly compiled these same obligations and compared an image against
a deliberately different global. That alternate oracle and demo are removed.
Seeded fuzz still shrinks actual failures through the ordinary backend.

Each source runs in a fresh bounded worker, sequentially. The parent validates
and aggregates the complete source inventory. Observation controls mutate one
answer or history while preserving all others. Source-dependent schedule and
history controls remain mandatory.

Dedicated scripts, patches, cloned source trees and build caches for restoring
old defects are removed from normal and extended validation. Native regressions,
the complete generated domain and illegal-record rejection tests remain.
Generated runtime inputs compile as library tests of the canonical core,
reusing its private passive operation observer. Each input is compiled once;
template inputs are packed by emitted source bytes as well as case count, so
large probe bodies do not accumulate in one compiler process. Every case is
retained intact and the runner requires their complete executable inventory.
The runner verifies the root manifest, executable inventory, source hash and
binary hash before and after execution. It runs every generated source case
exactly once across that inventory, including both poll orders and every
assigned script interval. Each bounded worker drives the positive baselines
and scoped probes for its own interval. The first interval also drives the
whole-Endpoint prefix matrix once for the source. Assembly requires both poll
orders for every script, and Lean checks the assembled executions and the
complete prefix inventory. Ordinary native tests run in their separate required
gate, rather than being repeated for every generated input. A second build
only to enumerate the same executables is not needed. Actual borrow targets,
resident images, copied actor state and producer captures remain associated
with their measured operation; interval assembly rejects missing queries or
changed resident-image inventories. These passive values do not select or
authorize Endpoint operations. Successful populations discard their private
build targets, consumed admission questions and internal cut states, and
normalized copies of those questions and images. Their sources, script and
query inventories, binary hashes, complete successful execution traces,
resident images, source answers, checker verdicts, schedule witnesses,
negative-control rejection logs and hashes of retired inputs remain.
Recreating the retired measurements requires rerunning that population;
their digests are provenance, not retained raw measurements. Partition and
control copies are removed after their contents have been combined and checked.
Failed populations retain their debugging evidence.


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

Ordinary production qualification and qualification of a changed verification
tool are different claims. Preserve the producer revision and scope of each
successful result. A Git revision change alone does not invalidate evidence:
reuse requires the tested production inputs, specification, finite input domain,
compiler, features and execution conditions to remain the same. Record their
content identities and the original commands and results. Never turn a stopped
or incomplete run into a successful result.

Logging, output-directory isolation and removal of duplicate invocations do not
require repeating unchanged Rust, Miri, capacity or conformance obligations.
A changed driver, normalizer or checker requires validation of that change;
old results do not certify the new tool. When the production and previously
checked obligations are unaffected, keep that tool's outstanding qualification
separate instead of restarting production qualification. Changes to the model's
meaning or the declared public-input domain require rechecking affected
conformance obligations. Unrelated successful obligations remain valid.

Deep source exploration increases the tested domain; it does not establish
refinement of arbitrary Rust executions. Retain known-defect minimal regressions
in the ordinary contract, but do not make every deep sample, all Kani harnesses
and full Miri a fresh prerequisite for every unchanged-production consumer
update. Perform affected extended checks when their guarantees change, and the
complete extended contract when freezing the final paper or release artifact.

A development source qualified locally may be published for consumer work
without waiting for a duplicate public CI run. Its record must identify reused
production evidence and any still-incomplete tool or extended qualification.
This is neither a successful public `core-contract` result nor extended
certification; those result names retain their complete-job requirements.

The separate `extended-certification` workflow runs on `ci-certify-*` tags, and
explicit manual requests. It does not rerun unchanged Kani/proof populations
on a nightly timer; seeded syntax samples remain part of every certification
and can be requested with a new seed. A certification
tag selects an immutable source revision; it is not a release or a passed result.
Branch pushes run the ordinary workflow. Certification tags invoke that same
workflow exactly once, rather than also triggering a duplicate ordinary run.
A final paper/release candidate is published through its certification tag. Only after all
certification jobs succeed, the same run fast-forwards main to that exact SHA
and deletes branch tips already contained in it. Both operations are atomic;
each deletion requires the observed branch tip to remain unchanged. Divergent
or concurrently advanced branches are preserved. Only this post-success job
has content-write permission. Its repository `GITHUB_TOKEN` does not recursively
trigger another push workflow, so advancing main does not rerun the same checks.
Manual certification requests verify their selected source without publishing it.
Root runs share one concurrency group across
branches and tags; a newer run cancels the previous source run. The reusable
core-checks child has a run-specific group so that it cannot cancel its own caller.
It adds the existing deep interaction/spine regressions and seeded
depth-4-through-8 syntax samples. The 32 frontier protocols already belong to
the ordinary constructor population. Their exhaustive scripts also include
the former separate frontier selection's complete scripts and borrowed-scope
probes, including its compile-fail counterexamples. They are not run again as
eight additional jobs. A generator regression checks these equalities for all
32 sources; removing the duplicate selection changes no ordinary case.
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
The representative role-column budgets retain their original limits, including
512 bytes for the larger matrix entries. Immutable lookup data is reported
separately and bounded by the published layout: two bytes per local successor,
two per route-index slot, and, where routes exist, ten per local event plus
eight per local route. The complete role image remains reported and checked
against those two parts. Linked artifact and total Flash/SRAM budgets remain
independent and unchanged; lookup data is never omitted from linked footprints.
The library and projected Pico example use the same section-budget reader.
Runtime stack/SRAM measurements are collected once in the authoritative source
and checked once, including the fixed 64-byte modeled SRAM headroom and the
aggregate non-growth rule. No measurement checkout or second cold build repeats
these results. There is no environment option to omit the snapshot checks.
The runtime-performance job likewise leaves the four warm-target regression
tests in the native suite. It retains the independent cold-target compile budget
and the runtime measurements. An external package-consumer fixture depends on
the untouched extracted archive and the public runtime revision selected by
the repository manifest. It executes the shipped example files directly and
runs the delegated example's tests; it does not copy the example source or
rewrite the archive's manifest. Cargo omits unversioned Git dev-dependencies
from published manifests, so the executor belongs to this consumer. A separate
compile-only pass of those same examples is unnecessary.
The compile-pressure inputs include the original 100-role, 4,000-event mixed
and hierarchical protocols under forbidden long const evaluation. Batched
receive facts use the existing physical-participant index for each subtree;
unrelated global actors are not copied at every fork. General causal-flow
theorems cover absent-actor stability, input agreement and cancellation of
disjoint route seed masks. Bounded scalar/batch comparisons and the symbolic
participant iterator check connect those rules to the Rust implementation;
they are not a general Rust refinement theorem.
The 16 delayed-parent schedules execute once in `check_miri.sh`, including
each scenario's completion record and unique source-specific evidence. The
extended entry point calls that gate and does not repeat the schedules.
The required Miri boundary checks and the additional Miri checks are disjoint
selections of the same script; its default still runs both. Overlapping broad
and individual test filters have one execution site, including the parked-view,
actor-progress, image, tap and delegated-offer regressions. Native and Miri
executions remain distinct evidence.
The standalone ownership elaborator uses its own test configuration in both
native and Miri runs and rejects the core-only fixture configuration at compile
time. The 460,800 mixed-shape dependency comparisons remain in the core native
suite; they are not accidentally imported into this separate crate's Miri run.

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

The full extended result remains required when fixing a paper/release
artifact. It is not placed on every development push's critical path. A required
CI success must never be reported as successful extended certification. After a
core change, run the required contract locally on fixed source as well; perform
extended checks for the final artifact and for changes to the properties they
specifically cover. Preserve failing inputs and verification stops.

## Development snapshot: 2026-11-12

The `progress-20261112` tag publishes the current development source, not a
release or a claim that the complete ordinary/extended gates passed. The public
`main` stays at its existing revision until the required integration succeeds.
The latest recorded focused qualification includes native library checks,
Clippy, no_std/embedded builds, offer-boundary Miri and one generated shared-
contract assignment. The complete normal generated population and all gate
aggregation are still unfinished. Five inputs changed after that focused
source manifest, including the parallel admission implementation and source
checker; its older results do not certify those changes.

The source-bound records are retained in
`output/verification/ordinary-offer-v1/qualification.json` and
`current-population-source.sha256`. Publishing this checkpoint does not replace
local validation or turn unexecuted assignments into successes.
