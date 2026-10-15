# Delayed ingress across parallel offer lanes

Base revision: 1b28efffe3cc5c93080dc21682c25b87ae5b84a3.

The regression projects independent production and receive/reclamation lanes,
then a parallel application-mode route. After receiving the selected mode,
`offer()` can choose the local reclamation controller before a data frame is
available. Its collecting continuation used to poll only that chosen lane.
A later descriptor on the independent receive lane could remain queued forever.
This reproduces on the repository TestTransport as well as a one-entry carrier;
it also reproduces on c3d89f78, so it is not introduced by the pending-event scan
optimization.

The collecting path now also polls existing active offer lanes while no frame
has been acquired. An actual frame transfers into the existing Selecting state
and follows the existing exact-observation route validation. The guard comes
from poll_offer_collecting: transport collection is entered only when stage.ingress
is empty. No accepted frame is abandoned, no readiness is fabricated, and no
state field, public API, capacity or wire representation is added.

Validation performed locally on the candidate:
- 477 internal unit tests; 8 existing ignored.
- cargo test --tests: 38 suites, 307 aggregate passes and 1 ignored. This count
  overlaps default library tests and is not added to the internal-unit count.
- strict all-target Clippy; thumbv6m-none-eabi no_std check.
- added regression under nightly-2026-05-28 Miri with strict provenance.
- QUIC consumer's ordinary regression suite, including 22 connected tests.

The QUIC consumer also needed to pin its role futures at its existing executor
join boundary to avoid transient copies exceeding libtest's default stack.
The connected tests then passed without enlarging the stack.

The minimal regression bounds executor polls and never substitutes sleeps,
capacity increases or a fixed response for real endpoint progress.

## Focused verification

The original runtime repair cbc42c9d228f58d9ce548d2e41f7b32c595b9a4c passed
the full remote quality-gates run 37584970627. The additional evidence below
qualifies its collecting guard and handoff; it changes no runtime code or API.

`Ingress.lean` checks 14 kernel theorems for opaque frame values. Acquired
ingress makes collecting non-pending; independent transport is inspected only
with empty ingress and pending selected evidence. A real independent frame
enters Selecting intact with the unchanged frontier-visit context. Absence
remains waiting and either error remains terminal. The finite active-lane scan
returns only an observed frame and preserves priority of an earlier error.
Its historical witness exhibits selected-only waiting despite a ready sibling.
The axiom inventory contains only propext where needed; no sorry or native
decision axiom is accepted by the kernel-output check.

`Ingress.smt2` checks the same collection guards over unbounded opaque frame
values: 8 counterexample obligations are UNSAT, and 2 ownership/delayed-arrival
premises are SAT. `check.sh` audits actual Lean output and the ordered Z3
results; it does not lint source paths or implementation spellings.

Source correspondence:

| Model | Runtime boundary |
| --- | --- |
| collect | poll_offer_collecting, including the existing nonempty-ingress guard |
| advance waiting | poll_offer_collecting's Pending arm |
| selecting frame/context | stage_transport, frontier_visited.take, carry_ingress |
| resolving/error | existing selected-evidence and terminal error arms |
| scan | poll_any_active_offer_transport_frame in active descriptor-lane order |

The production-endpoint regression now exercises six distinct payloads through
delayed rolled arrivals, abandons and re-enters every first offer preview, checks
exact payload and ACK equality, then requires the actual End/Done/Closed/Retired
join. This connects frame preservation and cancellation to real endpoint code.

Focused local checks on 2026-10-07:
- Lean 4.30.0: all 14 theorems and the exact output axiom inventory passed;
  Z3: ordered 2 SAT and 8 UNSAT results passed
  (/tmp/hibana-parallel-ingress-proof.PoJwmb).
- The strengthened production-endpoint test passed
  (/tmp/hibana-parallel-ingress-test.1D3gJb).
- Workspace all-target strict Clippy passed
  (/tmp/hibana-parallel-ingress-clippy.t7Pzoc).
