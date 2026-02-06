<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA — Compile-Time Choreography Engine for Rust" />

  <p>
    <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024">
    <img src="https://img.shields.io/badge/no__std-✓-success.svg" alt="no_std">
    <img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg" alt="License">
  </p>

  <p>
    <a href="#start-here">Start Here</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#snippet-conventions">Snippet Conventions</a> •
    <a href="#design-principles">Design Principles</a> •
    <a href="#choreography--localside">Choreography & Localside</a> •
    <a href="#control--epf">Control & EPF</a> •
    <a href="#transport--binding">Transport & Binding</a> •
    <a href="#troubleshooting">Troubleshooting</a> •
    <a href="#crates-and-demos">Crates and Demos</a> •
    <a href="#architecture">Architecture</a>
  </p>
</div>

# HIBANA
> The Compile-Time Choreography Engine for Rust

Hibana is an **Affine multiparty session types (Affine MPST)** library for high-assurance systems in Rust.
It verifies the **protocol definition** at compile time and enforces projected behavior at runtime
with allocation-free typestate checks on core execution paths.

> Stability note: Hibana is currently in **Preview**. Core ideas are stable, but APIs may evolve.

---

## Why Hibana?

Distributed systems are fragile; protocol drift is common and expensive.
Hibana moves protocol design from ad-hoc docs to **compile-time code**.
You define interactions as global choreographies, project them per role, and execute only
the permitted next step at runtime.

## Features

- **Compile-Time Verification**  
  Define protocols as global choreographies and project to localside at compile time.

- **Predictable, Low-Overhead Core**  
  Core protocol execution is `#![no_std]` / `#![no_alloc]` oriented, with explicit runtime costs.

- **Transport Agnostic**  
  Works with **QUIC**, **TCP**, **UDP**, or **In-Memory** transports.

- **Effect Policy Filter (EPF)**  
  An eBPF-inspired VM for dynamic, hot-reloadable policy decisions (rate limiting, routing).

- **Capability Control**  
  Capability tokens and control lanes provide explicit authorization and auditability.

## Start Here

Use this table as the fastest entry point.

