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
  -> kit.add_rendezvous_from_config(...)
  -> kit.rendezvous(...).session(...).role(...)
  -> role witness .enter(...)
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
hibana = "0.7.0"
```

The default feature set is empty. Hibana is `#![no_std]` and no-alloc-oriented
by default.

Enable `std` only for host-side tests, diagnostics, and documentation builds:

```toml
[dependencies]
hibana = { version = "0.7.0", features = ["std"] }
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

`flow()` and `offer()` are previews. Endpoint progress happens when
`flow().send()`, `recv()`, or `RouteBranch::decode()` succeeds. A failed preview
does not move the endpoint and does not choose an alternate route. Preview
evidence can wake or guide polling, but it cannot mint a continuation.

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
use hibana::integration::cap::control::RouteDecisionKind;

let accepted = g::seq(
    g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<30, (), RouteDecisionKind>,
        0,
    >(),
    g::send::<g::Role<0>, g::Role<1>, g::Msg<31, u32>, 0>(),
);
let rejected = g::seq(
    g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<32, (), RouteDecisionKind>,
        0,
    >(),
    g::send::<g::Role<0>, g::Role<1>, g::Msg<33, ()>, 0>(),
);
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
from the projected descriptor or from an explicit resolver decision at a
projected route point. Transport observation may only supply demux evidence that
is checked against descriptor metadata; a frame label, payload shape, or binding
hint is never an independent route decision.

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
diagnostic evidence. It does not authorize retry, reconnect, or a different
branch in the same generation.

There is intentionally no `recv_timeout`, `send_timeout`, public `cancel`, or
same-generation recovery API. If time should select a branch, model time in the
choreography itself: use a timer or clock role and an explicit route point, then
install a resolver for that route.

Protocol-invisible liveness detection belongs inside the transport adapter. A
UDP, serial, or custom carrier that decides an I/O wait is terminal must return
`TransportError` from `poll_send(...)` or `poll_recv(...)`; Hibana converts that
transport failure into terminal session evidence. Such watchdogs do not create
hidden route authority, retry policy, or same-generation recovery in Hibana.

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
// resident transport frame.
```

### Dynamic Policy

Dynamic policy is explicit. Mark the controller self-send that opens each
route or loop arm with `Program::policy::<POLICY_ID>()`, then let the
protocol crate install a resolver for that policy id. The policy annotation is
on the arm head, not on the `g::route(...)` wrapper.

```rust
use hibana::g;
use hibana::integration::cap::control::RouteDecisionKind;

const POLICY_ID: u16 = 7;

let left = g::send::<
    g::Role<0>,
    g::Role<0>,
    g::Msg<60, (), RouteDecisionKind>,
    0,
>()
.policy::<POLICY_ID>();

let right = g::send::<
    g::Role<0>,
    g::Role<0>,
    g::Msg<61, (), RouteDecisionKind>,
    0,
>()
.policy::<POLICY_ID>();

let routed = g::route(left, right);
```

Policy does not appear as driver `if`/`else` logic. It is a choreography point
resolved through the integration policy seam.

If a resolver returns `Defer`, the offer remains pending unless new route
evidence or a valid resolver decision appears. Hibana does not maintain
offer-time defer budgets, synthetic poll retries, progress-exhaustion escape
paths, or hidden deadline fuses.

### Control Messages

Control messages are ordinary choreography messages. Endpoint-owned local
controls are written as `g::Msg<LABEL, (), K>`. Explicit wire controls are
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

There are two public layers:

- `GenericCapToken<K>` plus `ControlResourceKind` is the choreography message
  shape for explicit wire control payloads. Local control payloads are `()`.
- `integration::cap::control::ControlOp` is the built-in descriptor opcode
  catalogue evaluated by the hibana control kernel.

Only route and loop decision owners are provided as built-in public kind
types:

```rust
use hibana::g;
use hibana::integration::cap::control::{LoopBreakKind, LoopContinueKind};

type Continue = g::Msg<80, (), LoopContinueKind>;
type Break = g::Msg<81, (), LoopBreakKind>;

let continue_step = g::send::<g::Role<0>, g::Role<0>, Continue, 0>();
let break_step = g::send::<g::Role<0>, g::Role<0>, Break, 0>();
```

`RouteDecisionKind`, `LoopContinueKind`, and `LoopBreakKind` are local
self-send controls. They are how route arms and route-loop heads carry explicit
controller decisions without adding a second choreography language. They may be
used with `Program::policy::<ID>()` when a resolver must choose the arm.

The full built-in control-op catalogue is:

