<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA - Affine MPST runtime for Rust" />

  <p>
    <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024" />
    <img src="https://img.shields.io/badge/no__std-yes-success.svg" alt="no_std" />
    <img src="https://img.shields.io/badge/no__alloc-oriented-blue.svg" alt="no_alloc oriented" />
  </p>

  <p>
    <a href="#constitution">Constitution</a> •
    <a href="#surface-map">Surface Map</a> •
    <a href="#app-surface">App Surface</a> •
    <a href="#runtime-semantics">Runtime Semantics</a> •
    <a href="#substrate-surface-protocol-implementors-only">Substrate Surface</a> •
    <a href="#policy-plane">Policy Plane</a> •
    <a href="#management-session">Management Session</a> •
    <a href="#validation">Validation</a>
  </p>
</div>

# HIBANA

`hibana` is a Rust 2024 `#![no_std]` / `no_alloc` oriented affine MPST runtime.

It has two surfaces:

- App surface: `hibana::g` plus `Endpoint`
- Substrate surface: protocol-neutral SPI for protocol implementors

Everything else is lower layer.

This README is the canonical manual for the public `hibana` surface. If a user
needs a public owner to build with `hibana`, it is introduced here.

## Constitution

`hibana` optimizes for a smaller concept count, stronger type-level guarantees,
and a cleaner split between app code and substrate code.

- Mission: keep `hibana` as a Rust 2024 `#![no_std]` / `no_alloc` oriented
  affine MPST runtime with one choreography language and one localside core API.
- Public shape: `hibana` has exactly two public faces - app surface and
  substrate surface. Everything else is lower layer.
- Protocol neutrality: `hibana` core stays protocol-neutral. Transport- and
  integration-specific vocabulary does not live in the crate's public surface.
- Route authority: route decisions remain `Ack | Resolver | Poll`. Hints,
  cached classifications, and rescue paths are not extra authority sources.
- Static route boundary: static route remains `Merged -> Dynamic -> compile-error`.
- Surface minimization: app authors stay on `hibana::g` + `Endpoint`; protocol
  implementors stay on the substrate SPI documented later in this README.
- No heuristics: `hibana` does not rely on protocol inference, stale
  classification caches, or timeout-based rescue logic.

## Surface Map

Inside `hibana`, responsibilities are intentionally split by surface:

| surface | role |
| --- | --- |
| App surface | `hibana::g` plus `Endpoint` for app authors |
| Compile-time substrate SPI | typed projection and preserved composition, documented in the substrate SPI section below |
| Runtime substrate SPI | attach / enter / binding / resolver / policy / transport seams, documented in the substrate SPI section below |
| Lower layer | endpoint kernel, typestate internals, and runtime machinery that are not part of the public app contract |

This README documents `hibana` itself. If a concept is not owned by one of the
surfaces above, it is lower layer and not part of the public contract.

## Cargo Features

- `std` — Enables transport/testing utilities and observability normalisers.
  The runtime remains slab-backed and `no_alloc` oriented in both modes; `std`
  does not switch the core localside path to heap-backed ownership.

## App Surface

App authors should stay on `g` and `Endpoint`.

If you are reaching for projection, attach, binding, resolver registration,
transport setup, or policy installation, you are already on the substrate
surface and should move to the protocol-implementor SPI section below.

The app surface is intentionally narrow:

| Job | Public owner |
| --- | --- |
| Define choreography | `hibana::g::{send, route, par, seq}` |
| Mark dynamic authority points | `Program::policy::<POLICY_ID>()` |
| Advance a localside endpoint | `flow().send()`, `offer()`, `recv()`, `decode()` |
| Handle a chosen branch | `RouteBranch::{label, decode}` |

The public language is fixed to:

- `hibana::g::{send, route, par, seq}`
- `Program::policy::<POLICY_ID>()`
- `RouteBranch::label()`
- `RouteBranch::decode()`
- `flow().send()`
- `offer()`
- `recv()`
- `decode()`

App code does not call projection, attach, binding, resolver registration, or
policy install directly.

### Write One Choreography

`g::Program` is the only public choreography representation.

```rust
use hibana::g;

let request_response = g::seq(
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>(),
    g::send::<g::Role<1>, g::Role<0>, g::Msg<2, u32>, 0>(),
);
```

### Route, Parallel, and Dynamic Policy

`g::route(left, right)` is always binary. The controller is derived from the
first self-send in each arm. `g::par(left, right)` is also binary and requires
disjoint `(role, lane)` ownership.

```rust
use hibana::g;

let left_arm = g::seq(
    g::send::<g::Role<0>, g::Role<0>, g::Msg<10, ()>, 0>().policy::<7>(),
    g::send::<g::Role<0>, g::Role<1>, g::Msg<11, [u8; 4]>, 0>(),
);
let right_arm = g::seq(
    g::send::<g::Role<0>, g::Role<0>, g::Msg<12, ()>, 0>().policy::<7>(),
    g::send::<g::Role<0>, g::Role<1>, g::Msg<13, u16>, 0>(),
);
let routed = g::route(left_arm, right_arm);

let parallel = g::par(
    g::send::<g::Role<0>, g::Role<1>, g::Msg<20, u32>, 1>(),
    g::send::<g::Role<0>, g::Role<1>, g::Msg<21, [u8; 8]>, 2>(),
);

let app = g::seq(routed, parallel);
```

Rules worth remembering:

