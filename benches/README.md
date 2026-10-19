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

Instruction counts are a separate metric from elapsed time. This first profile
collection has no committed instruction budget yet. A budget must be based on
the measured Linux profiles, with compiler/target and tolerance recorded, before
this job can be described as a regression gate.