| Opcode | Meaning | Usual use |
| --- | --- | --- |
| `ControlOp::RouteDecision` | Selects a binary route arm for a route scope. | `RouteDecisionKind` on the controller self-send, optionally resolver-backed. |
| `ControlOp::LoopContinue` | Selects the continue arm of a route loop. | `LoopContinueKind` at the loop head. |
| `ControlOp::LoopBreak` | Selects the break arm of a route loop. | `LoopBreakKind` at the loop head. |
| `ControlOp::Fence` | Orders or authorizes a protocol-visible control boundary without changing topology or transaction state. | Protocol-owned wire or local control barriers. |
| `ControlOp::StateSnapshot` | Records the current session/lane generation before a mutation. | Snapshot before transaction, abort, restore, or topology-sensitive mutation. |
| `ControlOp::StateRestore` | Restores previously snapshotted state after a failed or aborted mutation. | Rollback path paired with `StateSnapshot`. |
| `ControlOp::TxCommit` | Commits a snapshot-backed transaction and finalizes that lane generation. | At-most-once commit of a protocol mutation. |
| `ControlOp::TxAbort` | Aborts a snapshot-backed transaction and records the abort path. | Fail-closed transaction cancellation. |
| `ControlOp::AbortBegin` | Starts an explicit abort handshake. | First step of a protocol-owned abort sequence. |
| `ControlOp::AbortAck` | Acknowledges an abort handshake. | Idempotent acknowledgement for abort completion. |
| `ControlOp::TopologyBegin` | Opens a topology transition intent with source/destination rendezvous, lane, and generation facts. | Distributed lane/rendezvous reconfiguration. |
| `ControlOp::TopologyAck` | Validates and acknowledges a topology intent at the destination side. | Destination half of topology coordination. |
| `ControlOp::TopologyCommit` | Commits an acknowledged topology transition and bumps generation. | Source-side topology finalization. |

These opcodes are not new application commands. A protocol that needs topology,
transaction, abort, snapshot, or fence control still writes ordinary
choreography messages, usually with a protocol-owned `ControlResourceKind` that
maps to the relevant `ControlOp`. The runtime then consumes the projected
descriptor metadata fail-closed. Payload contents, labels, transport hints, and
driver `if`/`else` logic never become route or transaction authority.

`ControlPath` decides where the control is executed:

- `ControlPath::Local` is a local self-send. `g::send` rejects cross-role local
  controls.
- `ControlPath::Wire` is a wire-visible cross-role send. `g::send` rejects
  self-sent wire controls.

A custom wire control kind separates message label and control metadata:

```rust,ignore
use hibana::integration::cap::{CapShot, ControlResourceKind, ResourceKind};
use hibana::integration::cap::control::{
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

Use `()` for local endpoint-owned controls. Use an explicit
`GenericCapToken<K>` payload only for wire controls that carry a protocol-owned
token.

Topology and transaction control are integration-level tools, not application
state machines. Use them when the protocol itself needs a choreography-visible
state transition:

- topology: move or rebind a lane/rendezvous relation with
  `TopologyBegin -> TopologyAck -> TopologyCommit`;
- transaction: bracket a multi-step mutation with
  `StateSnapshot -> TxCommit` or `StateSnapshot -> TxAbort/StateRestore`;
- abort: make cancellation explicit with `AbortBegin -> AbortAck`;
- fence: insert a protocol-owned ordering or readiness boundary without adding
  domain-specific APIs to hibana core.

Do not add `g::topology`, `g::tx`, driver-side retry loops, or payload-driven
branch selection. The authority source remains the choreography plus the
projected descriptor.

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
use hibana::integration::runtime::{Config, CounterClock, RING_EVENTS};

let mut tap_buf = [integration::runtime::TapEvent::zero(); RING_EVENTS];
let mut slab = [0u8; 64 * 1024];

let clock = CounterClock::new();
let mut kit_storage = integration::SessionKitStorage::<MyTransport>::uninit();
let kit = kit_storage.init();

let config = Config::from_resources((&mut tap_buf, &mut slab), clock);
let rv = kit.add_rendezvous_from_config(config, transport)?;
let endpoint = kit.rendezvous(rv).session(SessionId::new(1)).role(&client).enter(None)?;
```

`SessionKitStorage::init()` is the only public construction path. It writes the
kit in place into caller-owned resident storage, returns the stable borrow used
by endpoint attach, and drops the initialized kit exactly once. The raw unsafe
initializer and `MaybeUninit` protocol are not part of the public surface.

`Config::from_resources` owns the rendezvous storage and clock authority. Lane
domain and endpoint lease capacity are not caller-selected config. A fresh
rendezvous starts with no materialized lane storage and no endpoint lease table.
Role attach reads the projected resident descriptor, grows exactly the lane
tables and endpoint lease entries it needs, and preserves existing session state
if a later projected role needs a wider lane span. Integration code must not
pass caller-chosen lane windows, endpoint counts, or deadline knobs.