- annotate only the control point that actually needs dynamic authority
- static route stays `Merged -> Dynamic -> compile-error`
- duplicate route labels are compile errors
- empty `g::par` arms are compile errors
- overlapping `(role, lane)` pairs in `g::par` are rejected

### Drive Localside

After integration code returns the first app endpoint, progress stays on the
localside core API.

```rust
use hibana::g;

let outbound = endpoint.flow::<g::Msg<1, u32>>()?.send(&7).await?;
let inbound = endpoint.recv::<g::Msg<2, u32>>().await?;

let branch = endpoint.offer().await?;
match branch.label() {
    30 => {
        let payload = branch.decode::<g::Msg<30, [u8; 4]>>().await?;
        let _ = (payload, outbound, inbound);
    }
    31 => {
        drop(branch);
        let () = endpoint.flow::<g::Msg<31, ()>>()?.send(&()).await?;
    }
    _ => unreachable!(),
}
```

Use each localside API for one job:

- `flow().send()` for sends you already know statically
- `recv()` for deterministic receives
- `offer()` when the next step is a route decision
- `decode()` when the chosen arm begins with a receive
- `drop(branch); endpoint.flow().send()` when the chosen arm begins with a send

### App Result and Error Types

The crate root keeps the app-facing result and error owners explicit:

- `hibana::SendResult<T>` is returned by `flow()`
- `hibana::RecvResult<T>` is returned by `recv()`, `offer()`, and `decode()`
- `hibana::SendError` reports send-side localside failures
- `hibana::RecvError` reports receive-side localside failures
- `hibana::RouteBranch` is the only route-branch owner app code handles directly

### Compile-Time Guarantees

The public surface is small because guarantees move into the type system, not
because guarantees were deleted.

- projection stays typed through `RoleProgram<'prog, ROLE, Mint>` and defaults to `RoleProgram<'prog, ROLE>`
- `g::route` rejects duplicate labels and controller mismatches before runtime
- `g::par` rejects empty fragments and role/lane overlap before runtime
- localside runtime is fail-closed for label and payload mismatches
- dynamic route is explicit and fail-closed; it does not silently appear at runtime

## Localside Shape

The connection shape is always explained as:

```text
transport prefix -> appkit prefix -> user app
```

or, on the choreography side:

```text
g::seq(transport prefix, g::seq(appkit prefix, APP))
```

`hibana` itself only owns the app surface and the protocol-neutral substrate
surface. Lower-layer integration code is responsible for composing prefixes,
projecting the connection, and returning the first attached app endpoint.

## Runtime Semantics

The runtime model is deliberately simple: choreography is defined first, then
lower-layer integration code composes prefixes, projects typed locals, and
hands app code a single endpoint that advances through the localside core API.

### Choreography First

- The connection shape is always `transport prefix -> appkit prefix -> user app`
- On the choreography side that means
  `g::seq(transport prefix, g::seq(appkit prefix, APP))`
- `hibana` does not expose a second public DSL or a second app-facing builder

### Driver and Branching

- The driver follows `offer()`; it does not invent decisions on its own
- Branch handling is just `match branch.label()`
- Use `branch.decode()` when the chosen arm begins with a receive
- Use `drop(branch); endpoint.flow().send()` when the chosen arm begins with a send
- `flow()` and `offer()` are preview-only; endpoint progress happens only when
  `send()` or `decode()` successfully consumes the preview, including policy
  input, transport-event flush, and policy audit/replay observation
- App code and generic driver logic do not call transport APIs directly

### Route Authority

Route authority has exactly three public sources:

- `Ack` for already materialized canonical control decisions
- `Resolver` for dynamic-route resolution (EPF first, Rust resolver second)
- `Poll` for transport-observable static evidence

Important negative rule: hint labels and binding classifications are
demux/readiness evidence, not a fourth authority source. When exact static
passive ingress is normalized into `Poll`-equivalent evidence, it is still the
same `Poll`-class wire fact, not a new authority category.

Loop meaning is metadata-authoritative. Wire labels remain representation only,
and any encode/decode or dynamic-label classification needed for loop control
stays an internal endpoint seam rather than a public authority source.

### Lane and Binding Discipline

- lane `0` is control
- lane `1` is early-data
- bindings own demux and channel resolution, not route authority
- app-lane ownership comes from the protocol/appkit contract, not from `hibana`
  guessing at runtime
- unknown lanes are errors, not absorption or fallback points

### Policy and Management Boundary

- `PolicySignalsProvider::signals(slot)` is the single public slot-input boundary
- EPF executes inside the resolver slot; it is not a second public policy API
- fail-closed remains the default for verifier, trap, or fuel failures
- policy distribution and activation belong to the management prefixes under
  `hibana::substrate::mgmt`, not to endpoint-local helpers

### Responsibility Matrix

| layer | writes | reads |
| --- | :---: | :---: |
| Transport | yes | yes |
| Resolver | no | yes |
| EPF | no | yes |
| Binder | no | yes |
| Driver | no | no |

## Substrate Surface (protocol implementors only)

Protocol implementors use the protocol-neutral SPI:

- `hibana::g` owns choreography composition
- `hibana::g::advanced` owns typed projection and compile-time control-message typing
- `hibana::substrate` owns attach / enter / binding / resolver / policy /
  transport seams
- the root app surface does not expose `SessionKit`, `BindingSlot`,
  `RoleProgram`, `PhaseCursor`, or typestate internals

Everyday substrate owners:

