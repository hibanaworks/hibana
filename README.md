<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA - Session-Typed Choreographic Programming for Rust" />

  <p>
    <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024" />
    <img src="https://img.shields.io/badge/no__std-yes-success.svg" alt="no_std" />
    <img src="https://img.shields.io/badge/no__alloc-oriented-blue.svg" alt="no_alloc oriented" />
  </p>

  <p>
    <a href="#install">Install</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#what-hibana-is">What Hibana Is</a> •
    <a href="#application-guide">Application Guide</a> •
    <a href="#protocol-runtime">Protocol Runtime</a> •
    <a href="#build-and-test">Build And Test</a>
  </p>
</div>

# HIBANA

`hibana` is a Rust 2024, `#![no_std]`, no-alloc-oriented runtime for executing
multiparty protocols described once as global choreographies. Its core is a
choreography-derived runtime enforcement kernel: projection produces compact
per-role descriptor programs, and an endpoint operation can commit only when it
matches an enabled descriptor event exactly. Endpoint ownership is affine,
message delivery is asynchronous, and protocol state remains compact runtime
data instead of becoming Rust continuation types.
The design draws from [Multiparty Asynchronous Session
Types](https://www.doc.ic.ac.uk/~yoshida/multiparty/multiparty.pdf), [Affine
Multiparty Session Types](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2022.4),
and the explicit-channel constraints of the [mechanised subject-reduction
development](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2025.31).
Those calculi are inputs, not Hibana's normative implementation. Hibana keeps
its compact descriptor runtime, affine Rust ownership, fixed eight-byte core
header, and Pico-class resource bounds where copying a paper calculus would
weaken the runtime.

Hibana's supported protocol core is finite-role and non-delegating: `send`,
`seq`, `par`, binary `route`, guarded `roll`, and zero-byte local effects. It
does not implement higher-order session/channel passing. Progress is proved per
session under the stated carrier and fairness assumptions; multi-session proofs
establish isolation, not deadlock freedom for arbitrary application code that
blocks one session on another. A descriptor is a finite protocol template, not
a whole deployment: the same projected program may back as many concurrent
`SessionId` instances as descriptor-derived slab capacity permits.