The protocol crate owns concrete `MyTransport` and any binding state. The
application receives only `Endpoint`.

Useful integration owners:

- `integration::program::{project, RoleProgram, MessageSpec}`
- `integration::SessionKit`
- `integration::runtime::{Config, CounterClock, DefaultLabelUniverse, LabelUniverse, RING_EVENTS}`
- `integration::ids::{EffIndex, Lane, RendezvousId, SessionId}`
- `integration::transport::Transport`
- `integration::binding::{BindingError, BindingSlot, Channel, IngressEvidence}`
- `integration::policy::{ResolverContext, ResolverError, ResolverRef, DecisionArm, DecisionResolution}`
- `integration::policy::signals::{PolicyInput, PolicySignals, PolicyAttrs}`
- `integration::wire::{Payload, WireEncode, WirePayload}`
- `integration::cap::{GenericCapToken, ResourceKind, ControlResourceKind, CapShot}`
- `integration::runtime::TapEvent`

Control descriptor constants live under `integration::cap::control`.

### Transport

Implement `integration::transport::Transport` to connect Hibana to an I/O system.

The transport owns:

- `open(port)` for the descriptor-derived role/session/lane port witness;
- `poll_send(...)` and `poll_recv(...)`;
- `cancel_send(...)` for transport cleanup when a send future is dropped after
  staging carrier state;
- `requeue(...)` as the required rollback path for a frame that descriptor
  checks cannot commit.

`open(port)` returns Tx/Rx handles whose lifetime is bound to the transport
borrow, so an embedded carrier can keep buffers, wakers, and DMA bookkeeping
inside the transport owner without allocating or exporting a separate context.

The only optional transport hook is:

- `recv_frame_hint(...)` as a non-blocking route-observation hint drain.

Transport sees bytes, frame labels, and readiness. It does not own choreography
meaning, route authority, retry policy, policy inputs, telemetry, or cancellation semantics.
`cancel_send(...)` is not an application cancellation API; it is only a cleanup
hook for an uncommitted send preview.
Protocol-invisible carrier watchdogs belong inside `poll_send(...)` and
`poll_recv(...)`: if the transport concludes that progress is impossible, it
returns `TransportError` and Hibana terminates the current session generation.

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

Resolver input belongs to binding / integration policy state, not transport.
Bindings expose only decision-policy signals; core audit slots use internal
zero-signal evidence unless the resolver asks for policy input.
`PolicyAttrs` stays core-defined, while each binding projects its own
policy-specific `PolicyInput` primary value for route or loop control decisions.

### Binding

Pass `None` to `enter(...)` when the transport can deliver the next payload
directly.

Use `BindingSlot` when the integration demuxes ingress into binding-owned
payload handles. A binding slot may return `IngressEvidence` for a lane and
later read from the selected handle:

```rust,ignore
impl hibana::integration::binding::BindingSlot for MyBinding {
    fn poll_incoming_for_lane(
        &mut self,
        lane: u8,
    ) -> Option<hibana::integration::binding::IngressEvidence> {
        self.next_evidence_for(lane)
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: hibana::integration::binding::Channel,
        scratch: &'a mut [u8],
    ) -> Result<
        hibana::integration::wire::Payload<'a>,
        hibana::integration::binding::BindingError,
    > {
        self.read_channel(channel, scratch)
    }

    fn policy_signals(
        &self,
    ) -> hibana::integration::policy::signals::PolicySignals {
        hibana::integration::policy::signals::PolicySignals::new(
            self.decision_input(),
            self.decision_attrs(),
        )
    }
}
```

`IngressEvidence` is demux evidence only. It may support descriptor-checked
route observation, but it is not an independent route decision and must not be
used as dynamic route authority without resolver authority.

### Resolver Policy

Resolvers are installed by the protocol crate for explicit policy points.
Route and loop control messages use the same decision vocabulary; loop is not
a separate user-facing resolver API:

```rust,ignore
struct DecisionState {
    preferred_arm: hibana::integration::policy::DecisionArm,
}

fn choose_decision(
    state: &DecisionState,
    ctx: hibana::integration::policy::ResolverContext,
) -> Result<hibana::integration::policy::DecisionResolution, hibana::integration::policy::ResolverError>
{
    if ctx.primary_input() != 0 {
        return Ok(hibana::integration::policy::DecisionResolution::Arm(state.preferred_arm));
    }

    Ok(hibana::integration::policy::DecisionResolution::Defer)
}

kit.rendezvous(rv).role(&client).set_resolver::<POLICY_ID>(
    hibana::integration::policy::ResolverRef::decision_state(&state, choose_decision),
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