- `hibana::g::advanced::{project, RoleProgram, CanonicalControl, ExternalControl, MessageSpec, ControlMessage, ControlMessageKind}`
- `hibana::substrate::SessionKit`
- `hibana::substrate::{AttachError, CpError, EffIndex, Lane, RendezvousId, SessionId}`
- `hibana::substrate::Transport`
- `hibana::substrate::runtime::{Clock, Config, CounterClock, DefaultLabelUniverse, LabelUniverse}`
- `hibana::substrate::binding::{BindingSlot, NoBinding}`
- `hibana::substrate::binding::NoBinding` is the canonical empty-binding owner
- `hibana::substrate::policy::{ContextId, ContextValue, DynamicResolution, PolicyAttrs, PolicySignals, PolicySignalsProvider, ResolverContext, ResolverError, ResolverRef}`
- `hibana::substrate::policy::PolicySignalsProvider` is the canonical slot-scoped policy input owner
- `hibana::substrate::cap::{CapShot, ControlResourceKind, GenericCapToken, Many, One, ResourceKind}`
- `hibana::substrate::wire::{Payload, WireDecode, WireEncode}`
- `hibana::substrate::transport::{Outgoing, TransportError, TransportEvent, TransportEventKind, TransportSnapshot}`
- `hibana::substrate::mgmt::{request_reply::PREFIX, observe_stream::PREFIX, tap::TapEvent}`

Advanced / deep-dive substrate owners:

- `hibana::substrate::policy::core::*` for fixed context-key ids
- `hibana::substrate::policy::epf::{Header, Slot}` for EPF image and slot ownership
- `hibana::substrate::cap::advanced` for mint details and the built-in control-kind catalogue
- `hibana::substrate::transport` detail owners for local send direction, algorithm reporting, and metrics translation

Everything in this section is protocol-neutral. If a protocol-specific concept
is needed, keep it outside `hibana`'s public surface.

## Protocol-Implementor Walkthrough

### 1. Compose `transport prefix -> appkit prefix -> user app`

Use ordinary `hibana::g::seq`. There is no second composition surface for
protocol implementors.

```rust
let app_connection = hibana::g::seq(APPKIT_PREFIX, APP);
let full_connection = hibana::g::seq(TRANSPORT_PREFIX, app_connection);
```

App code and protocol-implementor code both use the same `g::seq`. Segment
boundaries stay explicit in the program value itself; there is no separate
composition shim.

### 2. Project a Typed Role Witness

Projection stays typed, but protocol implementations do not need to spell the
projected local typelist in public code. The canonical path is
`Program<_>` -> `project(&PROGRAM)`.

```rust
use hibana::g;
use hibana::g::advanced::{RoleProgram, project};

const PROGRAM: g::Program<_> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>();

let client: RoleProgram<'_, 0> = project(&PROGRAM);
```

`RoleProgram` remains a typed compile-time witness, and the substrate recovers
the role-local projection internally. Protocol code passes that witness to
`SessionKit::enter(...)` and other substrate seams without spelling the
projected local typelist in the canonical path.

## Control Message Surface

There is no public `g::splice`, `g::delegate`, or `g::reroute`.

`delegate`, `splice`, `reroute`, `route`, `loop`, and management policy
operations are all expressed as `g::send()` steps whose message type carries a
capability token and a control handling marker.

The protocol-implementor compile-time owners are:

- `CanonicalControl<K>` for locally minted control tokens
- `ExternalControl<K>` for control tokens carried on the wire
- `MessageSpec` for label/payload/control typing
- `ControlMessage` and `ControlMessageKind` for control-message-only contracts
Handling rules are fixed by the implementation:

- `CanonicalControl<K>` is compile-time restricted to self-send
- `ExternalControl<K>` may cross roles and ride the wire
- `ControlHandling` is the canonical owner for the handling mode carried by a control kind
- the operation itself comes from the control kind's resource tag, not from a second DSL

## Transport Seam

`hibana::substrate::Transport` is the protocol-neutral I/O seam. It owns send,
recv, requeue, event draining, hint exposure, metrics, and pacing updates.
`Send` / `Recv` future types must be `Unpin`; hibana parks and re-polls the same
transport future until the operation completes.

```rust
struct MyTransport;

impl hibana::substrate::Transport for MyTransport {
    type Error = hibana::substrate::transport::TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = ()
    where
        Self: 'a;
    type Send<'a>
        = core::future::Ready<Result<(), Self::Error>>
    where
        Self: 'a;
    type Recv<'a>
        = core::future::Ready<Result<hibana::substrate::wire::Payload<'a>, Self::Error>>
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, local_role: u8, session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let local_role_value = local_role;
        let session_id_value = session_id;
        let _state = (local_role_value, session_id_value);
        ((), ())
    }

    fn send<'a, 'f>(
        &'a self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: hibana::substrate::transport::Outgoing<'f>,
    ) -> Self::Send<'a>
    where
        'a: 'f,
    {
        let tx_handle = tx;
        let payload_view = outgoing.payload;
        let send_meta = outgoing.meta;
        let _state = (tx_handle, payload_view, send_meta);
        core::future::ready(Ok(()))
    }

    fn recv<'a>(&'a self, rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        static EMPTY: [u8; 0] = [];
        let rx_handle = rx;
        let _state = rx_handle;
        core::future::ready(Ok(hibana::substrate::wire::Payload::new(&EMPTY)))
    }

    fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {
        let rx_handle = rx;
        let _state = rx_handle;
    }

    fn drain_events(
        &self,
        emit: &mut dyn FnMut(hibana::substrate::transport::TransportEvent),
    ) {
        emit(hibana::substrate::transport::TransportEvent::new(
            hibana::substrate::transport::TransportEventKind::Ack,
            10,
            1200,
            0,
        ));
    }

    fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8> {
        let rx_handle = rx;
        let _state = rx_handle;
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, interval_us: u32, burst_bytes: u16) {
        let pacing_state = (interval_us, burst_bytes);
        let _state = pacing_state;
    }
}
```

