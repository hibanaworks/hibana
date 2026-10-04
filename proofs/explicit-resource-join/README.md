# Explicit resource joins with the existing API

This bundle adds validation and an executable integration example. It does not
change Hibana's runtime, public API, or the meaning of `par`.

The executable example is `tests/explicit_resource_join.rs`. Run it with:

    cargo test --locked --test explicit_resource_join

## Responsibility boundary

The application defines actual completion. A worker must settle its native IO,
including cancellation and outstanding reservations, before reporting completion.
Hibana enforces the communication order declared in the choreography. Private
Rust ownership makes resource extraction and destruction inaccessible to workers.

The finite ending boundary is:

    seq(par(RX completion, TX completion), return-or-abort, return receipt)

Both notifications must actually be received by the joining role. Successful
send or cancellation-request acceptance is not successful IO completion.
The application checks operation identity and success before taking the normal
return branch. The runtime cannot infer those facts from an arbitrary payload.

The example's `Owner`, `Use`, and `Joined` are private, illustrative application
types, not new Hibana API. An integration with an existing joined owner should
reuse that ownership boundary rather than create a second manager or token.
`Joined` can only be constructed beside the actual receives. `Owner` checks the
identity of the borrowed operation and consumes the sole owned resource when it
returns it. Use borrows must end before that consuming call.

The `Cell` counter and oneshot gate are test fixtures. They are not a concurrent
native IO implementation, a DMA cancellation mechanism, or a hardware-stop
guarantee. A production backend must keep DMA/IO storage alive until its real
completion acknowledgement; merely dropping a Rust future is insufficient.

## Parallelism and emergency stop

RX and TX can finish in either order. An `offer` must inspect the actual label;
assuming the RX branch is offered first is incorrect. The tests deliberately
retain TX IO while RX progresses.

Emergency physical stop must run without waiting for these normal notifications.
Only subsequent resource release or reuse requires actual IO settlement.
The independent-stop test drives a resident `roll` while the finite ending
branch waits and again after it completes. It does not put a join around the
whole resident command loop. No extra synchronization is added per packet.

On failure, do not fabricate normal completion. The failed result path and
native cleanup responsibilities are explicit. Old operation messages cannot
authorize the next operation's normal return.

## What the checks establish

- Rust endpoint tests exercise the real current projection/runtime and a bounded
  in-memory carrier. They check normal and failed completion, stale serials,
  carrier acceptance without actual receive, and independent stop/resident roll.
- `Join.lean` imports the existing executable `Hibana.GlobalSemantics` and checks
  five concrete accepted/rejected histories. Four additional theorems concern
  the stated integration predicates, not a verified translation of Rust.
- `check_join.py` has six UNSAT obligations and four SAT non-vacuity/negative
  witnesses. It explicitly demonstrates that fabricated completion messages
  invalidate the physical-IO guarantee when the backend premise is removed.

These are scoped checks, not a proof of arbitrary Rust side effects, all possible
applications, hardware timing, or a new resource-aware kernel feature.
