<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA - Session-Typed Choreographic Programming for Rust" />

  <p>
    <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024" />
    <img src="https://img.shields.io/badge/no__std-yes-success.svg" alt="no_std" />
    <img src="https://img.shields.io/badge/no__alloc-oriented-blue.svg" alt="no_alloc oriented" />
  </p>

  <p>
    <a href="#install-and-run">Install and Run</a> |
    <a href="#how-hibana-works">How It Works</a> |
    <a href="#application-guide">Application Guide</a> |
    <a href="#protocol-runtime-guide">Protocol Runtime Guide</a> |
    <a href="#guarantees-and-assumptions">Guarantees</a> |
    <a href="#build-and-test">Build and Test</a>
  </p>
</div>

# HIBANA

`hibana` is a Rust 2024, `#![no_std]`, no-alloc-oriented runtime for executing
finite-role affine asynchronous multiparty protocols. A protocol is written
once as a global choreography, projected into compact per-role descriptor
programs, and enforced as each endpoint sends, receives, or enters a route.

Hibana is a **choreography-derived runtime enforcement kernel**. It is not a
generated Rust continuation type system and it does not turn a protocol into a
large family of endpoint types. Choreography structure ends at projection;
production endpoints execute message-erased descriptor rows with a fixed
eight-byte core frame header and caller-provided storage. Release gates compile
the public surface for `thumbv6m-none-eabi` and enforce the quantitative
resource envelope below.

The supported protocol core is deliberately finite and protocol-neutral:

- up to sixteen declared roles per session;
- asynchronous `send`, sequential `seq`, independent `par`, binary `route`,
  guarded `roll`, and zero-byte local effects;
- affine endpoint ownership and fail-closed operation admission;
- multiple isolated `SessionId` instances of one projected descriptor;
- explicit reconfiguration through a fresh finite session and artifact.

Hibana does not provide channel delegation, an unbounded live role set,
carrier authentication, cryptography, failure detection, or an application
algorithm. Those concerns may be implemented around Hibana, but they do not
become vocabulary or hidden authority inside its protocol kernel.

## Install And Run

