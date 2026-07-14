<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA - Choreography-Derived Runtime Enforcement for Rust" />

  <p>
    <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024" />
    <img src="https://img.shields.io/badge/no__std-yes-success.svg" alt="no_std" />
    <img src="https://img.shields.io/badge/allocator-not_required-success.svg" alt="Allocator not required" />
  </p>

  <p>
    <a href="#install-and-run">Install and Run</a> |
    <a href="#how-hibana-works">How It Works</a> |
    <a href="#application-guide">Application Guide</a> |
    <a href="#protocol-runtime-guide">Protocol Runtime Guide</a> |
    <a href="#guarantees-and-requirements">Guarantees</a> |
    <a href="#build-and-test">Build and Test</a>
  </p>
</div>

# HIBANA

`hibana` is a **choreography-derived runtime enforcement kernel** for Rust 2024.
Write a finite multiparty protocol once as a global choreography, project it
into compact per-role programs, and execute each role through one affine
`Endpoint`. Every send, receive, and route operation is checked against the
next permitted descriptor event before progress commits.

Hibana is `#![no_std]`, requires no allocator, uses caller-provided storage, and
keeps protocol state out of per-state continuation types. The runtime uses a
fixed eight-byte core frame header; the measured code, SRAM, and stack envelope
is published below.

The protocol language is finite and transport-neutral:

- up to sixteen declared roles per session;
- asynchronous `send`, sequential `seq`, independent `par`, binary `route`,
  guarded `roll`, and zero-byte local effects;
- affine endpoint ownership and fail-closed operation admission;
- multiple isolated `SessionId` instances of one projected descriptor;
- explicit reconfiguration through a fresh finite session and artifact.

Transport security, failure detection, scheduling, and application algorithms
remain deployment concerns. They integrate through `Transport`, explicit route
resolvers, and application state without adding protocol-specific concepts to
the Hibana core.

## Install And Run

