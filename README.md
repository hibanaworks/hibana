<div align="center">
  <img src="hibana-header.svg" width="600" alt="HIBANA - Choreography-Derived Runtime Enforcement for Rust" />

  <p>
    <a href="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml"><img src="https://github.com/hibanaworks/hibana/actions/workflows/quality-gates.yml/badge.svg" alt="Quality gates" /></a>
    <a href="https://crates.io/crates/hibana"><img src="https://img.shields.io/crates/v/hibana.svg" alt="crates.io" /></a>
    <a href="https://docs.rs/hibana"><img src="https://docs.rs/hibana/badge.svg" alt="docs.rs" /></a>
    <a href="LICENSE-APACHE"><img src="https://img.shields.io/crates/l/hibana.svg" alt="Apache-2.0 OR MIT" /></a>
  </p>
</div>

# Hibana

Hibana lets Rust programs execute a finite multiparty protocol from one global
choreography. The choreography is projected into a compact program for each
role, and every send, receive, or route step must match that program before
progress commits.

Hibana is a **choreography-derived runtime enforcement kernel**. It is
unconditionally `no_std`, needs no allocator, and uses caller-provided storage.
Protocol state is kept in compact descriptors instead of a distinct Rust
continuation type for every state, so protocol growth does not become endpoint
type growth.

- one choreography for up to 16 roles;
- asynchronous `send`, `seq`, `par`, binary `route`, and guarded `roll`;
- affine endpoints that may be dropped but cannot publish progress twice;
- transport-neutral integration through one `Transport` trait;
- the same public API on hosted and embedded targets.

Hibana does not implement a network stack or a distributed algorithm. It
enforces the protocol at each attached endpoint and states the carrier,
deployment, codec, and scheduling conditions needed to lift that local result
to a distributed guarantee.

## Quick Start

Add the crate:

```bash
cargo add hibana
```

Run the complete two-role example from a repository checkout:

```bash
cargo run --example ping_pong
```

```text
ping=7, pong=8
```

The example is self-checking, ships in the published package, and is executed
by the release gate. Read the complete source in
[`examples/ping_pong.rs`](examples/ping_pong.rs). Its protocol is small enough
to see in one place:

```rust
use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
};

fn ping_pong() -> (RoleProgram<0>, RoleProgram<1>) {
    let choreography = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<1, 0, Msg<2, u32>>(),
    );

    (project(&choreography), project(&choreography))
}
```

The executable example adds caller-owned runtime storage and an in-memory host
transport, attaches both role programs to one `SessionId`, and drives the two
endpoints with `send()` and `recv()`. An embedded deployment supplies its own
transport, storage budget, and executor; the choreography and endpoint
operations stay the same.

## Model

```text
global choreography
  -> project(&choreography) once per role
  -> compact RoleProgram descriptors
  -> attach each role to one SessionId
  -> drive Endpoint::send / recv / offer
  -> commit exactly one permitted step or fail the session generation
```

**Multiparty** means peer, direction, and order come from one choreography
projected for every role. **Asynchronous** means a successful send transfers a
frame to the carrier; it does not wait for the remote receive. **Affine** means
one live endpoint owner can advance a projected step at most once. The endpoint
may be dropped, but it cannot be cloned or used to publish duplicate progress.

Hibana has two public surfaces:

| Audience | Surface | Responsibility |
| --- | --- | --- |
| Role implementation | `hibana::g`, `Endpoint`, `RouteBranch`, `EndpointError` | Describe messages and drive one attached role |
| Protocol integration | `hibana::runtime` | Project roles, provide storage and transport, attach sessions, and install explicit resolvers |