| If you are... | Start with | Then read |
| :--- | :--- | :--- |
| Implementing an application protocol | [Quick Start](#quick-start) | [Choreography & Localside](#choreography--localside), [Design Principles](#design-principles) |
| Implementing a transport or binder | [Transport & Binding](#transport--binding) | [Transport context (ContextSnapshot)](#transport-context-contextsnapshot), [Troubleshooting](#troubleshooting) |
| Integrating policy/control logic | [Control & EPF](#control--epf) | [Resolver & HandlePlan](#resolver--handleplan), [Management session (EPF loadactivate)](#management-session-epf-loadactivate) |
| Debugging runtime behavior | [Troubleshooting](#troubleshooting) | [TapRing (observation)](#tapring-observation), [Architecture](#architecture) |

### Compatibility and scope

| Item | Current status |
| :--- | :--- |
| Rust edition | 2024 |
| `no_std` | Supported in core |
| `no_alloc` orientation | Core API is allocation-conscious; examples may allocate |
| Stability | Preview (APIs may evolve) |

---

## Quick Start

### Installation

```toml
[dependencies]
hibana = { git = "https://github.com/hibanaworks/hibana" }
```

Or:

```bash
cargo add hibana --git https://github.com/hibanaworks/hibana
```

### 1. Define the Protocol
Describe the interaction between roles as a global constant.

```rust
use hibana::{
    g::{self, Msg, Role, steps::{ProjectRole, SendStep, StepCons, StepNil}},
    NoBinding,
};

// Define Roles & Messages
type Client = Role<0>;
type Server = Role<1>;
type Ping = Msg<1, u32>;
type Pong = Msg<2, u32>;

// The Choreography: Client sends Ping, Server responds with Pong
type ProtocolSteps = StepCons<
    SendStep<Client, Server, Ping, 0>,
    StepCons<SendStep<Server, Client, Pong, 0>, StepNil>,
>;

const PING_PONG: g::Program<ProtocolSteps> = g::seq(
    g::send::<Client, Server, Ping, 0>(),
    g::send::<Server, Client, Pong, 0>(),
);

// Project to local behavior at compile-time
type ClientLocal = <ProtocolSteps as ProjectRole<Client>>::Output;
static CLIENT_PROG: g::RoleProgram<'static, 0, ClientLocal> =
    g::project::<0, ProtocolSteps, _>(&PING_PONG);
```

### 2. Prepare the Runtime (Transport / Config / Rendezvous / Cluster)
Define a transport and wire it into the runtime. A minimal skeleton:

```rust
use hibana::{
    observe::TapEvent,
    rendezvous::{Rendezvous, SessionId},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, RING_EVENTS},
    },
    transport::{Transport, TransportError, wire::Payload},
};

#[derive(Clone)]
struct MyTransport;

impl Transport for MyTransport {
    type Error = TransportError;
    type Tx<'a> = () where Self: 'a;
    type Rx<'a> = () where Self: 'a;
    type Send<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Self::Error>> + Send + 'a>>
        where Self: 'a;
    type Recv<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Payload<'a>, Self::Error>> + Send + 'a>>
        where Self: 'a;
    type Metrics = hibana::transport::NoopMetrics;

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }
    fn send<'a, 'f>(&'a self, _tx: &'a mut Self::Tx<'a>, _payload: Payload<'f>, _dest_role: u8)
        -> Self::Send<'a> where 'a: 'f { Box::pin(async { Ok(()) }) }
    fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
        Box::pin(async { Err(TransportError::Failed) })
    }
}

fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    Box::leak(Box::new([TapEvent::default(); RING_EVENTS]))
}
fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}
fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}

type Cluster = SessionCluster<'static, MyTransport, DefaultLabelUniverse, CounterClock, 4>;

let transport = MyTransport;
let config = Config::new(leak_tap_storage(), leak_slab(4096));
let rendezvous = Rendezvous::from_config(config, transport.clone());
let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
let rv_id = cluster.add_rendezvous(rendezvous)?;
let sid = SessionId::new(1);
```

Use the real transport and framing logic from `examples/tcp_tokio.rs` or
`examples/custom_binding.rs` for a complete runnable setup.

### 3. Run the Cursor
The compiler guides you. You *must* follow the protocol steps.

```rust
// Attach cursor to transport
let client = cluster.attach_cursor::<0, _, _, _>(rv_id, sid, &CLIENT_PROG, NoBinding)?;

// Step 1: Send Ping (Type-checked!)
let (client, _) = client.flow::<Ping>()?.send(&42u32).await?;

// Step 2: Receive Pong
let (client, pong) = client.recv::<Pong>().await?;

// Done! Session types ensure no steps are skipped.
```

### 4. Validate locally

```bash
cargo test -p hibana
```

### 5. Run examples

```bash
cargo run -p hibana --example tcp_tokio --features std
cargo run -p hibana --example custom_binding --features std
cargo run -p hibana --example mgmt_epf_control --features std
```

Expected result: examples compile and run, and each role can only execute choreography-valid steps.

### Features and test environment

- `std`: enables transport/testing utilities and observability normalization.
- `test-utils`: helper APIs for tests and examples.

Some tests require a large stack and additional case counts:

```bash
env RUST_MIN_STACK=268435456 HIBANA_TEST_STACK=268435456 \
    HIBANA_CANCEL_CASES=2048 HIBANA_ROLLBACK_CASES=2048 \
    cargo test -p hibana --features std
```

## Snippet conventions

- **Runnable** snippets are complete enough to adapt directly in a project.
- **Conceptual** snippets focus on one API rule and may omit setup types/imports.
- Unless noted otherwise, lane numbers shown in snippets are **logical lanes**.

---

## Minimal patterns (by feature)

Short, self-contained snippets for the core features. Each is intentionally minimal; see
the linked examples when you need a full runnable setup.

### 1) Protocol + projection (global → local) [Conceptual]

```rust
use hibana::g::{self, Msg, Role, steps::{ProjectRole, SendStep, StepCons, StepNil}};
type A = Role<0>;
type B = Role<1>;
type Ping = Msg<1, u32>;
type Pong = Msg<2, u32>;

type Steps = StepCons<SendStep<A, B, Ping, 0>, StepCons<SendStep<B, A, Pong, 0>, StepNil>>;
const PROTOCOL: g::Program<Steps> = g::seq(
    g::send::<A, B, Ping, 0>(),
    g::send::<B, A, Pong, 0>(),
);
type ALocal = <Steps as ProjectRole<A>>::Output;
static A_PROG: g::RoleProgram<'static, 0, ALocal> = g::project::<0, Steps, _>(&PROTOCOL);
```

**Notes**
- Compose only with `g::seq` / `g::par` / `g::route` (binary).
- Localside is always from `g::project` (no hand-written state machines).

### 2) Cursor flow (send/recv) [Conceptual]

```rust
let ep = cluster.attach_cursor::<0, _, _, _>(rv_id, sid, &A_PROG, NoBinding)?;
let (ep, _) = ep.flow::<Ping>()?.send(&1u32).await?;
let (ep, pong) = ep.recv::<Pong>().await?;
let _ = pong;
```

**Notes**
- `flow::<Msg>().send(&payload)` is the only send path.
- `recv::<Msg>()` is deterministic (no route).

### 3) Route + offer (wire branch) [Conceptual]

```rust
let branch = ep.offer().await?;
match branch.label() {
    10 => {
        let (ep, msg) = branch.decode::<Msg<10, u32>>().await?;
        ep
    }
    11 => {
        let (ep, msg) = branch.decode::<Msg<11, u32>>().await?;
        ep
    }
    _ => unreachable!(),
};
```

**Notes**
- `offer()` only at route decision points.
- Use `decode()` for wire recv; never decide arm manually.

### 4) ArmSendHint (offer → send) [Conceptual]

```rust
let branch = ep.offer().await?;
match branch.label() {
    LABEL_SEND_ONLY => {
        let ep = branch.into_endpoint();
        let (ep, _) = ep.flow::<Msg<LABEL_SEND_ONLY, ()>>()?.send(&()).await?;
        ep
    }
    _ => unreachable!(),
}
```

**Notes**
- If `ArmSendHint` occurs, do **not** `decode()`; send via `flow().send()`.

### 5) Canonical control (self-send) [Conceptual]

```rust
use hibana::{g, control::cap::{GenericCapToken, resource_kinds::CancelKind},
    runtime::consts::LABEL_CANCEL};
type CancelMsg = Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, g::CanonicalControl<CancelKind>>;
let (ep, outcome) = ep.flow::<CancelMsg>()?.send(()).await?;
let _ = outcome; // ControlOutcome::Canonical
```

**Notes**
- Canonical control is self-send; wire is skipped.
- Control labels must use `GenericCapToken<K>` payloads.

### 6) Dynamic control plan (resolver) [Conceptual]

```rust
use hibana::global::const_dsl::{DynamicMeta, HandlePlan};
use hibana::control::cluster::{DynamicResolution, ResolverContext};

const POLICY_ID: u16 = 0x1234;
const META: DynamicMeta = DynamicMeta::new();
const ROUTE: g::Program<_> = g::route(
    g::route_chain::<0, _>(
        g::with_control_plan(ARM0, HandlePlan::dynamic(POLICY_ID, META))
    ).and::<_>(ARM1),
);

let plan = CONTROLLER_PROGRAM
    .control_plans()
    .find(|info| info.label == ARM0_LABEL)
    .expect("route plan");
cluster.register_control_plan_resolver(rv_id, &plan, |_cluster, _meta, ctx: ResolverContext| {
    let _ = ctx; // use transport snapshot / scope hints here
    Ok(DynamicResolution::arm(0))
})?;
```

**Notes**
- Dynamic plans require registering a resolver *before* execution.
- The operation type is decided by the ResourceKind tag, not the plan.

### 7) BindingSlot (custom framing) [Conceptual]

```rust
unsafe impl BindingSlot for MyBinder {
    fn on_send_with_meta(&mut self, _meta: SendMetadata, _payload: &[u8])
        -> Result<SendDisposition, TransportOpsError> {
        Ok(SendDisposition::BypassTransport)
    }
    fn poll_incoming_for_lane(&mut self, _lane: u8) -> Option<IncomingClassification> {
        Some(IncomingClassification { label: 1, instance: 0, has_fin: false, channel: Channel::new(0) })
    }
    fn on_recv(&mut self, _ch: Channel, buf: &mut [u8])
        -> Result<usize, TransportOpsError> {
        // Fill buf with payload; return size
        Ok(0)
    }
}
```

**Notes**
- `BindingSlot::on_send_with_meta` must not block or perform I/O.
- Use `SendMetadata` + `IncomingClassification` for deterministic routing.

---

## Design Principles

- **Choreography first**: the global program is the protocol. Localside is always derived
  via `g::project`, never hand-written state machines.
- **Dumb driver**: localside only follows the projected steps. Decisions come from
  `offer().label()` or explicit control self-sends; no transport I/O in localside.
- **No runtime inference**: routes are binary and deterministic; lane mapping is explicit.
- **Small core API**: `g::send` / `g::route` / `g::par` / `g::seq` and
  `flow().send` / `recv` / `offer` / `decode`.

### Terminology quick map

| Term | Meaning in Hibana |
| :--- | :--- |
| Choreography | Global protocol specification (`g::send` / `g::seq` / `g::par` / `g::route`) |
| Localside | Per-role projected program executed by a cursor |
| Offer point | A route decision point where `offer()` is valid |
| Logical lane | Lane index used in choreography definitions |
| Physical lane | Transport/binding-side lane after optional mapping |
| Canonical control | Local self-send control path (no wire send) |

### Unified lifecycle

- Define the whole session from connect to close as **one choreography**.
- `attach_cursor()` is intended to be used **once per session/role**; the cursor owns the lane.
- Avoid multiple partial programs for handshake/interop. Compose with `g::seq` / `g::par` / `g::route`.

### Unified choreography and dumb driver

- Protocol composition is only `g::seq` / `g::par` / `g::route`.
- Drivers never call transport APIs; they only execute localside steps.
- Branching is **only** `offer()` → `branch.label()`; do not invent new branching logic.
- Self-send (CanonicalControl) is for local decisions (loop control, local sync), not wire effects.

### Lane segregation (summary)

- Lanes are **logical** and fixed by choreography; drivers do not choose lanes.
- Binders map logical lanes to physical lanes with `map_lane()`.
- If you integrate with the QUIC stack, follow its lane conventions; otherwise choose lanes explicitly.

### Route resolution (summary)

- `g::route` is **binary only** (3-arm is a const panic).
- Resolution order: Merged → WireFirst → Resolver → compile error.
- Dynamic resolution requires `HandlePlan::dynamic`; otherwise unprojectable routes fail at compile time.
- `offer()` is called only at route decision points; use `branch.decode::<Msg>()` for wire recv.

### Transport context separation

| Layer | Write | Read |
|---|:---:|:---:|
| Transport | ✅ | ✅ |
| Resolver | ❌ | ✅ |
| Binder | ❌ | ✅ |
| Driver | ❌ | ❌ |

## Choreography & Localside

### Writing choreography

- Use `g::send::<From, To, Msg, LANE>()` with explicit lanes.
- Compose with `g::seq`, `g::par`, and `g::route` (binary only).
- Lanes are **logical**. Binders can remap them with `map_lane()` when needed.

```rust
use hibana::g::{self, Msg, Role};
type A = Role<0>;
type B = Role<1>;
type Hello = Msg<10, u32>;
type World = Msg<11, u32>;

const PROTOCOL: g::Program<_> = g::seq(
    g::send::<A, B, Hello, 0>(),
    g::send::<B, A, World, 0>(),
);
```

#### g::par with multiple lanes

```rust
use hibana::g::{self, Msg, Role, steps::{ProjectRole, SendStep, StepCons, StepNil}};
type A = Role<0>;
type B = Role<1>;
type Ping = Msg<1, u32>;
type Pong = Msg<2, u32>;
type Note = Msg<3, u8>;

type Lane0 = StepCons<SendStep<A, B, Ping, 0>, StepCons<SendStep<B, A, Pong, 0>, StepNil>>;
type Lane1 = StepCons<SendStep<A, B, Note, 1>, StepNil>;
type ParSteps = <Lane0 as StepConcat<Lane1>>::Output;

const LANE0: g::Program<Lane0> = g::seq(
    g::send::<A, B, Ping, 0>(),
    g::send::<B, A, Pong, 0>(),
);
const LANE1: g::Program<Lane1> = g::send::<A, B, Note, 1>();

const PAR: g::Program<ParSteps> = g::par(
    g::par_chain(LANE0).and(LANE1)
);

type ALocal = <ParSteps as ProjectRole<A>>::Output;
static A_PROG: g::RoleProgram<'static, 0, ALocal> = g::project::<0, ParSteps, _>(&PAR);
```

### Localside driver patterns

- **Send**: `flow::<Msg>().send(&payload).await`
- **Deterministic recv**: `recv::<Msg>().await`
- **Route**: `offer().await` → `branch.label()` → `branch.decode::<Msg>().await`
- **Loops**: controller sends `LoopContinue`/`LoopBreak` (canonical self-send),
  passive side loops `offer()` until break.
- **Control self-send**: use `flow().send(())` inside helpers; avoid `into_endpoint()` in drivers.

```rust
let branch = endpoint.offer().await?;
match branch.label() {
    1 => {
        let (ep, msg) = branch.decode::<Msg<1, u32>>().await?;
        ep
    }
    2 => {
        let (ep, msg) = branch.decode::<Msg<2, u32>>().await?;
        ep
    }
    _ => unreachable!(),
};
```

### offer() patterns (complete)

`offer()` returns a `RouteBranch` that is classified internally; handle each case:

1. **WireRecv** (most common): decode the branch payload.
2. **ArmSendHint**: the selected arm starts with a send. Do **not** decode; convert
   back to endpoint and call `flow().send(...)`.
3. **LocalControl**: canonical self-send arm (no wire). `decode()` yields a synthetic
   payload; use it only for observers.
4. **EmptyArmTerminal**: empty arm (e.g., loop break with no recv). `decode()` is a
   no-op placeholder; do not treat it as wire data.

```rust
let branch = endpoint.offer().await?;
match branch.label() {
    LABEL_DATA => {
        let (ep, data) = branch.decode::<DataMsg>().await?;
        ep
    }
    LABEL_CONTROL => {
        // ArmSendHint: use flow().send(), not decode()
        let ep = branch.into_endpoint();
        let (ep, _) = ep.flow::<ControlMsg>()?.send(()).await?;
        ep
    }
    _ => unreachable!(),
}
```

#### Loop control (passive observer)

```rust
loop {
    let branch = endpoint.offer().await?;
    match branch.label() {
        LABEL_LOOP_CONTINUE => {
            // EmptyArmTerminal: no wire payload
            let (ep, _) = branch.decode::<LoopContinueMsg>().await?;
            endpoint = ep;
            continue;
        }
        LABEL_LOOP_BREAK => {
            let (ep, _) = branch.decode::<LoopBreakMsg>().await?;
            endpoint = ep;
            break;
        }
        LABEL_BODY => {
            let (ep, body) = branch.decode::<BodyMsg>().await?;
            endpoint = ep;
            handle_body(body);
        }
        _ => unreachable!(),
    }
}
```

#### ArmSendHint (passive side sends after offer)

```rust
let branch = endpoint.offer().await?;
match branch.label() {
    LABEL_SEND_ONLY => {
        let ep = branch.into_endpoint();
        let (ep, _) = ep.flow::<SendOnlyMsg>()?.send(&payload).await?;
        ep
    }
    _ => unreachable!(),
}
```

#### Local control observer (no wire)

```rust
let branch = endpoint.offer().await?;
if branch.label() == LABEL_LOOP_CONTINUE {
    // LocalControl: decode is synthetic; use only for observation
    let (ep, _) = branch.decode::<LoopContinueMsg>().await?;
    endpoint = ep;
}
```

## Control & EPF

### Built-in control messages

Hibana provides standard control kinds and labels in `hibana::runtime::consts`
and `hibana::control::cap::resource_kinds`:

- `LoopContinue` / `LoopBreak` (`LABEL_LOOP_CONTINUE` / `LABEL_LOOP_BREAK`)
- `Cancel` (`LABEL_CANCEL`)
- `Checkpoint` / `Rollback` (`LABEL_CHECKPOINT` / `LABEL_ROLLBACK`)
- `SpliceIntent` / `SpliceAck` (`LABEL_SPLICE_INTENT` / `LABEL_SPLICE_ACK`)

Canonical control is **self-send** and uses `send(())`:

```rust
use hibana::{
    g,
    g::Msg,
    control::cap::{GenericCapToken, resource_kinds::CancelKind},
    runtime::consts::LABEL_CANCEL,
};

type CancelMsg = Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, g::CanonicalControl<CancelKind>>;
let (ep, outcome) = ep.flow::<CancelMsg>()?.send(()).await?;
```

External control uses `ExternalControl<K>`; when `AUTO_MINT_EXTERNAL` is true the
token is auto-minted and returned via `ControlOutcome::External`.

External control **does go on the wire**. Use it for control messages that must
be observed/validated by the peer or a remote manager (e.g., management sessions,
splice intent/ack). Canonical control is local-only and never hits the wire.

#### Control label and payload rules
- Control labels (`LABEL_CONTROL_START..LABEL_CONTROL_END`) must use
  `GenericCapToken<K>` payloads and `CanonicalControl<K>` or `ExternalControl<K>`.
- Non-control labels use normal payloads (`WireEncode`) with `NoControl` (default).
- `ExternalControl` with `AUTO_MINT_EXTERNAL = false` requires caller-supplied tokens.

```rust
// External control without auto-mint: pass the token explicitly.
type AuditMsg = Msg<70, GenericCapToken<MyKind>, g::ExternalControl<MyKind>>;
let token: GenericCapToken<MyKind> = token_from_peer;
let (ep, outcome) = ep.flow::<AuditMsg>()?.send(&token).await?;
let _ = outcome;
```

### Capability tokens, OneShot/ManyShot

Control messages carry `GenericCapToken<K>`. Tokens encode:

- session id / lane / role
- caps mask
- **shot**: `CapShot::One` (one-shot) or `CapShot::Many` (reusable)

Use OneShot for affine, single-claim control (default). Use ManyShot when you
intentionally allow reuse (e.g., load balancing or replication).

```rust
use hibana::control::cap::CapShot;
let token_one = broker.mint_endpoint_token(sid, lane, role, CapShot::One);
let token_many = broker.mint_endpoint_token(sid, lane, role, CapShot::Many);
```

**CapToken wire format and safety (short):**
- Format: `[nonce(16B) | header(32B) | mac(16B)]` (total 64B).
- Header fields: `sid`, `lane`, `role`, `resource tag`, `shot`, `caps mask`, `handle bytes`.
- Security assumptions: nonces are CSPRNG-derived, MAC key stays secret, and claims always
  validate via rendezvous/cluster. Treat tokens as bearer capabilities and respect shot
  semantics (`One` is single-use, `Many` is reusable under MultiSafe constraints).

If you need to inspect a token payload (e.g., splice targets), use
`GenericCapToken::decode_handle()`:

```rust
let handle = token.decode_handle()?; // typed handle for the ResourceKind
```

### User-defined control messages

Define a new `ResourceKind` + `ControlResourceKind` (macro exported by hibana),
pick an unused label, and build a message with `GenericCapToken`.

```rust
use hibana::{g, g::Msg, control::cap::{CapsMask, GenericCapToken}};

hibana::impl_control_resource!(
    MyKind,
    handle: SessionScoped,
    tag: 0x90,
    name: "my-control",
    label: 70,
    scope: None,
    tap_id: 0,
    caps: CapsMask::empty(),
    handling: Canonical,
);

type MyMsg = Msg<70, GenericCapToken<MyKind>, g::CanonicalControl<MyKind>>;
```

Use labels that do not clash with `runtime::consts` and the management labels.
If you need labels beyond the default universe, define a custom `LabelUniverse`
and pass it via `Config::with_universe`.

### EPF usage

Two common paths:

1. **Offline VM**: build bytecode and execute via `epf::vm::Vm` (see
   `examples/epf_adaptive_control.rs`).
2. **Runtime policy**: load/activate/revert via the management session, which
   supports **remote EPF bytecode injection** over the wire (see
   `examples/mgmt_epf_control.rs` and `examples/mgmt_epf_observe.rs`).

Remote injection flow (management session):

- `LoadBegin` token + payload
- `LoadChunk` loop (continue/break) until all chunks sent
- `LoadCommit` token
- `Command::Activate` / `Command::Revert` / `Command::Stats`

EPF evaluates `ENDPOINT_SEND/RECV` events and can `ACT_ABORT`, `ACT_EFFECT`
(checkpoint/rollback), `TAP_OUT`, and route.

#### EPF bytecode quick notes
- Bytecode is raw `[u8]` executed by `epf::vm::Vm` (8 regs, fixed memory).
- Use opcode constants in `epf::ops::{instr, effect}` (e.g., `LOAD_IMM`, `GET_RETRY`,
  `JUMP_GT`, `ACT_EFFECT`, `ACT_ABORT`, `ACT_ROUTE`, `TAP_OUT`).
- Inputs: event fields (`GET_EVENT_*`), scope (`GET_SCOPE_*`), and transport metrics
  (`GET_LATENCY`, `GET_QUEUE`, `GET_CONGESTION`, `GET_RETRY`).
- Effects: `ACT_EFFECT` supports `SPLICE_BEGIN`, `SPLICE_COMMIT`, `SPLICE_ABORT`,
  `CHECKPOINT`, and `ROLLBACK`.
See `examples/epf_adaptive_control.rs` for a minimal program.

## Resolver & HandlePlan

Route arms and control decisions can be dynamic. Use `HandlePlan` on route/control
arms and register a resolver.

**Important clarifications:**
- `with_control_plan()` only declares *how to build a control handle*; it does **not**
  decide the operation itself.
- The **operation type is determined by the control message's ResourceKind tag**
  (RouteDecision/Loop, SpliceIntent/SpliceAck, Reroute, etc.).
- A dynamic resolver may return **RouteArm**, **Loop**, **Splice**, or **Reroute**
  depending on that tag.

```rust
use hibana::global::const_dsl::{DynamicMeta, HandlePlan};
use hibana::control::cluster::{DynamicResolution, ResolverContext};

const POLICY_ID: u16 = 0x1234;
const META: DynamicMeta = DynamicMeta::new();

const ROUTE: g::Program<_> = g::route(
    g::route_chain::<0, _>(
        g::with_control_plan(ARM0, HandlePlan::dynamic(POLICY_ID, META))
    ).and::<_>(ARM1),
);

cluster.register_control_plan_resolver(rv_id, &info, |_cluster, _meta, ctx: ResolverContext| {
    // Decide arm based on ctx (transport snapshot, session, lane, etc.)
    Ok(DynamicResolution::arm(0))
})?;
```

Available plans:
- `HandlePlan::none()` (default)
- `HandlePlan::dynamic(policy_id, meta)` (requires resolver)
- `HandlePlan::splice_local(dst_lane)` / `HandlePlan::reroute_local(dst_lane, shard)` (static)

**Static plans (no resolver):**
- `splice_local`: same-process lane handoff with a fixed destination lane. Use when the target
  is known at compile time and no policy decision is required.
- `reroute_local`: local lane/shard switch (self-send `Reroute`), used to retarget within the
  same rendezvous without cross-role wire control.

**Control handling quick map:**
- `CanonicalControl`: self-send only, `send(())`, token auto-minted from `HandlePlan`.
- `ExternalControl` + `AUTO_MINT_EXTERNAL = true` (e.g., SpliceIntent/Ack): `send(())`,
  token auto-minted from resolver/plan.
- `ExternalControl` (no auto-mint): `send(&GenericCapToken<K>)` (caller supplies token).
- `NoControl`: `send(&payload)` (regular wire message).

## Delegation (capability-based)

Delegation hands a session lane to another role/endpoint via a capability token.
Typical flow:

1. Mint or receive a delegation token (CapShot::One is typical).
2. The receiver calls `SessionCluster::delegate_claim(...)` to obtain a claim.
3. Attach a delegated cursor via `claim.attach_cursor(&PROGRAM)`.

**Wire shape note:** delegation tokens are plain payloads (`GenericCapToken<EndpointResource>`)
carried on **non-control labels** (EndpointResource is not a control kind). The receiver
extracts the token and calls `delegate_claim`.

**Forwarding note:** `transport::Forward` (relay/splice) is a runtime optimization and
does not replace type-level delegation; the choreography still includes delegation
messages to preserve MPST safety.

```rust
let claim = cluster.delegate_claim(rv_id, token)?;
let delegated = claim.attach_cursor::<ROLE, _, _>(&ROLE_PROGRAM)?;
```

## Transport & Binding

Hibana is transport-agnostic. You provide:

1. **`Transport`**: raw byte I/O (`open`, `send`, `recv`) plus optional metrics for EPF.
2. **`BindingSlot`** (optional): framing and label classification for stream-style transports.

Guidelines:

- `BindingSlot::on_send_with_meta` **must not** block or perform network I/O.
  Return `BypassTransport` to let core call `Transport::send`, or `Handled` if
  the binder performed the wire write.
- `poll_incoming_for_lane` + `on_recv` implement route-aware receives.
- `map_lane` lets you remap logical lanes to physical lanes (avoids conflicts).
- Transport context is read via `TransportContextProvider`; drivers do not read transport state.

#### BindingSlot flow (send/recv)

1. `flow().send()` → `on_send_with_meta` (sync) → `Transport::send()` if `BypassTransport`.
2. `offer()` → `poll_incoming_for_lane` to pick arm/label.
3. `decode()` / `recv()` → `on_recv` to pull framed payload.

#### Logical vs physical lanes

- **Logical lane**: the lane number in choreography (`g::send::<..., LANE>()`).
- **Physical lane**: the rendezvous lane used by transport/binding.
- Map via `BindingSlot::map_lane()` to avoid conflicts or multiplex streams.

## Transport context (ContextSnapshot)

Resolvers can read transport state via a snapshot; drivers never read it directly:

```rust
use hibana::transport::context::{ContextSnapshot, ContextKey, protocol};

fn resolver(ctx: hibana::control::cluster::ResolverContext) -> Result<_, ()> {
    let snap: ContextSnapshot = ctx.metrics;
    if let Some(value) = snap.query(ContextKey::new(protocol::QUIC, 0)) {
        // interpret ContextValue (bool/u32/u64)
    }
    Ok(hibana::control::cluster::DynamicResolution::arm(0))
}
```

## TapRing (observation)

- Two rings: **User** (TAP_OUT, id < 0x0100) and **Infra** (ENDPOINT_SEND/RECV, etc.)
- Streaming observes **User** ring only to avoid observer feedback.
- Poll directly with `observe::for_each_since` when you need full coverage.

```rust
let mut cursor = 0usize;
observe::for_each_since(&mut cursor, |event| {
    if event.id < 0x0100 {
        // User ring (EPF TAP_OUT)
    } else {
        // Infra ring (ENDPOINT_SEND/RECV, etc.)
    }
});
```

See `examples/mgmt_epf_observe.rs`.

## Management session (EPF load/activate)

Minimal remote injection sequence:

1. `LoadBegin` token + payload (slot, code_len, hash)
2. `LoadChunk` loop (continue/break)
3. `LoadCommit` token
4. `Command::Activate` (or `Revert` / `Stats`)

See `examples/mgmt_epf_control.rs` and `examples/mgmt_epf_observe.rs`.

## Troubleshooting

### Common failures and first checks

| Symptom | Likely cause | First action |
| :--- | :--- | :--- |
| `PhaseInvariant` | Localside/choreography step mismatch | Re-check `offer`/`decode`/`flow().send` order |
| `PolicyAbort` | Missing/mismatched dynamic resolver | Register resolver and verify resolution tag |
| `LabelMismatch` | Wrong message type for current branch | Verify `branch.label()` match arm |
| Transport/Binding errors | I/O/framing failure | Validate binder framing and transport state |

### Error handling guidelines

- `PhaseInvariant`: choreography/localside mismatch or wrong control handling
  (`offer`/`decode`/`send` order, wrong arm). Fix driver logic first.
- `PolicyAbort`: dynamic plan used without a resolver, or resolver returns the wrong
  resolution type for the tag (Route/Loop/Splice/Reroute). Register the resolver and
  match the tag.
- `LabelMismatch`: `offer`/`recv` used with the wrong message label → check branch selection.
- `Transport` / `Binding` errors: treat as I/O failure; retry or terminate session.

Examples:

- `examples/tcp_tokio.rs`: `Transport` only (no binding).
- `examples/custom_binding.rs`: label-prefixed framing via `BindingSlot`.

Use `NoBinding` if your transport already provides raw protocol frames.

---

## Crates and Demos

`hibana` is the core crate. Other projects are public demos that prove practical usability.

| Project | Positioning | What it shows |
| :--- | :--- | :--- |
| **`hibana`** | Core crate | Affine MPST semantics, projection, runtime, control, and EPF. |
| **`hibana-quic`** | Integration demo | QUIC-oriented transport integration and interop-style end-to-end session driving. |
| **`hibana-agent`** | Application demo | AI control automation driven by Hibana session types. |

---

## Contributing

Contributions are welcome. Please open an issue describing the change and the expected behavior.

## License

Licensed under MIT or Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

---

## Architecture

Hibana separates **Control**, **Data**, and **Observation** for maximum reliability.

```
      Global Choreography
              │
       const projection
              │
              ▼
         Role Program
              │
        attach_cursor
              │
              ▼
       Cursor Endpoint
              │
 flow.send / offer / decode / recv
              │
              ▼
┌─────── Runtime Core ───────┐
│ CapFlow • Control • EPF VM │
└──────┬──────────┬──────────┘
       │          │
   Transport   Observe
 (BindingSlot) (Dual-Ring)
```

---

## FAQ

<details>
<summary><strong>Is this production ready?</strong></summary>
Hibana is currently in <strong>Preview</strong>. While the core verification logic is sound, APIs may change.
</details>

<details>
<summary><strong>How does it handle branching?</strong></summary>
Use <code>g::route</code> for branching logic. The type system ensures all branches are handled.
</details>

<details>
<summary><strong>What makes Hibana "Affine MPST"?</strong></summary>
Each role follows a projected localside program where session capabilities are consumed as steps progress. This affine discipline prevents duplicate/invalid protocol progression while keeping execution deterministic.
</details>

<details>
<summary><strong>What does <code>no_std</code> / <code>no_alloc</code> mean here?</strong></summary>
The core crate is designed for <code>#![no_std]</code> and allocation-conscious execution. Some examples and integration paths (for instance std transports or CLI tooling) use <code>std</code>/<code>alloc</code> outside the core protocol semantics.
</details>

<details>
<summary><strong>When should I use <code>recv()</code> vs <code>offer()</code> + <code>decode()</code>?</strong></summary>
Use <code>recv()</code> for deterministic, non-branching receives. Use <code>offer()</code> only at route decision points, then branch by <code>label()</code> and read payloads with <code>decode()</code>.
</details>

<details>
<summary><strong>When is a resolver required?</strong></summary>
A resolver is required only when a route/control arm uses <code>HandlePlan::dynamic(...)</code>. Without it, dynamic decisions fail with <code>PolicyAbort</code> (or compile-time unprojectable errors where applicable).
</details>

<details>
<summary><strong>Why is route binary-only, and how do I model 3+ choices?</strong></summary>
Binary routes keep projection and runtime resolution deterministic and simple. Model 3+ choices by composing nested binary routes (for example, <code>route(A, route(B, C))</code>).
</details>

<details>
<summary><strong>How do I migrate from a hand-written state machine?</strong></summary>
Start by writing one global choreography for the whole session, project per role, then replace manual transitions with localside primitives (<code>flow().send</code>, <code>recv</code>, <code>offer</code>, <code>decode</code>). Migrate one protocol boundary at a time.
</details>

<details>
<summary><strong>What should I check first for <code>PhaseInvariant</code> / <code>PolicyAbort</code>?</strong></summary>
For <code>PhaseInvariant</code>, check step order and branch handling (<code>offer</code>/<code>decode</code>/<code>send</code>). For <code>PolicyAbort</code>, verify resolver registration, policy id/meta, and returned resolution type for the control tag.
</details>

<details>
<summary><strong>How do <code>hibana</code> and <code>hibana-quic</code> differ?</strong></summary>
<code>hibana</code> is the core Affine MPST crate. <code>hibana-quic</code> is a QUIC-focused integration demo that shows how to run real transport flows on top of Hibana choreography/localside APIs.
</details>

<details>
<summary><strong>Can localside driver code call transport APIs directly?</strong></summary>
No. Keep drivers in choreography primitives only (<code>flow().send</code>, <code>recv</code>, <code>offer</code>, <code>decode</code>). Transport-side effects belong in transport/binding or resolver integration points.
</details>

<details>
<summary><strong>How do I inspect performance and behavior?</strong></summary>
Use TapRing and management/observe examples to trace control and data events, and benchmark with your transport/binder configuration. Keep hot paths deterministic: route at offer points, avoid extra lane scans, and keep binding classification O(1).
</details>

<details>
<summary><strong>Why "Hibana"?</strong></summary>
"Hibana" is Japanese for "spark"—the glowing trail of a senko-hanabi that arcs and hands off to the next ember. Each spark is a participant in a session type; the connections between them mirror the multi-party edges that Hibana proves correct.
</details>

---

<div align="center">
  <p>
    Licensed under <a href="LICENSE-MIT">MIT</a> or <a href="LICENSE-APACHE">Apache-2.0</a>.
  </p>
  <p>
    <a href="https://github.com/hibanaworks/hibana">GitHub</a> •
    <a href="https://github.com/hibanaworks/hibana/issues">Issues</a>
  </p>
</div>