Add Hibana from [crates.io](https://crates.io/crates/hibana):

```bash
cargo add hibana
```

The library itself has no external dependencies. Runtime code is `no_std` and
requires no allocator; host-only dependencies are used only by tests and
examples.
The complete API reference is available on [docs.rs](https://docs.rs/hibana).

### Complete Runnable Example

From a Hibana checkout:

```bash
cargo run --example ping_pong
```

Output:

```text
ping=7, pong=8
```

The complete source below is [`examples/ping_pong.rs`](examples/ping_pong.rs),
and CI executes this exact example.

<!-- ping-pong-example:start -->
```rust
#[path = "support/in_memory.rs"]
mod in_memory;

use futures::executor::block_on;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use in_memory::InMemoryTransport;

// This host example's measured runtime budget; deployments measure their own descriptor set.
const RUNTIME_SLAB_BYTES: usize = 3 * 1024;

fn main() {
    let choreography = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<1, 0, Msg<2, u32>>(),
    );
    let client_program: RoleProgram<0> = project(&choreography);
    let server_program: RoleProgram<1> = project(&choreography);

    let mut slab = [0_u8; RUNTIME_SLAB_BYTES];
    let mut storage = SessionKitStorage::<InMemoryTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, InMemoryTransport::new())
        .expect("create rendezvous");
    let session = SessionId::new(1);
    let mut client = rendezvous
        .enter(session, &client_program)
        .expect("attach client");
    let mut server = rendezvous
        .enter(session, &server_program)
        .expect("attach server");

    let (ping, pong) = block_on(async {
        client.send::<Msg<1, u32>>(&7).await.expect("send ping");
        let ping = server.recv::<Msg<1, u32>>().await.expect("receive ping");
        server
            .send::<Msg<2, u32>>(&(ping + 1))
            .await
            .expect("send pong");
        let pong = client.recv::<Msg<2, u32>>().await.expect("receive pong");
        (ping, pong)
    });

    assert_eq!((ping, pong), (7, 8));
    println!("ping={ping}, pong={pong}");
}
```
<!-- ping-pong-example:end -->

The example-local `InMemoryTransport` is host support, not a second Hibana
runtime. A deployment supplies its own `Transport`; the choreography,
projection call, endpoint type, and endpoint operations stay unchanged.

Endpoint progress happens when `send()`, `recv()`, or a route branch first-step
operation succeeds. `offer()` previews a route and does not commit progress by
itself.

### Measured `no_std` Resource Envelope

The tracked `thumbv6m-none-eabi` projection example uses the same `hibana::g` and
`project(&program)` surface without an SDK, allocator, host transport, or
target-specific Hibana API:

```bash
rustup target add --toolchain 1.95.0 thumbv6m-none-eabi
cargo +1.95.0 check --manifest-path examples/pico/Cargo.toml \
  --target thumbv6m-none-eabi
```

[`examples/pico/src/lib.rs`](examples/pico/src/lib.rs) is the `no_std`
projection sample compiled by the resource checks. With Rust `1.95.0`, the
repository records these measurements and release ceilings:

| Hibana-owned quantity | Current measurement | Release ceiling |
| --- | ---: | ---: |
| `SessionKitStorage` | 24 B | 32 B |
| Fixed per-rendezvous storage, including the 512 B tap ring | 720 B | 952 B |
| Peak live runtime slab across tracked heavy shapes | 2,425 B | 4,323 B |
| Runtime operation stack high-water | 2,831 B | 3,663 B |
| Modeled runtime SRAM envelope | 5,920 B | 8,954 B |
| Minimal linked protocol artifact | 356 B | 2,048 B |
| Largest linked artifact in the tracked protocol matrix | 1,852 B | 16,384 B |
| Complete no-default `libhibana.rlib` sections | 129,910 B | 169,965 B |
| Library `.data + .bss` | 0 B | 0 B |

The linked artifact and library rows are `thumbv6m-none-eabi` release
measurements. Flash is `.text + .rodata + .data`; the complete rlib is not the
size paid by one linked protocol. Runtime stack high-water is measured around
Hibana operations on the pinned `aarch64-apple-darwin` measurement host. The
modeled SRAM envelope is:

```text
thumb .data/.bss + SessionKitStorage + fixed per-rendezvous storage
  + peak live runtime slab + runtime operation stack
```

Each modeled SRAM total is computed within one measured shape before the
maximum is selected. Component maxima in the table may come from different
shapes and must not be added as one observed run.

It is a Hibana-owned runtime envelope, not a whole-device memory claim. The
application, concrete transport buffers, executor, interrupt stacks, codec
scratch, and platform startup remain deployment-owned and must be budgeted
separately. `bash ./.github/scripts/run_final_form_gates.sh` regenerates the
measurements and rejects a release above any ceiling.

## How Hibana Works

One value describes the global communication order:

```rust
use hibana::g;

let choreography = g::seq(
    g::send::<0, 1, g::Msg<10, u32>>(),
    g::send::<1, 0, g::Msg<11, u32>>(),
);
```

The first step sends message label `10` from role `0` to role `1`; the second
sends label `11` back. A protocol crate projects the same value once for every
participating role. Each endpoint then owns one local continuation:

```text
global choreography
  -> project(&choreography) for every role
  -> compact RoleProgram descriptors
  -> attach each role to one SessionId generation
  -> Endpoint::send / recv / offer
  -> exact descriptor transition or terminal error
```

There are two public surfaces:

| Surface | Owner | Main names |
| --- | --- | --- |
| Application | role implementation | `hibana::g`, `Endpoint`, `RouteBranch`, `EndpointError` |
| Protocol runtime | protocol and carrier integration | `hibana::runtime`, `runtime::program`, `SessionKitStorage`, `Transport` |

If you are writing an application, stay on `hibana::g` and `Endpoint`. If you
are implementing a protocol crate, use `hibana::runtime` to project, attach,
bind transport, install explicit route resolvers when needed, and return
endpoints to application code.

### Multiparty, Asynchronous, And Affine

- **Multiparty** means one global choreography is projected for every role, so
  peer, direction, and ordering come from one protocol description.
- **Asynchronous** means a successful send transfers a frame to the carrier; it
  does not wait for or prove the remote receive.
- **Affine** means one live owner may advance an endpoint at most once per
  projected step. An endpoint may be dropped, but it cannot be duplicated or
  publish the same progress twice.

### Choreography Language

| Form | Meaning |
| --- | --- |
| `g::send::<FROM, TO, g::Msg<LABEL, PAYLOAD>>()` | one visible asynchronous message |
| `g::seq(left, right)` | `left` precedes `right` |
| `g::par(left, right)` | independent arms may progress concurrently |
| `g::route(left, right)` | one of two projected protocol arms |
| `route.resolve::<ID>()` | an explicit resolver owns the route decision |
| `body.roll()` | a guarded structural region may re-enter |

A same-role `send` is a local effect. It must use the canonical zero-byte `()`
schema; it does not encode data into a private hidden queue. Projection rejects
unsupported route shape, empty parallel arms, overlapping operations that
cannot be selected exactly, lane conflicts, and unguarded re-entry before an
endpoint is created.

`Program<S>` is the temporary typed choreography value. `RoleProgram<ROLE>` is
the compact projected descriptor. Endpoint futures do not carry `S`, the
choreography tree, or payload types beyond the operation being called. This is
how Hibana keeps protocol structure out of endpoint type growth and production
runtime metadata.

### Affine Progress

An `Endpoint` exclusively owns the live `(rendezvous, SessionId, role)`
identity. A successful endpoint operation consumes exactly one permitted local
step. A rejected operation, dropped preview, or staged observation returned by
`requeue(...)` consumes no protocol step. A committed mismatch or carrier fault
terminates that session generation; it never opens another branch or retry path.

Protocol authority cannot come from shared flags, ambient device state, payload
parsing, carrier hints, or application guesses. Such information must first
become one of:

- a descriptor-checked received frame;
- the projected first visible action of an intrinsic route; or
- a typed resolver decision registered at an explicit resolver site.

Memory, interrupts, DMA, operating-system primitives, and device registers may
remain private implementation mechanics of a carrier or resolver owner. They
do not replace `send()`, `recv()`, `offer()`, or route branch first-step
operations.

## Application Guide

Everyday application endpoint code uses these names:

- `hibana::g::{Msg, send, seq, par, route}`;
- `Endpoint::{send, recv, offer}`;
- `RouteBranch::{label, send, recv}`;
- `EndpointError`.

Choreography authors additionally use `.roll()` and `.resolve::<ID>()`.

### Messages And Payloads

`g::Msg<L, P>` names one logical protocol message. `L` is its choreography
label and `P` is its wire payload type. A logical label is not a carrier frame
label and is not resolved-route authority.

Every payload implements both `WireEncode` and `WirePayload`:

- `WireEncode` writes deterministic send bytes;
- `WirePayload` validates exact receive bytes and decodes them;
- `WirePayload::SCHEMA_ID` identifies the canonical wire contract inside the
  protocol descriptor.

The schema id is not a cross-binary Rust nominal type id and is not sent in the
core frame header. Incompatible encodings or validators must use distinct ids.
Different Rust wrappers may share an id only when they deliberately implement
the same canonical wire schema. Schema `0` belongs to the exact zero-byte unit
schema.

Built-in codecs cover `()`, `bool`, integers, borrowed byte slices, and fixed
byte arrays. Fixed-width decoders reject trailing bytes. Decoded byte slices
may borrow from the received frame for the endpoint borrow.

A custom exact four-byte schema looks like this:

```rust
use hibana::runtime::wire::{CodecError, Payload, WireEncode, WirePayload};

struct FourBytes([u8; 4]);

impl WireEncode for FourBytes {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.0);
        Ok(4)
    }
}

impl WirePayload for FourBytes {
    const SCHEMA_ID: u32 = 0x4000_0000;

    type Decoded<'a> = FourBytes;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 4 {
            Ok(())
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload(input: Payload<'_>) -> Self::Decoded<'_> {
        let bytes = input.as_bytes();
        FourBytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}
```

### Sending And Receiving

`send()` and `recv()` name the expected choreography message. The projected
descriptor supplies direction, peer, lane, event identity, and schema:

```rust,ignore
endpoint
    .send::<g::Msg<10, [u8; 4]>>(&[1, 2, 3, 4])
    .await?;

let reply = endpoint.recv::<g::Msg<11, u16>>().await?;
```

Use `recv()` when the next local receive can be uniquely committed from the
observed transport evidence and the projected descriptor. A wrong label,
direction, peer, lane, event identity, schema, or payload shape fails closed
before it can become ordinary protocol progress.

### Routes

`g::route(left, right)` is binary. Both arms must have one first-visible
controller. There are two authority forms:

- an **intrinsic route** is selected by its projected first visible endpoint
  operation;
- a **resolved route** is selected by `ResolverRef::decide()` at the route
  marked with `.resolve::<ID>()`.

Prefer in-band choice: make the real branch-selecting message the first visible
action in each route arm. When a timer, readiness signal, budget, or other
non-message signal owns the decision, use `.resolve::<ID>()` and install its
typed resolver through `runtime::resolver`. A resolver is not a shared oracle
that makes competing first senders projectable.

```rust
use hibana::g;

let accepted = g::send::<0, 1, g::Msg<31, u32>>();
let rejected = g::send::<0, 1, g::Msg<33, ()>>();
let routed = g::route(accepted, rejected);
```

At a route, `offer()` previews the selected arm. The first send or receive is
performed through the returned branch:

```rust,ignore
let branch = server.offer().await?;

match branch.label() {
    31 => {
        let value = branch.recv::<g::Msg<31, u32>>().await?;
        handle_accept(value);
    }
    33 => {
        branch.recv::<g::Msg<33, ()>>().await?;
    }
    label => panic!("unexpected route label {label}"),
}
```

`RouteBranch::label()` reports the selected arm's first logical message label.
For resolved routes this label is not branch authority; the registered resolver
decision is. Resolved arms may reuse a logical message label after resolver
selection. Payload shape, queue position, carrier id, and driver observations
are never branch authority.

### Parallel And Repeated Regions

`g::par(left, right)` combines independent flows. Projection owns lane
assignment and rejects operations whose simultaneous local visibility would be
ambiguous. Logical message labels need not be globally unique.

```rust
use hibana::g;

let left = g::send::<0, 1, g::Msg<50, u32>>();
let right = g::send::<2, 3, g::Msg<50, u32>>();
let parallel = g::par(left, right);
```

`.roll()` marks a structural region that may re-enter:

```rust,ignore
let body = g::seq(
    g::send::<0, 1, g::Msg<30, u32>>(),
    g::send::<1, 0, g::Msg<31, ()>>(),
).roll();

let program = g::seq(body, g::send::<0, 1, g::Msg<32, ()>>());
```

For a resolved repeated route, resolve the route and then roll the surrounding
region:

```rust,ignore
const ROUTE_DECISION: u16 = 7;

let repeated = g::route(
    g::send::<0, 1, g::Msg<40, u32>>(),
    g::send::<0, 1, g::Msg<41, ()>>(),
)
    .resolve::<ROUTE_DECISION>()
    .roll();
```

The reverse order is rejected by the Rust type shape because
`resolve::<ID>()` belongs to `Program<Route<...>>`, not to the resulting
`Program<Roll<Route<...>>>`. Nested rolls follow the same rule. Repeated regions
add no iteration field to endpoint types, descriptors, or wire frames, so a
transport must prevent frames from a retired generation from reappearing.

### Failure And Cancellation

Endpoint operations return `Result<T, EndpointError>` and are normally used
with `?`:

```rust,ignore
endpoint.send::<g::Msg<1, u32>>(&7).await?;
let reply = endpoint.recv::<g::Msg<2, u32>>().await?;
let branch = endpoint.offer().await?;
let payload = branch.recv::<g::Msg<3, [u8; 4]>>().await?;
```

The outcomes are intentionally narrow:

```text
Ok(progress)          one next descriptor state exists
Err(domain evidence)  this session generation is terminal
```

Errors are not route arms. Decode failure, protocol mismatch, or terminal
carrier failure poisons the affected generation and wakes local waiters. There
is no public same-generation retry, reselection, timeout, or cancel operation.
A retry or reconfiguration is a fresh session generation; a protocol-visible
timeout is modeled by a role whose visible action selects a route.

Protocol-invisible liveness detection belongs inside the transport. When a
carrier concludes that an I/O wait cannot progress, `poll_send(...)` or
`poll_recv(...)` returns `TransportError`; Hibana turns that failure into
terminal session evidence without creating hidden route authority.

## Protocol Runtime Guide

Protocol crates use the same `hibana::g` language as applications. There is no
second composition language.

### Compose And Project

A protocol crate may compose its own visible prefix around an application
choreography, then project each role:

```rust
use hibana::g;
use hibana::runtime::program::{RoleProgram, project};

let prefix = g::seq(
    g::send::<0, 1, g::Msg<1, ()>>(),
    g::send::<1, 0, g::Msg<2, ()>>(),
);
let app = g::seq(
    g::send::<0, 1, g::Msg<10, u32>>(),
    g::send::<1, 0, g::Msg<11, u32>>(),
);
let program = g::seq(prefix, app);

let client: RoleProgram<0> = project(&program);
let server: RoleProgram<1> = project(&program);
```

`project(&program)` is the boundary between the temporary typed choreography
and the compact runtime descriptor.

### Storage And Attach

Runtime storage is borrowed and caller-provided. In this fragment,
`runtime_slab: &mut [u8]` is a deployment-owned region selected from measured
descriptor and runtime requirements:

```rust,ignore
use hibana::runtime;
use hibana::runtime::ids::SessionId;

let mut kit_storage = runtime::SessionKitStorage::<MyTransport>::uninit();
let kit = kit_storage.init();
let rv = kit.rendezvous(runtime_slab, transport)?;
let endpoint = rv.enter(SessionId::new(1), &client)?;
```

`SessionKitStorage::init()` is the only public construction path. A rendezvous
borrows one slab, then materializes descriptor-required lane storage, endpoint
leases, receive scratch, resolver ownership, and evidence storage from that
region. A second live attach for the same `(rendezvous, SessionId, role)` is
rejected. Dropping the endpoint releases its lease.

The first local role binds its session generation to one exact compiled program
image. Later local roles with byte-different images are rejected before
allocation. Roles needed by a resolver must attach before the rendezvous seals
local membership at first resolver execution.

Useful runtime owners:

- `runtime::program::{project, Projectable, RoleProgram}`;
- `runtime::{SessionKitStorage, SessionKit}`;
- `runtime::ids::SessionId`;
- `runtime::transport::{Transport, TransportError, PortOpen, Outgoing,
  ReceivedFrame, FrameHeader, FrameLabel}`;
- `runtime::resolver::{DecisionArm, ResolverError, ResolverRef}`;
- `runtime::wire::{CodecError, Payload, WireEncode, WirePayload}`;
- `runtime::tap::{Evidence, TapEvent, TapPort}`.

### Transport

`runtime::transport::Transport` connects Hibana to an I/O system. It owns byte
storage, readiness, carrier framing, ingress demux, and wakeups. It does not own
choreography meaning, route authority, resolver input, telemetry policy, or
application cancellation semantics.

A transport implements:

- `open(port)` for a descriptor-derived role/session/lane port;
- `poll_send(...)` over an `Outgoing` frame;
- `poll_recv(...)` returning one borrowed `ReceivedFrame`;
- `cancel_send(...)` to make a dropped staged send unobservable, or retire a
  direction whose framing can no longer remain valid;
- `requeue(...)` to return an accepted staged observation when descriptor
  checks cannot commit it.

`PortOpen` supplies local role, session id, and lane. `Outgoing` supplies frame
label, target role, lane, and payload. The handles returned by `open` borrow the
transport, so buffers, wakers, DMA records, and demux data can remain inside one
embedded owner without allocation.

Ingress demux state belongs inside the transport owner. Every received frame is
checked against the exact session, lane, role, direction, frame label, event,
and schema expected by the endpoint descriptor. Reordering, repetition, and
mismatch are not silently repaired.

Headerless receive is only valid when direct `recv()` selects one live
descriptor, or after `RouteBranch::recv()` already owns a materialized receive
descriptor. `ReceivedFrame::deterministic(...)` is valid only for a single
deterministic direct receive or an already materialized route branch receive
descriptor.
`ReceivedFrame::framed(...)` is the framed construction path.
Route offer and unresolved route demux require
`ReceivedFrame::framed(FrameHeader::from_bytes(header_bytes), payload)`;
Hibana checks the carrier-owned eight bytes before any endpoint progress.

`poll_send(...) -> Ready(Ok(()))` transfers a frame to the carrier. It does not
prove remote receipt. Hibana still validates each observed operation against
the local descriptor, but global fidelity and progress additionally require:
authenticated peer/direction binding, FIFO delivery, no unsolicited replay,
generation isolation, and eventual delivery or observable terminal closure.

Fresh transport-instance state is a sufficient carrier generation. Address
migration may remain inside one generation. Reusing a session identity after
retirement requires fresh carrier state that cannot expose an older frame.
For a multiplexed carrier, closure may retire one mapped direction while
unrelated sessions remain live.

### Resolvers

A resolver owns an explicit non-message route decision. Register one typed
`ResolverRef` for the route id projected into a role program.
`ResolverRef::decision_state` is the only registration constructor:

```rust,ignore
use hibana::g;
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::resolver::{DecisionArm, ResolverError, ResolverRef};

const ROUTE_RESOLVER: u16 = 7;

struct RouteState {
    accept: bool,
}

fn route_decision(state: &RouteState) -> Result<DecisionArm, ResolverError> {
    if state.accept {
        Ok(DecisionArm::Left)
    } else {
        Ok(DecisionArm::Right)
    }
}

let accept_body = g::send::<0, 1, g::Msg<60, u32>>();
let reject_body = g::send::<0, 1, g::Msg<61, ()>>();
let routed = g::route(accept_body, reject_body).resolve::<ROUTE_RESOLVER>();
let role0: RoleProgram<0> = project(&routed);
let state = RouteState { accept: true };

rv.set_resolver(
    &role0,
    ResolverRef::<ROUTE_RESOLVER>::decision_state(&state, route_decision),
)?;
```

Resolver state is the external input owner. Resolver failure rejects the step;
it does not select another arm. A resolver registered in one rendezvous does not
create cross-device agreement. Participants that act before receiving in-band
branch evidence must receive the same decision through the deployment's
protocol or carrier design.

### Sessions, Reconfiguration, And Tap

A descriptor is a finite session template, not a singleton deployment. Distinct
`SessionId` instances have independent cursors, queues, leases, resolver state,
and failure domains. Retrying an interaction or changing the finite participant
set creates a fresh session and, when the choreography changes, a fresh
projected artifact.

Persistent application state, membership, scheduling, recovery, and algorithm
invariants remain application-owned. Larger distributed systems can be built
from explicit families of finite sessions.

`RendezvousKit::tap()` returns a read-only `TapPort` over the retained evidence
ring. Tap is not a logger or route input. Each `TapEvent` is an immutable
16-byte record containing compact causal evidence for endpoint operations,
transport observations, faults, lanes, route selection, and resolver decisions.
Public code can read events but cannot construct or push them. The ring retains
the latest 32 events and supports postmortem reads after a failure.

## Guarantees And Requirements

Hibana enforces protocol progress locally; a deployment supplies the properties
of the medium and scheduler. Keeping that boundary explicit makes the guarantee
useful without assuming a particular network stack.

### Local Enforcement

For every attached endpoint:

- a successful operation commits exactly one permitted descriptor transition;
- a dropped preview, rejected operation, or `requeue(...)` commits no
  transition;
- peer, direction, lane, event, label, schema, and payload mismatches fail
  closed;
- one endpoint owner cannot publish the same progress twice;
- a committed codec or transport fault terminates that session generation.

`project(&program)` rejects unsupported or ambiguous choreographies and emits
the compact role programs used by this enforcement. The repository's
machine-checked global theorems apply to exact role images accepted by the
independent protocol artifact checker.

### When Deadlock Freedom Holds

A choreography that projects successfully is not, by itself, a distributed
deadlock-freedom guarantee. Hibana provides per-session protocol deadlock
freedom when all of the following hold:

1. every role runs the exact image accepted for the same projectable
   choreography;
2. peers agree on each canonical wire schema and use conforming codecs;
3. the transport binds the expected peers and directions, preserves FIFO order,
   excludes replay across session generations, and eventually delivers each
   accepted frame or reports terminal closure;
4. the executor eventually polls operations that remain enabled.

The accepted artifact establishes that every reachable live, unfinished model
state has an enabled protocol transition. The transport and scheduling
requirements connect that semantic result to deployed execution. An intentional
infinite `.roll` may continue forever, and application cycles spanning separate
sessions remain application-level scheduling concerns.

| Hibana enforces | Integration requirement |
| --- | --- |
| Exact descriptor admission and affine endpoint progress | All roles install the accepted images |
| Complete operation-key and payload-schema checks | Codecs implement the declared canonical schemas |
| First-fault preservation and local waiter wakeup | Terminal peer closure becomes observable |
| Conditional fidelity, progress, and retirement | Peer-bound FIFO delivery, no replay, and fair polling |

The [Unix datagram carrier](proofs/unix-carrier/README.md) is an executable
conformance example for peer binding, FIFO delivery, replay exclusion, closure
wakeup, and generation isolation. Other transports provide the same contract in
the way appropriate to their medium.

### Verification

The repository checks different parts of this boundary with complementary
tools:

| Tool | Checked responsibility |
| --- | --- |
| Lean | global and role-local semantics, projection artifacts, fidelity, progress, cancellation, and repeated-region freshness |
| Kani/CBMC | bounded exhaustive checks of compact prepare/commit kernels and ownership state |
| Miri | strict provenance, borrowing, drop, cancellation, waiter, and callback re-entry behavior |
| Rust tests and release gates | executable examples, compile-time rejection, carrier conformance, package contents, and resource regressions |

The [Lean proof boundary](proofs/lean/README.md) lists the exact theorems,
requirements, and generated artifacts. Each tool remains evidence for the part
it actually checks.

### Scope

Hibana covers finite-role sessions executed through its endpoint API. Carrier
authentication and cryptography, failure-detector accuracy, application
algorithm correctness, unbounded role sets, channel delegation, and code that
bypasses the endpoint are outside the kernel's guarantee.

## Build And Test

From a repository checkout, the main Cargo checks are:

```bash
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 check --lib -p hibana
cargo +1.95.0 test -p hibana --test ui
cargo +1.95.0 test -p hibana --test lane_lifecycle_tap
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

The published package contains the runnable example and its compile, UI, API,
and runtime behavior tests. Repository release checks additionally cover proof
artifacts and measured resource budgets.

For release decisions from a repository checkout, run:

```bash
bash ./.github/scripts/run_final_form_gates.sh
```

The repository gate executes the runnable example, Rust tests, `no_std` target
checks, documentation and package checks, Miri, Lean, and resource measurements.
CI runs the pinned Kani/CBMC inventory as a separate required job.

Hibana is licensed under either Apache-2.0 or MIT, at your option.
