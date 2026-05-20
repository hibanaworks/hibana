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
  -> integration::SessionKit::enter(...)
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
bind transport, install policy, and return endpoints.

## Install

Add Hibana from [crates.io](https://crates.io/crates/hibana):

```bash
cargo add hibana
```

Or write the dependency explicitly:

```toml
[dependencies]
hibana = "0.6.1"
```

The default feature set is empty. Hibana is `#![no_std]` and no-alloc-oriented
by default.

Enable `std` only for host-side tests, diagnostics, and documentation builds:

```toml
[dependencies]
hibana = { version = "0.6.1", features = ["std"] }
```

## What Hibana Is

Hibana is for communication systems where the protocol shape should be known
before runtime.

You write one global choreography:

```rust
use hibana::g;

let app = g::seq(
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>(),
    g::send::<g::Role<1>, g::Role<0>, g::Msg<2, u32>, 0>(),
);
```

The choreography says:

- role `0` sends message label `1` with a `u32` payload to role `1` on lane `0`;
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
attached transport, or an explicit resolver decision at a projected route /
loop policy point. Role code must not read shared memory, shared atomics,
global flags, device registers, or side-channel state to decide whether a
route is ready, a loop continues, or a message may be skipped.

Shared memory is especially not protocol authority. An integration crate may
use memory, atomics, interrupts, DMA, or OS primitives as private transport or
resolver implementation mechanics, but those mechanics must first become
transport frames, descriptor-checked binding evidence, or resolver inputs at
explicit policy points. They never replace `flow().send()`, `recv()`,
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

`flow()` and `offer()` are previews. Endpoint progress happens when a send or
decode succeeds. A failed preview does not move the endpoint and does not choose
an alternate route. Preview evidence can wake or guide polling, but it cannot
mint a continuation.

## Application Guide

Application authors only need these names:

- `hibana::g::{Role, Msg, Program, send, seq, route, par}`
- `Endpoint`
- `RouteBranch`
- `EndpointResult<T>`
- `EndpointError`

The normal choreography language is:

```rust
use hibana::g;

let request = g::send::<g::Role<0>, g::Role<1>, g::Msg<10, [u8; 4]>, 0>();
let response = g::send::<g::Role<1>, g::Role<0>, g::Msg<11, u16>, 0>();
let program = g::seq(request, response);
```

Keep choreography terms local. Compose them once and let the protocol crate
project them immediately. `Program<S>` is the typed choreography witness; it is
not a transport handle, heap object, or reusable runtime object.

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

The message type carries the choreography label, payload type, and optional
control kind. The runtime checks the projected descriptor and fails closed if
the label, lane, payload shape, or control/data kind does not match.

### Routes

`g::route(left, right)` is binary. Branch labels must be unique within the
route shape.

```rust
use hibana::g;

let accepted = g::send::<g::Role<1>, g::Role<0>, g::Msg<30, u32>, 0>();
let rejected = g::send::<g::Role<1>, g::Role<0>, g::Msg<31, ()>, 0>();
let routed = g::route(accepted, rejected);
```

When the endpoint reaches a route decision, call `offer()`:

```rust,ignore
let branch = endpoint.offer().await?;

match branch.label() {
    30 => {
        let value = branch.decode::<g::Msg<30, u32>>().await?;
        handle_accept(value);
    }
    31 => {
        let () = branch.decode::<g::Msg<31, ()>>().await?;
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
from the projected descriptor or from an explicit resolver decision at a
projected route point. Transport observation may only supply demux evidence that
is checked against descriptor metadata; a frame label, payload shape, or binding
hint is never an independent route decision.

### Failure, Deadlines, And Cancellation

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

Errors are not route arms. An operational deadline, transport close, decode
failure, or protocol invariant failure poisons the affected session generation
and returns diagnostic evidence. It does not authorize retry, reconnect, or a
different branch in the same generation.

There is intentionally no `recv_timeout`, `send_timeout`, public `cancel`, or
same-generation recovery API. If time should select a branch, model time in the
choreography itself: use a timer or clock role and an explicit route point, then
install a resolver for that route. Runtime deadlines are integration fuses; they
kill the generation instead of becoming protocol-visible choices.

The public evidence envelopes are domain-specific:

- `EndpointError` for `flow`, `send`, `recv`, `offer`, and `decode`;
- `ResolverError` for resolver registration and resolver decisions;
- `AttachError` for rendezvous and endpoint attach.

There is no public wide `HibanaError`, and public error-kind enums are not part
of the application decision surface. The `Debug` output records the operation
and callsite so top-level runners and panic handlers can report where a failure
was observed without requiring wrapper errors at every call.

### Parallel Composition

`g::par(left, right)` combines independent local flows. Empty arms and
overlapping `(role, lane)` ownership are rejected by projection.

```rust
use hibana::g;

let left = g::send::<g::Role<0>, g::Role<1>, g::Msg<50, u32>, 1>();
let right = g::send::<g::Role<0>, g::Role<2>, g::Msg<51, u32>, 2>();
let parallel = g::par(left, right);
```

Lanes are protocol-owned separation units. Application code should follow the
lane contract exposed by its protocol crate rather than assigning global lane
meaning inside `hibana` itself.

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

    fn decode_payload(input: Payload<'_>) -> Result<Self::Decoded<'_>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 4 {
            return Err(CodecError::Invalid("FourBytes requires exactly 4 bytes"));
        }
        Ok(FourBytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}
```

Decoded values may borrow from the received frame:

```rust
type BorrowedBytes = &'static [u8];

// In a message type, use `g::Msg<LABEL, &[u8]>`.
// The decoded value returned by recv/decode is borrowed from the transport frame.
```

### Dynamic Policy

Dynamic policy is explicit. Mark the controller self-send that opens each
route or loop arm with `Program::policy::<POLICY_ID>()`, then let the
protocol crate install a resolver for that policy id. The policy annotation is
on the arm head, not on the `g::route(...)` wrapper.

```rust
use hibana::g;
use hibana::integration::cap::GenericCapToken;
use hibana::integration::cap::advanced::RouteDecisionKind;

const POLICY_ID: u16 = 7;

let left = g::send::<
    g::Role<0>,
    g::Role<0>,
    g::Msg<60, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
    0,
>()
.policy::<POLICY_ID>();

let right = g::send::<
    g::Role<0>,
    g::Role<0>,
    g::Msg<61, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
    0,
>()
.policy::<POLICY_ID>();

let routed = g::route(left, right);
```

Policy does not appear as driver `if`/`else` logic. It is a choreography point
resolved through the integration policy seam.

If a resolver returns `Defer`, the offer remains pending unless new route
evidence or a valid resolver decision appears. Hibana does not maintain
offer-time defer budgets, synthetic poll retries, or progress-exhaustion escape
paths.
An operational deadline may still kill the session generation, but that is a
terminal fault, not a protocol branch.

### Control Messages

Control messages are ordinary choreography messages. A control message is
written as `g::Msg<LABEL, GenericCapToken<K>, K>`, where `K` implements the
protocol-neutral control-kind traits.

```rust
use hibana::g;
use hibana::integration::cap::GenericCapToken;

type Grant = g::Msg<70, GenericCapToken<MyControlKind>, MyControlKind>;

let control_step = g::send::<g::Role<0>, g::Role<1>, Grant, 0>();
```

The message label is choreography identity. Control meaning comes from the
control kind's descriptor metadata, not from reserved numeric labels.

A custom wire control kind separates message label and control metadata:

```rust,ignore
use hibana::integration::cap::{CapShot, ControlResourceKind, ResourceKind};
use hibana::integration::cap::advanced::{
    CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
};
use hibana::integration::ids::{Lane, SessionId};

const CUSTOM_WIRE_MSG_LABEL: u8 = 200;
const CUSTOM_WIRE_TAP_ID: u16 = 0x03c8;

struct CustomWireKind;

impl ResourceKind for CustomWireKind {
    type Handle = ();
    const TAG: u8 = 0x90;
    const NAME: &'static str = "CustomWire";

    fn encode_handle(_: &Self::Handle) -> [u8; CAP_HANDLE_LEN] { [0; CAP_HANDLE_LEN] }
    fn decode_handle(_: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> { Ok(()) }
    fn zeroize(_: &mut Self::Handle) {}
}

impl ControlResourceKind for CustomWireKind {
    const SCOPE: ControlScopeKind = ControlScopeKind::None;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = CUSTOM_WIRE_TAP_ID;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = false;

    fn mint_handle(_: SessionId, _: Lane, _: ScopeId) -> Self::Handle { () }
}

type CustomWireMsg =
    g::Msg<{ CUSTOM_WIRE_MSG_LABEL }, GenericCapToken<CustomWireKind>, CustomWireKind>;
```

Use `AUTO_MINT_WIRE = true` only when the endpoint can mint the wire token from
descriptor-backed policy inputs. Otherwise send an explicit
`GenericCapToken<K>` payload.

## Protocol Integration

Protocol crates use the same `hibana::g` language as applications. There is
no second composition language.

### Compose And Project

A protocol crate may place transport or appkit prefixes before the application
choreography, then project each role.

```rust
use hibana::g;
use hibana::integration::program::{project, RoleProgram};

let prefix = g::seq(
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, ()>, 0>(),
    g::send::<g::Role<1>, g::Role<0>, g::Msg<2, ()>, 0>(),
);

let app = g::seq(
    g::send::<g::Role<0>, g::Role<1>, g::Msg<10, u32>, 0>(),
    g::send::<g::Role<1>, g::Role<0>, g::Msg<11, u32>, 0>(),
);

let program = g::seq(prefix, app);

let client: RoleProgram<0> = project(&program);
let server: RoleProgram<1> = project(&program);
```

`project(&program)` is the projection boundary. Runtime code consumes the
projected descriptor; it does not rediscover protocol shape.

### Attach An Endpoint

The canonical integration path is borrowed and caller-provided:

```rust,ignore
use hibana::integration;
use hibana::integration::ids::SessionId;
use hibana::integration::runtime::{Config, CounterClock, DefaultLabelUniverse};

let mut tap_buf = [integration::tap::TapEvent::zero(); 128];
let mut slab = [0u8; 64 * 1024];
let config = Config::from_resources(&mut tap_buf, &mut slab, CounterClock::new());

let clock = CounterClock::new();
let kit: integration::SessionKit<'_, MyTransport, DefaultLabelUniverse, CounterClock, 4> =
    integration::SessionKit::new(&clock);

let rv = kit.add_rendezvous_from_config(config, transport)?;
let endpoint = kit.enter(rv, SessionId::new(1), &client, integration::binding::NoBinding)?;
```

`Config::from_resources` takes only storage and clock. Lane domain, endpoint
lease capacity, and operational wait fuses are not caller-selected config. A
fresh rendezvous starts with no materialized lane storage and no endpoint lease
table. Role attach reads the projected resident descriptor, grows exactly the
lane tables and endpoint lease entries it needs, and preserves existing session
state if a later projected role needs a wider lane span. Operational fuses
belong to the transport/substrate owner and are reported by the transport
instance; expiry poisons the session generation and never selects a protocol
branch. Integration code must not pass caller-chosen lane windows, endpoint
counts, or deadline knobs.

Attach does not lower a projected role. Attach reads the pre-existing
`CompiledRoleImage` owned by the projected program image and initializes only
endpoint/session state. The role image already carries its `CompiledProgramRef`;
attach must not reconstruct that program ref from a transient role builder or
attach-time descriptor build path. The resident `CompiledRoleImage` is the
ROM/static descriptor input to attach, not a product of attach-time descriptor
construction. A role with no resident descriptor is not attachable.

The resident compiled image is the source of truth. Attach must not rebuild the
role descriptor or program descriptor through an alternate materialization path,
and must not reserve lowering scratch. Immutable queries against the resident
`CompiledProgramImage` are descriptor reads; they are not attach lowering and
must not allocate, clone, or reserve scratch. Runtime route-frontier workspace
is separate: it is descriptor-derived endpoint/session workspace for live
offer/decode state, not attach-time lowering scratch, and it must not overlap
payload scratch. If stable Rust cannot express a particular exact-sized static
layout, Hibana changes the resident image representation; it does not keep
attach-time lowering logic.

Runtime frontier entries are compact headers. They may remember live lane,
scope, frontier, summary, and selection bits, but they must not cache
descriptor-derived frame-label metadata, arm-materialization tables, route
dispatch rows, or observed-state summaries. Those facts are read from the
resident descriptor or recomputed from live evidence at the wait site. This keeps
offer/frontier progress from reintroducing attach-time materialization through a
different name.

The protocol crate owns concrete `MyTransport` and any binding state. The
application receives only `Endpoint`.

Useful integration owners:

- `integration::program::{project, RoleProgram, MessageSpec, StaticControlDesc}`
- `integration::SessionKit`
- `integration::runtime::{Config, CounterClock, DefaultLabelUniverse, LabelUniverse}`
- `integration::ids::{EffIndex, Lane, RendezvousId, SessionId}`
- `integration::Transport`
- `integration::binding::{BindingSlot, NoBinding}`
- `integration::policy::{ResolverContext, ResolverError, ResolverRef, RouteResolution, LoopResolution}`
- `integration::policy::signals::{PolicySlot, PolicySignals, PolicyAttrs, ContextId, ContextValue}`
- `integration::wire::{Payload, WireEncode, WirePayload}`
- `integration::cap::{GenericCapToken, ResourceKind, ControlResourceKind, CapShot, One, Many}`
- `integration::tap::TapEvent`

Advanced buckets under `integration::binding::advanced`,
`integration::transport::advanced`, and `integration::cap::advanced` are for custom
integration code that needs demux metadata, transport observation, or
control-kind descriptor constants.

### Transport

Implement `integration::Transport` to connect Hibana to an I/O system.

The transport owns:

- `open(local_role, session_id, lane)` for role/session/lane-specific handles;
- `poll_send(...)` and `poll_recv(...)`;
- `cancel_send(...)` for transport cleanup when a send future is dropped;
- `requeue(...)` for frames that descriptor checks cannot consume yet;
- `recv_frame_hint(...)` as a non-blocking route-observation hint drain;
- `drain_events(...)`, `metrics()`, and `apply_pacing_update(...)`.

Transport sees bytes, frame labels, readiness, and metrics. It does not own
choreography meaning, route authority, retry policy, or cancellation semantics.
`cancel_send(...)` is not an application cancellation API; it is only a cleanup
hook for an uncommitted send preview.

The `lane` passed to `open(...)` is the logical lane owned by the returned
handles. A transport that multiplexes lanes over one carrier must preserve that
lane in carrier metadata and demultiplex before yielding payload bytes to the
endpoint. `recv_frame_hint(...)` must not consume payload bytes, but it is a
hint-drain: once it yields a frame label, it must not yield the same observation
again until `poll_recv(...)` or `requeue(...)` stages fresh receive state.
Route-observation hints are lane-scoped. A frame label alone is not route
authority; the endpoint checks any hint against projected lane and descriptor
metadata, and a hint can never select a route arm without resolver / route /
payload evidence.

Transport observation reaches resolvers as packed `PolicyAttrs`; custom
transports expose that view through
`transport::advanced::TransportMetrics::attrs()`.

### Binding

Use `integration::binding::NoBinding` when the transport can deliver the next
payload directly.

Use `BindingSlot` when the protocol has multiplexed streams or channels. A
binding slot may return `IngressEvidence` for a lane and later read from the
selected channel:

```rust,ignore
impl hibana::integration::binding::BindingSlot for MyBinding {
    fn poll_incoming_for_lane(
        &mut self,
        lane: u8,
    ) -> Option<hibana::integration::binding::advanced::IngressEvidence> {
        self.next_evidence_for(lane)
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: hibana::integration::binding::advanced::Channel,
        scratch: &'a mut [u8],
    ) -> Result<
        hibana::integration::wire::Payload<'a>,
        hibana::integration::binding::advanced::TransportOpsError,
    > {
        self.read_channel(channel, scratch)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn hibana::integration::policy::PolicySignalsProvider> {
        Some(self)
    }
}
```

`IngressEvidence` is demux evidence only. It may support descriptor-checked
route observation, but it is not an independent route decision and must not be
used as dynamic route authority without resolver authority.

### Resolver Policy

Resolvers are installed by the protocol crate for explicit policy points:

```rust,ignore
fn choose_route(
    state: &RouteState,
    ctx: hibana::integration::policy::ResolverContext,
) -> Result<hibana::integration::policy::RouteResolution, hibana::integration::policy::ResolverError>
{
    if ctx.input(0) != 0 {
        return Ok(hibana::integration::policy::RouteResolution::Arm(state.preferred_arm));
    }

    Ok(hibana::integration::policy::RouteResolution::Defer)
}

kit.set_resolver::<POLICY_ID, 0>(
    rv,
    &client,
    hibana::integration::policy::ResolverRef::route_state(&state, choose_route),
)?;
```

Policy inputs are slot-scoped. Resolver failure rejects the step; it does not
fall through to a different semantic path.

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
- operational deadlines poison the current session generation and never select
  route arms;
- payload decode is exact;
- message logical labels and transport frame labels are separate concepts;
- control semantics are descriptor metadata, not reserved numeric labels;
- route authority is limited to projected facts and explicit resolver
  decisions; descriptor-checked transport observation may only confirm or demux
  projected facts.

What application code should not do:

- call transport APIs directly from localside logic;
- choose route arms by parsing payloads;
- model dynamic policy as driver-side branching;
- treat binding hints or frame labels as route authority;
- match endpoint errors to continue the same generation on a hidden alternate path;
- use shared memory, shared atomics, global flags, or side-channel state as
  route readiness, loop-control, or protocol-progress authority;
- expose protocol-specific APIs through the `hibana` crate root.

## Validation

For a published crate consumer, the useful checks are ordinary Cargo commands:

```bash
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 test -p hibana --features std
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

For a repository checkout, maintainers should run the repository gate suite
before release. That suite protects the public surface, `no_std` build,
projection boundary, descriptor streaming, future layout, route authority, and
size measurements. It is intentionally kept outside the crate package.