Transport rules:

- `recv()` must yield borrowed payload views
- `requeue()` is how transport hands an unconsumed frame back
- `drain_events()` feeds protocol-neutral transport observation
- `recv_label_hint()` is a demux hint, not route authority
- `metrics()` returns `TransportSnapshot` through `TransportMetrics`

## Bootstrap `SessionKit` and Enter Typed Endpoints

`hibana::substrate::SessionKit::new(&clock)` is the canonical starting point.
The borrowed / `no_alloc`-oriented path is the canonical substrate path: keep the
clock, config storage, projected program, and resolver state borrowed; add
rendezvous once; then `enter()`.

```rust
let mut tap_buf = [hibana::substrate::mgmt::tap::TapEvent::zero(); 128];
let mut slab = [0u8; 64 * 1024];
let clock = hibana::substrate::runtime::CounterClock::new();
let config = hibana::substrate::runtime::Config::new(&mut tap_buf, &mut slab);

let cluster: hibana::substrate::SessionKit<
    '_,
    MyTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    hibana::substrate::runtime::CounterClock,
    4,
> = hibana::substrate::SessionKit::new(&clock);

let transport = MyTransport;
let rv_id = cluster.add_rendezvous_from_config(config, transport)?;

let endpoint = cluster.enter(
    rv_id,
    hibana::substrate::SessionId::new(1),
    &CLIENT,
    hibana::substrate::binding::NoBinding,
)?;
```

The same borrowed `RoleProgram` can be passed to `set_resolver()` and `enter()`,
and the same borrowed resolver state can stay outside the cluster through
`ResolverRef::from_state()`.

The canonical substrate story stays borrowed and caller-provided even under
`std`: lower-layer storage is slab-backed, not heap-backed, and app/protocol
integration should provide stable buffers and clocks without `Box`, `Vec`, or
stack-growth helpers.

```rust
let mut tap_buf = [hibana::substrate::mgmt::tap::TapEvent::zero(); 128];
let mut slab = [0u8; 64 * 1024];
let clock = hibana::substrate::runtime::CounterClock::new();

let config = hibana::substrate::runtime::Config::new(&mut tap_buf, &mut slab);

let cluster: hibana::substrate::SessionKit<
    '_,
    MyTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    hibana::substrate::runtime::CounterClock,
    4,
> = hibana::substrate::SessionKit::new(&clock);
```

On embedded targets, these borrowed buffers still need stable storage owned by
the integration layer. The canonical docs do not use `static mut`; keep any
boot-storage `unsafe` private to the binary or BSP glue.

`SessionKit::new(&clock)` is always paired with a rendezvous config. The
runtime config owner is `hibana::substrate::runtime::Config`, and the public
customisation points are:

- `Config::new(tap_buf, slab)` to allocate tap storage and the rendezvous slab
- `Config::tap_storage()` and `Config::slab()` to inspect or reuse the owned buffers
- `Config::with_lane_range(range)` to reserve lane space for the transport/appkit split; the configured window must still include reserved control lane `0`
- `Config::lane_range()` to inspect the configured lane ownership window
- `Config::with_universe(universe)` to install a custom label universe
- `Config::universe()` to inspect the active label universe
- `Config::with_clock(clock)` to move from `CounterClock` to another clock owner
- `Config::clock()` to inspect the active clock owner

If cluster bootstrap fails before attachment, the substrate errors are
`hibana::substrate::CpError` and `hibana::substrate::AttachError`.

## BindingSlot Contract

`BindingSlot` is the transport-adapter seam for framed streams, multiplexed
channels, and slot-scoped policy signals. It is also the place where protocol
code supplies `PolicySignalsProvider`.

`BindingSlot` is demux and transport observation only. It does not decide route arms.

```rust
use hibana::substrate::policy::epf;

struct MyBinding {
    signals: hibana::substrate::policy::PolicySignals,
}

impl hibana::substrate::policy::PolicySignalsProvider for MyBinding {
    fn signals(
        &self,
        slot: epf::Slot,
    ) -> hibana::substrate::policy::PolicySignals {
        let route_slot = slot;
        let _state = route_slot;
        self.signals
    }
}

impl hibana::substrate::binding::BindingSlot for MyBinding {
    fn poll_incoming_for_lane(
        &mut self,
        logical_lane: u8,
    ) -> Option<hibana::substrate::binding::IncomingClassification> {
        let lane_value = logical_lane;
        let _state = lane_value;
        Some(hibana::substrate::binding::IncomingClassification {
            label: 40,
            instance: 0,
            has_fin: false,
            channel: hibana::substrate::binding::Channel::new(7),
        })
    }

    fn on_recv(
        &mut self,
        channel: hibana::substrate::binding::Channel,
        buf: &mut [u8],
    ) -> Result<usize, hibana::substrate::binding::TransportOpsError> {
        let channel_value = channel;
        let _state = channel_value;
        buf[..4].copy_from_slice(&[1, 2, 3, 4]);
        Ok(4)
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn hibana::substrate::policy::PolicySignalsProvider> {
        Some(self)
    }
}
```

