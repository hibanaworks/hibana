# Rolled-route ownership regressions

The 2026-10-02 handoff targets development commit
`3aef31ba015c75ea824b8b41f5603b03f5dd336b` (Hibana 0.9.6). Its three
failures are valid adjacent offers being rejected, a later phase's packet
being consumed as an earlier unchosen suffix, and nested result reentry
failing after a fresh request. The supplied candidate was marked unverified;
the production repair does not apply that patch wholesale.

## Runtime contract

An observed packet selects exactly one immutable receive descriptor satisfying
the full source/lane/frame-color identity and the same dependency, conflict,
progress and resident-step guards as typed receive. Offer entry comes from a
projected first-visible receive or the first receive of a route arm on that
lane. Cursor proximity and completion of a sibling do not supply authority.
Wire colors remain distinct from application message labels.

Consumed events and unchosen suffixes passed by the current visit require an
authorized fresh roll visit. A materialized lane head bounds that visit. For
a parked lane, a later committed event on the same lane establishes past
progress; an absent head alone does not. Completion uses committed arm
history, while conflict checks use the prospective visit. A candidate arm
cannot rewrite the history used to authorize reentry.

A prepared fresh-visit commit clears owned completion, route selections and
readiness evidence together. It materializes the first resident row once and
rewinds that row's owned lanes; resetting a later row must not overwrite its
prefix. Committed events outside the reset scope remain intact. Lane offers
are then rebuilt from the resulting state. These changes stay within the
existing preflight/publish boundary and add no fallible mutation after it.

The public API, wire format, endpoint storage and dependency set are unchanged.
The observed receive shortcuts and duplicate done checks are removed. The
private `EventArmView` distinguishes committed history from candidate preview;
it is a callback argument, not a new stored state machine.

## Proof scope and reproduction

Run `bash proofs/rolled-route-ownership/check.sh [evidence-directory]` from any
directory. It uses the canonical Core/Std-only Lean 4.30.0 toolchain and Z3's
SMT-LIB CLI. CI also invokes this check.

- `TraceValidity.lean`, `PhaseOwnership.lean` and `NestedReentry.lean` execute
  the actual `Hibana.GlobalSemantics` rules on the minimal histories: valid
  adjacent choices, current-phase continuation, renewed earlier roll visits,
  alternating nested results, and rejection of suffixes without new prefixes.
- `EligibleIngress.lean` proves that unique selection retains every modeled
  descriptor guard and rejects disabled or ambiguous candidates.
- `ReentryAdmission.lean` proves the revised past-event guard, preservation of
  forward and authorized elastic events, same-lane completion evidence for a
  parked lane, and the boundary established by a freshly reset head.
- `ResetAlignment.lean` models first-row materialization, lane-head reset,
  evidence ownership, committed versus preview arm views, and no-ingress
  descendant alignment.
- `Admission.smt2` checks ten negated obligations as UNSAT. Every obligation's
  premises are also SAT; six additional SAT witnesses cover positive and
  historical-counterexample cases. The checker expects exactly 16 SAT and
  10 UNSAT results and rejects errors or unknown results.

These are executable semantic histories and source-linked abstractions, not
a universal refinement proof of arbitrary compiled Rust. They do not model
transport implementation, scheduling fairness, hardware, or malicious frame
provenance. The existing canonical Lean certificate gate separately checks
descriptor correspondence, parallel dependencies, causal flow, runtime
atomic-failure cases and the public-operation kernel.

The permanent Rust regressions are in
`tests/security_report_regressions/rolled_routes.rs`; existing visible-reentry,
parallel-join and generated cursor tests additionally exercise the repair.
To run the handoff's six larger reproductions, populate its pinned `repro/support`
files using the handoff instructions and run:

```sh
bash proofs/rolled-route-ownership/reproduce.sh /path/to/hibana-core-handoff-20261002 /path/to/evidence
```

This builds the current local core, verifies the support-source hashes, runs
each reproduction with a 30-second limit, and removes all Rust products on
success or failure. The Q1 five-phase and TLS-shaped histories are core
protocol reproductions; they do not establish real QUIC network interoperability.

## Validated repair

Local checks on 2026-10-03 used Rust 1.95.0, Lean 4.30.0 and the local patched
Hibana 0.9.6. The remote main and both development/fix references were checked
again before publication; this repair extends the handoff's development base.

