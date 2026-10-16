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
history; no choice is committed by this dependency view. Identity, conflicts and
visit freshness must pass before admission reports an outstanding predecessor.
Lane-head admission is checked when that predecessor has completed. A future
event need not already be a current head: the pending predecessor may itself
prevent that head from becoming available. Successful admission still requires
both the predecessor and the lane-head check.

An exact receive waiting for that predecessor is returned to its own transport
port. Ordered future receives can reuse one wire key; retaining their frame
does not select any of those events. Enabled candidates must still be unique,
and an enabled match takes priority over pending aliases. Other lanes remain
observable. A 256-lane bitmap local to one
poll prevents reading the same deferred lane again in that poll. It changes no
protocol progress, route choice, persistent offer cache, Endpoint field or
future layout. It is discarded on return; it is not a second progress owner.
The kernel does not wake itself just because it requeued a future frame.

All newly collected frames return to the same exact matching path, including
frames that arrive after an offer started waiting. A preselected scope alone
cannot turn a late ordinary or future frame into branch authority. Unknown,
ambiguous enabled, stale and conflicting frames retain their rejection paths. Payload
validation still occurs at the actual receive commit.

| Model | Rust boundary |
| --- | --- |
| `previewArm` | dependency view inside `EventCursor::event_enabled` |
| `checkEvent` | identity, conflict and reentry checks; dependency wait; lane-head check before enabled admission |
| `action`, `selectIngress` | unique enabled selection or retention without event selection in `select_observed_ingress` |
| `requeue` | successful affine `requeue_offer_transport_payload` on the original port |
| `remaining` | poll-local `deferred_lanes`, checked by both offer transport scans |
| matching after collection | newly received frames transfer from Collecting back to Selecting |

Lean checks 24 kernel theorems, including strict reduction of the lanes remaining
in one poll, preservation of opaque frame data and independence of other lanes.
Z3 checks 23 UNSAT obligations and three SAT witnesses for the old missing-parent
guard, premature lane-head rejection and early next-arm rejection.
Run `bash proofs/offer-prefix-dependency/check.sh` to audit exact outputs
and their axiom inventory.

These models assume correct descriptor decoding, exact unique matching and
lane-to-port binding. They model successful requeue, not Rust pointer provenance
or every transport implementation. The Endpoint test delays an already accepted
prefix in the test network, polls while the later frame remains ready, delivers
the prefix with a wakeup and abandons its first preview. Distinct nonempty
payloads must survive. It covers frames queued before offer and frames arriving
after its first pending poll. The ordinary-input regression also includes late
arrival. Strict Miri checks exercise the actual receipt and buffer path.

`tests/sequential_resolver_parallel_prefix.rs`, included by the same integration
target, places one rolled resolver in a parallel fragment and another resolver
after that fragment. Its controller follows the projected protocol; the observer
must retain suffix input while its other-lane prefix is delayed. Two ordered
suffix receives reuse a wire key. The test checks repeated pending polls, actual
wakeup, an abandoned preview and all distinct nonempty payloads. This separates
current-head rejection from the earlier nested-guard failure.

`tests/future_rolled_choice.rs`, included by that same integration target,
compares two legal delivery timings of one three-role global. A controller can
send the next rolled visit's different-arm entry before the receiver finishes
its current nested resolver. The kernel used to reject that early entry, even
though sending it later succeeds. The receiver now retains it on the original
port, completes its current nested choice and then consumes the next entry.
Dropping and reacquiring the nested preview must not change that order, and all
nonempty payloads must survive.

`FutureEntry` models the additional descriptor/history guard in
`future_route_entry_waits_for_current_arm`. The caller has already matched the
exact lane, peer and wire key. Retention requires a reentrant scope, a different
arm's first entry on that lane, and an already consumed user receive at the
current arm's lane entry while that arm remains unfinished. It cannot preview
or commit a new arm. After the current visit finishes, the retained frame still
passes ordinary event admission; this guard does not authorize its consumption.
Existing `requeue` and `remaining` theorems apply unchanged. This is a model of
the guard and retention boundary, not a verification of every Rust cursor query.
