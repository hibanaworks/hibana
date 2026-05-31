# Hibana Protocol Integration Guide

This note is for protocol crates that implement transport, ingress binding, or
protocol-owned control kinds. Application authors should start with
`README.md`.

## Control Messages

Control messages are ordinary choreography messages. Public protocol-owned
controls are explicit wire tokens:
`g::Msg<LABEL, GenericCapToken<K>>`, where `K` implements the
protocol-neutral `WireControlKind` trait. Endpoint-owned local minting is
crate-owned and exposed only through Hibana's built-in route/loop decision
kinds.

The message label is choreography identity. Control meaning comes from the
control kind's descriptor metadata, not from reserved numeric labels.

There are two public layers:

- `GenericCapToken<K>` plus `WireControlKind` is the choreography message
  shape for explicit wire control payloads.
- `integration::cap::WireControlEffect` is the protocol-visible effect set
  evaluated by the hibana control kernel.

Only route and loop decision owners are provided as built-in public kind types:

- `RouteDecisionKind`
- `LoopContinueKind`
- `LoopBreakKind`

The full protocol-visible wire effect catalogue is:

| Effect | Meaning | Usual use |
| --- | --- | --- |
| `WireControlEffect::Fence` | Orders or authorizes a protocol-visible control boundary without changing topology or transaction state. | Protocol-owned explicit wire barrier. |
| `WireControlEffect::StateSnapshot` | Records the current session/lane generation before a mutation. | Snapshot before transaction, abort, restore, or topology-sensitive mutation. |
| `WireControlEffect::StateRestore` | Restores previously snapshotted state after a failed or aborted mutation. | Rollback path paired with `StateSnapshot`. |
| `WireControlEffect::TxCommit` | Commits a snapshot-backed transaction and finalizes that lane generation. | At-most-once commit of a protocol mutation. |
| `WireControlEffect::TxAbort` | Aborts a snapshot-backed transaction and records the abort path. | Fail-closed transaction cancellation. |
| `WireControlEffect::AbortBegin` | Starts an explicit abort handshake. | First step of a protocol-owned abort sequence. |
| `WireControlEffect::AbortAck` | Acknowledges an abort handshake. | Idempotent acknowledgement for abort completion. |
| `WireControlEffect::TopologyBegin` | Opens a topology transition intent with source/destination rendezvous, lane, and generation facts. | Distributed lane/rendezvous reconfiguration. |
| `WireControlEffect::TopologyAck` | Validates and acknowledges a topology intent at the destination side. | Destination half of topology coordination. |
| `WireControlEffect::TopologyCommit` | Commits an acknowledged topology transition and bumps generation. | Source-side topology finalization. |

These effects are not new application commands. A protocol that needs topology,
transaction, abort, snapshot, or fence control still writes ordinary
choreography messages, usually with a protocol-owned `WireControlKind` that
maps to the relevant `WireControlEffect`. The runtime consumes projected descriptor
metadata fail-closed. Payload contents, labels, transport hints, and driver
`if`/`else` logic never become route or transaction authority.

Explicit wire controls always use the public wire path and reusable descriptor
semantics. Local route/loop decisions stay Hibana-owned and are exposed only as
the built-in `RouteDecisionKind`, `LoopContinueKind`, and `LoopBreakKind`.

Topology and transaction control are integration-level tools. Use them only
when the protocol itself needs a choreography-visible transition:

- topology: move or rebind a lane/rendezvous relation with
  `TopologyBegin -> TopologyAck -> TopologyCommit`;
- transaction: bracket a multi-step mutation with
  `StateSnapshot -> TxCommit` or `StateSnapshot -> TxAbort/StateRestore`;
- abort: make cancellation explicit with `AbortBegin -> AbortAck`;
- fence: insert a protocol-owned ordering or readiness boundary without adding
  domain-specific APIs to hibana core.