| Check | Result |
| --- | --- |
| Six handoff reproductions, including Q1 five-phase and TLS-shaped histories | Passed |
| Workspace tests, source/API guards and UI checks | 442 passed; 3 documented doctests ignored |
| Internal runtime/reference/generated-corpus tests | 436 passed; 8 explicit exporter/measurement tests ignored in the default run |
| Canonical Lean gate, including explicitly run exporters | Passed: 709 static and 506 generated theorems; 182 parallel and 36 causal correspondences; runtime and public-operation audits |
| Supplemental Lean and Z3 check | Six Lean files; 10 UNSAT obligations and 16 SAT premises/witnesses |
| Strict workspace/all-target Clippy | Passed with warnings denied |
| Runtime operation-count and compile-pressure gate | Passed with one build job and debug information disabled |
| Core and projection example, no default features | Passed for `thumbv6m-none-eabi` and `thumbv8m.main-none-eabi` |
| Stackchan radio firmware ABI check, Clippy and link | Passed using the local core path and pinned binary SDK |
| Targeted strict-provenance Miri checks | Five rolled-route regressions and nine descriptor/decoder/image-identity cases passed |
| Complete Kani/CBMC CI on `08bba17a` | 200 harnesses verified; zero failures |
| Complete strict-provenance Miri CI on `08bba17a` | 223 passed; two intentionally ignored |

The existing fixed-snapshot size gate passed: measured sample peak stack was
2,655 bytes, modeled sample runtime SRAM 5,322 bytes, and aggregate core rlib
code/read-only data 88,874 bytes. The gate's corresponding limits are 3,663,
8,954 and 169,965 bytes. These are the gate's host/sample and object-section
measurements, not a universal hardware stack proof or a linked whole-firmware
flash measurement. No endpoint storage field or wire-header byte was added.

Local evidence is retained under `/tmp/hibana-core-handoff.4GYP1K/`; the firmware
build evidence is `/tmp/dots-radio-evidence.bJyR7D/`. Rust verification products
were removed. Targeted Miri evidence is
`/tmp/hibana-miri-repair-evidence.BBiRQD/`; the five regressions also record a
successful command exit under the existing 480-second deep-route limit.
The complete final-form suite remains a CI responsibility.

CI exposed stale verification fixtures inherited from the development base:
Kani's reviewed inventory had the same 200 harnesses in a different order, its
runner omitted ripgrep, one role-image proof placed empty trailing columns at
offset zero, and Miri still selected the former sparse atom representation.
The runner now rejects missing/failed assumption audits, the inventory and
proof fixture follow the canonical layout, and Miri checks the current dense
lookup, decoder boundaries and image identity. The five regressions join the
Miri gate, bringing its reviewed total to 223 passed and two intentionally
ignored tests. No harness is removed and no assumption is added. The first
complete CBMC attempt verified 199 of 200; the failing proof-fixture assertion
was repaired. The subsequent [CI run on `08bba17a`](https://github.com/hibanaworks/hibana/actions/runs/37038053959)
verified all 200 harnesses with zero failures. Its Kani log is retained as
`kani-ci-08bba17a.log` in the handoff evidence directory. The same run passed
all 223 Miri tests (two intentional ignores), the canonical Lean gate, and
Unix carrier conformance. Its final-form job stopped at the existing source
lowering hygiene rule: the scope-boundary reference test used an optional
search with an end-position default. That reference now counts the sorted
prefix below the query, preserving all 256 marker layouts, floors and query
boundaries. The four source-arena tests and the unchanged hygiene rule pass.
The full job log is retained as
`/tmp/hibana-proof-audit-evidence.Ftyoo2/final-form-ci-08bba17a.log`.

The canonical and supplemental Lean source audits now use the existing
fail-closed search helper. Missing ripgrep, unreadable search inputs and other
search errors reject the gate before any proof run. A fault-injection
regression exercises each script with missing, failed, forbidden and clean
searches; the clean control reaches the proof runner while the three negative
cases cannot. The 124 semantic-surface tests and strict Clippy passed, and the
canonical Lean gate, six supplemental Lean files and Z3 obligations were
checked again. Evidence is retained under
`/tmp/hibana-proof-audit-evidence.Ftyoo2/`; Rust verification products were
removed after each check.
