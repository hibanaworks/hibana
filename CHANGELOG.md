# Changelog

## 0.10.0 (unreleased)

### Added

- Endpoint delegation through the existing `send` and `recv` operations. An
  `Endpoint` payload moves its logical role and current continuation to the
  receiver. Initial issuance uses one `EndpointBinding` supplied by the resource
  integration. Dropping a value or timing out does not restore transferred
  ownership.
- Session-aware initial binding selection through
  `Transport::initial_endpoint_binding(session, role)`. A transport can select
  distinct resources with the same role in concurrent sessions. A new session
  does not renew issuance for an existing physical resource.
- Endpoint ownership diagnostics returned as values by `g::diagnose`.
- `OutOfOrder` errors for unfinished local predecessors, distinct from internal
  progress invariant errors. Both stop the affected local session run.
- `Endpoint::par()` and nested parallel borrows. An array pattern selects the
  number of independent fragments, such as
  `let [mut left, mut right] = endpoint.par()?;`. The executor can poll both
  fragments concurrently. The parent endpoint remains borrowed until they end.

### Fixed

- Separated each transport call's handle borrow from the lifetime of storage
  inside its `Tx` or `Rx`. Safe transport implementations can be polled repeatedly
  without the kernel creating overlapping full-lifetime mutable borrows.
- Preserved queued continuation messages when another role reenters a rolled
  body. In 0.9.9, an accepted continuation could later fail with `PhaseInvariant`
  after a new visit began.
- Assigned separate lanes to sequential receives from unrelated senders and
  preserved their source order across lane changes.
- Distinguished every static message position on the wire. An early message
  after a choice could otherwise be accepted as the first receive of an
  unselected branch when the selected branch used another lane.

### Performance

- Reused subtree sizes, scope entries and branch conditions during projection.
  Local dependency checks still compare the generated order with the typed source.
- Indexed receive candidates by logical role and message label.
- Filtered completed and unrelated send candidates before decoding their full
  metadata.
- Skipped completed events before inspecting roll completion dependencies.
- Read immutable descriptor fields directly and reused projected lane masks.
  Dependency checks can still inspect several events, so operation cost can
  depend on the protocol.

The core remains `no_std`, uses caller-supplied storage and has no normal
dependencies or heap allocation.

### Updating a transport implementation

0.10.0 changes the required `Transport` method signatures. Keep the lifetime
inside a GAT handle separate from the borrow made by one call:

```rust
fn poll_send<'payload>(
    &self,
    tx: &mut Self::Tx<'_>,
    outgoing: Outgoing<'payload>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), TransportError>>;

fn cancel_send(&self, tx: &mut Self::Tx<'_>);

fn poll_recv<'poll>(
    &'poll self,
    rx: &'poll mut Self::Rx<'_>,
    cx: &mut Context<'_>,
) -> Poll<Result<ReceivedFrame<'poll>, TransportError>>;
```

The send and cancellation borrows end when the call returns. A received frame
may borrow the receive handle until that frame is released. Replace signatures
that tie the handle borrow to its internal lifetime, such as
`&'a mut Self::Tx<'a>` or `&'a mut Self::Rx<'a>`.

Do not retain an outgoing payload pointer across `Pending`. Keep pending I/O
state in the transport. Cancellation must discard staged bytes or disable that
logical send direction if its bytes can no longer be withdrawn.

The frame header is nine bytes: session ID (four, big endian), lane, source,
target, and the global message position (two, big endian). Preserve all nine
bytes when forwarding a framed message. Repetitions reuse their static message
position, so the transport must still prevent duplication and replay.

A transport that supports delegation must keep a logical inbox available to the
new holder, including frames accepted before the transfer. It must provide peer
binding, FIFO delivery without duplication or replay, and isolation between
session runs. A deadline or I/O error does not prove nondelivery and does not
authorize reissuing an Endpoint.