- The same test passed nightly-2026-05-28 Miri with strict provenance,
  including each cancelled preview (/tmp/hibana-parallel-ingress-miri.hbzZIW).
  Its target was removed after checking; clean-final.log records removal.

The workflow executes the new kernel/SMT checks. A successful run of the earlier
runtime-repair commit is not evidence of this later proof/check revision's CI.

These mathematical guards are not a formal refinement of all Rust execution.
Descriptor decoding/active-lane enumeration, transport fairness, wake delivery,
affine lease implementation and final branch validation remain Rust/model-check
and existing proof boundaries. Selecting a carried frame does not itself grant
offer completion or admission of an invalid frame. Runtime layout, capacities,
wire representation and partial-publication rejection remain unchanged.

## Ordinary input during parallel roll reentry

`tests/shared_query_tail_reentry/ordinary_parallel.rs` adds two public-endpoint
regressions. One role receives ordinary data in parallel with a message-based
choice, then handles another choice before repeating. Sending the next ordinary
data before the parallel choice previously made `offer()` return
`PhaseInvariant`. The failure reproduces without resolvers or a following
continuation on both f1c4dd4bab211b005687cfec15bb53a1f77500ed and
c2001ec06944d7f3b299e16b5b3efb5b0ba0366a.

A previous visit's suffix is not necessarily enabled at the next visit's head.
The first repair checked the cached branch heads before I/O. That repair passed
these two tests but later blocked a valid service request after review rework:
the new visit could begin in a different route. The cached-suffix guard has
therefore been removed. Exact ingress classification now retains an ordinary
receive for `recv` and clears only that lane's stale offer cache. The second
regression observes an independent choice before completing its lane, then
queues ordinary data. Both regressions retain four distinct payloads, repeatedly
poll and abandon pending previews, and check exact ordinary receive data. See
[`ordinary-offer-ingress`](../ordinary-offer-ingress) for the replacement boundary
and its explicit proof assumptions.

Strict Miri also found an independent ownership defect while cancelling the
pending preview. A staged frame held a shared slice into an inline Rx buffer.
Passing that frame by value protected the slice during `requeue_on`, which
then mutably borrowed the same Rx. The affine receipt now retains the original
slice pointer and lifetime without materializing a shared slice during requeue.
Validation and successful receive commit reconstruct the borrowed payload.
The existing receipt still proves exact port identity and one-shot resolution.
The pointer representation has the same size as the borrowed slice and adds
no allocation or public API.

The two new regressions pass with strict provenance under nightly-2026-05-28
Miri, including pending-preview cancellation with a nonempty inline payload.
The existing send-selected receive continuation, 28 public runtime ownership
tests, and six receive-receipt tests also pass. These tests use Tokio's current
thread executor without an OS I/O driver when they require no OS I/O. Production
runtime behavior has no Miri-only alternative.

The Lean ingress model and the existing dependency, selector, and resolver
models retain their stated boundaries. Their checks do not establish Rust
pointer provenance or prove the entire cache implementation. The new production
regressions and strict Miri checks cover those concrete boundaries.

The first cached-suffix candidate passed 885 workspace Rust tests (12 ignored), workspace
all-target Clippy, the no_std host and thumbv6m builds, the full Lean gate,
the related selector/dependency/resolver/ingress Lean and Z3 checks, and nine
Kani harnesses for frontier selection and receive-receipt ownership. The
size snapshot and subsystem layout gates passed: aggregate measured flash
94,694 bytes, SRAM 5,418 bytes, and maximum measured stack 2,751 bytes in their
checked programs. These measurements do not bound arbitrary consumer protocols.
These historical checks do not qualify the replacement ingress classification.
QUIC interoperability must be checked with the selected repair actually built.

The later prefix-dependency repair also sends newly collected primary-lane
frames through Selecting before any branch authority is used. This extends the
same exact classification to arrivals after an offer first became pending.
Its separate model and concrete delayed-frame regression are documented in
[`offer-prefix-dependency`](../offer-prefix-dependency).
