# Changelog

## 0.10.0 (unreleased)

### Added

- Endpoint delegation through the existing `send` and `recv` operations. An
  Endpoint payload moves its logical role and current continuation to the
  receiver. Initial issuance uses one `EndpointBinding` supplied by the resource
  integration. Dropping a value or timing out does not restore transferred
  ownership.
- `Endpoint::par()` and nested parallel borrows. An array pattern selects the
  number of independent fragments, such as
  `let [mut left, mut right] = endpoint.par()?;`. The executor can poll both
  fragments concurrently. The parent Endpoint remains borrowed until they end.

### Fixed

- Separated each transport call's handle borrow from the lifetime of storage
  inside its `Tx` or `Rx`. Safe transport implementations can be polled repeatedly
  without the kernel creating overlapping full-lifetime mutable borrows.
- Preserved queued continuation messages when another role reenters a rolled
  body. In 0.9.9, an accepted continuation could later fail with `PhaseInvariant`
  after a new visit began.
- Preserved branch knowledge, causal order and independent operations across
  nested routes, rolled protocols and Endpoint holder changes.
- Returned Endpoint ownership errors from `g::diagnose` as diagnostic values.
- Distinguished an unfinished local predecessor (`OutOfOrder`) from other
  progress invariant errors. Both stop the affected local session run.
- Passed the session to `Transport::initial_endpoint_binding`, allowing one
  transport to select distinct resources with the same role in concurrent
  sessions. It does not renew issuance for an existing physical resource.

### Performance

- Indexed receive candidates by logical role and message label.
- Filtered completed and unrelated send candidates before decoding their full
  metadata.
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

A transport that supports delegation must keep a logical inbox available to the
new holder, including frames accepted before the transfer. It must provide peer
binding, FIFO delivery without duplication or replay, and isolation between
session runs. A deadline or I/O error does not prove nondelivery and does not
authorize reissuing an Endpoint.
