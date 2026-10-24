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

The gate rejects assumptions in production harnesses. Constrained symbolic
inputs use total generators whose images retain the valid domain. Fixed
rejection witnesses are distinguished from universally quantified rejection
properties. Finite matching and route-correlation domains also have exhaustive
checks that call the actual production query with assertions enabled.

Run from the repository root:

```sh
bash .github/scripts/check_kani.sh
```

The version is pinned in [`.github/kani-version`](../../.github/kani-version).
Kani uses its required compiler and CBMC. The entry point already invokes the
resource guard and runs one job at a time. Do not wrap it in another guard or
run another heavy local check beside it. A 3GB memory-budget stop is interrupted
verification, never success.

The separate [ownership-equation harness](../checks/projection/endpoint-delegation/kani)
includes the production source validator. The [CI workflow](../../.github/workflows/quality-gates.yml)
checks both packages. Miri covers executed pointer, borrowing and destruction
behavior; the [Lean models](../lean/README.md) cover the stated protocol and
transport rules.
