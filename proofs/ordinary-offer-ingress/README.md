# Ordinary receives observed by offer

An offer can inspect a lane whose cached branch belongs to a completed visit.
The next frame may instead be an ordinary receive at the head of a new visit.
Rejecting that valid frame faults the endpoint. Rejecting both cached branch
heads before I/O can also hide a new visit whose first branch is different.

The existing exact ingress matcher classifies a unique enabled user receive.
A first-visible branch receive follows the branch selection path. An ordinary
receive is requeued on its original port, and only that lane's cached offer is
removed. The protocol cursor and selected arm do not advance. Actual receive
progress refreshes the cache. Missing, ambiguous and internal frames retain the
existing rejection path; neither payload contents nor a cached suffix grants
branch authority.

| Model | Rust boundary |
| --- | --- |
| `Match` | `select_observed_ingress`, using the existing exact key and enabled-event checks |
| `Action.restore` | `ObservedIngressSelection::OrdinaryReceive` |
| `restore.receive` | successful `requeue_offer_transport_payload` on the affine receipt's original port |
| `restore.position` / `selected` | ordinary-ingress return before branch selection or operation commit |
| `restore.offers` | `clear_lane_offer_cache` for the observed lane |

`Check.lean` proves eight properties of this boundary, including exact opaque
payload preservation and independence of other lanes. `Check.smt2` checks eight
corresponding counterexample obligations as UNSAT and one historical witness as
SAT: an invalid cached suffix does not imply that the actual incoming branch is
invalid. Run `bash proofs/ordinary-offer-ingress/check.sh` to audit both outputs.

The model assumes correct classification of a unique enabled receive, successful
transport requeue, and correct lane-to-port binding. It does not prove the entire
descriptor matcher, cache implementation, wake delivery or Rust pointer rules.
Production Endpoint regressions in `shared_query_tail_reentry/ordinary_parallel.rs`
retain distinct nonempty payloads through repeated pending offer polls and
cancellation. `nested_resolver_reentry.rs` tests a new service request after
review rework while an independent parallel flow stays idle. Strict Miri checks
exercise their actual receipt and transport implementation.