Binding rules:

- `poll_incoming_for_lane()` is lane-local demux only
- `on_recv()` reads from the already selected channel
- `policy_signals_provider()` is the only public input source for slot-scoped signals

Supporting binding owners:

- `Channel`, `ChannelDirection`, and `ChannelKey` identify stream/channel endpoints
- `ChannelStore` is the storage contract when the binding owns multiple channels
- `TransportOpsError` is the canonical binding-side I/O error

Transport-owned send owners:

- `hibana::substrate::transport::Outgoing<'f>` is the canonical transport-owned send object
- transport-local send direction and metadata details stay in the transport bucket for transport-detail inspection, not as day-to-day app-facing owners

## Policy Plane

Dynamic policy remains explicit:

- annotate the choreography with `Program::policy::<POLICY_ID>()`
- register a resolver with `set_resolver::<POLICY_ID, ROLE, _>(rv_id, program, resolver)`
- use `ResolverContext::input(index)` and `ResolverContext::attr(id)`
- return `Result<DynamicResolution, ResolverError>`

`ResolverContext` is intentionally small: `input(index)` and `attr(id)` are the
only public accessors.

The public EPF owner surface is intentionally narrow:

- `hibana::substrate::policy::epf::{Header, Slot}` owns the image header and slot identity
- `Slot` is `Forward | EndpointRx | EndpointTx | Rendezvous | Route`
- active EPF is consulted inside the same resolver slot, before the Rust resolver callback
- if EPF does not decide, the same slot continues into the Rust resolver stage
- there is no public VM-run API separate from the resolver/policy surface
- `PolicySignalsProvider::signals(slot)` is the only public input boundary for slot-scoped policy data
- policy execution is fail-closed; verifier, trap, and fuel failures reject rather than falling through
- policy activation switches at the decision boundary through staged active/pending epochs
- load / activate / revert stay on the management prefixes under
  `hibana::substrate::mgmt`

Input semantics also come from the implementation contract:

- `Route`, `EndpointTx`, and `EndpointRx` may consume `PolicySignalsProvider` input
- `Forward` and `Rendezvous` run with zero policy input
- the public authority source remains the resolver slot, not a second EPF API

```rust
use hibana::substrate::policy::epf;

const POLICY_ID: u16 = 7;
const CUSTOM_INPUT0: hibana::substrate::policy::ContextId =
    hibana::substrate::policy::ContextId::new(0x9001);

struct RoutePolicy {
    preferred_arm: u8,
}

fn route_resolver(
    policy: &RoutePolicy,
    ctx: hibana::substrate::policy::ResolverContext,
) -> Result<hibana::substrate::policy::DynamicResolution, hibana::substrate::policy::ResolverError>
{
    if ctx.input(0) != 0 {
        return Ok(hibana::substrate::policy::DynamicResolution::RouteArm {
            arm: policy.preferred_arm,
        });
    }

    if ctx
        .attr(hibana::substrate::policy::core::QUEUE_DEPTH)
        .is_some_and(|value| value.as_u32() > 128)
    {
        return Err(hibana::substrate::policy::ResolverError::Reject);
    }

    if ctx.attr(CUSTOM_INPUT0).is_some_and(|value| value.as_u32() == 99) {
        return Ok(hibana::substrate::policy::DynamicResolution::Loop {
            decision: true,
        });
    }

    Ok(hibana::substrate::policy::DynamicResolution::Defer { retry_hint: 1 })
}

let route_policy = RoutePolicy { preferred_arm: 1 };

cluster.set_resolver::<POLICY_ID, 0, _>(
    rv_id,
    &CLIENT,
    hibana::substrate::policy::ResolverRef::from_state(&route_policy, route_resolver),
)?;
```

`ResolverRef::from_fn()` remains available as sugar for stateless callbacks, but
the canonical public path is the borrowed-state form above.

Advanced policy metadata arrives through `hibana::substrate::policy::core::*`:

- `RV_ID`
- `SESSION_ID`
- `LANE`
- `TAG`
- `LATENCY_US`
- `QUEUE_DEPTH`
- `SRTT_US`
- `PTO_COUNT`
- `PACING_INTERVAL_US`
- `IN_FLIGHT_BYTES`
- `CONGESTION_WINDOW`
- `CONGESTION_MARKS`
- `RETRANSMISSIONS`
- `LATEST_ACK_PN`
- `TRANSPORT_ALGORITHM`

The public policy data owners are:

- `ContextId` and `ContextValue` for fixed-width policy inputs and attrs
- `PolicyAttrs` for the attribute bag copied into resolver context
- `PolicySignals` for slot-scoped inputs delivered by `PolicySignalsProvider`
- `Header` and `Slot` in the `hibana::substrate::policy::epf` module for EPF image and slot ownership

Useful value helpers:

- `ContextId::new(raw)` and `ContextId::raw()` for opaque attribute ids
- `ContextValue::{NONE, FALSE, TRUE}` for sentinel and boolean-style values
- `ContextValue::from_u8`, `from_u16`, `from_u32`, `from_u64`, and `from_pair` for encoding
- `ContextValue::as_bool`, `as_u8`, `as_u16`, `as_u32`, `as_u64`, `as_pair`, and `raw` for decoding
- `PolicyAttrs::new()`, `insert(id, value)`, and `query(id)` for the fixed-size attribute map