Add Hibana from [crates.io](https://crates.io/crates/hibana):

```bash
cargo add hibana
```

The library itself has no external dependencies. Runtime code is `no_std` and
no-alloc-oriented; host-only dependencies are used only by tests and examples.

### Complete Runnable Example

From a Hibana checkout:

```bash
cargo run --example ping_pong
```

Output:

```text
ping=7, pong=8
```

The complete source below is [`examples/ping_pong.rs`](examples/ping_pong.rs).
The documentation gate requires this block and the executable source to remain
byte-for-byte identical.

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

[`examples/pico/src/lib.rs`](examples/pico/src/lib.rs) is the canonical
projected-program input to the resource gate. With Rust `1.95.0`, the current
tree and checked-in release ceilings are:

| Hibana-owned quantity | Current measurement | Release ceiling |
| --- | ---: | ---: |
| `SessionKitStorage` | 24 B | 32 B |
| Fixed per-rendezvous storage, including the 512 B tap ring | 720 B | 952 B |
| Peak live runtime slab across tracked heavy shapes | 2,425 B | 4,323 B |
| Localside runtime stack high-water | 2,831 B | 3,663 B |
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
  + peak live runtime slab + localside runtime stack
```

Each modeled SRAM total is computed within one measured shape before the
maximum is selected. Component maxima in the table may come from different
shapes and must not be added as one observed run.

It is a Hibana-owned runtime envelope, not a whole-device memory claim. The
application, concrete transport buffers, executor, interrupt stacks, codec
scratch, and platform startup remain deployment-owned and must be budgeted
separately. `bash ./.github/scripts/run_final_form_gates.sh` regenerates the
measurements and rejects a release above any ceiling. Public capacities
must come from role, lane, descriptor, slab, and tap requirements rather than an
unrelated fixed budget.

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
| Application | localside protocol code | `hibana::g`, `Endpoint`, `RouteBranch`, `EndpointError` |
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
let branch = endpoint.offer().await?;

match branch.label() {
    31 => {
        let value = branch.recv::<g::Msg<31, u32>>().await?;
        handle_accept(value);
    }
    33 => {
        branch.send::<g::Msg<33, ()>>(&()).await?;
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
    g::send::<0, 1, g::Msg<30, Chunk>>(),
    g::send::<1, 0, g::Msg<31, Ack>>(),
).roll();

let program = g::seq(body, g::send::<0, 1, g::Msg<32, Done>>());
```

For a resolved repeated route, resolve the route and then roll the surrounding
region:

```rust,ignore
let repeated = g::route(left, right)
    .resolve::<ROUTE_DECISION>()
    .roll();
```

The reverse order is rejected by the Rust type shape because
`resolve::<ID>()` belongs to `Program<Route<...>>`, not to the resulting
`Program<Roll<Route<...>>>`. Nested rolls follow the same rule. The formal
elastic history distinguishes occurrences across nested iterations; production
descriptors, endpoint types, and wire frames carry no iteration ordinal.

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
and the compact runtime descriptor. Facades that cannot name a choreography's
concrete `Program<_>` type may return sealed `impl Projectable`; callers still
use the same projection function.

### Storage And Attach

The canonical runtime path is borrowed and caller-provided. In this fragment,
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
prove remote receipt. A raw carrier provides exact local operation monitoring,
but global fidelity and progress require stronger deployment evidence:
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
and failure domains. Their transitions commute in the Lean model. Retrying an
interaction or changing the finite participant set creates a fresh session and,
when the choreography changes, a fresh verified artifact.

Persistent application state, membership policy, scheduling policy, recovery
policy, and algorithm invariants stay outside Hibana. The application can build
larger distributed systems from repeated finite session families without
placing those algorithms or their names in Hibana core.

`RendezvousKit::tap()` returns a read-only `TapPort` over the retained evidence
ring. Tap is not a logger or route input. Each `TapEvent` is an immutable
16-byte record containing compact causal evidence for endpoint operations,
transport observations, faults, lanes, route selection, and resolver decisions.
Public code can read events but cannot construct or push them. The ring retains
the latest 32 events and supports postmortem reads after a failure.

## Guarantees And Assumptions

Hibana separates protocol facts from deployment premises. This distinction is
essential: compiling a Rust choreography with `project(&program)` is not by
itself a Lean proof artifact, and implementing `Transport` does not by itself
claim network behavior.

### Protocol Artifact Guarantee

For a `VerifiedProtocolCertificate` accepted by the independent checker, Lean
proves that:

- all exact role descriptor images refine one projectable choreography;
- every admitted operation uses the complete operation key and preserves
  subject reduction and session fidelity;
- wrong peer, direction, lane, event, label, schema, orphan delivery, and
  duplicate consumption cannot become ordinary progress;
- rejection and preview restoration are zero-transition behavior;
- route publication is affine, repeated occurrences remain fresh, and
  cancellation preserves first fault and reaches finite model retirement under
  its explicit transport-state premises;
- every reachable live, unfinished distributed model state has at least one
  enabled transition.

The last statement is **semantic per-session deadlock freedom**, also called
semantic unstuckness. It begins with an accepted certificate, not merely with a
Rust choreography that projects successfully.

### Deployed Execution Guarantee

The stronger end-to-end result additionally requires:

1. exact role-image and canonical schema agreement across the deployment;
2. the carrier properties selected by its `CarrierProfile`;
3. `GlobalFairnessAssumptions` for execution scheduling;
4. the explicit cross-tool Rust kernel refinement premise.

Under exact deployment agreement, the strong affine-delivery contract, and
`GlobalFairnessAssumptions`, every operation that remains recurrently enabled is
eventually scheduled. Together with semantic unstuckness, this gives
**per-session protocol deadlock freedom under the stated deployment premises**.

This is not a termination theorem. An infinite `.roll` may continue forever.
Hibana cannot force host code to poll an endpoint and does not prove deadlock
freedom for arbitrary application cycles spanning multiple sessions.

The responsibility boundary is strict:

| Hibana establishes | A deployment supplies |
| --- | --- |
| Exact projection and descriptor admission | The concrete carrier implementation |
| Affine endpoint ownership and fail-closed local progress | Peer authenticity and the delivery properties it claims |
| Canonical wire schema identity | Correct downstream codec implementations |
| Static exact-image certificate checking | Evidence that certified role images were installed |
| Conditional progress and retirement theorems | Fair scheduling and observable terminal closure when claimed |

External premises are inputs to stronger deployment-indexed theorems, not
missing Hibana runtime features. A mandatory core handshake, cryptographic
scheme, or carrier-specific sequence field would weaken protocol neutrality and
inflate the core runtime. Exact deployment agreement may instead come from a
static certificate, authenticated manifest, or a separate verified bootstrap
session.

### Carrier Profiles

Lean orders carrier evidence in a strict chain:

```text
Mediated -> Authentic -> Ordered -> Closing -> Fair
```

Each step adds a premise; it does not alter `Endpoint`, descriptor rows, or the
wire header. The weakest profile supports exact local monitoring. Stronger
profiles add peer binding, ordering and replay exclusion, observable closure and
finite retirement, then fair delivery. A concrete carrier must establish the
profile it claims. Hibana does not prove an arbitrary `Transport`
implementation.

The repository's [Unix datagram proof carrier](proofs/unix-carrier/README.md)
shows that the contract can be implemented by a real OS transport. It is an
example and proof target, not a required production carrier.

### Elastic Re-entry And Erasure

An atomic reset model cannot represent legal asynchronous pre-receive traffic
across repeated regions. Hibana's Lean model therefore records elastic
occurrence history, including nested `roll` occurrences, long enough to prove
freshness, affinity, and fidelity. A general erasure theorem removes occurrence
ordinals from the production trace.

The result is proof-visible freshness with no epoch in endpoint types,
descriptor rows, runtime operation keys, or the fixed core header. A deployment
still needs carrier generation isolation and replay exclusion before it can
claim that old physical frames cannot enter a fresh session generation.

### Cross-tool Evidence

The end-to-end argument divides responsibilities without presenting one tool's
result as another tool's proof:

| Evidence | Responsibility |
| --- | --- |
| Lean | global/local semantics, artifact checking, fidelity, progress, cancellation, carrier assumptions, and erasure |
| Kani/CBMC | bounded exhaustive checks of compact production prepare/commit kernels and owner inventories |
| Miri | strict provenance, borrowing, drop, cancellation, waiter, and callback re-entry behavior |
| Rust tests and gates | executable examples, UI rejection, transport conformance, package surface, and target resource regressions |

The main composition theorem is
`assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement`. It combines
accepted descriptor bytes, deployment agreement, codec coverage, the selected
carrier profile, elastic trace erasure, and an explicit
`RustKernelRefinement` premise.

This is a conditional cross-tool refinement. It is not a source-level Lean
proof of every Rust statement. The generated witness covers the finite
production operation and owner inventory enforced by repository gates. Kani and
Miri remain named evidence boundaries. The complete theorem inventory,
assumptions, counterexamples, and generated artifact coverage are documented in
the [Lean proof boundary](proofs/lean/README.md).

### What Hibana Does Not Claim

Hibana does not claim:

- termination of an intentionally infinite repeated protocol;
- progress when application code withholds an endpoint operation;
- deadlock freedom for arbitrary cycles across independent sessions;
- correctness of every carrier, codec, scheduler, or installed binary;
- authentication, secrecy, denial-of-service resistance, or correct failure
  detection;
- safety or liveness of an application algorithm merely because its messages
  follow a choreography;
- cross-binary equality of nominal Rust types;
- channel delegation, an unbounded role set, arbitrary message reordering, or
  complete monitoring of code that bypasses the endpoint API;
- that no other system has the same combination of ideas.

These exclusions keep claims precise. They are not alternate runtime paths.

### Research Context

The design draws from [Multiparty Asynchronous Session
Types](https://www.doc.ic.ac.uk/~yoshida/multiparty/multiparty.pdf), [Affine
Multiparty Session Types](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2022.4),
and the explicit-channel constraints of the [mechanised subject-reduction
development](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2025.31).
Those calculi inform Hibana but do not override its compact runtime constraints.

Hibana's research direction is the composition of assumption-indexed carrier
guarantees, byte-exact translation validation, affine cancellation, in-band
choice knowledge, elastic proof-only iteration history, and erasure into one
resource-bounded message-erased runtime. Novelty is a research claim to be
established by comparison and review, not by README wording.

## Build And Test

Published-crate checks use ordinary Cargo commands:

```bash
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 check --lib -p hibana
cargo +1.95.0 test -p hibana --test ui
cargo +1.95.0 test -p hibana --test lane_lifecycle_tap
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

The package ships self-contained compile, UI, API, and runtime behavior tests.
Repository-only checks for source hygiene, proof inventories, generated
artifacts, and measured resource budgets stay outside the crate package.

For release decisions from a repository checkout, run:

```bash
bash ./.github/scripts/run_final_form_gates.sh
```

The repository gate executes the runnable example, all explicit Rust test
targets, `no_std` target checks, docs and package checks, Miri owner tests,
Lean proofs, artifact generation, public-surface guards, compile-pressure and
resource measurements. The quality workflow runs the pinned Kani/CBMC inventory
as a separate required job because it has different host dependencies. A
missing harness or zero-test selection fails its gate.
