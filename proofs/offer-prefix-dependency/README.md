# Preserve a visible prefix before a nested receive

The Endpoint regression in `tests/nested_resolver_reentry.rs` projects a
four-role protocol. Role D receives Start, then an outer choice whose left arm
begins with a receive on lane 1 and continues to a nested choice on lane 0.
The controller can already have sent both frames. Previously, `offer` could
preview the nested receive before the visible prefix; consuming it was rejected.
The reproduction fails on main f1c4dd4 and repair c03c7af3.

An unseen containing choice used to disable its conditional predecessor row.
Event admission now evaluates that row under the candidate's projected path
when the preview has no choice. This includes a completed parent's masked
choice during reentry. The separate freshness check still reads committed
history; no choice is committed by this dependency view. Identity, conflicts, visit freshness and
lane-head checks must all pass before admission can report an outstanding
predecessor rather than an invariant failure.

An exact, unique receive waiting for that predecessor is returned to its own
transport port. Other lanes remain observable. A 256-lane bitmap local to one
poll prevents reading the same deferred lane again in that poll. It changes no
protocol progress, route choice, persistent offer cache, Endpoint field or
future layout. It is discarded on return; it is not a second progress owner.
The kernel does not wake itself just because it requeued a future frame.

All newly collected frames return to the same exact matching path, including
frames that arrive after an offer started waiting. A preselected scope alone
cannot turn a late ordinary or future frame into branch authority. Unknown,
ambiguous, stale and conflicting frames retain their rejection paths. Payload
validation still occurs at the actual receive commit.

| Model | Rust boundary |
| --- | --- |
| `previewArm` | dependency view inside `EventCursor::event_enabled` |
| `checkEvent` | checked identity, route conflict, reentry and lane-head guards before `PredecessorPending` |
| `action` | unique enabled/pending matches in `select_observed_ingress` |
| `requeue` | successful affine `requeue_offer_transport_payload` on the original port |
| `remaining` | poll-local `deferred_lanes`, checked by both offer transport scans |
| matching after collection | newly received frames transfer from Collecting back to Selecting |

Lean checks 13 kernel theorems, including strict reduction of the lanes remaining
in one poll, preservation of opaque frame data and independence of other lanes.
Z3 checks 12 UNSAT obligations and a SAT witness for the old missing-parent
guard. Run `bash proofs/offer-prefix-dependency/check.sh` to audit exact outputs
and their axiom inventory.

These models assume correct descriptor decoding, exact unique matching and
lane-to-port binding. They model successful requeue, not Rust pointer provenance
or every transport implementation. The Endpoint test delays an already accepted
prefix in the test network, polls while the later frame remains ready, delivers
the prefix with a wakeup and abandons its first preview. Distinct nonempty
payloads must survive. It covers frames queued before offer and frames arriving
after its first pending poll. The ordinary-input regression also includes late
arrival. Strict Miri checks exercise the actual receipt and buffer path.