[Explicit connection actions](https://www.doc.ic.ac.uk/~rhu/scribble/explicit.html)
motivate optional and dynamic participants, but Hibana does not mutate the role
set of a live descriptor. Reconfiguration creates a fresh finite session and
verified artifact. This keeps topology replacement atomic and avoids turning
runtime membership into generated Rust continuation types.

Hibana's concrete model is direct: a protocol crate describes communication
once as a global choreography, projects each participant into a compact local
program, attaches transport and storage, and hands application code a small
affine `Endpoint`.

The complete path is:

```text
hibana::g choreography
  -> runtime::program::project(&program)
  -> runtime::SessionKitStorage::uninit().init()
  -> kit.rendezvous(&mut slab, transport)
  -> registered rendezvous.enter(..., ...)
  -> Endpoint
  -> send() / recv() / offer() / RouteBranch::send() / RouteBranch::recv()
```

## Install

Add Hibana from [crates.io](https://crates.io/crates/hibana):

```bash
cargo add hibana
```

Hibana's runtime code is `#![no_std]` and no-alloc-oriented.

## Quick Start

Run the complete two-role protocol from a Hibana checkout:

```bash
cargo run --example ping_pong
```

It prints:

```text
ping=7, pong=8
```

[`examples/ping_pong.rs`](examples/ping_pong.rs) defines one choreography,
projects both roles, attaches two endpoints, and executes a ping followed by a
pong. Its support carrier is host-only and example-local; production deployments
supply their own `Transport` without changing the choreography or endpoint API.
`offer()` only previews route selection. Endpoint progress happens when
`send()`, `recv()`, or a route branch first-step operation succeeds.

### Pico / `no_std`

The tracked Pico projection example uses the same choreography and public
projection API without an SDK, allocator, host transport, or alternate Hibana
surface:

```bash
rustup target add --toolchain 1.95.0 thumbv6m-none-eabi
cargo +1.95.0 check --manifest-path examples/pico/Cargo.toml \
  --target thumbv6m-none-eabi
```

The repository's
[`examples/pico/src/lib.rs`](https://github.com/hibanaworks/hibana/blob/main/examples/pico/src/lib.rs)
is also the canonical
projected-program input to the repository's Pico size regression gate. The full
gate reports descriptor image, endpoint scratch, live slab, stack, SRAM, flash,
and compile-pressure measurements from the current tree; it does not substitute
an arbitrary fixed runtime capacity for those measurements.

## Verification Boundary

Hibana's claim is split into a protocol theorem and deployment-indexed
composition theorems so deployment assumptions cannot be smuggled in by a
protocol certificate. For one accepted artifact,
`verified_protocol_establishes_execution_guarantees` establishes:

- exact role descriptors refine one projectable choreography;
- subject reduction, session fidelity, and reachable-state progress;
- full operation-key admission with rejection as a zero-transition;
- affine route publication, elastic `.roll` freshness, and finite cancellation
  retirement under their stated premises.

Deployment guarantees are indexed by the strict `CarrierProfile` chain
`Mediated -> Authentic -> Ordered -> Closing -> Fair`; each stronger step
requires explicit carrier evidence. Exact role-image agreement may come from a
static certificate, authenticated manifest, or verified bootstrap session, so
the core wire header needs no mandatory protocol handshake.
`VerifiedCodec` binds canonical wire schemas rather than nominal Rust types.
`StaticDeploymentCertificate.check` rejects missing, extra, reordered, or
byte-different role images and schema identities before release.

The responsibility boundary is strict:

| Hibana establishes | A deployment supplies |
| --- | --- |
| Exact projection and descriptor admission | The concrete carrier implementation |
| Affine endpoint ownership and fail-closed local progress | Peer authentication, delivery, ordering, and closure evidence claimed by its selected profile |
| Conditional composition from explicit premises | Exact remote installation or bootstrap evidence |
| Canonical schema identity at the protocol boundary | Correct downstream codec implementations |

External premises are inputs to stronger deployment-indexed theorems, not
missing Hibana runtime features. Hibana must reject unsupported claims; it must
not absorb carrier, cryptography, failure detection, or application algorithms
into its protocol-neutral core.

The main theorem,
`assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement`, composes
independently checked descriptor bytes, exact deployment role images, verified
codec coverage, one explicit `RustKernelRefinement` premise, exactly the
selected carrier-profile guarantees, and general elastic trace erasure. Lean
does not disguise Kani output as a Lean proof of Rust source. Kani checks finite
packed production kernels, Miri checks strict provenance and re-entry, and Lean
checks exact artifacts and abstract transition laws. The generated witness
covers the finite production kernel inventory; it is not a source-level Lean
proof of arbitrary Rust. The gate also measures that proof metadata is absent
from production Rust, endpoint types, and the fixed eight-byte core header.
Elastic `.roll` histories carry proof-only occurrence ordinals across nested
iterations, then erase them from production traces, endpoint types, descriptor
rows, and wire frames. The production capability artifact covers communication,
sequencing, parallel composition, intrinsic and resolved choice, and recursion;
it does not transfer algorithm-specific safety or liveness automatically.

This establishes the checked Hibana protocol/model contract; it does not
establish that no other system has the same combination. This remains a
conditional cross-tool refinement, not an unqualified source-level Lean
verification of every Rust statement. Kani and Miri remain evidence for the
listed production owners. Carrier liveness, authentication, close notification,
failure detection, and scheduler fairness remain explicit deployment premises.
The Unix datagram proof carrier checks one concrete OS transport; Hibana does
not claim correctness of an arbitrary `Transport` implementation or a fully
verified arbitrary distributed deployment.

The complete theorem inventory, assumptions, counterexamples, and generated
artifact coverage live in the
[Lean proof boundary](https://github.com/hibanaworks/hibana/blob/main/proofs/lean/README.md).
The concrete carrier is documented in the
[Unix datagram proof carrier](https://github.com/hibanaworks/hibana/blob/main/proofs/unix-carrier/README.md).

There are only two public surfaces:

| Surface | Used by | Main names |
| --- | --- | --- |
| Application surface | application code | `hibana::g`, `Endpoint`, `RouteBranch`, `EndpointError` |
| Runtime surface | protocol and runtime crates | `hibana::runtime`, `hibana::runtime::program` |

If you are writing an application, stay on `hibana::g` and `Endpoint`. If you
are implementing a protocol crate, use `hibana::runtime` to project, attach,
bind transport, install explicit route resolvers when needed, and return endpoints.

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
resolver decision at a projected route resolver site. Role code must not read
shared memory, shared atomics, global flags, device registers, or side-channel
state to decide whether a route is ready, a rolled region re-enters, or a message may
be omitted.

Shared memory is especially not protocol authority. A runtime crate may
use memory, atomics, interrupts, DMA, or OS primitives as private transport or
resolver implementation mechanics, but those mechanics must first become
transport frames, descriptor-checked ingress evidence, or route resolver inputs
at explicit resolver sites. They never replace `send()`, `recv()`,
`offer()`, or route branch first-step operations.

### Boundary Contract

Hibana keeps the public API small because the projection boundary carries the
proof work:

- runtime code is `no_std` and no-alloc-oriented;
- descriptor storage is caller-provided, borrowed, static, or slab-backed;
- route shape, ambiguous simultaneous endpoint operations, malformed
  choreography paths, and lane ownership errors are rejected before endpoint
  execution;
- one physical receive lane may change sender only across mutually exclusive
  route arms or after an in-band communication chain proves that the earlier
  frame was consumed; operations from another parallel arm or an unrelated
  route arm are not accepted as causal evidence, and every `roll` is checked
  through one explicit unfolding so its tail cannot hand the next iteration's
  FIFO to a different sender without closing the causal cycle;
- runtime cursor progress is one-way and affine;
- failed endpoint and route branch operations do not authorize hidden progress;
- payload decode is exact;
- message logical labels and transport frame labels are separate concepts;
- route-decision semantics are descriptor metadata, first visible branch
  actions, or explicit resolver decisions, not protocol label numbers.

Application code should not call transport APIs directly from localside logic,
choose route arms by parsing payloads, model resolver decisions as driver-side
branching, treat carrier hints as protocol authority, or match endpoint errors
to continue the same session generation on a hidden alternate path.

## Application Guide

### Endpoint Surface

Everyday application endpoint code uses these names:

- `hibana::g::{Msg, send, seq, route, par}`
- `Endpoint`
- `RouteBranch`
- `EndpointError`

Choreography authors also use the `Program` value returned by the structural
combinators, plus `.roll()` for repeated regions and `.resolve::<ID>()` for
routes whose branch authority is an explicit resolver. Runtime resolver and
transport authority stay inside Hibana; choreography authors describe visible
protocol traffic with `g::Msg` and the structural combinators.

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

The syntax-tree type ends at `project`. Endpoint futures and send/receive/offer
kernels consume message-erased runtime descriptors and do not monomorphize over
the choreography or payload type. The repository gate cold-compiles 1, 64, and
256 distinct payload schemas as `no_std` `thumbv6m-none-eabi` consumers without
raising Rust's recursion limit, then bounds rustc RSS/time and the resulting
`.text`/`.rodata` and rlib growth.

### Messages And Payloads

`g::Msg<L, P>` names one logical protocol message: `L` is the choreography
label and `P` is the payload type. The label describes visible protocol traffic;
it is not a transport frame label and it is not resolved-route branch authority.

Built-in exact codecs cover `()`, `bool`, integers, borrowed byte slices, and
fixed byte arrays. Fixed-width decoders reject trailing bytes.

Every choreography payload implements both `WireEncode` and `WirePayload` so
sender and receiver share one complete wire contract. `WirePayload::SCHEMA_ID`
is that canonical wire contract's compact protocol-local identity, not Rust
nominal type identity. Projection stores it in the descriptor, and endpoint
send/recv rejects a different schema before publishing or consuming protocol
progress. Incompatible encodings or validators must use distinct ids. Different
Rust wrappers may share an id only when they intentionally implement the same
canonical wire schema. Schema `0` is the canonical zero-byte unit schema used by
`()`. A wrapper claiming that schema must also encode and validate exactly zero
bytes; local actions check the zero-byte encoding before committing progress.

The first local attach binds a session generation to one exact compiled program
image; later local roles with different bytes are rejected before allocation.
Local roles needed by a dynamic resolver must attach before that session first
evaluates one: the runtime seals its current local membership before entering
external resolver code and rejects later local attaches before allocation.
Across devices, one global protocol is an initial deployment agreement, not a
`Transport` handshake. A verified host artifact binds every role descriptor to
that choreography, while the live endpoint runtime checks each received frame
against its local descriptor and fails closed at the first divergence. The
`Transport` trait neither receives descriptor bytes nor negotiates program
images.

A deployment may use a versioned application profile or an explicit
application-level bootstrap when it needs runtime version negotiation.
Replayable early traffic may become Hibana protocol evidence only when the
resumed application configuration and replay policy make that traffic safe;
otherwise it must be rejected or delayed. Hibana's compact core header does not
claim to prove cross-binary Rust type identity or carry a program image.

Custom payloads implement the two halves directly:

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

Decoded values may borrow from the received frame:

```rust
// In a message type, use `g::Msg<LABEL, &[u8]>`.
// The decoded value returned by recv is borrowed from the endpoint
// transport frame currently owned by the endpoint.
```

### Sending And Receiving

Use `send()` when the next local step is a send known from the
choreography:

```rust,ignore
endpoint
    .send::<g::Msg<10, [u8; 4]>>(&[1, 2, 3, 4])
    .await?;
```

Use `recv()` when the next local receive can be uniquely committed from the
observed transport evidence and the projected descriptor:

```rust,ignore
let value = endpoint.recv::<g::Msg<11, u16>>().await?;
```

The message type carries the choreography label and payload type. The runtime
checks the projected descriptor and fails closed if the label, lane, or payload
shape does not match.

### Parallel Composition

`g::par(left, right)` combines independent local flows. Projection rejects empty
arms and overlapping endpoint operations that cannot be selected unambiguously.
It does not require message labels to be globally unique.

```rust
use hibana::g;

let left = g::send::<0, 1, g::Msg<50, u32>>();
let right = g::send::<2, 3, g::Msg<50, u32>>();
let parallel = g::par(left, right);
```

Lanes are projection-owned separation units. Application code describes role and
message structure; Hibana assigns the internal lanes needed to preserve affine
parallel progress.

### Routes

`g::route(left, right)` is binary. Intrinsic routes recover the branch from the
first visible endpoint operation; resolved routes use the explicit resolver
decision as branch authority. Both forms require one first-visible controller
across the two arms. `.resolve::<ID>()` disambiguates that controller's branch;
it is not a shared oracle that makes competing first senders projectable.

```rust
use hibana::g;

let accepted = g::send::<0, 1, g::Msg<31, u32>>();
let rejected = g::send::<0, 1, g::Msg<33, ()>>();
let routed = g::route(accepted, rejected);
```

Route choice is a protocol fact, not a transport guess. Prefer in-band choice:
put the real branch-selecting message at the head of each route arm. When a
branch is decided by a local timer, device readiness, budget, or another
non-message signal, mark the route with `.resolve::<ID>()` and install a
resolver through `runtime::resolver`.

When the endpoint reaches a route decision, call `offer()`:

```rust,ignore
let branch = endpoint.offer().await?;

match branch.label() {
    31 => {
        let value = branch.recv::<g::Msg<31, u32>>().await?;
        handle_accept(value);
    }
    33 => {
        let () = branch.recv::<g::Msg<33, ()>>().await?;
        handle_reject();
    }
    label => panic!("unexpected route label {label}"),
}
```

`RouteBranch::label()` reports the selected arm's first logical message label.
For resolved routes this label is not branch authority; `ResolverRef::decide()`
is. For intrinsic routes the first visible endpoint operation is branch
authority. Do not treat `label()` as an exhaustive discriminator for resolved
route authority; resolved route arms may reuse a logical message label when the
resolver decision has already selected the arm.

If the chosen route arm begins with a send, send the first message through the
branch:

```rust,ignore
let branch = endpoint.offer().await?;

match branch.label() {
    40 => {
        branch.send::<g::Msg<40, ()>>(&()).await?;
    }
    41 => {
        let bytes = branch.recv::<g::Msg<41, [u8; 8]>>().await?;
        use_bytes(bytes);
    }
    label => panic!("unexpected route label {label}"),
}
```

The route is never selected by parsing payload bytes. Route authority comes
from the projected descriptor and the first visible branch action. Transport
observation may only supply ingress evidence that is checked against descriptor
metadata; a frame label, payload shape, queue position, or carrier hint is never
an independent route decision.

### Repeated Regions

Repeated protocol regions are structural. Put the re-enterable region in
`seq`, `route`, or `par`, then call `.roll()` on that region.

```rust,ignore
let body = g::seq(
    g::send::<0, 1, g::Msg<30, Chunk>>(),
    g::send::<1, 0, g::Msg<31, Ack>>(),
).roll();

let program = g::seq(body, g::send::<0, 1, g::Msg<32, Done>>());
```

`resolve::<ID>()` marks the route node; `.roll()` marks the surrounding
reentry region. Resolve first, then roll:

```rust,ignore
let looped = g::route(left, right)
    .resolve::<ROUTE_DECISION>()
    .roll();
```

This means that `ROUTE_DECISION` decides this route, and the resolved route is
itself re-enterable. Each reentry reaches the route decision site again.

To widen the repeated region, put `.roll()` on the enclosing `seq`, `route`, or
`par`, while keeping `.resolve::<ID>()` attached to the route itself:

```rust,ignore
let body = g::seq(
    open,
    g::route(write_wait, proc_exit).resolve::<TRAFFIC_DECISION>(),
).roll();
```

Here `open -> resolved route` is the rolled body. The reverse order is invalid:

```rust,ignore
g::route(left, right).roll().resolve::<ID>()
```

`resolve::<ID>()` is only available on `Program<Route<...>>`; after `.roll()`,
the type is `Program<Roll<Route<...>>>`. Semantically, the resolver belongs to
the route decision site, not to the rolled reentry region.

Nested reentry follows the same rule:

```rust,ignore
let inner = g::route(a, b)
    .resolve::<INNER_DECISION>()
    .roll();

let outer = g::seq(prefix, inner).roll();
```

### Failure And Cancellation

Endpoint operations return `core::result::Result<T, EndpointError>`, so
application code should use ordinary `?`:

```rust,ignore
endpoint.send::<g::Msg<1, u32>>(&7).await?;
let reply = endpoint.recv::<g::Msg<2, u32>>().await?;
let branch = endpoint.offer().await?;
let payload = branch.recv::<g::Msg<3, [u8; 4]>>().await?;
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
same-generation retry/reselection API. If time should select a branch, model
time in the choreography itself: use a timer or clock role whose first visible
route action carries the decision.

Protocol-invisible liveness detection belongs inside the transport
implementation. A datagram, serial, or custom carrier that decides an I/O wait is
terminal must return `TransportError` from `poll_send(...)` or `poll_recv(...)`;
Hibana converts that transport failure into terminal session evidence. Such
watchdogs do not create hidden route authority or an alternate branch inside the
current generation.

The public evidence envelopes are domain-specific: `EndpointError` for endpoint
and route branch progress, `ResolverError` for resolver registration and
resolver decisions, and `AttachError` for rendezvous and endpoint attach. There
is no public wide `HibanaError`, and public error-kind enums are not part of the
application decision surface. Error values travel as their domain type; runtime
evidence uses tap, and host `Debug` output records the compact boundary name.
Pico-facing code does not need string accessors or source-path accessors.

## Protocol Runtime

Protocol crates use the same `hibana::g` language as applications. There is
no second composition language.

### Compose And Project

A protocol crate may place transport or runtime prefixes before the application
choreography, then project each role.

```rust
use hibana::g;
use hibana::runtime::program::{project, RoleProgram};

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
return `impl runtime::program::Projectable`, and callers still use the same
`project(&program)` entry. `Projectable` is a sealed choreography bound, not a
second choreography language and not a runtime authority. It has no runtime
storage parameter; facade runtimes keep storage and configuration on their own
types, not on the choreography projection bound.

### Attach An Endpoint

The canonical runtime path is borrowed and caller-provided. The snippet assumes
`runtime_slab: &mut [u8]` is the deployment-owned region selected from measured
program and runtime budgets:

```rust,ignore
use hibana::runtime;
use hibana::runtime::ids::SessionId;

let mut kit_storage = runtime::SessionKitStorage::<MyTransport>::uninit();
let kit = kit_storage.init();

let rv = kit.rendezvous(runtime_slab, transport)?;
let endpoint = rv.enter(SessionId::new(1), &client)?;
```

`SessionKitStorage::init()` is the only public construction path. It writes the
kit in place into caller-owned storage, returns the stable borrow used
by endpoint attach, and drops the initialized kit exactly once. The raw unsafe
initializer and `MaybeUninit` protocol are not part of the public surface.

`SessionKit::rendezvous` borrows the caller-owned runtime slab directly. A
fresh rendezvous carves its runtime prefix from that slab and starts with no
materialized lane storage and no endpoint lease table. Role attach reads the
projected descriptor, grows exactly the lane tables and endpoint lease entries it
needs, and preserves existing session state if a later projected role needs a
wider lane span. Runtime code obtains all runtime resources from that one slab;
lane windows, endpoint counts, and diagnostics capacity stay descriptor- or
runtime-derived.

Endpoint ownership is exclusive at the live `(rendezvous, SessionId, role)`
identity. A second `enter()` for the same live session-role fails at the attach
boundary; dropping the endpoint releases that lease. Different sessions for the
same role, and different roles in the same session, may coexist when the slab has
the descriptor-derived storage for their endpoint leases.

The protocol crate owns concrete `MyTransport` and any ingress demux state. The
application receives only `Endpoint`.

Useful runtime owners:

- `runtime::program::{project, RoleProgram}`
- `runtime::SessionKit`
- `runtime::ids::SessionId`
- `runtime::transport::{Transport, TransportError, PortOpen, Outgoing, ReceivedFrame, FrameHeader, FrameLabel}`
- `runtime::resolver::{ResolverError, ResolverRef, DecisionArm}`
- `runtime::wire::{Payload, WireEncode, WirePayload}`
- `runtime::tap::{TapEvent, TapPort}` and `runtime::tap::Evidence`

### Transport

Implement `runtime::transport::Transport` to connect Hibana to an I/O
system. The transport sees bytes, frame labels, and readiness; it does not own
choreography meaning, route authority, resolver inputs, telemetry, or application
cancellation semantics.

Protocol-invisible carrier watchdogs belong inside `poll_send(...)` and
`poll_recv(...)`: if the transport concludes that progress is impossible, it
returns `TransportError` and Hibana terminates the current session generation.

The transport owns:

- `open(port)` for the descriptor-derived role/session/lane port witness;
- `poll_send(...)` and `poll_recv(...)`; receive returns a borrowed `ReceivedFrame`
  view from transport-managed receive storage, carrying payload bytes and
  carrier observation as one receive value;
- `cancel_send(...)` for transport cleanup when a send future is dropped after
  staging carrier state. It must make the staged frame unobservable; if a byte
  stream has already accepted an irrevocable partial frame, it retires that
  logical direction instead of resuming with corrupt framing;
- `requeue(...)` as the required return path for an accepted staged frame
  that descriptor checks cannot commit.

`PortOpen` exposes the descriptor-derived `local_role`, `session_id`, and
`lane` for the returned Tx/Rx handles. `Outgoing` exposes the projected
`frame_label`, `target_role`, `lane`, and borrowed payload for a send. A
transport may map those facts to carrier-specific headers or queues, but it must
not invent route authority or mutate Hibana's endpoint state.

`open(port)` returns Tx/Rx handles whose lifetime is bound to the transport
borrow, so an embedded carrier can keep buffers, wakers, and DMA bookkeeping
inside the transport owner without allocating or exporting a separate context.

Every `ReceivedFrame` is checked as one carrier observation against exact
session/lane/role/frame-label descriptor authority. Reordering or repetition is
never silently repaired: the observation names a currently enabled event or the
session fails closed. `poll_send(...) -> Ready(Ok(()))` transfers ownership to
the carrier but does not prove that a remote endpoint received the bytes. Hibana
does not authenticate carrier-supplied role identity.

Global fidelity and progress require the stronger affine-delivery premise:
each observation is authentically bound to its mapped peer/direction, logical
events arrive FIFO without unsolicited replay, and eventually arrive or end in
observable terminal closure. A raw carrier may omit that premise only when the
protocol explicitly owns authentication, loss, retry, and freshness. It retains
exact local protocol safety, but not the global affine-delivery conclusion.
Explicit `requeue(...)` restores one staged observation and never creates
another commit.

Fresh transport-instance state is a sufficient carrier generation. Logical
address migration and identifier rotation remain inside that generation; a new
carrier instance starts a new one. Authenticated ordered subchannels can
implement Hibana lanes directly or through an internal demux. Unordered messages
may instead be admitted as protocol-owned raw observations; sequence identity,
replay rejection, loss recovery, and ordering then remain in the protocol rather
than becoming false `Transport` guarantees. Input may remain bound to an
untrusted ingress role until peer identity has been validated.

Under the strong affine-delivery profile, peer closure is scoped to the mapped
Hibana direction, not necessarily to the physical carrier connection. A
multiplexed carrier may retire one mapped subchannel while leaving unrelated
sessions and subchannels intact.

Lean proves exact observation admission and a separate strong affine carrier
profile, not arbitrary `Transport` implementations. A carrier claiming global
fidelity must gate authenticated peer binding, FIFO framing, explicit requeue,
cancelled-send invisibility, close wakeup, generation isolation, and replay
policy. This conformance work does not enlarge Hibana's runtime API.

The canonical receive-side observation is the `ReceivedFrame` returned by
`poll_recv(...)`. Payload and carrier observation cross the transport boundary
together; there is no separate receive-observation hook.
`ReceivedFrame::deterministic(...)` is valid only for a single deterministic
direct receive or an already materialized route branch receive descriptor.
Route offer and unresolved route demux require
`ReceivedFrame::framed(FrameHeader::from_bytes(header_bytes), payload)`, where
the transport supplies one carrier-owned eight-byte observation and Hibana
performs the session/lane/role/label comparison internally before any endpoint
progress can consume the payload. Route/session/progress authority remains in
Hibana.

### Session Templates

Hibana is protocol-neutral, but not limited to one static connection. Dynamic
interaction instances, request/reply attempts, and coordination rounds are
separate `SessionId` values instantiated from the same finite descriptor.
Distinct session transitions commute in the Lean model, fresh attach cannot
overwrite live authority, and retired identities may be reused only through a
fresh transport generation. Production tests interleave two instances of one
descriptor and prove that an endpoint fault in one does not poison the other.
No protocol state becomes a Rust continuation type, so this composition does not
create type explosion.

A lossy or reordering carrier can expose raw message observations while keeping
authentication, sequence identity, retry, timers, recovery, and ordering in
algorithm-owned payload and local state. Once an authenticated ordered
subchannel exists, its logical events may satisfy the strong affine-delivery
premise. Concurrent logical channels use separate sessions or
descriptor-derived lanes; closing one mapped direction must not close unrelated
instances.

A stateful distributed algorithm can instantiate repeated finite interaction
templates while keeping persistent state, membership, scheduling, and recovery
policy outside Hibana's protocol runtime. Fresh sessions isolate retries and
peer faults; reconfiguration creates a fresh verified artifact for the new
finite role set. Hibana proves communication conformance and session isolation.
Algorithm-specific safety and liveness invariants require their own proofs.

One session has at most sixteen declared roles and no channel delegation. A
protocol requiring an unbounded role set in one atomic choreography is outside
this core. Dynamic deployments remain expressible when their communication can
be factored into finite verified templates and explicit reconfiguration
sessions.

### Ingress Demux

Ingress demux state belongs inside the transport owner. `poll_recv(...)`
returns payload bytes and descriptor-checked ingress evidence as one receive
value, so endpoint progress can verify the frame against the projected
descriptor before previewing an `offer()` or committing a `recv()` or
route branch first-step operation.

Headerless receive is only valid when direct `recv()` can select one live
descriptor from the observed lane, or when `RouteBranch::recv()` already owns a
materialized receive descriptor. Branch observation and unresolved route demux
require framed, descriptor-checked evidence. Payload shape, frame label, queue
position, and carrier-local hints do not select route arms.

### Receive Evidence

Receive evidence is checked against the projected descriptor. `ReceivedFrame`
has two construction paths: headerless deterministic frame construction is valid
only for direct `recv()` or for `RouteBranch::recv()` after `offer()` has already
materialized a unique receive descriptor. `offer()` and unresolved route demux
require `ReceivedFrame::framed(...)` with descriptor-checked frame metadata.
Payload shape, queue position, carrier id, and driver observations are never
branch authority.

### Resolvers

Resolvers are installed by the protocol crate for explicit route resolution
sites. A resolved route uses `ResolverRef::decide()` as branch authority; an
intrinsic route derives branch authority from projected first visible endpoint
evidence. Resolver state is the external input owner: use
`ResolverRef::decision_state(...)` when a resolver needs protocol-specific
observations. Resolver failure rejects the step; it does not authorize any
alternate semantic path.

Within one rendezvous generation, every attached role uses the same registered
resolver authority. Across independent devices, a resolver does not synthesize
cross-runtime agreement. Projection rejects route arms with competing
first-visible controllers even when `.resolve::<ID>()` is present. The protocol
or carrier must still supply the same decision to observers that act before
receiving branch evidence; otherwise use an intrinsic route whose first in-band
message communicates the choice.

```rust,ignore
use hibana::g;
use hibana::runtime::program::{project, RoleProgram};
use hibana::runtime::resolver::{DecisionArm, ResolverError, ResolverRef};

const ROUTE_RESOLVER: u16 = 7;

struct RouteState {
    accept: bool,
}

fn route_decision(state: &RouteState) -> Result<DecisionArm, ResolverError> {
    let arm = if state.accept {
        DecisionArm::Left
    } else {
        DecisionArm::Right
    };
    Ok(arm)
}

let routed = g::route(accept_body, reject_body).resolve::<ROUTE_RESOLVER>();
let role0: RoleProgram<0> = project(&routed);
let state = RouteState { accept: true };

rv.set_resolver(&role0, ResolverRef::<ROUTE_RESOLVER>::decision_state(&state, route_decision))?;
```

External resolver state uses the same explicit registration path. Route
decisions come from the typed resolver registered for the route site. The
external owner stays outside Hibana, owns its input state, and installs a typed
`ResolverRef` for the route decision site. When that owner has a decision, it
returns `DecisionArm` for that resolver id; otherwise it delegates to
another user-registered `ResolverRef`. That local resolver is still explicit
route authority; it does not transfer authority to external state. Hibana never
treats external telemetry, payload bytes, or transport readiness as route
authority by itself.

```rust,ignore
struct ExternalResolverOwner {
    loaded: bool,
    local_resolver: ResolverRef<'static, ROUTE_RESOLVER>,
}

static LOCAL_ROUTE_STATE: () = ();

fn local_decision(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn external_decision(
    owner: &ExternalResolverOwner,
) -> Result<DecisionArm, ResolverError> {
    if owner.loaded {
        Ok(DecisionArm::Right)
    } else {
        owner.local_resolver.decide()
    }
}

let routed = g::route(local_arm, external_arm).resolve::<ROUTE_RESOLVER>();
let role0: RoleProgram<0> = project(&routed);
let owner = ExternalResolverOwner {
    loaded: true,
    local_resolver: ResolverRef::<ROUTE_RESOLVER>::decision_state(
        &LOCAL_ROUTE_STATE,
        local_decision,
    ),
};

rv.set_resolver(&role0, ResolverRef::<ROUTE_RESOLVER>::decision_state(
        &owner,
        external_decision,
    ))?;
```

### Tap Evidence

`RendezvousKit::tap()` returns a read-only `TapPort` over Hibana's retained
runtime evidence ring. Tap is not a logger and not a user telemetry channel; it
is the minimal postmortem surface for endpoint send/recv, transport frame and
fault evidence, lane acquire/release, route arm selection, and resolver audit.

Each `TapEvent` is an immutable 16-byte record. Public code can read `ts()`,
`id()`, `causal_key()`, `arg0()`, `arg1()`, and `evidence()`, but cannot
construct or push tap events. `Evidence` exposes only `kind()`, `reason()`, and
`input()`. Canonical event ids and reasons live in `runtime::tap`; code should
compare against those constants rather than hard-coded numbers.

The ring retains the latest 32 events. A new `rv.tap()` created after a failure
starts at the oldest retained event and then reads forward in event order, so
postmortem inspection does not need a previously-open port.

## Build And Test

For a published crate consumer, the useful checks are ordinary Cargo commands:

```bash
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 check --lib -p hibana
cargo +1.95.0 test -p hibana --test ui
cargo +1.95.0 test -p hibana --test lane_lifecycle_tap
cargo +1.95.0 doc -p hibana --no-deps --no-default-features
```

The crate package ships self-contained compile/UI/API and runtime behavior tests.
Repository-only gates that read `.github`, public-surface allowlists,
measurement snapshots, or maintainability budgets stay outside the production
crate package.

For a repository checkout, maintainers should run the repository gate suite
before release:

```bash
bash ./.github/scripts/run_final_form_gates.sh
```

Use that gate rather than raw `cargo test` for release decisions; repo-only unit
tests are enabled through `hibana_repo_tests`. The repository suite protects the
public surface, `no_std` build, projection boundary, descriptor publication,
future layout, route authority, size measurements, every explicit `[[test]]`
target, the pinned Miri owner suite declared by `.github/miri-toolchain`, and the
Core/Std-only Lean proof package. It also requires the Pico message-heavy type
pressure matrix, which fails on superlinear projection image, metadata, or
compiler-memory growth. The quality workflow runs the complete pinned
Kani/CBMC inventory as a separate required job because it has different host
dependencies; a zero-test or missing-harness match fails its gate.