## Control Messages and Capability Kinds

Control messages are regular `g::send()` steps whose payload carries a
capability token and control kind. The public owner for shot discipline is
`hibana::substrate::cap::{One, Many}`. The public owner for capability payloads
is `hibana::substrate::cap::GenericCapToken`.

Built-in control kinds live under `hibana::substrate::cap::advanced`. This is
the deep-dive bucket for mint details and standard control-kind owners; day-to-
day capability handling stays on `hibana::substrate::cap::{CapShot,
ControlResourceKind, GenericCapToken, Many, One, ResourceKind}`. The public
control-message handling markers are `CanonicalControl<K>` and
`ExternalControl<K>`, and the handling enum is `ControlHandling`.

```rust
use hibana::g;
use hibana::g::advanced::{CanonicalControl, ExternalControl};
use hibana::substrate::cap::{ControlResourceKind, GenericCapToken};
use hibana::substrate::cap::advanced::{
    LoopBreakKind, LoopContinueKind, SpliceIntentKind,
};

let loop_continue = g::send::<
    g::Role<0>,
    g::Role<0>,
    g::Msg<
        { <LoopContinueKind as ControlResourceKind>::LABEL },
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
    0,
>();

let splice_intent = g::send::<
    g::Role<0>,
    g::Role<1>,
    g::Msg<
        { <SpliceIntentKind as ControlResourceKind>::LABEL },
        GenericCapToken<SpliceIntentKind>,
        ExternalControl<SpliceIntentKind>,
    >,
    0,
>();
```

Custom control kinds are ordinary trait impls:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteLeftKind;

impl hibana::substrate::cap::ResourceKind for RouteLeftKind {
    type Handle = hibana::substrate::cap::advanced::RouteDecisionHandle;
    const TAG: u8 =
        <hibana::substrate::cap::advanced::RouteDecisionKind as hibana::substrate::cap::ResourceKind>::TAG;
    const NAME: &'static str = "RouteLeftDecision";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(
        handle: &Self::Handle,
    ) -> [u8; hibana::substrate::cap::advanced::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::substrate::cap::advanced::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, hibana::substrate::cap::advanced::CapError> {
        hibana::substrate::cap::advanced::RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = hibana::substrate::cap::advanced::RouteDecisionHandle::default();
    }

    fn caps_mask(
        _handle: &Self::Handle,
    ) -> hibana::substrate::cap::advanced::CapsMask {
        hibana::substrate::cap::advanced::CapsMask::empty()
    }

    fn scope_id(
        handle: &Self::Handle,
    ) -> Option<hibana::substrate::cap::advanced::ScopeId> {
        Some(handle.scope)
    }
}

impl hibana::substrate::cap::advanced::SessionScopedKind for RouteLeftKind {
    fn handle_for_session(
        _sid: hibana::substrate::SessionId,
        _lane: hibana::substrate::Lane,
    ) -> Self::Handle {
        hibana::substrate::cap::advanced::RouteDecisionHandle::default()
    }

    fn shot() -> hibana::substrate::cap::CapShot {
        hibana::substrate::cap::CapShot::One
    }
}

impl hibana::substrate::cap::advanced::ControlMint for RouteLeftKind {
    fn mint_handle(
        _sid: hibana::substrate::SessionId,
        _lane: hibana::substrate::Lane,
        scope: hibana::substrate::cap::advanced::ScopeId,
    ) -> Self::Handle {
        hibana::substrate::cap::advanced::RouteDecisionHandle { scope, arm: 0 }
    }
}

