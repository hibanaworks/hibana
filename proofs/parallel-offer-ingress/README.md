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
from poll_collect_offer_evidence: Pending is possible only when stage.ingress
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

No new Lean theorem or full remote CI result is claimed for this candidate.
The minimal regression bounds executor polls and never substitutes sleeps,
capacity increases or a fixed response for real endpoint progress.
