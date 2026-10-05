# One census for direct receive lanes

Direct receive formerly checked the requested contract, then repeated a complete
role-row scan for every logical lane. An idle receive on a role with sixteen
parallel lanes decoded the same immutable metadata repeatedly, even though its
requested label was eligible on only one lane.

The receive path now makes one census of enabled, non-session receive occurrences
with the requested label and schema. It polls the resulting lanes in the same
ascending order. The two separate contract scans are removed. The temporary set
uses the existing u8 wire lane domain and existing lane-word/view operations: at
most eight u32 words, or 32 bytes, on the poll stack. It adds no persistent state,
heap allocation, public API, descriptor index or future field. A lane bit is a
transport search restriction, never an event commit or a completion receipt.

The actual metadata decoder, route and reentry eligibility checks, full inbound
key unique matching, codec validation, prepared commit and atomic publication
remain in force. In particular, collecting two occurrences on one lane does not
resolve their ambiguity: full-key matching still examines every occurrence and
rejects multiple matches. Schema errors retain the last enabled schema when no
requested-schema occurrence exists. No eligible occurrence gives PhaseInvariant.
Every row is decoded by the census; a forged invalid row may therefore be rejected
earlier than a former early-return scan. No malformed descriptor is newly admitted.

`Census.lean` proves the census equals the previous existential per-lane query,
preserves the ordered lane polling list, requires an actual eligible occurrence,
and bounds a wire lane's word/bit coordinates. Its arbitrary row eligibility is
the unchanged Rust label/schema/origin/event_enabled predicate at the start of
one poll. `Census.smt2` checks the exact u8 lane and eight-u32-word insertion,
noninterference, eligibility and zero initialization with five UNSAT obligations
and a SAT witness for omitted eligibility. `check.sh` audits exact theorem axioms
and solver output. The source bridge is recorded in `sources.sha256`.

These are scoped refinement obligations. They do not prove Rust decoding,
unsafe pointer correctness, physical I/O, timing bounds or unconditional network
availability. During the census there is no transport polling; transport preamble
polling does not change the cursor or selected-route state. The temporary view is
borrowed from initialized words and cannot outlive the poll. The existing full-key
matching and commit preparation recheck actual evidence before publication.

`tests/cursor_send_recv/recv_lane_census.rs` includes a real Endpoint pending
receive timing probe with sixteen independent lanes, and a parallel same-label,
different-schema case that must retain the other lane's queued frame. The probe
is explicitly ignored in normal tests and run in release mode for measurements;
wall-clock timing is a diagnostic, not a correctness assertion. Before the change,
three 10,000-poll samples took 91.904, 80.206 and 80.259 ms on this Mac. The baseline
log is `/tmp/hibana-recv-census-baseline.Vc56cC/tests.log`. After the change, the
same probe took 11.155, 5.847 and 6.468 ms; the median changed from 80.259 to
6.468 ms. This approximately 12-fold improvement is specific to an idle receive
with sixteen independent lanes on this Mac. It does not establish MCU latency.

`/tmp/hibana-recv-census-check.skHc6W` records 474 internal tests, 128 receive,
route/reentry/parallel and resource-join tests, and 159 API/source-surface tests,
with strict all-target library Clippy and a thumbv6m library check. Existing
explicitly ignored tests remain ignored; the new timing probe is run separately.
`/tmp/hibana-recv-census-final-form.BkChyy/measurements.log` records the complete
final-form measurement gate, including thumbv6m protocol artifacts, existing
fixed flash/SRAM/stack budgets and compile-pressure cases. All gates passed
without changing their budgets. The complete no-default thumb library archive
has 97,344 bytes of measured sections; this archive sum is not a deployed
firmware image. The host operation stack probe measured a maximum of 2,623
bytes, and the runtime model's maximum SRAM was 5,290 bytes. Neither is a
measurement of physical Pico stack use. Full remote CI and real-device latency
qualification remain separate gates.
