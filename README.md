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
  -> substrate::program::project(&program)
  -> substrate::SessionKit::enter(...)
  -> Endpoint
  -> flow().send() / recv() / offer() / RouteBranch::decode()
```

There are only two public surfaces:

| Surface | Used by | Main names |
| --- | --- | --- |
| Application surface | application code | `hibana::g`, `Endpoint`, `RouteBranch`, `SendResult`, `RecvResult` |
| Substrate surface | protocol and integration crates | `hibana::substrate`, `hibana::substrate::program` |

If you are writing an application, stay on `hibana::g` and `Endpoint`. If you
are implementing a protocol crate, use `hibana::substrate` to project, attach,
bind transport, install policy, and return endpoints.

## Install

Add Hibana from [crates.io](https://crates.io/crates/hibana):

```bash
cargo add hibana
```

Or write the dependency explicitly:

```toml
[dependencies]
hibana = "0.2.0"
```

The default feature set is empty. Hibana is `#![no_std]` and no-alloc-oriented
by default.

Enable `std` only for host-side tests, diagnostics, and documentation builds:

```toml
[dependencies]
hibana = { version = "0.2.0", features = ["std"] }
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
decode succeeds.

## Application Guide

Application authors only need these names:

- `hibana::g::{Role, Msg, Program, send, seq, route, par}`
- `Endpoint`
- `RouteBranch`
- `SendResult<T>` and `RecvResult<T>`
- `SendError` and `RecvError`

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
from the projected descriptor, explicit resolver policy, or transport-observed
evidence consumed through descriptor checks.

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

Custom payloads implement `hibana::substrate::wire::WireEncode` for sending
and `hibana::substrate::wire::WirePayload` for receiving:

```rust
use hibana::substrate::wire::{CodecError, Payload, WireEncode, WirePayload};

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

Dynamic policy is explicit. Mark a route or loop decision point with
`Program::policy::<POLICY_ID>()`, then let the protocol crate install a
resolver for that policy id.

```rust
use hibana::g;

const POLICY_ID: u16 = 7;

let left = g::send::<g::Role<0>, g::Role<1>, g::Msg<60, ()>, 0>();
let right = g::send::<g::Role<0>, g::Role<1>, g::Msg<61, ()>, 0>();
let routed = g::route(left, right).policy::<POLICY_ID>();
```

Policy does not appear as driver `if`/`else` logic. It is a choreography point
resolved through the substrate policy seam.

### Control Messages

Control messages are ordinary choreography messages. A control message is
written as `g::Msg<LABEL, GenericCapToken<K>, K>`, where `K` implements the
protocol-neutral control-kind traits.

```rust
use hibana::g;
use hibana::substrate::cap::GenericCapToken;

type Grant = g::Msg<70, GenericCapToken<MyControlKind>, MyControlKind>;

let control_step = g::send::<g::Role<0>, g::Role<1>, Grant, 0>();
```

The message label is choreography identity. Control meaning comes from the
control kind's descriptor metadata, not from reserved numeric labels.

A custom wire control kind separates message label and control metadata:

```rust,ignore
use hibana::substrate::cap::{CapShot, ControlResourceKind, ResourceKind};
use hibana::substrate::cap::advanced::{
    CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
};
use hibana::substrate::ids::{Lane, SessionId};

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
use hibana::substrate::program::{project, RoleProgram};

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

The canonical substrate path is borrowed and caller-provided:

```rust,ignore
use hibana::substrate;
use hibana::substrate::ids::SessionId;
use hibana::substrate::runtime::{Config, CounterClock, DefaultLabelUniverse};

let mut tap_buf = [substrate::tap::TapEvent::zero(); 128];
let mut slab = [0u8; 64 * 1024];
let config = Config::new(&mut tap_buf, &mut slab);

let clock = CounterClock::new();
let kit: substrate::SessionKit<'_, MyTransport, DefaultLabelUniverse, CounterClock, 4> =
    substrate::SessionKit::new(&clock);

