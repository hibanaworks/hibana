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
| [checks/projection](checks/README.md#projection) | Source ownership, branch knowledge, dependencies and frame allocation |
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

The generated-shape check builds protocol trees around an Endpoint loan and
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
request/reply exchange. The default operator budget is two; a budget of
five enumerates 2,946,142 three- and four-role candidate cases in these families. This
budget counts generated constructors across both choice arms and the containing
context, rather than binary-tree depth. The fixed outer loan and ordinary
exchange leaves contain four messages; a delegated-body exchange has two.
Their internal constructors are not counted.
The `protocol-shapes` workflow accepts budgets three, four and five. Budgets
three and four use window zero. Budget five requires all sixteen windows,
zero through fifteen. A Rust check assigns each shard to exactly one job and
window and rejects invalid scopes. One successful window does not establish
complete coverage. The workflow saves each job's assigned shard list with its
source hashes, inputs and observations for thirty days.
Projection diagnostics run before endpoint code is compiled. Accepted cases
continue through execution and model comparison even when another case is
rejected. `catalog.json` retains every generated tree, and `diagnostics.tsv`
records every result. An unclassified rejection fails the overall check.
It is not counted as a successful negative test or removed from the catalog.
Known invalid resource-dependent reentry is checked by a separate build. Each
delegated-body candidate also produces a parallel transfer/use conflict. Every
such case is built independently and must fail with the expected `E0080`
diagnostic. A different compiler error or successful build fails the check.
Rejected protocols are never run as if they were legal executions.

Each `roll` runs twice, including nested repetition. Choices and parallel-child
order vary independently between visits. Receivers use `offer`, drop and retake
the preview before continuing. Prefix probes also consume previews through
`RouteBranch::recv`.
Root parallel groups also run through `Endpoint::par` with concurrently polled
Tokio futures. Each such run checks that independent borrows of one participant
were pending together. Endpoints move between separate runtimes through the
public API.
Every case generates direct local Rust code. Every
nested parallel composition uses public `par()` borrows and Tokio futures.
Each source execution retains its branch choices, parallel-child order and
two visits of every `roll`. The generated localsides keep the corresponding
borrows until each visit completes. These executions run in both child orders
and with concurrent Tokio futures, including different choices in later visits.
Distinct visit boundaries remain separate inputs when their message words
are equal. The complete set of executed words is checked against the trace
inventory. A flat word alone does not specify which repeated group has ended.
The observer counts successful sends and receives and compares their complete
multiset with the source execution. It also observes pending branch previews,
so a waiting `offer` counts towards concurrent work. Observation never chooses
the next operation or endpoint owner.

Generated localsides use optimization level 1 and Hibana uses level 2, with
debug assertions and overflow checks enabled. Tests check both settings and
the schedule's message order. Generated
test binaries contain at most 128 cases to bound rustc memory; all binaries
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
Other later negative probes require
continuation-aware occurrence unfoldings; a one-copy bound cannot determine
whether another visit is permitted.

Complete traces do not enumerate every schedule. Run
`bash .github/scripts/check_protocol_shapes.sh [BUDGET [SHARD SHARDS]]` under the
resource guard. The output records the compiler, source revision, source and
input hashes, and separate compilation, execution and comparison times. Source
hashes are checked again after execution. The generated families use distinct
message labels and one resource role. Initial admission and the finite-prefix
probes are compared with Lean. Other traces check successful execution, with
both parallel-child orders and concurrent root groups; they do not enumerate
every interleaving of nested parallel operations or I/O polls.
The binary family allows multiple compound parallel pairs in one tree. Its
operator budget applies to the whole tree, not separately to each child.
Multiple resources and reused labels remain outside this grammar. A generated catalog is a test domain,
not evidence that all its cases passed.

Choice-arm lane alignment prefers existing colors, then uses the remaining
wire-lane domain when necessary. It can map a whole arm or separate one
conflicting occurrence within an arm. Sequential operations on different lanes
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
duplicated, incorrect and malformed inputs. It covers complete exchanges and
ticks, rather than partially polled operations. Run
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