impl hibana::substrate::cap::ControlResourceKind for RouteLeftKind {
    const LABEL: u8 = 70;
    const SCOPE: hibana::substrate::cap::advanced::ControlScopeKind =
        hibana::substrate::cap::advanced::ControlScopeKind::Route;
    const TAP_ID: u16 =
        <hibana::substrate::cap::advanced::RouteDecisionKind as hibana::substrate::cap::ControlResourceKind>::TAP_ID;
    const SHOT: hibana::substrate::cap::CapShot = hibana::substrate::cap::CapShot::One;
    const HANDLING: hibana::substrate::cap::advanced::ControlHandling =
        hibana::substrate::cap::advanced::ControlHandling::Canonical;
}
```

After declaring a custom control kind, use it through
`GenericCapToken<RouteLeftKind>` and `CanonicalControl<RouteLeftKind>` or the
appropriate external control kind.

Built-in kind catalogue:

- route and loop: `RouteDecisionKind`, `LoopContinueKind`, `LoopBreakKind`
- checkpoint and recovery: `CheckpointKind`, `CommitKind`, `RollbackKind`, `CancelKind`, `CancelAckKind`
- splice and reroute: `SpliceIntentKind`, `SpliceAckKind`, `RerouteKind`
- policy lifecycle: `PolicyLoadKind`, `PolicyActivateKind`, `PolicyRevertKind`, `PolicyAnnotateKind`
- management load protocol: `LoadBeginKind`, `LoadCommitKind`

The implementation-level distinction that matters:

- `CanonicalControl<K>` covers self-send control such as route, loop, checkpoint, rollback, cancel, policy annotate, and reroute
- `ExternalControl<K>` covers cross-role control such as `SpliceIntentKind`, `SpliceAckKind`, `LoadBeginKind`, and `LoadCommitKind`
- delegation is an effect, not a separate public DSL node; the built-in public kinds that carry delegation semantics are `LoopContinueKind` and `RerouteKind`

Capability-building owners live in two layers:

- `hibana::substrate::cap::{One, Many}` for affine shot discipline
- `hibana::substrate::cap::{CapShot, ResourceKind, ControlResourceKind}` for runtime capability representation
- `hibana::substrate::cap::advanced::{MintConfig, MintConfigMarker, ControlMint, SessionScopedKind, AllowsCanonical, EpochTbl, CAP_HANDLE_LEN, CapError, CapsMask}` for protocol-implementor mint configuration
- `hibana::substrate::cap::advanced::{ControlScopeKind, ScopeId, ControlHandling}` for control-scope metadata

Canonical control minting for built-in control kinds happens automatically
through localside send paths such as `flow().send()`.

## Wire and Transport Observation

`hibana::substrate::wire::{Payload, WireDecode, WireEncode}` is the canonical
codec seam. `hibana::substrate::transport::{TransportEvent, TransportEventKind,
TransportSnapshot}` is the canonical transport observation seam.

If a payload type crosses the wire and is not already a codec type, implement
`WireEncode` and `WireDecode` for it.

Transport telemetry is surfaced two ways:

- resolvers read snapshot data through `ResolverContext::attr()` and
  `hibana::substrate::policy::core::*`
- transports emit semantic events through `TransportEvent` and `TransportEventKind`
- codecs report parse/encode failures through `CodecError`
- transport implementations report send/recv failures through `TransportError`
- `TransportMetrics` is the owner trait that turns implementation-specific counters into `TransportSnapshot`

```rust
let snapshot = hibana::substrate::transport::TransportSnapshot::new(Some(500), Some(2))
    .with_retransmissions(Some(1))
    .with_congestion_window(Some(65_536))
    .with_in_flight(Some(4096))
    .with_algorithm(Some(hibana::substrate::transport::TransportAlgorithm::Cubic));

let transport_event = hibana::substrate::transport::TransportEvent::new(
    hibana::substrate::transport::TransportEventKind::Ack,
    42,
    1200,
    0,
);

let queue_depth = snapshot.queue_depth;
let packet_number = transport_event.packet_number;
let _state = (queue_depth, packet_number);
```

`TransportSnapshot` uses builder-style enrichment:

- `TransportSnapshot::new(latency_us, queue_depth)`
- `with_congestion_marks`, `with_pacing_interval`, `with_retransmissions`
- `with_pto_count`, `with_srtt`, `with_latest_ack`
- `with_congestion_window`, `with_in_flight`, `with_algorithm`

`TransportAlgorithm` identifies the congestion-control owner carried by the snapshot.

## Management Session

Policy distribution belongs to `hibana::substrate::mgmt`.

Management is split into exactly two ordinary choreography prefixes:

- `hibana::substrate::mgmt::request_reply::PREFIX`
- `hibana::substrate::mgmt::observe_stream::PREFIX`

The public role owners are:

- `hibana::substrate::mgmt::ROLE_CONTROLLER`
- `hibana::substrate::mgmt::ROLE_CLUSTER`

EPF image injection and execution live on the request/reply prefix. The public
request vocabulary is:

- `hibana::substrate::mgmt::Request::Load(LoadRequest)`
- `hibana::substrate::mgmt::Request::LoadAndActivate(LoadRequest)`
- `hibana::substrate::mgmt::Request::Activate(SlotRequest)`
- `hibana::substrate::mgmt::Request::Revert(SlotRequest)`
- `hibana::substrate::mgmt::Request::Stats(SlotRequest)`

`LoadRequest` owns `slot`, `code`, `fuel_max`, and `mem_len`. `SlotRequest`
owns only `slot`. That split is intentional: staged upload and command-only
requests are different shapes, and the public surface keeps them different.

Management payload and reply owners:

- `LoadBegin` starts a staged code image upload
- `LoadChunk::mid(offset, chunk)` and `LoadChunk::last(offset, chunk)` stream the image body
- `LoadRequest` and `SlotRequest` are the typed public request payloads
- `LoadRequest` and `SlotRequest` carry `hibana::substrate::policy::epf::Slot` as their public slot owner
- `Request` is the public request sum type for the request/reply prefix
- `SubscribeReq` configures stream-tap subscription
- `Reply`, `LoadReport`, `MgmtError`, `StatsResp`, and `TransitionReport` are the canonical response owners
- `hibana::substrate::mgmt::tap::TapEvent` is the minimal public tap surface for observe streaming
- tap batching is a lower-layer observe-stream detail; there is no public `TapBatch` surface

The public management surface intentionally stops at payload owners and prefix
owners. There is no public management helper family beyond those ordinary
choreography owners.

EPF lifecycle and result surfaces:

- `Request::Load` returns `Reply::Loaded(report)`
- `Request::LoadAndActivate` returns `Reply::ActivationScheduled(report)`
- `Request::Activate` returns `Reply::ActivationScheduled(report)`
- `Request::Revert` returns `Reply::Reverted(report)`
- `Request::Stats` returns `Reply::Stats { stats, staged_version }`
- `TransitionReport` carries the activated or reverted version plus `policy_stats`
- `LoadReport` carries the staged version when code was uploaded without scheduling activation
- `StatsResp` carries `traps`, `aborts`, `fuel_used`, and `active_version`
- after activation, the image executes when its `Slot` is reached; immediate command completion returns over the request/reply prefix, and continuing observation stays on the stream prefix
- `Request::LoadAndActivate` and `Request::Activate` schedule activation at the management command boundary; `Request::Revert` restores the previous active version and clears pending activation state

Canonical substrate usage composes the management prefix, projects typed roles,
enters them through `SessionKit::enter`, then progresses the attached endpoints
with the same localside core API as any other choreography:

```rust
use hibana::g;
use hibana::g::advanced::project;

