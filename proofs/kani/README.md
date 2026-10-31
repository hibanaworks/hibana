# Bounded implementation verification

Kani harnesses call production functions to check bounded properties of source
ownership, descriptor layout, storage arithmetic, operation transitions,
codecs, frame headers and resolver identity. Their symbolic domains and unwind
bounds are part of each harness's scope. They do not prove arbitrary-source
compiler correctness or distributed delivery.

[`harness-inventory.json`](harness-inventory.json) is the reviewed list of names.
The gate compares it with Kani's structured discovery output, assigns every
harness to a proof group and requires an exact successful summary for each
group. Missing, extra, empty, skipped or failed groups are errors.

The gate streams CBMC's JSON through a Rust diagnostic filter. It omits only the
trace of a Kani-generated reachability probe. All property identities, statuses
and other fields pass through, including real failure traces. Kani still checks
assertion reachability and decides the verification result. Assertion, memory
and unwind-failure controls must remain failures before the core proofs run.

Descriptor-heavy cursor cases run separately with CBMC's array and arithmetic
refinement. Every input, assertion and unwind check remains enabled. Fixed
history/query products are partitioned before compilation to fit the memory
budget, rather than merged into a single large verification equation.
The sequential two-event predecessor check partitions its lower bound into
`0`, `1` and `2`, the complete image of the former symbolic `u8 % 3` input.
Actor identifiers, seed bits and path selection remain symbolic in each part;
all three parts keep the original assertions and unwind bound.

The three-event linear and structured receive checks use a separate pairwise
reference: the first receiver knows that receive, and each intervening message
can relay it from its sender to its receiver. This reference stores reached
role identifiers directly; it does not call the production flow traversal or
build its indexes. All nine sender, receiver and lane inputs remain arbitrary
`u8` values in both proofs.
The structured fixture has three messages in one marker-free block. Its
message loop uses a bound of four and its block loop a bound of two, including
their exit tests. Function identifiers come from the actual compiled metadata;
all other loops and recursion retain the five-step bound. Unwinding assertions
remain enabled, so a changed source requiring more iterations fails verification.

The fixed receive-lane translation fixture has only one input, a `u8` receiver.
Its complete 256-value domain is evaluated by mandatory const assertions and
the native/Miri `lane_class_checks_every_receiver` test in
`src/global/const_dsl/allocation/receive_lanes/tests.rs`. The four 64-value const
blocks and runtime enumeration retain the same production call, wrapping peers
and five result assertions. This finite check is counted separately from the
Kani inventory. The Miri gate runs the entire receive-lane test module with
strict provenance; it does not sample receiver identifiers. Symbolic direction
exclusion, actor, ownership and matching obligations remain Kani checks.

The gate rejects assumptions in production harnesses. Constrained symbolic
inputs use total generators whose images retain the valid domain. Fixed
rejection witnesses are distinguished from universally quantified rejection
properties. Finite matching and route-correlation domains also have exhaustive
checks that call the actual production query with assertions enabled.
The [projection checks](../checks/projection/endpoint-delegation) run those
finite domains with checked native execution; they are distinct from Kani's
symbolic verification.

Run from the repository root:

```sh
bash .github/scripts/check_kani.sh
```

The version is pinned in [`.github/kani-version`](../../.github/kani-version).
Kani uses its required compiler and CBMC. The entry point already invokes the
resource guard and runs one job at a time. Do not wrap it in another guard or
run another heavy local check beside it. A 3GB memory-budget stop is interrupted
verification, never success.

The ownership-equation proofs in
[`src/g/source/ownership/kani.rs`](../../src/g/source/ownership/kani.rs) run in
this same gate against the production source validator. Miri covers executed pointer, borrowing and destruction
behavior; the [Lean models](../lean/README.md) cover the stated protocol and
transport rules.
