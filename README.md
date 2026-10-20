<div align="center">
  <img src="https://raw.githubusercontent.com/hibanaworks/hibana/v0.9.9/.github/assets/hibana-header.svg" width="600" alt="HIBANA — Choreographic Programming in Rust" />

  <p>
    <a href="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml"><img src="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml/badge.svg" alt="Quality gates" /></a>
    <a href="https://crates.io/crates/hibana"><img src="https://img.shields.io/crates/v/hibana.svg" alt="crates.io" /></a>
    <a href="https://docs.rs/hibana"><img src="https://docs.rs/hibana/badge.svg" alt="docs.rs" /></a>
    <a href="LICENSE-APACHE"><img src="https://img.shields.io/crates/l/hibana.svg" alt="Apache-2.0 OR MIT" /></a>
  </p>
</div>

# Hibana — Choreographic Programming in Rust

[日本語](README.ja.md)

Hibana is a Rust library for choreographic programming. You describe who sends
what to whom, and in what order, in a shared protocol. Participants can be tasks
in one program or programs on different machines.

You write each participant's Rust code. Hibana derives its communication steps
from the shared protocol and checks its sends and receives against those steps
at runtime.

The core uses `no_std`, requires no heap allocator, and runs on stable Rust 1.95
or newer. The runtime uses memory supplied by your program.