Do not add `g::topology`, `g::tx`, driver-side repeat loops, or payload-driven
branch selection. The authority source remains the choreography plus the
projected descriptor.

## Custom Wire Control

```rust,ignore
use hibana::g;
use hibana::integration::cap::{WireControlKind, GenericCapToken, WireControlEffect};

const CUSTOM_WIRE_MSG_LABEL: u8 = 200;
const CUSTOM_WIRE_TAP_ID: u16 = 0x03c8;

struct CustomWireKind;

impl WireControlKind for CustomWireKind {
    const TAG: u8 = 0x90;
    const NAME: &'static str = "CustomWire";
    const TAP_ID: u16 = CUSTOM_WIRE_TAP_ID;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
}

type CustomWireMsg =
    g::Msg<{ CUSTOM_WIRE_MSG_LABEL }, GenericCapToken<CustomWireKind>>;
```

Use the built-in `RouteDecisionKind`, `LoopContinueKind`, and `LoopBreakKind`
with `()` payloads for local route/loop decisions. Use an explicit
`GenericCapToken<K>` payload for protocol-owned wire controls. Explicit wire
controls use reusable descriptor semantics; Hibana does
not mint or register their token bytes.

## Transport

Implement `integration::transport::Transport` to connect Hibana to an I/O
system. The transport owns:

- `open(port)` for the descriptor-derived role/session/lane port witness;
- `poll_send(...)` and `poll_recv(...)`;
- `cancel_send(...)` for transport cleanup when a send future is dropped after
  staging carrier state;
- `requeue(...)` as the required rollback path for a frame that descriptor
  checks cannot commit.

`open(port)` returns Tx/Rx handles whose lifetime is bound to the transport
borrow, so an embedded carrier can keep buffers, wakers, and DMA bookkeeping
inside the transport owner without allocating or exporting a separate context.

The only optional transport hook is `recv_frame_hint(...)`, a non-blocking
route-observation hint-drain. It must not consume payload bytes. Once it yields
a frame label, it must not yield the same observation again until
`poll_recv(...)` or `requeue(...)` stages fresh receive state.

Transport sees bytes, frame labels, and readiness. It does not own choreography
meaning, route authority, carrier recovery policy, policy inputs, telemetry, or
cancellation semantics. `cancel_send(...)` is only cleanup for an uncommitted
send preview. Protocol-invisible carrier watchdogs belong inside
`poll_send(...)` and `poll_recv(...)`: if the transport concludes that progress
is impossible, it returns `TransportError` and Hibana terminates the current
session generation.

## Binding

Use `enter()` when the transport can deliver the next payload directly.

Use `EndpointSlot` when the integration demuxes ingress into binding-owned
payload handles, and attach with `enter_with_binding(...)`. A binding slot
returns `IngressEvidence` for a lane and later reads from the selected handle:

```rust,ignore
impl hibana::integration::binding::EndpointSlot for MyBinding {
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
}
```

`IngressEvidence` is demux evidence only. It may support descriptor-checked
route observation, but it is not an independent route decision and must not be
used as dynamic route authority without resolver authority.

## Resolver Policy

Resolvers are installed by the protocol crate for explicit policy points. Route
and loop control messages use the same decision vocabulary; loop is not a
separate user-facing resolver API.

Resolver state is the policy input owner:

```rust,ignore
struct DecisionState {
    preferred_arm: hibana::integration::policy::DecisionArm,
    input: core::cell::Cell<u32>,
}

fn choose_decision(
    state: &DecisionState,
) -> Result<hibana::integration::policy::DecisionResolution, hibana::integration::policy::ResolverError>
{
    if state.input.get() != 0 {
        return Ok(hibana::integration::policy::DecisionResolution::Arm(state.preferred_arm));
    }

    Ok(hibana::integration::policy::DecisionResolution::Defer)
}
```

Resolver failure rejects the step; it does not fall through to a different
semantic path.
