<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA - Affine Multiparty Session Types for Rust" />

  <p>
    <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024" />
    <img src="https://img.shields.io/badge/no__std-yes-success.svg" alt="no_std" />
    <img src="https://img.shields.io/badge/no__alloc-oriented-blue.svg" alt="no_alloc oriented" />
  </p>

  <p>
    <a href="#install">Install</a> •
    <a href="#what-hibana-is">What Hibana Is</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#application-guide">Application Guide</a> •
    <a href="#protocol-integration">Protocol Integration</a> •
    <a href="#guarantees">Guarantees</a>
  </p>
</div>

# HIBANA

`hibana` is a Rust 2024, `#![no_std]`, no-alloc-oriented runtime for
affine multiparty session types.

It lets a protocol crate describe communication once as a global choreography,
project each participant into a compact local program, attach transport and
storage, and hand application code a small affine `Endpoint`.

The complete path is:

```text
hibana::g choreography
  -> integration::program::project(&program)
  -> integration::runtime::Config::from_resources(...)
  -> integration::SessionKitStorage::uninit().init()
  -> kit.rendezvous(...)
  -> registered rendezvous .session(...).role(...)
  -> role witness .enter()
  -> Endpoint
  -> flow().send() / recv() / offer() / RouteBranch::decode()
```

There are only two public surfaces:

| Surface | Used by | Main names |
| --- | --- | --- |
| Application surface | application code | `hibana::g`, `Endpoint`, `RouteBranch`, `EndpointResult`, `EndpointError` |
| Integration surface | protocol and integration crates | `hibana::integration`, `hibana::integration::program` |

If you are writing an application, stay on `hibana::g` and `Endpoint`. If you
are implementing a protocol crate, use `hibana::integration` to project, attach,
bind transport, install explicit route or loop resolvers when needed, and return endpoints.

## Install

