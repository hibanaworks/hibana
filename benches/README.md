# Runtime cost

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

CI records these measurements. Wall-clock results vary with the host and are
not an instruction-count regression gate. Keep profiler results separate from
the timed samples. A profile of the whole process also includes attachment and
tail draining.

The `runtime-instructions` CI job separately runs this same benchmark under
Callgrind on Linux x86-64. Client requests enable instrumentation only around
the operation windows above, excluding attachment, parallel-borrow setup and
tail draining. Each process executes three warmups and one sample, so its
inclusive `Ir` count covers 512 endpoint operations. Each of the four cases is
run twice; raw profiles, exact tool versions and counts are uploaded. Profiling
uses no alternate kernel, old implementation or extra core dependency. Other
hosts retain the same benchmark without the platform-specific client requests.

The Rust reader checks the final `totals` against the sum of self costs, excluding
inclusive call costs to avoid double counting. Callgrind 3.22.0 can report zero
in its optional `summary` after these instrumentation windows; that header is
not used as the instruction count. Missing, truncated or inconsistent profiles
fail the job.

The instruction budget in
`.github/measurement_snapshots/hibana-runtime-instructions.tsv` records Linux
x86-64, Rust 1.99.1 and Valgrind 3.22.0. Each limit is the larger of the two
actual f8c32d44 CI counts plus 5%, rounded up. Both runs must fit. Compiler or
target changes require reviewing the new profiles, without automatically
increasing the limits. This guards against further regressions from that
checkpoint. It does not establish parity with historical main or remove the
remaining performance work; instruction counts and elapsed time are separate
measurements.