const PROGRAM: g::Program<_> =
    g::seq(hibana::substrate::mgmt::request_reply::PREFIX, APP);

let controller_program = project(&PROGRAM); // typed controller-side projection
let cluster_program = project(&PROGRAM); // typed cluster-side projection

let controller = cluster.enter(
    rv_id,
    sid,
    &controller_program,
    hibana::substrate::binding::NoBinding,
)?;
let cluster_role = cluster.enter(
    rv_id,
    sid,
    &cluster_program,
    hibana::substrate::binding::NoBinding,
)?;

let _state = (controller, cluster_role);
```

The request/reply prefix and observe stream prefix are ordinary choreography
artifacts. Protocol implementors compose `Program` values with ordinary
`hibana::g::seq`, project them with `project()`, attach them with
`SessionKit::enter(...)`, and drive the resulting endpoints with
`flow().send()`, `recv()`, `offer()`, and `decode()` just like any other
attached session.

## Validation

Push-quality validation means more than "the examples compile". At minimum, the
surface gates, protocol-neutrality, typed projection, and policy replay checks
should stay green. CI is intentionally split between stable verification and a
nightly rustdoc-JSON semantic surface lane. The canonical Pico gate is
SRAM-first and route-heavy: it projects, attaches, and runs a huge localside
sample to completion and reserves 96 KiB free on RP2040. The practical
shape-matrix contract is tracked per `route_heavy`, `linear_heavy`, and
`fanout_heavy` build:

- `flash <= 768 KiB`
- `static SRAM <= 48 KiB`
- `kernel stack <= 24 KiB`
- `peak SRAM <= 96 KiB`

`check_pico_smoke.sh` reserves a `24 KiB` kernel stack in the Pico linker
script, then combines target flash/static-SRAM bytes with a host-executed
canonical runtime measurement on a `32 KiB` thread stack to report the existing
hard-gated upper bounds (`kernel stack reserve`, `peak stack upper-bound`, and
`peak SRAM upper-bound bytes`) plus shadow/report-only executed metrics
(`measured peak stack bytes`, live slab usage, and `measured peak SRAM bytes`).
The measured peak SRAM report subtracts the statically reserved Pico slab, then
adds back the measured live slab bytes plus measured peak stack bytes so the
Phase 0a instrumentation tracks executed runtime usage without weakening the
current practical hard gate. `check_pico_size_matrix.sh` keeps the same budget
class for linear-heavy and fanout-heavy huge choreography shapes and also prints
the measured resident shape matrix:

- route resident bytes
- loop resident bytes
- endpoint resident bytes
- `CompiledProgramImage` header / persistent bytes
- `CompiledRoleImage` header / persistent bytes

`check_subsystem_budget_gates.sh` promotes the subsystem-local exact gates that
pin compiled-role bytes, route-heavy resident regressions, and the
descriptor-driven localside send/recv/offer/decode and policy hot paths.

```bash
# Stable hygiene and boundary gates
bash ./.github/scripts/check_policy_surface_hygiene.sh
bash ./.github/scripts/check_surface_hygiene.sh
bash ./.github/scripts/check_lowering_hygiene.sh
bash ./.github/scripts/check_summary_authority_hygiene.sh
bash ./.github/scripts/check_exact_layout_hygiene.sh
bash ./.github/scripts/check_route_frontier_owner.sh
bash ./.github/scripts/check_boundary_contracts.sh
bash ./.github/scripts/check_plane_boundaries.sh
bash ./.github/scripts/check_mgmt_boundary.sh
bash ./.github/scripts/check_resolver_context_surface.sh
bash ./.github/scripts/check_warning_free.sh
bash ./.github/scripts/check_direct_projection_binary.sh
bash ./.github/scripts/check_no_std_build.sh
bash ./.github/scripts/check_huge_choreography_budget.sh
bash ./.github/scripts/check_subsystem_budget_gates.sh
bash ./.github/scripts/check_pico_smoke.sh
bash ./.github/scripts/check_pico_size_matrix.sh

# Core builds
cargo check --all-targets -p hibana
cargo check --no-default-features --lib -p hibana

# Core test suites
cargo test -p hibana --features std
cargo test -p hibana --test ui --features std
cargo test -p hibana --test policy_replay --features std
cargo test -p hibana --test public_surface_guards --features std
cargo test -p hibana --test substrate_surface --features std
cargo test -p hibana --test docs_surface --features std

# Nightly semantic public surface gate
bash ./.github/scripts/check_hibana_public_api.sh
```

Before pushing, verify these invariants in addition to green commands:

- `hibana/src/**/*.rs` stays protocol-neutral
- route authority stays `Ack | Resolver | Poll`
- static unprojectable route stays compile-error, not runtime rescue
- typed projection and public-surface compile-fail tests stay intact
- substrate names do not leak back into the app surface

## Integration Boundary

`hibana` stops at the first typed app endpoint. Prefix composition, transport
setup, and any integration-specific policy stay outside the crate and should
arrive at app code only as the already-attached endpoint plus the public
localside core API.