[hibana-quic](https://github.com/hibanaworks/hibana-quic) uses Hibana to implement
QUIC v1/v2 and HTTP/3. It uses the same approach for communication within a
connection and for application messages sent over QUIC.

## Add to your project

Run this in your project directory:

```sh
cargo add hibana@0.9.9
```

This adds the following dependency to `Cargo.toml`:

```toml
[dependencies]
hibana = "0.9.9"
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
git clone https://github.com/hibanaworks/hibana.git
cd hibana
cargo +1.95.0 run --locked --example ping_pong
```

It prints:

```text
ping=7, pong=8
```

Protocols can also describe choices, independent communication in parallel,
and repetition. Participant code uses the same `send`, `recv`, and `offer`
operations.

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
among all participants. A **role** identifies a participant in that protocol.
**Endpoint projection** derives a **local protocol** for each role.

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
Here, *affine* means that a communication resource can be used at most once.
It may be dropped without completing the protocol.

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

Projection checks whether roles can learn the choices they need to follow and
whether parallel operations can be distinguished. Protocols that fail these
checks are rejected during compilation.

Projection generates communication data, not participant functions. You write
the functions and their computations in Rust. The global protocol is checked
at compile time. Each function's sends and receives are checked against its
local protocol at runtime.

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
Reading or dropping the
preview does not.

Adding `.resolve::<ID>()` to a choice connects it to an application decision
function. This changes how the choice is made, but does not bypass other roles'
projection checks or send the decision to another machine. The application or
deployment must supply any agreement needed before a choice message arrives.

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

Each role keeps one completion bit per repeated region alongside its current
event completions. This lets it receive an earlier continuation after starting
another visit. New sends and operations inside the region still need their
current predecessors. Restarting an outer region clears its children's retained
completions. This bit does not identify a message's visit or replace the
transport's ordering and replay guarantees.

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
`[1]` is also a valid `u8`. If the carrier duplicates that bool frame before a
following u8 message with the same wire label, the u8 receive can accept `1`.
It cannot recover the intended value from these bytes. The carrier must provide
replay exclusion and any sequence numbers, authentication or end-to-end
protection required by the deployment.

An accepted frame must eventually arrive, or closure of its direction must
become observable. After accepted frames have been drained or isolated,
closure must wake a waiting receiver and return an error. A known closed
direction must not remain pending forever.

These requirements apply to the transport implementation, not just its Rust
interface. The
[Unix datagram example](https://github.com/hibanaworks/hibana/tree/main/proofs/unix-carrier)
tests them for one implementation.

### Deadlines, errors, and cancellation

Hibana has no built-in clock or failure detector. The transport decides whether
an I/O operation has exceeded its deadline and reports `TransportError::Deadline`.
The other transport errors are `Offline`, `Capacity`, and `Failed`.

These transport errors stop the current session run in the runtime that
observes them. The endpoint operation returns `EndpointError`. The runtime preserves the first fault and wakes local
waiters. The failed operation does not advance the protocol. There is no
automatic retry, deadline extension, or switch to another branch. The current
run cannot continue in that runtime after such an error. Accepted transfers
are not rolled back. Other runtimes learn of a failure through their transport.

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
preserves its guarantees. Channel delegation is not supported by this model.

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

Runtime checks enforce each endpoint's local protocol. Before an operation
commits, the runtime checks direction, peer, lane, message identity, and payload
schema. An endpoint can complete a local step at most once before moving on.
Repetition can make that step available again. A failed operation does not
advance the protocol.

The Lean proofs establish session fidelity, protocol progress, and cancellation
properties for their models. The global results apply to the exact generated
protocol data accepted by an independent checker. Successful Rust projection
alone does not prove distributed deadlock freedom.

The earlier distributed `roll` model requires a common reset boundary. It does
not cover every schedule where roles enter new visits independently while old
messages remain unread. The finite occurrence model and bounded Endpoint/model
comparisons cover selected such schedules. A general refinement for all of
them is still unfinished. The progress claim below applies within the selected
model's scope and premises.

Applying these results to a running system requires:

1. Every role follows the accepted local protocol derived from the same global
   protocol.
2. Peers agree on wire formats and use conforming codecs.
3. The transport provides the peer binding, ordering, replay exclusion, and
   observable closure required by the selected model.
4. The application eventually issues the communication required by its local
   continuation, and the executor continues polling its pending futures.
   Authority-bearing futures and fragments remain reachable until they complete
   or are dropped normally.
5. The runtime implementation follows the model's operation and ownership rules.

Creating a future and polling it are separate responsibilities. For independent
parallel fragments, use `endpoint.par()` and drive the fragments concurrently
when progress needs both. Polling a receive cannot create an independent send
that the application never calls. These application and executor assumptions
do not establish that a particular Endpoint is owned, callable or admitted;
the runtime and its implementation correspondence must still establish those
properties.

Leaking an authority-bearing future or fragment with `mem::forget` violates
the progress assumptions. Its `Drop` does not run, so the runtime does not
automatically cancel it, report a fault or restore its authority. The peer may
keep waiting until the transport reports a deadline or closure. This does not
duplicate authority, but it can prevent progress.

Under these assumptions, a live, unfinished protocol has an enabled
communication transition. This is **protocol deadlock freedom within one
session**. It does not establish termination of repeated communication, freedom
from deadlocks inside arbitrary Rust computations, or absence of dependency
cycles between separate sessions.

The cancellation results cover the specified finite transport states and rules
for disabling a direction. Deadlines and failure detection are assumptions
about the transport. Branch agreement also assumes that the choosing role
follows its endpoint. Authentication alone cannot ensure agreement if a
participant sends conflicting choices to different peers.

[The Lean proof notes](https://github.com/hibanaworks/hibana/tree/main/proofs/lean)
state the exact claims and assumptions. They do not verify every line of Rust
or every `Transport` implementation. Kani checks bounded kernel cases. Miri
checks borrowing, pointer provenance, and drop behavior in the executed tests.
Rust tests cover projection failures, operation ordering, payload validation,
and transport contracts. Rust/Lean comparisons check agreement for generated
test cases.

## Reference

The direct inspiration is Lagaillardie, Neykova, and Yoshida's
[Stay Safe under Panic: Affine Rust Programming with Multiparty Session Types](https://arxiv.org/abs/2204.13464)
(ECOOP 2022).

## Build and check

The crate uses Rust 2024, has empty default features, and has no normal
dependencies. Run these checks from the repository root:

```sh
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 test -p hibana
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

The [CI workflow](https://github.com/hibanaworks/hibana/blob/main/.github/workflows/quality-gates.yml)
also runs Lean, Kani, Miri, and embedded builds. It checks code size, stack and
storage budgets, and endpoint and future layouts. These measurements describe
the checked programs and builds. Memory use depends on the protocol and
configuration.

## License

Hibana is licensed under Apache-2.0 or MIT, at your option.