Add Hibana from [crates.io](https://crates.io/crates/hibana):

```bash
cargo add hibana
```

Or write the dependency explicitly:

```toml
[dependencies]
hibana = "0.8.0"
```

The default feature set is empty. Hibana is `#![no_std]` and no-alloc-oriented
by default.

Enable `std` only for host-side tests, diagnostics, and documentation builds:

```toml
[dependencies]
hibana = { version = "0.8.0", features = ["std"] }
```

## What Hibana Is

Hibana is for communication systems where the protocol shape should be known
before runtime.

You write one global choreography:

```rust
use hibana::g;

let app = g::seq(
    g::send::<0, 1, g::Msg<1, u32>>(),
    g::send::<1, 0, g::Msg<2, u32>>(),
);
```

The choreography says:

- role `0` sends message label `1` with a `u32` payload to role `1`;
- role `1` then sends message label `2` with a `u32` payload back to role `0`.

A protocol crate composes any required prefixes, projects the choreography for
each role, attaches transport and storage, and returns an `Endpoint`. The
application then drives only its local endpoint.

### Affine Ownership, Not Shared Protocol State

Hibana's semantics are affine endpoint ownership and endpoint progress. The
current protocol state is the projected continuation owned by an `Endpoint`;
it is not a shared flag, shared table, shared memory cell, or ambient runtime
variable.

Each role must advance through its endpoint. The only evidence that may affect
protocol progress is evidence admitted by the projected descriptor through the
attached transport, a projected first visible route action, or an explicit
resolver decision at a projected route or loop resolver site. Role code must not read
shared memory, shared atomics, global flags, device registers, or side-channel
state to decide whether a route is ready, a loop continues, or a message may
be skipped.

Shared memory is especially not protocol authority. An integration crate may
use memory, atomics, interrupts, DMA, or OS primitives as private transport or
resolver implementation mechanics, but those mechanics must first become
transport frames, descriptor-checked ingress evidence, or loop resolver inputs
at explicit policy points. They never replace `flow().send()`, `recv()`,
`offer()`, or `RouteBranch::decode()`.

## Quick Start

Application code usually sees an endpoint that a protocol crate has already
attached.

```rust,ignore
use hibana::g;

endpoint.flow::<g::Msg<1, u32>>()?.send(&7).await?;
let reply = endpoint.recv::<g::Msg<2, u32>>().await?;
```

That is the main user path:

1. define messages and choreography with `hibana::g`;
2. receive an attached `Endpoint` from your protocol crate;
3. call `flow().send()`, `recv()`, `offer()`, and `RouteBranch::decode()`.

`flow()` and `offer()` are previews. Endpoint progress happens when
`flow().send()`, `recv()`, or `RouteBranch::decode()` succeeds. A failed preview
does not move the endpoint and does not choose an alternate route. Preview
evidence can wake or guide polling, but it cannot mint a continuation.

## Application Guide

Application authors only need these names:

- `hibana::g::{Msg, Program, send, seq, route, par}`
- `Endpoint`
- `RouteBranch`
- `EndpointResult<T>`
- `EndpointError`

Protocol crates that compose runtime/control protocol events into the same
choreography additionally use `g::ControlMsg` with Hibana-defined
`g::control::*` markers. Application payload messages normally stay on
`g::Msg`. `ControlMsg` is a protocol surface, not an application extension
surface: the marker set is closed by Hibana, and user-defined control kinds do
not become messages.

The normal choreography language is:

```rust
use hibana::g;

let request = g::send::<0, 1, g::Msg<10, [u8; 4]>>();
let response = g::send::<1, 0, g::Msg<11, u16>>();
let program = g::seq(request, response);
```

Keep choreography terms local. Compose them once and let the protocol crate
project them immediately. `Program<S>` is the unprojected typed choreography
term; `RoleProgram<ROLE>` is the projected runtime descriptor. Neither is a
transport handle, heap object, or reusable runtime object.

### Sending And Receiving

Use `flow().send()` when the next local step is a send known from the
choreography:

```rust,ignore
endpoint
    .flow::<g::Msg<10, [u8; 4]>>()?
    .send(&[1, 2, 3, 4])
    .await?;
```

Use `recv()` when the next local step is a deterministic receive:

```rust,ignore
let value = endpoint.recv::<g::Msg<11, u16>>().await?;
```

The message type carries the choreography label and payload type. The runtime
checks the projected descriptor and fails closed if the label, lane, or payload
shape does not match.

### Routes

`g::route(left, right)` is binary. Branch labels must be unique within the
route shape.

```rust
use hibana::g;

let accepted = g::send::<0, 1, g::Msg<31, u32>>();
let rejected = g::send::<0, 1, g::Msg<33, ()>>();
let routed = g::route(accepted, rejected);
```

When the endpoint reaches a route decision, call `offer()`:

```rust,ignore
let branch = endpoint.offer().await?;

match branch.label() {
    31 => {
        let value = branch.decode::<g::Msg<31, u32>>().await?;
        handle_accept(value);
    }
    33 => {
        let () = branch.decode::<g::Msg<33, ()>>().await?;
        handle_reject();
    }
    _ => unreachable!(),
}
```

If the chosen route arm begins with a send, drop the preview branch and send
the first message in that arm:

```rust,ignore
let branch = endpoint.offer().await?;

match branch.label() {
    40 => {
        drop(branch);
        endpoint.flow::<g::Msg<40, ()>>()?.send(&()).await?;
    }
    41 => {
        let bytes = branch.decode::<g::Msg<41, [u8; 8]>>().await?;
        use_bytes(bytes);
    }
    _ => unreachable!(),
}
```

The route is never selected by parsing payload bytes. Route authority comes
from the projected descriptor and the first visible branch action. Transport
observation may only supply ingress evidence that is checked against descriptor
metadata; a frame label, payload shape, queue position, or carrier hint is never
an independent route decision.

### Failure And Cancellation

Endpoint operations return `EndpointResult<T>`, so application code should use
ordinary `?`:

```rust,ignore
endpoint.flow::<g::Msg<1, u32>>()?.send(&7).await?;
let reply = endpoint.recv::<g::Msg<2, u32>>().await?;
let branch = endpoint.offer().await?;
let payload = branch.decode::<g::Msg<3, [u8; 4]>>().await?;
```

This shape has only two committed outcomes:

```text
Ok(progress)          next choreography state exists
Err(domain evidence)  current session generation is terminal
```

Errors are not route arms. Transport close, decode failure, or protocol
invariant failure poisons the affected session generation and returns
diagnostic evidence. It does not authorize reconnect or a different
branch in the same generation.

There is intentionally no `recv_timeout`, `send_timeout`, public `cancel`, or
same-generation recovery API. If time should select a branch, model time in the
choreography itself: use a timer or clock role whose first visible route action
carries the decision.

Protocol-invisible liveness detection belongs inside the transport adapter. A
UDP, serial, or custom carrier that decides an I/O wait is terminal must return
`TransportError` from `poll_send(...)` or `poll_recv(...)`; Hibana converts that
transport failure into terminal session evidence. Such watchdogs do not create
hidden route authority, carrier recovery policy, or same-generation recovery in Hibana.

The public evidence envelopes are domain-specific:

- `EndpointError` for `flow`, `send`, `recv`, `offer`, and `decode`;
- `ResolverError` for resolver registration and resolver decisions;
- `AttachError` for rendezvous and endpoint attach.

There is no public wide `HibanaError`, and public error-kind enums are not part
of the application decision surface. The `Debug` output records the operation
and callsite so top-level runners and panic handlers can report where a failure
was observed without requiring a second error layer at every call.

### Parallel Composition

`g::par(left, right)` combines independent local flows. Empty arms and
overlapping `(role, lane)` ownership are rejected by projection.

```rust
use hibana::g;

let left = g::send::<0, 1, g::Msg<50, u32>>();
let right = g::send::<0, 2, g::Msg<51, u32>>();
let parallel = g::par(left, right);
```

Lanes are projection-owned separation units. Application code describes role and
message structure; Hibana assigns the internal lanes needed to preserve affine
parallel progress.

### Payloads

Built-in exact codecs cover `()`, `bool`, integers, borrowed byte slices, and
fixed byte arrays. Fixed-width decoders reject trailing bytes.

Custom payloads implement `hibana::integration::wire::WireEncode` for sending
and `hibana::integration::wire::WirePayload` for receiving:

```rust
use hibana::integration::wire::{CodecError, Payload, WireEncode, WirePayload};

struct FourBytes([u8; 4]);

impl WireEncode for FourBytes {
    fn encoded_len(&self) -> Option<usize> {
        Some(4)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.0);
        Ok(4)
    }
}

impl WirePayload for FourBytes {
    type Decoded<'a> = FourBytes;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 4 {
            Ok(())
        } else {
            Err(CodecError::Invalid("FourBytes requires exactly 4 bytes"))
        }
    }

    fn decode_validated_payload(input: Payload<'_>) -> Self::Decoded<'_> {
        let bytes = input.as_bytes();
        FourBytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}
```

Decoded values may borrow from the received frame:

```rust
// In a message type, use `g::Msg<LABEL, &[u8]>`.
// The decoded value returned by recv/decode is borrowed from the endpoint
// transport frame currently owned by the endpoint.
```

### Branching, Resolvers, And Receive Evidence

Route choice is a protocol fact, not a transport guess. Prefer in-band choice:
put the real branch-selecting message at the head of each route arm.

```rust
let program = g::route(
    g::seq(g::send::<0, 1, g::Msg<10, Approved>>(), approve_body),
    g::seq(g::send::<0, 1, g::Msg<20, Denied>>(), deny_body),
);
```

When a branch is decided by a local timer, device readiness, budget, or another
non-message signal, use an explicit resolver attached through
`Program<Route<...>>::resolve` and `integration::resolver`. Built-in local
protocol events such as loop continue/break are the exception: they are written
as `g::ControlMsg` self-sends, stay local to the endpoint, and still appear as
normal choreography events.

State and transaction controls use the same shape. A snapshot starts the local
transaction lifecycle; commit, abort, and restore are local `g::ControlMsg`
self-sends that lower to state control rows and fail closed if the snapshot
generation does not exist.

```rust,ignore
let program = g::seq(
    g::send::<0, 0, g::ControlMsg<30, g::control::StateSnapshot>>(),
    g::send::<0, 0, g::ControlMsg<31, g::control::TxnCommit>>(),
);
```

Topology controls are the distributed side of the same protocol surface. A
protocol crate can author begin/ack/commit events in choreography, but they are
not user payloads; runtime send and receive bind them to the projected session,
lane, and topology header.

```rust,ignore
let program = g::seq(
    g::send::<0, 1, g::ControlMsg<40, g::control::TopologyBegin>>(),
    g::seq(
        g::send::<1, 0, g::ControlMsg<41, g::control::TopologyAck>>(),
        g::send::<0, 1, g::ControlMsg<42, g::control::TopologyCommit>>(),
    ),
);
```

```rust,ignore
use hibana::g;
use hibana::integration::ids::SessionId;
use hibana::integration::program::{project, RoleProgram};
use hibana::integration::resolver::{DecisionArm, DecisionResolution, ResolverError, ResolverRef};

const ROUTE_POLICY: u16 = 7;

struct RouteState {
    accept: bool,
}

fn route_decision(state: &RouteState) -> Result<DecisionResolution, ResolverError> {
    let arm = if state.accept {
        DecisionArm::Left
    } else {
        DecisionArm::Right
    };
    Ok(DecisionResolution::Arm(arm))
}

let routed = g::route(accept_body, reject_body).resolve::<ROUTE_POLICY>();
let role0: RoleProgram<0> = project(&routed);
let state = RouteState { accept: true };

rv.session(SessionId::new(1))
    .role(&role0)
    .set_resolver(ResolverRef::<ROUTE_POLICY>::decision_state(&state, route_decision))?;
```

Receive evidence is checked against the projected descriptor. `ReceivedFrame`
uses fail-closed `IngressEvidence`: deterministic receive is valid only when a
single active receive can be selected, while `offer()` and
`RouteBranch::decode()` require framed descriptor-checked evidence. Payload shape, queue position, carrier id, and driver observations are never branch authority.

## Protocol Integration

Protocol crates use the same `hibana::g` language as applications. There is
no second composition language.

### Compose And Project

A protocol crate may place transport or integration prefixes before the application
choreography, then project each role.

```rust
use hibana::g;
use hibana::integration::program::{project, RoleProgram};

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

`project(&program)` is the projection boundary. Runtime code consumes the
projected descriptor; it does not rediscover protocol shape.

Generated protocol packages and composition facades may hide the concrete
`Program<_>` step-list type when returning an unnamed choreography value. They
return `impl integration::program::Projectable`, and callers still use the same
`project(&program)` entry. `Projectable` is a sealed choreography bound, not a
second choreography language and not a runtime authority. It has no
runtime-universe type parameter; facade runtimes keep their universe on their
own storage/configuration types, not on the choreography projection bound.

### Attach An Endpoint

The canonical integration path is borrowed and caller-provided:

```rust,ignore
use hibana::integration;
use hibana::integration::ids::SessionId;
use hibana::integration::runtime::{Config, CounterClock, RING_EVENTS};

let mut tap_buf = [integration::runtime::TapEvent::zero(); RING_EVENTS];
let mut slab = [0u8; 64 * 1024];

let clock = CounterClock::new();
let mut kit_storage = integration::SessionKitStorage::<MyTransport>::uninit();
let kit = kit_storage.init();

let config = Config::from_resources((&mut tap_buf, &mut slab), clock);
let rv = kit.rendezvous(config, transport)?;
let endpoint = rv.session(SessionId::new(1)).role(&client).enter()?;
```

`SessionKitStorage::init()` is the only public construction path. It writes the
kit in place into caller-owned storage, returns the stable borrow used
by endpoint attach, and drops the initialized kit exactly once. The raw unsafe
initializer and `MaybeUninit` protocol are not part of the public surface.

`Config::from_resources` owns the rendezvous storage and clock authority. Lane
domain and endpoint lease capacity are not caller-selected config. A fresh
rendezvous starts with no materialized lane storage and no endpoint lease table.
Role attach reads the projected descriptor, grows exactly the lane
tables and endpoint lease entries it needs, and preserves existing session state
if a later projected role needs a wider lane span. Integration code must not
pass caller-chosen lane windows, endpoint counts, or deadline knobs.

The protocol crate owns concrete `MyTransport` and any ingress demux state. The
application receives only `Endpoint`.

Useful integration owners:

- `integration::program::{project, RoleProgram}`
- `integration::SessionKit`
- `integration::runtime::{Config, CounterClock, DefaultLabelUniverse, LabelUniverse, RING_EVENTS}`
- `integration::ids::{EffIndex, SessionId}`
- `integration::transport::Transport`
- `integration::resolver::{ResolverError, ResolverRef, DecisionArm, DecisionResolution}`
- `integration::wire::{Payload, WireEncode, WirePayload}`
- `integration::runtime::TapEvent`

### Transport

Implement `integration::transport::Transport` to connect Hibana to an I/O
system. The transport sees bytes, frame labels, and readiness; it does not own
choreography meaning, route authority, policy inputs, telemetry, or application
cancellation semantics.

Protocol-invisible carrier watchdogs belong inside `poll_send(...)` and
`poll_recv(...)`: if the transport concludes that progress is impossible, it
returns `TransportError` and Hibana terminates the current session generation.

The transport owns:

- `open(port)` for the descriptor-derived role/session/lane port witness;
- `poll_send(...)` and `poll_recv(...)`; receive returns a borrowed `ReceivedFrame`
  view from transport-managed receive storage, carrying payload bytes and
  transport `IngressEvidence` as one receive value;
- `cancel_send(...)` for transport cleanup when a send future is dropped after
  staging carrier state;
- `requeue(...)` as the required rollback path for an accepted staged frame
  that descriptor checks cannot commit.

`open(port)` returns Tx/Rx handles whose lifetime is bound to the transport
borrow, so an embedded carrier can keep buffers, wakers, and DMA bookkeeping
inside the transport owner without allocating or exporting a separate context.

The canonical receive-side frame observation is the `IngressEvidence` inside the
`ReceivedFrame` returned by `poll_recv(...)`. Payload and evidence cross the
transport boundary together; there is no separate receive-observation hook.
`Deterministic` evidence is valid only for a single deterministic receive;
route, offer, and decode demux require framed evidence that Hibana compares with
the endpoint's expected session/lane/role/label context before any endpoint
progress can consume the payload. Route/session/progress authority remains in
Hibana.

### Ingress Demux

Ingress demux state belongs inside the transport owner. `poll_recv(...)`
returns payload bytes and descriptor-checked ingress evidence as one receive
value, so endpoint progress can verify the frame against the projected
descriptor before previewing an `offer()` or committing a `recv()` or
`RouteBranch::decode()`.

Headerless receive is only valid when the projected frontier contains one
deterministic receive. Branch observation and route decode require framed,
descriptor-checked evidence. Payload shape, frame label, queue position, and
carrier-local hints do not select route arms.

### Resolvers

Resolvers are installed by the protocol crate for explicit route or loop
resolution sites. Route choices are otherwise derived from projected first
visible branch actions, including built-in local `g::ControlMsg` loop decisions
when those are the branch head. Resolver state is the external input owner: use
`ResolverRef::decision_state(...)` when a resolver needs protocol-specific
observations. Resolver failure rejects the step; it does not fall through to a
different semantic path.

## Guarantees

Hibana keeps the public API small because the projection boundary carries the
proof work.

Core guarantees:

- Rust 2024 and stable Rust `1.95`;
- default features are empty;
- runtime code is `no_std` and no-alloc-oriented;
- descriptor storage is caller-provided, borrowed, static, or slab-backed;
- route shape, duplicate labels, malformed control paths, and lane ownership
  errors are rejected before endpoint execution;
- runtime cursor progress is one-way and affine;
- protocol state is affine endpoint ownership, not shared atomic or shared
  memory state;
- failed sends, receives, offers, and decodes do not authorize hidden progress;
- payload decode is exact;
- message logical labels and transport frame labels are separate concepts;
- control semantics are descriptor metadata, not reserved numeric labels;
- route authority is limited to projected facts, first visible branch actions,
  and explicit resolver decisions; descriptor-checked transport observation may
  only confirm or demux projected facts.

What application code should not do:

- call transport APIs directly from localside logic;
- choose route arms by parsing payloads;
- model resolver decisions as driver-side branching;
- treat carrier hints, queue position, payload shape, or frame labels as route
  authority;
- match endpoint errors to continue the same generation on a hidden alternate path;
- use shared memory, shared atomics, global flags, or side-channel state as
  route readiness, loop-control, or protocol-progress authority;
- expose protocol-specific APIs through the `hibana` crate root.

## Validation

For a published crate consumer, the useful checks are ordinary Cargo commands:

```bash
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 check --features std --lib -p hibana
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

The full test suite is repository-only; it depends on source-tree fixtures that
are intentionally excluded from the production crate package.

For a repository checkout, maintainers should run the repository gate suite
before release:

```bash
bash ./.github/scripts/run_final_form_gates.sh
```

Use that gate rather than raw `cargo test`; repo-only unit tests are enabled
through `hibana_repo_tests`. The suite protects the public surface, `no_std` build,
projection boundary, descriptor publication, future layout, route authority, and
size measurements. It is intentionally kept outside the crate package.