let rv = kit.add_rendezvous_from_config(config, transport)?;
let endpoint = kit.enter(rv, SessionId::new(1), &client, substrate::binding::NoBinding)?;
```

The protocol crate owns concrete `MyTransport` and any binding state. The
application receives only `Endpoint`.

Useful substrate owners:

- `substrate::program::{project, RoleProgram, MessageSpec, StaticControlDesc}`
- `substrate::SessionKit`
- `substrate::runtime::{Config, CounterClock, DefaultLabelUniverse, LabelUniverse}`
- `substrate::ids::{EffIndex, Lane, RendezvousId, SessionId}`
- `substrate::Transport`
- `substrate::binding::{BindingSlot, NoBinding}`
- `substrate::policy::{ResolverContext, ResolverError, ResolverRef, RouteResolution, LoopResolution}`
- `substrate::policy::signals::{PolicySlot, PolicySignals, PolicyAttrs, ContextId, ContextValue}`
- `substrate::wire::{Payload, WireEncode, WirePayload}`
- `substrate::cap::{GenericCapToken, ResourceKind, ControlResourceKind, CapShot, One, Many}`
- `substrate::tap::TapEvent`

Advanced buckets under `substrate::binding::advanced`,
`substrate::transport::advanced`, and `substrate::cap::advanced` are for custom
integration code that needs demux metadata, transport observation, or
control-kind descriptor constants.

### Transport

Implement `substrate::Transport` to connect Hibana to an I/O system.

The transport owns:

- `open(local_role, session_id)` for role/session-specific handles;
- `poll_send(...)` and `poll_recv(...)`;
- `cancel_send(...)` for dropped affine send futures;
- `requeue(...)` for frames that descriptor checks cannot consume yet;
- `recv_frame_hint(...)` as a non-blocking demux hint;
- `drain_events(...)`, `metrics()`, and `apply_pacing_update(...)`.

Transport sees bytes, frame labels, readiness, and metrics. It does not own
choreography meaning or route authority.

Transport observation reaches resolvers as packed `PolicyAttrs`; custom
transports expose that view through
`transport::advanced::TransportMetrics::attrs()`.

### Binding

Use `substrate::binding::NoBinding` when the transport can deliver the next
payload directly.

Use `BindingSlot` when the protocol has multiplexed streams or channels. A
binding slot may return `IngressEvidence` for a lane and later read from the
selected channel:

```rust,ignore
impl hibana::substrate::binding::BindingSlot for MyBinding {
    fn poll_incoming_for_lane(
        &mut self,
        lane: u8,
    ) -> Option<hibana::substrate::binding::advanced::IngressEvidence> {
        self.next_evidence_for(lane)
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: hibana::substrate::binding::advanced::Channel,
        scratch: &'a mut [u8],
    ) -> Result<
        hibana::substrate::wire::Payload<'a>,
        hibana::substrate::binding::advanced::TransportOpsError,
    > {
        self.read_channel(channel, scratch)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn hibana::substrate::policy::PolicySignalsProvider> {
        Some(self)
    }
}
```

`IngressEvidence` is demux evidence only. It may support descriptor-checked
route observation, but it is not an independent route decision.

### Resolver Policy

Resolvers are installed by the protocol crate for explicit policy points:

```rust,ignore
fn choose_route(
    state: &RouteState,
    ctx: hibana::substrate::policy::ResolverContext,
) -> Result<hibana::substrate::policy::RouteResolution, hibana::substrate::policy::ResolverError>
{
    if ctx.input(0) != 0 {
        return Ok(hibana::substrate::policy::RouteResolution::Arm(state.preferred_arm));
    }

    Ok(hibana::substrate::policy::RouteResolution::Defer { retry_hint: 1 })
}

kit.set_resolver::<POLICY_ID, 0>(
    rv,
    &client,
    hibana::substrate::policy::ResolverRef::route_state(&state, choose_route),
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
- failed sends, receives, offers, and decodes do not authorize hidden progress;
- payload decode is exact;
- message logical labels and transport frame labels are separate concepts;
- control semantics are descriptor metadata, not reserved numeric labels;
- route authority is limited to projected facts, explicit resolver decisions,
  and descriptor-checked transport observation.

What application code should not do:

- call transport APIs directly from localside logic;
- choose route arms by parsing payloads;
- model dynamic policy as driver-side branching;
- treat binding hints or frame labels as route authority;
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
