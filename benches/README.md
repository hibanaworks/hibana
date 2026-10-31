# Measurements

## Source projection

For source-lowering profiles, compile with `RUSTFLAGS='--cfg hibana_profile'`
and call `hibana::g::profiling::source(&global)`. It runs the normal receive-lane
allocator natively, using projection's cached source and actor image. It reports
full/resumed inspections, alignment attempts and recolorings. Time the call in
the host program or inspect its stacks with a native profiler. These counters
are absent from normal builds. This measures lane assignment, not all compile
time or the later local-program checks. Normal builds still perform const
projection with `long_running_const_eval` forbidden.

## Runtime cost

Run `cargo +stable bench --locked --bench runtime_cost` from the repository root.
The benchmark uses the current kernel, Tokio's current-thread executor, and the
example `InMemoryTransport`.

Two roles execute 64 fixed communications, followed by either 8 or 256 unit
messages. The sequential case contains 32 request/reply pairs. The parallel
case contains 32 requests in one fragment and 32 replies in another. Both roles
use `par()` and all four local flows are polled concurrently. The source uses
balanced composition, without repetition scopes or delegation.

A sample times 128 successful endpoint operations: 64 sends and 64 receives.
Attachment, parallel-borrow construction, tail draining, and destruction are
outside the timer. Payload assertions remain enabled. Three samples warm up
the process, followed by 30 measured samples. Output reports the minimum and
median nanoseconds per endpoint operation.

`HIBANA_PERF_SAMPLES`, `HIBANA_PERF_CASE` (`seq`, `par`, or `all`), and
`HIBANA_PERF_TAIL` (`8`, `256`, or `all`) select measurement inputs. They do not
select kernel execution paths.

Wall-clock results vary with the host. Keep profiler results separate from the
timed samples. A profile of the whole process also includes attachment and tail
draining.

## Portable runtime work gate

Run `bash .github/scripts/measure_runtime_work.sh` under the repository's
resource guard. The `runtime-work` job uses the same check on macOS and Linux;
neither operating system is required by the counter implementation. The core
also builds with instrumentation for the Pico and Pico 2 bare-metal targets.

With `--cfg hibana_profile`, each actor's caller-supplied storage holds eight
`u64` counters (64 bytes). There is no clock, OS call, heap allocation, lock or
global counter. Normal builds omit the counters and their updates. The host
test adapter runs real endpoint operations and formats the observations after
each measured window. It uses the same protocols and transport as the timing
benchmarks: 128 operations for seq/par, 512 for roll. Attachment, `par()` setup
and tail draining are excluded.

The counters record operation-identity lookups, conflict lookups, inspected
outbound and inbound candidates, framed-receive candidate queries, admission
checks, inspected predecessor rows, and route-row queries. Each is counted at
the corresponding query or loop body. These are named Rust work counts, not CPU
instructions. They detect additional searches and checks at those sites; they
do not detect a more expensive implementation of an unchanged query. Timing
and native CPU profiles remain necessary for that question. EPF diagnoses the
existing communication events; those events do not expose these internal
search counts.

Each case runs in two fresh sessions. Counts must agree exactly, and every case
and metric must be present once per session. The reviewed limits in
`.github/measurement_snapshots/hibana-runtime-work.tsv` add 1% to the initial
counts, rounded up; zero counts have zero allowance. The gate cannot rewrite
these limits. Empty, duplicate, missing or inconsistent observations fail.
Results include the source hashes, current executable hash, compiler version
and raw observations. The gate does not claim constant-time execution for
arbitrary protocols.

The optional `.github/scripts/measure_runtime_instructions.sh` profiler remains
available on native Linux x86-64 with Callgrind. Its historical instruction
budgets and raw profiles are separate from the portable gate. It counts `Ir`
inside the benchmark windows and validates `totals` against summed self costs.
It requires no container or VM.

Run `cargo +stable bench --locked --bench roll_cost` to measure repeated
request/reply communication. The roll body contains 1, 8, or 32 round trips.
Every sample executes the same 128 round trips (512 endpoint operations),
completing 128, 16, or 4 visits respectively. Request and reply labels are reused
at every position in the body, exercising the search among matching messages.
Projection, attachment and
destruction are outside the timer. Payload assertions remain enabled, and
Tokio polls both participants concurrently on its current-thread executor.

`HIBANA_PERF_BODY` (`1`, `8`, `32`, or `all`) and `HIBANA_PERF_SAMPLES` select
inputs. The default is three warmups and 30 measured samples per body size.
Callgrind windows use the same boundaries as the timer; with one measured
sample their totals cover 2,048 endpoint operations. These workloads do not
exercise nested repetition, branching or endpoint delegation. The portable
gate measures each body size separately. The optional Callgrind profiles cover
2,048 operations after including warmups; the portable counters cover the single
512-operation window. Reusing a label at many positions still increases
candidate search cost.
