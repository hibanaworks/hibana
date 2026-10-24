# Verification

Hibana uses Lean models, SMT checks, bounded Rust verification, and executable
tests to check protocol projection and execution. The models state assumptions
about codecs, transports and participant behavior. They do not verify arbitrary
Rust programs or arbitrary implementations of `Transport`.

## Layout

| Directory | Contents |
| --- | --- |
| [lean](lean/README.md) | Protocol, ownership, runtime and transport models in one Lean package |
| [lean/claims](lean/claims) | The theorem inventories checked by the Lean gate |
| [checks/projection](checks/README.md#projection) | Source ownership, branch knowledge, dependencies, lane allocation and message identity |
| [checks/image](checks/README.md#protocol-images) | Packed descriptor fields, lookup and bounds |
| [checks/runtime](checks/README.md#local-execution) | Operation admission, choice, parallel work and repetition |
| [kani](kani/README.md) | Reviewed inventory and entry point for production Kani harnesses |
| [carriers/unix](carriers/unix/README.md) | A concrete Unix datagram transport and conformance tests |

The [check index](checks/README.md) links each group to the implementation it
models. Shared definitions live in `lean/Hibana`. Individual checks import
those definitions rather than keeping copies. Test fixtures are inputs to
reproducible checks, not reports of a previous successful run.

## What is established

The protocol models prove session fidelity, affine ownership and progress
under the assumptions in their theorem statements. The occurrence model
handles finite unfoldings with choices, parallel work, independent repetition
and Endpoint transfers. Its ownership and progress results use the same
source, carrier and custody history.

Transport assumptions include peer and direction binding, FIFO delivery,
nonduplication, replay exclusion, separation of session runs and receive access
after an Endpoint changes holder. A deadline or I/O error does not prove that a
frame was undelivered, and cannot restore a transferred Endpoint.

Rust/model conformance tests compare permitted and rejected operations over
bounded schedules, including separate transport acceptance and sender
completion, close and abort. Mutation checks establish that the comparisons
can detect incorrect ownership, unwanted serialization and lost dependencies.
This is executable correspondence testing, not a complete Rust refinement proof.

The syntax-depth catalog generates trees from individual messages, `seq`,
`par`, `route`, resolved `route` and `roll`. Both children of every binary
constructor independently range over the grammar. A message has depth zero;
each constructor adds one to its deepest child. A resolved route counts as one
constructor. Leaves use roles 0 through 3 and either unit or `Endpoint<3>` as
payload. Labels and resolver IDs are assigned in source order. Self messages
and invalid Endpoint carriers are included. These payload and label choices
are the catalog's test domain, not coverage of every possible Rust payload or
of several independently delegated resources.

```sh
bash .github/scripts/check_protocol_shapes.sh --syntax-depth DEPTH FIRST COUNT
```

`FIRST` is an arbitrary-precision rank in the exact-depth population. The
generator counts subtrees and reads only the requested range. Each accepted
candidate has a separate Rust test target, uses public projection, and runs
generated localsides with actual parallel borrows. Known invalid candidates
are built separately and must fail for their source-derived reason. Unknown
projection rejections fail the check rather than count as expected failures.
The source ownership check considers every possible initial owner, every choice
arm and each transfer. A repetition must return the endpoint to its incoming
owner. A parallel transfer must not overlap a use of the same resource.
The output records the population, range and per-case identities. Completing
one range does not establish coverage of the whole depth. Finite execution
tests use two visits per repetition. Every parallel node polls both children
with Tokio, varying which child is polled first.

The source-representative catalog supports depths zero through two:

```sh
bash .github/scripts/check_protocol_shapes.sh --representatives DEPTH FIRST COUNT
```

`FIRST` is an ordinal in the role-normalized catalog. Each case also records its
raw syntax rank. Construction checks ownership, choice notifications and
resource-dependent repetition before invoking Hibana. A child is tested with
role 3 as an ordinary participant and with each possible resource holder.
For example, after a transfer to role 1, a choice between receives at roles 1
and 3 can be observed by the same participant. Pruning that child by its
standalone verdict would lose a legal protocol.
Choice knowledge is kept separate across parallel children until their join.
The parent is checked again after composition. This source predicate defines
the catalog; it is not a proof of the Rust compiler. Every selected case still
uses public projection, real Endpoints and the independent Lean checker.
An unexpected rejection fails the check. Raw syntax testing retains invalid
cases and their separate compile-fail builds.

Resolver probes use the decisions on every source path, plus each input with
one resolver changed. Sequential and parallel children combine their inputs;
a choice keeps only its selected arm's inputs. Records list the resolver IDs
that select the right arm, without a fixed-width bitmask. Lean reconstructs
this input set from the source and rejects missing or unexpected records.

Seeded syntax sampling supports depths zero through eight. Build the tools,
then run the search binary directly; each probe invokes Cargo itself:

```sh
bash .github/scripts/lib/resource_guard.sh \
  cargo +stable build --locked --manifest-path .github/ci-tests/Cargo.toml \
  --bin protocol-shapes --bin protocol-shape-controls
bash .github/scripts/lib/resource_guard.sh \
  target/debug/protocol-shapes syntax-fuzz "$PWD" 8 42 4 target/fuzz-example
```

Run this under the resource guard. The result separates executed protocols
from expected compile failures. `samples.tsv` retains each full rank and seed.
Large ranks use several directory components, and each generated package has
its own target directory. A failed check stops sampling and retains its logs.
For a recognized semantic mismatch, the tool tries removing subtrees and
repeats the complete probe. It accepts a reduction only if the same mismatch
remains, then verifies the final case again. `best.tsv` identifies the result;
the trial files retain every attempted source, command and output.
The result is minimal under subtree removal, rather than a claim about every
equivalent protocol. Unknown tool failures retain the source for inspection.

The scheduled CI job samples depths four through eight using the UTC date as
its seed. It also checks two fixed mixed protocols and a shrinker control.
That control runs real Endpoints, then deliberately removes a sequential
dependency from the comparison model while keeping the probe inventory.
It must reduce the resulting mismatch to two ordered messages.

An existing failure can also be reduced explicitly:

```sh
bash .github/scripts/lib/resource_guard.sh \
  target/debug/protocol-shapes syntax-shrink "$PWD" DEPTH RANK target/reduced \
  -- target/debug/protocol-shapes syntax-probe
```

The interaction catalog constructs nested contexts around a resource exchange:

```sh
bash .github/scripts/check_protocol_shapes.sh --interactions LENGTH FIRST COUNT
```

Its nine contexts put a send or receive before or after the nested protocol,
place it on either side of a parallel composition, make a message-selected or
resolved choice, or repeat it. Both parallel children are compound protocols.
Each context word has two variants: a resource exchange and an additional
Endpoint round trip. Their independent parallel children use a sequential
exchange and a parallel send with repetition, respectively. The population
is `2 * 9^LENGTH`, through length five.
Length counts contexts, while `case-identities.tsv` records actual syntax
depth. Choices notify the other owner when its branch needs that participant.
If the service could start another visit after the resource has been returned,
the resource holder sends permission inside each visit. The original form
without that permission is retained as an independent compile-fail case.
Unexpected projection rejection remains a failed test. Each accepted case uses the same
public Endpoint driver and independent source checker as the syntax catalog.

To exercise constructor order without cloning every choice subtree, use:

```sh
bash .github/scripts/check_protocol_shapes.sh --spines LENGTH FIRST COUNT
```

The nested path uses `par.left`, `par.right`, `route.left`, `route.right`,
`resolved.left`, `resolved.right`, and `roll.body`. Each word has a resource
exchange and an additional Endpoint round-trip variant, for `2 * 7^LENGTH`
sources through length five. A choice has the nested protocol in the named
arm and a shorter protocol in the other arm. Both arms notify their participants.
The choosing view receives a message before the choice. Parallel siblings
contain independent compound work. The original catalog with two deep choice
arms remains a separate test population.

`source-coverage.tsv` records constructor words and their locations in the
generated source. `syntax` keeps all nine constructor edges, including `seq`.
`nested-sides` omits intervening `seq` nodes and keeps seven edges.
`nested-operators` also omits left/right distinctions and has four operators.
`delegated-sides` and `delegated-operators` restrict these words to paths
leading to a resource operation or transfer in a source containing delegation.
Words have length one through five. The corresponding counts are counts of
distinct words, not protocols, ownership patterns, or operation schedules.

Only after Endpoint execution, source comparison, mutation controls, and input
hash checks succeed does the script publish `executed-coverage.tsv` and
`executed-coverage-counts.tsv`. To list nested side words still missing from
successful runs:

```sh
target/debug/protocol-shapes coverage-uncovered 5 target/missing.tsv \
  target/protocol-shapes/spines-5/range-FIRST-count-COUNT
```

The output gives each missing delegated-side word and a source rank for `--spines`. Omit run
directories to make an initial plan. An unexecuted source inventory cannot
remove a target. Covering these words does not establish all syntax trees or
all interleavings at that depth.

`protocol-shapes syntax-counts DEPTH` also reports the number of syntax trees
modulo role renaming. It fixes the resource role when delegation occurs and
preserves labels, constructor positions and resolver identities. These counts
include invalid syntax; they are not counts of accepted protocols.

The operator-budget catalog provides a separate regression population. It
builds protocol trees around an Endpoint loan and
return. It combines sequential prefixes, parallel work, choices with and without
a resolver, and repetition. Choice arms have independently generated subtrees.
A second family places compound resource and ordinary-message protocols in
parallel, in both source orders. Such a pair also replaces each loan leaf of
the generated sequential, parallel, choice and repetition contexts. Prefixes
include a message received by the choosing participant before its choice.
The binary family constructs both children of `seq`, `par` and `route`
recursively. Within each parallel pair, only one side lends the resource;
the other side contains ordinary request/reply exchanges. Both source orders
are generated. A request followed by its reply can precede any subtree, so
the choosing participant can depend on a receive rather than its own send.
The same binary grammar also generates ordinary three-role protocols without
an Endpoint loan. These runs attach no resource endpoint or initial binding.
The delegated-body family puts `seq`, `par`, choices, resolvers and repetition
inside the received resource role. It also passes that endpoint onward and
back. Both parallel children can have compound local subtrees. An onward
transfer can surround parallel work, but a child cannot move its borrowed
parent. Choices notify the service and the other possible holder explicitly.
Swapping participants zero and one
exercises both controllers. Ordinary leaves include one message and a complete
request/reply exchange. The binary and delegated-body families also mix exchanges
with sends from different roles to one receiver. In an ordinary leaf, a
participant tells a worker to start, then both send to a collector. In a
delegated leaf, the resource role and its holder both send to the service
before its reply. These three-message leaves can occur alongside request/reply
leaves in either child of any constructor. The collector is attached whenever
the source mentions it, even when the resource is served by the initial owner.
The default operator budget is two. Larger budgets enumerate the same grammar. This
budget counts generated constructors across both choice arms and the containing
context, rather than binary-tree depth. The fixed outer loan and ordinary
exchange leaves contain four messages; a delegated-body exchange has two.
Their internal constructors are not counted.
The budget does not establish exhaustive coverage at a given syntax depth.
`case-depths.tsv` records each expanded protocol's depth and operator count,
including the constructors inside these leaves. A message has depth zero;
each `seq`, `par`, `route` or `roll` adds one to its deepest child. A resolved
route has the same depth as a message-based route. The inventory reports the
largest depth in the catalog and in the emitted runtime inputs separately.
Results are stored under `target/protocol-shapes/operator-budget-BUDGET/`.
The `protocol-shapes` workflow accepts budgets three, four and five. Budget
three uses window zero. Budget four requires all 32 windows, zero through 31,
covering 32,768 shards. Budget five uses sixteen windows, zero through fifteen.
A Rust check assigns each shard to exactly one job and
window and rejects invalid scopes. One successful window does not establish
complete coverage. The workflow saves each job's assigned shard list with its
source hashes, inputs and observations for thirty days.
Projection diagnostics run before endpoint code is compiled. Accepted cases
continue through execution and model comparison even when another case is
rejected. `catalog.json` retains every generated tree, and `diagnostics.tsv`
records every result. An unclassified rejection fails the overall check.
It is not counted as a successful negative test or removed from the catalog.
Invalid resource-dependent reentry and choices where the collector participates
in only one arm are derived from the source and checked by a separate build. Each
root choice between two single messages also checks the choosing role and which
participants learn its choice. Ownership is solved first, including a resource
view's initial holder. Several violations or affected participants can coexist;
the reported rule and participant must match one of these source witnesses.
This rule does not classify compound choice arms, where earlier messages may
provide the missing knowledge.
Each
delegated-body candidate also produces a parallel transfer/use conflict. Every
such case is built independently and must fail with the expected `E0080`
diagnostic rule, affected role and help. Source locations may add detail to that
diagnostic. A different compiler error or successful build fails the check.
Rejected protocols are never run as if they were legal executions.

Each `roll` runs twice, including nested repetition. Choices and parallel-child
order vary independently between visits. Receivers use `offer`, drop and retake
the preview before continuing. Prefix probes also consume previews through
`RouteBranch::recv`.
Parallel groups run through `Endpoint::par` with concurrently polled Tokio
futures. A separate test delays two senders and checks that two real receives
on the same endpoint can remain pending together and then complete. Operations
that finish immediately need not produce a pending I/O observation. Endpoints
move between separate runtimes through the public API.
Every case generates direct local Rust code. Every
nested parallel composition uses public `par()` borrows and Tokio futures.
Each source execution retains its branch choices, parallel-child order and
two visits of every `roll`. The generated localsides keep the corresponding
borrows until each visit completes. Both poll orders use concurrent Tokio
futures, including different choices in later visits.
When a choice transfers an endpoint in only one arm, the continuation uses the
endpoint value actually received. No separate owner flag selects its operations.
Tests read source-derived execution schedules one record at a time from JSON
Lines and check that every planned record was consumed.
Borrowed-scope plans and observations also use JSON Lines. Each record covers
one source execution; the checker requires both schedules and every planned
probe before advancing to the next record.
Distinct visit boundaries remain separate inputs when their message words
are equal. Completed source plans are checked against the catalog. Their poll
preferences do not fix the order of completed calls. That actual order is
recorded and checked separately. A flat word alone does not specify which
repeated group has ended.
The observer records successful sends and receives in completion order, with
the participant, endpoint role, message label and direction. It reads the role
from the actual endpoint type. Lean replays each record against the source
occurrence model with two visits at every repetition site, including nested
repetition. Every source operation must complete exactly once. Script counts
and ranks come from the source tree and check for missing or duplicate plans.
Controls change the participant or role, reorder a receive before its send,
and omit or duplicate a call; each must fail the comparison.
The observer also records pending branch previews, so a waiting `offer` counts
towards concurrent work. Observation never chooses the next operation or owner.

Generated localsides use optimization level 1 and Hibana uses level 2, with
debug assertions and overflow checks enabled. Tests check both settings and
the schedule's message order. Generated
test binaries contain at most 32 cases to bound rustc memory; all binaries
are built and run.
The Lean checker computes each prefix
once for adjacent probes, while retaining every probe in its original order.

Every message is also tried as the first send and receive, for every resolver
assignment. A compiled Lean checker compares these observations with the existing
source occurrence semantics. The expected initial operations come from that
model, rather than the first textual message: an uninvolved sender may queue
a later frame before the initial loan.

For trees without `roll`, every prefix of the generated complete traces also
probes every send and receive, including already completed and unselected
operations. Prefixes include the cut between a send and its matching receive,
and continue through choices, delegation and the final return. Each probe uses
a fresh runtime that replays the prefix first. Lean derives the required prefix
inventory from the source tree and advances its occurrence model through those
operations. These trees also exhaust every permitted order of the first four
public operations, probing every next operation at those cuts. Missing or
duplicate probes fail the comparison. Separate controls
change a continuation result and omit or duplicate a continuation probe.
A single outer `roll` with a finite body probes every prefix of its two-visit
traces. The source model has three copies, allowing the next probe to start
another visit. Previously observed choices stay fixed; fresh choices try every
resolver assignment. These probes include the cut after a send and before its
matching receive. More complex repetitions still use complete execution traces
and initial admission checks. At every nested parallel borrow, including later
visits, additional probes try each ordinary message outside the borrowed
fragment on that fragment's endpoint. The source tree determines scope
membership; each visit has a separate occurrence number. Every probe starts
a fresh run, executes the actual prefix with its choices and borrows, and must
return an error immediately. The check compares the planned and observed
probe inventories and rejects missing, duplicate or accepted probes. Controls
verify those failures. Probe plans are read from generated JSON at runtime;
the generated Rust contains the protocol operations and visit structure.
These checks cover scope exclusion at borrow entry,
not every operation within a fragment or every asynchronous cut.
Further trials try a send immediately before a necessary receive on the same
logical endpoint, retaining the real execution prefix and parallel borrows.
Each trial starts a fresh session. A source-level sequential dependency must
require that receive on every relevant choice path, within the same roll
scopes; message labels must identify unique source sites. These restrictions
exclude independent siblings, optional receives and retained messages from
earlier visits. Lean checks each dependency against the original source tree.
A control replaces it with a self-dependency and must be rejected even when
every recorded runtime outcome remains unchanged.
Other later negative probes require
continuation-aware occurrence unfoldings; a one-copy bound cannot determine
whether another visit is permitted.

Complete traces do not enumerate every schedule. Run
`bash .github/scripts/check_protocol_shapes.sh [BUDGET [SHARD SHARDS]]` under the
resource guard. The output records the compiler, source revision, source and
input hashes, and separate compilation, execution and comparison times. Source
hashes are checked again after execution. The generated families use distinct
message labels and one resource role. Initial admission and the finite-prefix
probes are compared with Lean. Complete execution records are also compared
with Lean, using both parallel-child orders and concurrent Tokio futures.
These records do not enumerate every interleaving of nested parallel operations
or split transport acceptance from send completion.
The binary family allows multiple compound parallel pairs in one tree. Its
operator budget applies to the whole tree, not separately to each child.
Multiple resources and reused labels remain outside this grammar. A generated catalog is a test domain,
not evidence that all its cases passed.

Receive lane allocation prefers existing colors, then uses the remaining
wire-lane domain when necessary. It can map a whole choice arm or separate one
conflicting occurrence inside or outside a choice. Sequential operations on different lanes
retain their source dependencies. `GlobalAllocationCorrectness` proves that an
injective matching introduces no incompatible collision when its cross-boundary
constraints are satisfied. Native tests compare pairwise receive constraints
with the bulk causal check and an independent schedule analysis, both before
and after lane alignment. The theorem does not prove equivalence
of those Rust checks or completeness of the lane search. Public endpoint tests
cover differing parallel-child orders, delayed notices and retained previews;
Miri checks their buffer and borrow use.

The nested-roll check uses `(Prefix; (Exchange+ | Tick)+)+`, where `|` is
parallel composition. It enumerates prefixes of at most seven complete exchanges
or ticks using accepted source unfoldings with at most twenty message occurrences.
For every permitted next send, it retains every distinct parallel-group boundary
in those unfoldings and drives public `par`/`send`/`recv` operations with Tokio.
Forbidden sends are also tried; the parent cannot be used while its children are
borrowed, as checked separately by compile-fail tests. A flat-call check rejects
any runtime admission outside the source language. It does not require a call at
one concrete cut to admit alternatives from a different grouping of the same
message prefix. Lean examples demonstrate why those cuts differ.
The check regenerates its complete query inventories and tests missing,
duplicated, incorrect and malformed inputs. The enumeration covers complete
exchanges and ticks. Separate public tests keep an acknowledgment receive
pending while the independent tick completes, and reject a new outer prefix
that would discard the pending exchange. A second source family,
`(LeftExchange | RightExchange+)+`, checks that earlier completed child
iterations cannot substitute for the unfinished exchange at a later parent
join. The retained completion is a single consumable credit. Repeated child
visits do not accumulate credits for later parent visits. Run
`bash .github/scripts/check_nested_roll_admission.sh` under the resource guard.
The command fails if either the runtime or the source comparison fails.

The same command checks `(Prefix; Exchange+; Tail)+`, with ordinary messages
and with an endpoint loan, resource request, response and return. Its source
unfoldings include inner repetitions before, after and alongside the tail.
For every reachable prefix of at most seven complete operations, public
endpoints try all three next sends. Each attempt starts a fresh run. This
checks late inner resumption and fresh outer entry, including rejected sends.
These cases complete each exchange before the next probe; they do not enumerate
partially polled sends or every nested parallel borrow.

Repeat-entry checks compare the chosen scope with the operation's projected
predecessors. They cover a child continuing after its tail, a prefix on another
lane, and a fresh parent that must execute its own prefix. Source-model examples,
public endpoint tests and independently compiled Kani cases cover these boundaries.

Offer checks cover choices inside nested parallel branches and receives left
enabled after independent work completes. Scope lookup retains the enclosing
choice. Moving the lookup position preserves completion and branch selection.
Public endpoint tests, source-model examples and Kani checks cover these
receives, rejected alternatives and attempts to execute a completed step again.

Kani checks bounded properties of production functions. Miri checks aliasing,
pointer provenance and destruction in executed tests. Native tests exercise
actual Endpoints, codecs, borrowed buffers and separate-process communication.
The Unix and TCP fixtures do not establish authentication or forwarding
correctness for other deployments.

The TCP peer fixture also places inboxes in the participant processes, without
a central broker. A remote holder reads bytes queued at the old peer and returns
the Endpoint before exiting. Logical inbox addresses stay fixed; this does not
qualify dynamic address migration or recovery after loss of the inbox host.

## Run the checks

Use the latest stable Rust for ordinary builds. Lean is pinned by
[`lean-toolchain`](lean/lean-toolchain). Miri and Kani use the compilers required
by the versions pinned in `.github/`.

From the repository root:

```sh
TOOLCHAIN=stable bash .github/scripts/lib/resource_guard.sh \
  bash .github/scripts/check_lean_proofs.sh
bash .github/scripts/check_kani.sh
TOOLCHAIN=stable bash .github/scripts/lib/resource_guard.sh \
  bash .github/scripts/check_miri.sh
```

Run these commands sequentially. The guard limits the whole child process tree
to 3,000,000,000 bytes. The Kani entry point already invokes it.

The [CI workflow](../.github/workflows/quality-gates.yml) also runs the individual
checks, transport tests, embedded builds, size measurements and instruction
budgets. Generated Lean inputs and logs go to build or temporary directories.
The checked theorem inventories reject missing or extra claims and forbidden
axioms. Common model theorems and kernel-only generated claims permit only `propext`
and `Quot.sound`. Finite regression examples and explicitly classified
acceptance certificates use `native_decide`; they also trust Lean's compiler
and runtime. The compiled schedule oracle is an executable test. The
[Lean audit description](lean/README.md#run-and-audit) identifies these boundaries.
The separate implementation checks under [`checks/`](checks/README.md) print
their own dependencies. Some use Lean's `Classical.choice`, such as the
[lowest-bit proof](checks/runtime/pending-events/Scan.lean).
