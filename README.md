<div align="center">
  <img src="https://raw.githubusercontent.com/hibanaworks/hibana/v0.10.0/.github/assets/hibana-header.svg" width="600" alt="HIBANA — Choreographic Programming in Rust" />

  <p>
    <a href="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml"><img src="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml/badge.svg" alt="Quality gates" /></a>
    <a href="https://crates.io/crates/hibana"><img src="https://img.shields.io/crates/v/hibana.svg" alt="crates.io" /></a>
    <a href="https://docs.rs/hibana"><img src="https://docs.rs/hibana/badge.svg" alt="docs.rs" /></a>
    <a href="LICENSE-APACHE"><img src="https://img.shields.io/crates/l/hibana.svg" alt="Apache-2.0 OR MIT" /></a>
  </p>
</div>

# Hibana — Choreographic Programming in Rust

You write a global protocol and each participant's Rust code. Hibana projects
the protocol and checks each send and receive at runtime.

[日本語](README.ja.md)

The global protocol describes who sends what to whom, and in what order.
Participants can be tasks in one program or programs on different machines.

The core uses `no_std`, requires no heap allocator, and runs on stable Rust 1.95
or newer. The runtime uses memory supplied by your program.

[hibana-quic](https://github.com/hibanaworks/hibana-quic) uses Hibana to implement
QUIC v1/v2 and HTTP/3. It uses the same approach for communication within a
connection and for application messages sent over QUIC.

## Add to your project

Add Hibana to your project:

```sh
cargo add hibana@0.10.0
```

## A small exchange

A client sends a number. The server adds one and sends it back. The client must
send before receiving the reply. The server must receive before replying.

Write that order once:

```rust
use hibana::g;

const CLIENT: u8 = 0;
const SERVER: u8 = 1;
type Request = g::Msg<1, u32>;
type Reply = g::Msg<2, u32>;

let exchange = g::seq(
    g::send::<CLIENT, SERVER, Request>(),
    g::send::<SERVER, CLIENT, Reply>(),
);
```

`Request` and `Reply` both carry a `u32`. Their labels, `1` and `2`, distinguish
the message kinds. `g::seq` puts the request before the reply.

Each participant gets an endpoint, a handle for its part of the protocol. The
client sends `7` and waits for the answer:

```rust
client.send::<Request>(&7).await?;
let answer = client.recv::<Reply>().await?;
```

The server receives the number and replies:

```rust
let number = server.recv::<Request>().await?;
server.send::<Reply>(&(number + 1)).await?;
```

These are excerpts. The
[complete example](https://github.com/hibanaworks/hibana/blob/main/examples/ping_pong.rs)
also supplies memory, sets up an in-memory transport, and attaches both
endpoints. Libraries such as hibana-quic perform this setup for their
application endpoints.

Calling `client.recv::<Reply>()` before sending `Request` compiles as Rust, but
Hibana returns an error at runtime because the receive is out of order. Hibana
also checks the message kind and payload format before completing an operation.

## Try it

Clone the repository and run the example:

```sh
git clone --branch v0.10.0 https://github.com/hibanaworks/hibana.git
cd hibana
cargo +stable run --locked --example ping_pong
```

It prints:

```text
ping=7, pong=8
```

Protocols can also describe choices, independent communication in parallel,
and repetition. Participant code uses the same `send`, `recv`, and `offer`
operations.

## Passing an endpoint

A message can carry an `Endpoint`. Sending it moves
the right to use that logical role. The receiver continues the same protocol
at its current position.

The endpoint stays in the same session run. The sender and receiver are roles
named in the global protocol. To choose among workers at runtime, put those
alternatives in a `g::route`. Passing a channel from an unrelated session is
not supported.

For example, `OWNER` lends its `DB` endpoint to `WORKER`. The worker sends a
request as `DB`, receives the service's reply, and returns the endpoint:

```rust
use hibana::{g, Endpoint};

const OWNER: u8 = 0;
const WORKER: u8 = 1;
const DB: u8 = 2;
const SERVICE: u8 = 3;

type Loan<'r> = g::Msg<1, Endpoint<'r, DB>>;
type Read = g::Msg<2, u32>;
type Value = g::Msg<3, u32>;

let query = g::seq(g::send::<DB, SERVICE, Read>(), g::send::<SERVICE, DB, Value>());
let global = g::seq(
    g::send::<OWNER, WORKER, Loan<'_>>(),
    g::seq(query, g::seq(g::send::<WORKER, OWNER, Loan<'_>>(), query)),
);
```

The owner's code moves the value into `send` and receives it back:

```rust
owner.send::<Loan<'_>>(db).await?;
let mut db = owner.recv::<Loan<'_>>().await?;
db.send::<Read>(&41).await?;
let answer = db.recv::<Value>().await?;
```

The worker uses the same `send` and `recv` operations:

```rust
let mut db = worker.recv::<Loan<'_>>().await?;
db.send::<Read>(&7).await?;
let answer = db.recv::<Value>().await?;
worker.send::<Loan<'_>>(db).await?;
```

Rust prevents use of the moved value. Hibana checks the transfer, the holder
and the continuation against the global protocol. The resource integration
supplies one initial `EndpointBinding`; receivers do not create another one.
Sending an endpoint does not send its Rust address.

Dropping an unfinished owned-send future, including when a timeout in `tokio::select!` drops
it, abandons the endpoint and faults the local run. It does not return the
endpoint to the sender. Put a recoverable timeout decision in the protocol.

The [complete Tokio example](examples/endpoint_delegation.rs) also uses `g::par`
and `db.par()` to send an audit record while a query waits for its reply.
It supplies separate runtimes and a shared in-memory transport. Run it in this
checkout with `cargo run --locked --example endpoint_delegation`.

## Using it over QUIC

The [hello-quic example](https://github.com/hibanaworks/hibana-quic/tree/main/examples/hello-quic)
has a client send numbers over a QUIC stream and a server return their squares.
Its [global protocol](https://github.com/hibanaworks/hibana-quic/blob/main/examples/hello-quic/global.rs)
defines both sides of the exchange. The client and server each use an endpoint
to run their part.

Hibana also describes communication within hibana-quic. Its
[QUIC protocol](https://github.com/hibanaworks/hibana-quic/blob/main/src/quic/global.rs)
defines how packet processing, TLS, transmission, and timers communicate. The
Rust functions implementing those roles use endpoints to follow the protocol.
UDP I/O and cryptographic operations are implemented in hibana-quic and
hibana-tls.

hibana-quic is experimental. The core protocol proofs do not prove the
correctness of its QUIC or TLS implementation.

## How Hibana works

### Global and local protocols

In multiparty session types (MPST), a **global protocol** describes communication
among all participants. A **role** is a participant's logical identity in that
protocol. In Hibana, a participant can hold several roles through their endpoints.
**Endpoint projection** derives a **local protocol** for each role.

Many choreographic programming languages derive each participant's implementation
from the choreography, as in
[Carbone and Montesi (POPL 2013)](https://doi.org/10.1145/2429069.2429101).
Montesi's [Endpoint Projection chapter](https://www.cambridge.org/core/books/introduction-to-choreographies/endpoint-projection/15B92B24200EC1EFC1B70DFFDDFF9546)
describes this compilation. Hibana uses MPST-based projection to derive each
role's local protocol and checks handwritten participant code against it at runtime.

For the exchange above, projection gives:

| Global communication | Client's local protocol | Server's local protocol |
| --- | --- | --- |
| Client sends `Request` to Server | Send `Request` | Receive `Request` |
| Server sends `Reply` to Client | Receive `Reply` | Send `Reply` |

A role gets no send or receive for a message it does not participate in.
Sequential and parallel composition determine the order and dependencies of
its remaining operations.

Hibana began with
[affine multiparty session types (AMPST)](https://arxiv.org/abs/2204.13464).
Here, *affine ownership* means that an endpoint cannot be copied and may be
moved or dropped before the protocol ends. Each step can complete at most once
per repetition. The same endpoint can execute successive steps.

AMPST includes propagation of cancellation to other participants. Hibana records
faults locally and relies on the transport to make closure observable to peers.
The paper's cancellation-termination result does not automatically apply to a
Hibana transport.

The AMPST implementation, MultiCrusty, represents local session types as Rust
types. As Hibana's protocols grew, representing every remaining communication
step in endpoint types became too complex. Hibana instead compiles local
protocols into data. An endpoint tracks its progress through that data rather
than changing its Rust type after each operation.

Messages and the global protocol still use Rust types. Large source protocols
can therefore still produce complex types.

### Projection

The global protocol is a typed, zero-sized value. `project` returns the protocol
data for one role:

```rust
use hibana::runtime::program::project;

let client_program = project::<CLIENT>(&exchange);
let server_program = project::<SERVER>(&exchange);
```

Projection constructs and validates this data through Rust's const evaluation.
The implementation uses associated constants, so the calls above do not need
to appear in a `const` declaration.

These checks run when Rust generates code for a projection. Use `cargo build`
or `cargo test` after changing a global protocol. `cargo check` alone can accept
a protocol that code generation will reject, and editor diagnostics may miss
it. `g::diagnose(&protocol)` returns the role, events and help for a projection
error without creating an endpoint. The main error can point into Hibana's
projection code. Rust's instantiation note also shows the call to `project`.

Each `RoleProgram` records sends and receives, peers, message labels, payload
schemas, dependencies, and branch membership. It also records which
communication channels can progress independently. The transport API calls
these channels **lanes**.

Projection also includes the steps of roles that the participant can receive
as endpoints. Its runtime keeps one shared progress record for those roles.
Each endpoint gives access to one role within that record.

Projection checks whether roles can learn the choices they need to follow and
whether parallel operations can be distinguished. Protocols that fail these
checks are rejected during compilation.

### Selection, branching, and knowledge of choice

**Selection** is the choosing role's action. **Branching** is another role's
response to that choice. **Knowledge of choice** means that a role can learn
which branch was selected before it must act differently in that branch.

For a message-based choice, both branches must have the same choosing role.
Other participating roles must learn the choice from incoming messages.
Projection follows each role's local path through both branches. Common
incoming steps may come first, but the paths must eventually provide
distinguishable incoming steps before that role sends. A role absent from both
branches needs no notification. A choice with a role present in only one branch
is rejected.

The current rule is conservative. It supports only some of the merges allowed
by MPST projection. For example, it does not accept every case with identical
outgoing continuations. Hibana rejects choices it cannot check under this rule.
It does not insert broadcasts. To inform a third role of a choice, include a
message to that role in the global protocol.

At runtime, the received frame identifies a permitted branch. `offer()` returns
a `RouteBranch`, which lets the participant inspect the selected branch without
advancing the protocol. `label()` returns that branch's first logical message
label. The branch's first successful send or receive advances the protocol.
Reading or dropping the preview does not.

Adding `.resolve::<ID>()` to a choice connects it to a **resolver**, a local
application function that selects a branch. This changes how the choice is
made, but does not bypass other roles' projection checks or send the decision
to another machine. The application or deployment must supply any agreement
needed before a choice message arrives.

### Parallel communication and repetition

`g::par(left, right)` allows independent communication in its two parts.
Projection checks their ownership and separates their operations. Both retain
the dependencies from preceding sequential steps. Neither depends on the
other's intermediate progress. A later sequential continuation waits for the
parallel work required by that role to finish. Your executor schedules the
Rust functions.

To drive independent fragments of one endpoint concurrently, borrow them with
`let [mut requests, mut notices] = endpoint.par()?;` and poll their operations
with your executor. Each borrow uses the same `send`, `recv`, and `offer`
operations. The parent endpoint is unavailable until the borrows end. The
global protocol determines the fragments; `par()` only borrows them and does
not schedule work or choose a branch. Adjacent parallel groups are flattened,
so the local bindings do not repeat the global protocol's binary grouping.
For parallel work inside a choice, first observe it with
`let label = endpoint.offer().await?.label();`, then borrow its fragments.
They reuse the observed choice without waiting for the first message to finish.
These fragments are borrows, so they cannot themselves be delegated. A fragment
can send another owned endpoint when the global protocol permits that transfer.

`body.roll()` allows a completed protocol fragment to run again. Projection
requires guarded repetition. Each repetition reuses the compiled protocol
data, and the fragment's steps can be executed again. FIFO delivery, replay
exclusion, and separation of session runs are required to distinguish new
messages from old ones. A repeated protocol may run indefinitely.

Repetition can coexist with a later continuation. Projection rejects an
outgoing operation that could start a repeated fragment and also select that
continuation through the same role, label, and payload schema. An intervening
receive after the fragment does not resolve this ambiguity. A receive at the
start of the fragment can distinguish a later reply. Independent `par` arms,
ordinary sequential reuse, and reuse in exclusive route arms remain supported.

### Local execution

An attached endpoint borrows the storage for one role in one session. It cannot
be cloned to repeat a completed step. Each successful operation advances its
local protocol once.

For a send, the runtime checks the requested operation and message schema before
passing the frame to the transport. For a receive, it checks the frame against
the permitted local step and validates the exact payload format before
advancing.

A successful asynchronous send means that the transport accepted the frame,
not that the peer received it.

Hibana checks the communication performed by handwritten participant functions.
It does not detect or repair deadlock cycles by inspecting running tasks.
Protocol deadlock freedom depends on the accepted protocol and the execution
assumptions in [Guarantees and verification](#guarantees-and-verification).

## Specification

### Protocol language and endpoint operations

| Form | Meaning |
| --- | --- |
| `g::send::<FROM, TO, g::Msg<LABEL, PAYLOAD>>()` | One asynchronous message |
| `g::seq(left, right)` | Sequential composition |
| `g::par(left, right)` | Independent parallel composition |
| `g::route(left, right)` | Binary choice |
| `route.resolve::<ID>()` | Use an application decision function for a choice |
| `body.roll()` | Guarded repetition |

A named protocol can be a function returning `impl Projectable`. This trait is
sealed to Hibana's protocol constructors. Compose protocols before projecting
each role.

Logical message labels need not be globally unique. A `send` whose source and
target are the same role is a zero-byte local operation and must use the unit
payload `()`.

Projection rejects malformed or unobservable choices, ambiguous simultaneous
operations, conflicting parallel ownership, empty parallel parts, and unguarded
repetition. It also checks causal order when successive receives on a lane
change sender.

| Operation | Result |
| --- | --- |
| `Endpoint::send::<M>(&payload)` | Advances after the transport accepts the permitted message |
| `Endpoint::recv::<M>()` | Validates the permitted message and returns its decoded payload |
| `Endpoint::offer()` | Returns a branch preview without advancing |
| `RouteBranch::label()` | Returns the first logical message label of the selected branch |
| `RouteBranch::send` / `recv` | Consumes the preview and executes the branch's first visible step |

An operation **commits** when it advances the local protocol. Dropping a branch
preview, dropping an operation before commit, or rejecting an operation does
not advance it. Requeueing an uncommitted received frame does not advance it
either.

### Payloads

`g::Msg<L, P>` associates logical label `L` with payload type `P`. Built-in
codecs support unit, booleans, integers, byte slices, and fixed byte arrays.
Fixed-width decoders reject trailing bytes. A decoded value may borrow from
the transport's receive buffer.

Custom payloads implement `WireEncode` and `WirePayload` through
`hibana::runtime::wire`. Encoding writes deterministic bytes into supplied
storage. Validation checks the exact canonical format before the operation
commits. Decoding reads the validated bytes.

`WirePayload::SCHEMA_ID` identifies the wire format and its validation rules,
not the Rust type. It is not sent in the core frame header. Peers must agree on
encoding and validation. Assigning the same ID does not establish that
agreement. Different formats require different schema IDs. Schema `0` is the
zero-byte unit format.

### Transport

The transport provides buffers, framing, I/O readiness, and wakeups. It supplies
borrowed transmit and receive handles through `hibana::runtime::transport`.

| Operation | Contract |
| --- | --- |
| `open(PortOpen)` | Bind handles to the projected role, session, and lane |
| `poll_send` | Accept one frame or return pending without retaining the supplied payload pointer |
| `cancel_send` | Discard a dropped pending send. If its bytes cannot be withdrawn, disable that logical send direction for the rest of the run |
| `poll_recv` | Return one borrowed frame, or return pending and record the current waker |
| `requeue` | Return the most recently received, uncommitted frame to the same receive handle |

A later send poll supplies the same encoded content, but its address may change.
Pending I/O state must therefore belong to the transport, not the temporary
payload buffer. Dropping a send must not leave bytes that a later send can
transmit.

`Tx` and `Rx` may borrow storage from the transport. Their internal lifetimes
are separate from each call's mutable handle borrow. A send or cancellation
cannot keep that handle borrow after returning. A receive may return a frame
borrowed for its current call. That frame must be released before the same
receive handle is used again.

A framed receive uses this eight-byte header:

```text
session id (4 bytes, big endian) | lane | source role | target role | frame label
```

The transport owns these bytes. The frame label is a wire identifier assigned
during projection, distinct from the logical message label in `Msg`. The
runtime checks the header and requested message against the local protocol.

`ReceivedFrame::deterministic` omits the header. It is valid only when the
lane-bound receive and requested message identify exactly one enabled local
receive, or when a branch preview already identifies that receive. An
unresolved branch requires `ReceivedFrame::framed`.

For the global guarantees to apply, the transport must bind messages to the
expected peer and direction, preserve FIFO order within each mapped direction,
and prevent unsolicited replay and messages from earlier session runs from
entering the current run. It must not duplicate incoming messages. Returning
an uncommitted frame through `requeue` must not make it count as a second
message.

Frame validation rejects structural mismatches. It does not establish data
integrity or detect every replay. Ordered messages can reuse a frame label,
and schema IDs are not sent in the header. For example, a `bool` encoded as
`[1]` is also a valid `u8`. If the transport duplicates that bool frame before a
following u8 message with the same wire label, the u8 receive can accept `1`.
It cannot recover the intended value from these bytes. The transport must provide
replay exclusion and any sequence numbers, authentication or end-to-end
protection required by the deployment.

An accepted frame must eventually arrive, or closure of its direction must
become observable. After accepted frames have been drained or isolated,
closure must wake a waiting receiver and return an error. A known closed
direction must not remain pending forever.

These requirements apply to the transport implementation, not just its Rust
interface. The
[Unix datagram example](https://github.com/hibanaworks/hibana/tree/main/proofs/carriers/unix)
tests them for one implementation.

### Deadlines, errors, and cancellation

Hibana has no built-in clock or failure detector. The transport decides whether
an I/O operation has exceeded its deadline and reports `TransportError::Deadline`.
The other transport errors are `Offline`, `Capacity`, and `Failed`.

These transport errors stop the current session run in the runtime that
observes them. The endpoint operation returns `EndpointError`. The runtime
preserves the first fault and wakes local waiters. The failed operation does
not advance the protocol. There is no automatic retry, deadline extension, or
switch to another branch. The current
run cannot continue in that runtime after such an error. Accepted transfers
are not rolled back. Other runtimes learn of a failure through their transport.

An unfinished predecessor on the calling view or its owned payload produces
`OutOfOrder` and faults the local run as a protocol violation. `PhaseInvariant`
reports other inconsistencies in the operation's progress or descriptor facts.

A transport close does not signal successful protocol completion. `roll()`
lets a completed fragment run again and has no end message. To tell a worker
that work has ended, include a message or a route in the global protocol.
Hibana does not infer that decision from EOF or from a role's absence in a
later continuation. A drained close and an abrupt failure can both return
`Offline` at the same local position.

If a timeout or cancellation must lead to further protocol communication,
represent it as a message or an explicit choice in the global protocol.
Restart after a transport error with a fresh session run. The transport must
drain or isolate accepted frames so they cannot be delivered in the new run.

Dropping a pending send invokes `cancel_send`. The transport must discard
staged bytes. If some bytes can no longer be withdrawn, it must disable that
logical send direction for the rest of the run. A local fault cannot notify a
remote process by itself. Distributed cancellation also requires the transport
to make closure observable to the peer.

| Where an error occurs | Error type |
| --- | --- |
| Storage setup and endpoint attachment | `AttachError` |
| Decision registration or callback | `ResolverError` |
| Transport send, receive, or requeue | `TransportError` |
| Endpoint or branch operation | `EndpointError` |

### Sessions and storage

A session has a fixed protocol and a fixed, finite set of roles. A **session
run** is one execution of that session. Restarting after failure begins a new
run, even if the application reuses its `SessionId`.

Integration code uses `hibana::runtime` to project roles, supply memory and a
transport, and attach endpoints. `SessionKitStorage` holds runtime storage.
`rendezvous` takes a byte slab and a transport. `enter` attaches a projected
role to a `SessionId`. An undersized slab or failed attachment returns
`AttachError` without a partially attached endpoint.

Sessions have separate progress and failure state. Within one runtime, a role
cannot have two live attachments to the same session. The runtime also rejects
different protocol bytes for a session that already has a protocol bound to it.
Across machines, the deployment must ensure that peers use the same protocol.
Hibana does not perform a handshake to establish this agreement.

Reusing a `SessionId` requires fresh transport state that cannot expose frames
from its previous run. Changing the participant set or protocol requires a
fresh session. Address migration can stay within a run if the transport
preserves its guarantees. An `Endpoint` payload can transfer a logical role's
operation right while preserving its protocol continuation.

The physical resource integration owns its sole initial `EndpointBinding`.
All runtime instances and role aliases that can initially issue that resource
must use the same binding. Remote holders receive the endpoint and do not
construct another binding. Its unsafe constructor requires the integration to enforce that
physical uniqueness; the core cannot infer aliases between device or remote
handles. Drop, timeout, runtime replacement and a new `SessionId` do not
replenish an issued binding. Replacing it requires fencing all old rights and
accepted frames at the physical boundary.

`initial_endpoint_binding(session, role)` can select distinct physical resources
for concurrent sessions that use the same role number. Aliases of one physical
resource must return the same binding across sessions. Local completion alone
does not fence earlier remote rights or frames, so it does not renew issuance.

Attach the role that initially holds the resource before attaching the resource
endpoint, in the same runtime and `SessionId`. The global protocol determines
that initial holder. If its role is absent or has been dropped, attachment returns
`AttachError` without consuming the binding.

An owned `send` future takes its `Endpoint` payload when constructed. The old
holder cannot use it while the send is pending. Once the transport accepts the
frame, only its receiver can obtain the endpoint by a validated receive.
Error or cancellation does not return the endpoint to the sender. A deadline
does not prove non-delivery. The transport must preserve the logical receive
direction, including earlier accepted frames, across holder changes. Separate
machines need no shared Rust memory, but their transport must provide this
receive continuity as well as FIFO, non-replay and run isolation.

The runtime uses caller-supplied storage and borrowed buffers, with no heap
fallback. Endpoints are tied to their local runtime and are neither `Send`
nor `Sync`. Communication with another runtime goes through the transport.
The application is responsible for scheduling, persistent application state,
and restart policy.

### Capacity limits

| Item | Limit |
| --- | ---: |
| Role identifiers | 256, from `0` through `255` |
| Event identifiers | 65,535, with `u16::MAX` reserved for absence |
| Compiled global protocol | 65,535 bytes |
| Compiled local protocol | 65,535 bytes per role |
| Message-only global protocol | Up to 7,281 events, at nine bytes per event |
| Branch and repetition regions | Limited by protocol bytes, at least ten bytes per region |
| Decision function identifiers | 65,536, the full `u16` range |
| Transport lanes | 256 |
| Competing wire identifiers | 256 for one source, receiver, and lane |

Both the global and local protocols must fit their byte limits. Branches,
repetition, and dependencies consume additional bytes, so the event identifier
range alone does not determine how many events fit. Branch metadata also uses
this byte budget.

Wire identifiers distinguish receives that can compete. Ordered messages can
reuse identifiers, as can messages from different sources. The 256-identifier
limit is not a limit of 256 messages per protocol. Projection allocates lanes
according to the communication and can reuse them when ownership does not
conflict.

The source protocol remains a Rust type tree. Large generated protocols should
use balanced composition. Deeply nested protocols may need a larger Rust
`recursion_limit`. Source compilation limits and runtime storage limits are
separate.

Large protocols can also reach Rust's `long_running_const_eval` limit during
projection. If this happens, add `#![allow(long_running_const_eval)]` to the
crate that calls `project`. This permits longer const evaluation. It does not
skip projection checks or increase the protocol's byte limits.

## Guarantees and verification

Hibana checks each send and receive before it advances the local protocol.
It checks the direction, peer, lane, message identity and payload format.
Moving an endpoint transfers the right to continue its logical role. A failed
operation does not advance the protocol or restore a transferred endpoint.

The Lean models establish session fidelity, affine ownership and protocol
progress under their stated assumptions. For accepted finite unfoldings, the
occurrence model relates message order, source progress and the ownership of
resources retained by the participants. It includes choices, parallel work,
repetition and endpoint transfers. Roles can start another repetition while
messages from a previous repetition remain queued.

Applying those results to a running system requires:

1. All participants follow local protocols derived from the same global protocol.
2. Peers agree on payload formats and use codecs that implement those formats.
3. The transport binds messages to the right peer and direction, preserves FIFO
   order, excludes duplicates and replay, isolates session runs, and keeps
   accepted messages available to the next endpoint holder.
4. Participants call the permitted operations, retain their endpoints, futures
   and parallel borrows until completion or ordinary drop, and drive independent
   fragments concurrently when progress requires it. The executor keeps polling
   their futures.
5. The runtime implements the model's operation and ownership rules.

Under these assumptions, a live, unfinished modeled session has an enabled
communication or local transition. This is **protocol deadlock freedom within
one session**. It does not prove termination of repeated communication, progress
of arbitrary Rust computations, or absence of dependency cycles between separate
sessions. `mem::forget` can prevent progress because it runs neither the pending
operation nor its cancellation.

Deadlines and failure detection belong to the transport. An I/O error cannot
restore ownership or roll back a remote operation. Branch agreement also
requires the choosing participant to follow its endpoint. Authentication alone
does not prevent a participant from sending conflicting choices to different peers.

[Verification](proofs/README.md) describes the models and their connection to
the implementation. Lean checks the model theorems. Rust/model conformance tests
compare both allowed and rejected operations over bounded schedules. Some finite
acceptance checks use `native_decide` and also trust Lean's compiler and runtime.
They are distinguished from kernel-only proofs in the verification notes. Kani checks
bounded implementation properties, and Miri checks borrowing, pointer provenance
and drop behavior in executed tests. These checks do not verify every line of
Rust or every transport implementation. Rust projection alone is not a proof of
distributed deadlock freedom for an application.

## Reference

The direct inspiration is Lagaillardie, Neykova, and Yoshida's
[Stay Safe under Panic: Affine Rust Programming with Multiparty Session Types](https://arxiv.org/abs/2204.13464)
(ECOOP 2022).

## Build and check

The crate uses Rust 2024, has empty default features, and has no normal
dependencies. Run these checks from the repository root:

```sh
cargo +stable check --no-default-features --lib -p hibana
cargo +stable test -p hibana
cargo +stable doc -p hibana --no-deps --no-default-features
```

The [CI workflow](https://github.com/hibanaworks/hibana/blob/main/.github/workflows/quality-gates.yml)
also runs Lean, Kani, Miri, and embedded builds. It checks code size, stack and
storage budgets, and endpoint and future layouts. These measurements describe
the checked programs and builds. Memory use depends on the protocol and
configuration.

The [benchmarks](benches/README.md) measure the current runtime. Receive lookup
uses an immutable index of logical roles and message labels. Checking each
candidate's dependencies can still inspect multiple projected events, so
execution cost depends on protocol size.
The instruction budgets bound regressions for the measured workloads; they are
not a constant-time guarantee.

## License

Hibana is licensed under Apache-2.0 or MIT, at your option.