Application code normally stays on `hibana::g` and `Endpoint`. The
[`runtime` API](https://docs.rs/hibana/latest/hibana/runtime/) is for the crate
that integrates a protocol with its carrier and execution environment.

## Protocol Language

| Form | Meaning |
| --- | --- |
| `g::send::<FROM, TO, g::Msg<LABEL, PAYLOAD>>()` | One visible asynchronous message |
| `g::seq(left, right)` | `left` precedes `right` |
| `g::par(left, right)` | Independent arms may progress concurrently |
| `g::route(left, right)` | One of two protocol arms |
| `route.resolve::<ID>()` | An explicit resolver owns a non-message choice |
| `body.roll()` | A guarded structural region may re-enter |

Projection rejects unsupported route shapes, ambiguous simultaneous endpoint
operations, conflicting parallel lanes, empty parallel arms, and unguarded
re-entry. Logical message labels identify choreography messages; they need not
be globally unique.

A same-role `send` is a zero-byte local effect and must use `()`. It does not
hide payload data in a private queue. Repeated regions add no iteration field to
endpoint types, descriptor rows, or the eight-byte core frame header.

### Messages And Endpoints

`g::Msg<L, P>` names logical label `L` and wire payload `P`. Built-in exact
codecs cover `()`, `bool`, integers, byte slices, and fixed byte arrays. Custom
payloads implement `WireEncode` and `WirePayload`; `WirePayload::SCHEMA_ID`
identifies the canonical wire schema checked by the descriptor.

The schema id is not a cross-binary Rust nominal type id and is not sent in the
core frame header. Two implementations may share an id only when they implement
the same canonical bytes and validation rules. Received values may borrow from
the carrier-owned frame for the endpoint borrow.

The role API is deliberately small:

| Operation | Commit rule |
| --- | --- |
| `Endpoint::send::<M>(&payload)` | Commits after the descriptor accepts `M` and the carrier accepts the frame |
| `Endpoint::recv::<M>()` | Commits after carrier evidence, `M`, and its payload all match |
| `Endpoint::offer()` | Previews a selected route without committing by itself |
| `RouteBranch::send` / `recv` | Commits the selected arm's first visible step |

Endpoint progress happens when `send()`, `recv()`, or a route branch first-step
operation succeeds. A dropped preview, rejected operation, or successful
`requeue(...)` consumes no protocol step. A committed mismatch or carrier fault
terminates the affected session generation instead of opening a retry path.

### Route Choice

Prefer in-band choice: make the message that selects a branch its first visible
action. `RouteBranch::label()` then reports that selected arm's first logical
message label, and the first send or receive is performed through the branch.
Payload contents, queue position, and carrier observations are never branch
authority.

When a timer, readiness signal, budget, or another non-message signal owns the
choice, mark the route with `.resolve::<ID>()` and install a typed
`ResolverRef::<ID>::decision_state(...)`. Resolver failure rejects the step; it
does not select another arm. Resolver state is local input, so roles that act
before receiving in-band branch evidence need deployment-level agreement on the
decision.

## Runtime Boundary

The runtime borrows one caller-provided byte region and derives its internal
layout from the projected descriptors. `SessionKitStorage::uninit().init()` is
the single construction path; `SessionKit::rendezvous(...)` binds the storage
region and one transport, and `RendezvousKit::enter(...)` attaches a projected
role. The runnable example shows the complete sequence.

The first local attach binds a session generation to one exact compiled program
image. A byte-different image or a second live attach for the same
`(rendezvous, SessionId, role)` is rejected. Retrying or changing participants
uses a fresh finite session and, when the choreography changes, a fresh
projected artifact.

| Boundary | Hibana owns | Integration owns |
| --- | --- | --- |
| Choreography | Projection and exact local descriptor admission | Distributing the accepted role images |
| Endpoint | Affine, fail-closed progress | Driving enabled operations fairly |
| Payload | Schema identity and validation at the endpoint | Correct canonical codec implementations |
| Carrier | Checking observed session, lane, peer, direction, event, label, and schema | Peer binding, FIFO delivery, replay exclusion, generation isolation, and closure notification |
| Security | No hidden security claim | Authentication, cryptography, admission control, and failure detection |

### Transport

`Transport` owns byte buffers, framing, readiness, ingress demultiplexing, and
wakeups. Hibana owns choreography meaning and route authority. A successful
`poll_send` transfers a frame to the carrier but does not prove delivery. A
`poll_recv` result is checked against the next descriptor event before endpoint
progress commits.

Protocol-invisible liveness detection also belongs to the transport. A wait
that cannot progress returns `TransportError` from `poll_send` or `poll_recv`;
it does not create a hidden timeout branch. The full framing, cancellation,
requeue, and borrowed-buffer contract is documented on
[`Transport`](https://docs.rs/hibana/latest/hibana/runtime/transport/trait.Transport.html).

Hibana does not require an in-band protocol-image handshake. A deployment may
establish exact role-image agreement through its build artifact, authenticated
manifest, or an application protocol. This keeps protocol negotiation out of
the core and allows transports with very different framing and bootstrap
requirements.

### Failure And Observation

`EndpointError` is terminal evidence for the current session generation. A
codec mismatch or carrier failure poisons the generation and wakes local
waiters. Remote cancellation termination additionally requires the carrier to
make peer closure observable after accepted frames are drained or quarantined.

`RendezvousKit::tap()` exposes a read-only ring of the latest 32 compact
16-byte evidence records. It is for diagnostics; it cannot construct progress
or select a route.

## Guarantees

For every attached endpoint, Hibana enforces:

- exactly one permitted descriptor transition for each successful operation;
- zero transitions for previews, rejection, drop before commit, and requeue;
- fail-closed checks of peer, direction, lane, event, label, schema, and payload;
- no duplicate publication by one endpoint owner;
- first-fault preservation and terminal local waiter wakeup;
- isolation between live `SessionId` generations in one runtime.

`project(&choreography)` also rejects unsupported or ambiguous choreographies.
The machine-checked global theorems apply to exact role images accepted by the
independent protocol artifact checker.

### When Deadlock Freedom Holds

Successful projection alone is not a distributed deadlock-freedom guarantee.
Hibana provides per-session protocol deadlock freedom when all of the following
hold:

1. every role executes the exact accepted image of the same projectable
   choreography;
2. peers agree on each canonical wire schema and use conforming codecs;
3. the carrier binds the expected peers and directions, preserves FIFO order,
   excludes replay across session generations, and eventually delivers each
   accepted frame or reports terminal closure;
4. the executor eventually polls operations that remain enabled.

Under those conditions, every reachable live, unfinished protocol state has an
enabled transition. An intentional infinite `.roll` may continue forever, and
application cycles spanning separate sessions remain application scheduling
concerns.

The [Unix datagram carrier](https://github.com/hibanaworks/hibana/tree/main/proofs/unix-carrier)
is an executable conformance example for peer binding, FIFO delivery, replay
exclusion, closure wakeup, and generation isolation. A production carrier
realizes the same contract in the way appropriate to its medium.

## Measured Footprint

The repository compiles the public choreography and projection API for
`thumbv6m-none-eabi` without an allocator, SDK, host transport, or target-only
Hibana API. With Rust `1.95.0`, the tracked release measurements are:

| Hibana-owned quantity | Current | Release ceiling |
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

The linked-artifact and library rows are `thumbv6m-none-eabi` release
measurements. The complete rlib is not the flash cost paid by one linked
protocol. Stack high-water is measured around runtime operations on the pinned
`aarch64-apple-darwin` measurement host.

The modeled SRAM envelope combines the target's Hibana `.data/.bss`, storage
owners, one measured live slab shape, and runtime operation stack. Component
maxima may come from different shapes and must not be added as one observed
run. Application state, concrete transport buffers, executor state, interrupt
stacks, codec scratch, and platform startup are outside this Hibana-owned
envelope.

[`examples/pico/src/lib.rs`](examples/pico/src/lib.rs) is the tracked `no_std`
projection sample. The release gate regenerates these measurements and rejects
any value above its ceiling.

## Verification

Hibana uses complementary tools rather than attributing every guarantee to one
checker:

| Tool | Responsibility |
| --- | --- |
| Lean | Global and role-local semantics, projection artifacts, fidelity, progress, cancellation, and repeated-region freshness |
| Kani/CBMC | Bounded exhaustive checks of compact prepare/commit kernels and ownership state |
| Miri | Strict provenance, borrowing, drop, cancellation, waiter, and callback re-entry behavior |
| Rust tests and release gates | Executable examples, compile-time rejection, carrier conformance, package contents, and resource regressions |

The [Lean proof boundary](https://github.com/hibanaworks/hibana/blob/main/proofs/lean/README.md)
lists the exact theorems, assumptions, and generated artifacts. Each tool is
evidence only for the part it checks.

Run the repository release gate with the pinned toolchain:

```bash
bash ./.github/scripts/run_final_form_gates.sh
```

It executes the example, Rust tests, `no_std` target checks, rustdoc, package
checks, Miri, Lean, and resource measurements. CI runs the pinned Kani/CBMC
inventory as a separate required job.

## Scope

Hibana covers finite-role sessions executed through its endpoint API. It does
not claim correctness for code that bypasses the endpoint, arbitrary transport
implementations, application algorithms, carrier authentication or
cryptography, failure-detector accuracy, unbounded role sets, or channel
delegation. Cross-binary agreement is defined over canonical wire schemas, not
Rust nominal type identity.

Hibana is licensed under either Apache-2.0 or MIT, at your option.
