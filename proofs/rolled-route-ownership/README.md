# Rolled-route ownership regressions

## Intrinsic entry selection repair, 2026-10-06

The StackChan touch sampling contract reproduced another entry-selection defect
on `fac137e3`. An outer rolled route uses Read in its first arm and Return in its
second. The Read arm contains a nested failed-read Return with the same logical
label and payload schema. After a successful Read/Sampled/Received cycle, a fresh
outer Return was rejected as PhaseInvariant even with the ordinary test carrier,
without I2C or intercore I/O.

Intrinsic send-preview selection scanned the entire first arm before checking
the second arm's actual entry. That scan selected the unchosen nested Return.
Intrinsic choice now considers only actual controller entries. The separately
authorized selected-arm continuation scan and all dependency, conflict, resolver,
reentry and atomic publication checks remain. There is no public API, stored
field, wire-format or capacity addition.

`SendEntry.lean` was kernel-checked before the production edit. Three quantified
selector obligations exclude an arbitrary interior occurrence and preserve a
matching second entry. Six canonical GlobalSemantics histories cover initial
Return, successful-read reentry, wrong inner Return rejection, the actual failed
read return path, early-return rejection and repeated reads. `SendEntry.smt2`
checks two UNSAT negated obligations with SAT premises and a concrete SAT witness
of the former body-before-entry selection. These abstractions and histories do
not claim universal refinement of arbitrary Rust or native hardware behavior.
The supplemental runner checks eight Lean files and the exact new Z3 result
sequence, with 14 UNSAT obligations and 22 SAT premises/witnesses overall.

The ordinary workspace run passed 723 tests (12 explicitly ignored cases),
strict workspace/all-target Clippy and the Pico no-default projection build.
The canonical Lean gate passed its complete 709 static/506 generated theorem
inventory, 182 parallel and 36 causal correspondences, and atomic-failure/public
operation audits. All six send-continuation regressions passed strict-provenance
Miri, including the 0/1/2/64-read case and either nested initial arm. Miri products
were cleaned at its actual nested target directory after the generic root clean
rejected its missing cache tag. The resource gate passed with thumb rlib sections
97,276 bytes, sample peak stack 2,623 bytes and modeled sample SRAM 5,290 bytes,
within unchanged limits. These are sample/object-section measurements, not a
linked StackChan flash or universal stack proof. Evidence is retained in
`/tmp/hibana-send-entry-evidence.00tMTi/`; disposable Rust products are removed.
The running generation-92 body remains selected. The touch implementation is not
yet deployed and no petting reaction is claimed from this core qualification.

## Nested send continuation repair, 2026-10-03

The Path handoff on `ca5a6fc3` exposes a send-preview defect independent of the
elastic wire-color allocator. An enabled ResultTaken occurrence following Applied
was classified as a controller decision solely because it belonged to the
containing arm. The label/schema rescan then replaced that occurrence with an
earlier, unchosen abandonment acknowledgement having the same logical contract.
The later eligibility guard correctly rejected that wrong occurrence.

Controller decisions now use the descriptor's actual arm-entry index. Arm
membership never turns an interior continuation into a new decision. Route-start
and unlabeled-node decisions remain intact. The existing dependency, conflict,
lane, reentry, resolver, carrier and commit checks remain in place. No new public
API, endpoint field, wire byte or capacity is introduced; the preflight/publish
boundary is unchanged.

`SendContinuation.lean` was checked before the production edit. It proves the
entry/continuation distinction for the decision abstraction, and executes the
canonical GlobalSemantics histories for Applied/ResultTaken, opposite-arm same
label rejection, missing reply, duplicate acknowledgement, alternating reentry
and first Retire. `SendContinuation.smt2` checks two negated boundary obligations
as UNSAT, both SAT premises and a SAT witness of the old membership shortcut.
The full supplemental runner now checks seven Lean files, 12 UNSAT obligations
and 19 SAT premises/witnesses. These are source-linked abstractions and executable
histories, not a universal proof of Rust selector refinement.

Permanent Rust tests in `send_continuations.rs` run on ordinary test stacks and
join the strict-provenance Miri inventory. The handed-off diagnostic matrix is
also rerun with its original 64 MiB thread setting: all 12 controls and failing
variants pass, including the complete Path prefix. That diagnostic stack is not
a Pico stack measurement. Real full-owner QUIC traffic and Neqo interoperability
remain separate integration qualifications; the supplied Linux HQ adapter and
Neqo runner cannot be exercised by this Mac-only environment without the absent
Linux/container/Neqo tooling. No integration success is inferred from core traces.

The previously failed canonical descriptor proof is also repaired: the Lean
canonical compiler applies the same final innermost elastic-owner color pass as
Rust. The complete 709 static/506 generated theorem inventory is retained; one
label theorem now states the final allocation rather than the intermediate
allocation. Its axiom closure becomes smaller. Captured auditor failures now
retain their stdout diagnostics and original nonzero exit status.

Fresh local qualification: 865 workspace tests passed (8 explicit internal
exporters/measurement cases and 3 documented doctests ignored by the default
workspace run); all 17 security regressions passed; the 3 new tests passed Miri;
strict workspace/all-target Clippy passed. The canonical Lean gate separately ran
its exporters and passed 709 static/506 generated theorems, 182 parallel and 36
causal correspondences. The full portable Lean/Z3 replay passed with Z3 4.16.0.
Core and projection examples passed no-default checks on thumbv6m and thumbv8m.
The resource gate measured thumb rlib sections at 87,968 bytes, sample stack at
2,639 bytes and modeled sample SRAM at 5,306 bytes on this Mac, within unchanged
ceilings. GNU publication-host SRAM remains a separate README measurement.
Evidence: `/tmp/hibana-path-residual-evidence/`. Rust products are removed after
each validation; running body owners are preserved.

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
| Complete Kani/CBMC CI on `fe09aad8` | 200 harnesses verified; zero failures |
| Complete strict-provenance Miri CI on `fe09aad8` | 223 passed; two intentionally ignored |

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

The [CI run on `fe09aad8`](https://github.com/hibanaworks/hibana/actions/runs/37042194990)
again passed Kani, Miri, canonical Lean and carrier conformance, and passed the
unchanged source-lowering hygiene rule. Its final-form job reached the README
measurement consistency check: the published table still described the earlier
runtime. The measured publication-host stack was 2,639 bytes, modeled SRAM
5,322 bytes and thumbv6m rlib sections 88,874 bytes. The table now records these
measurements; release ceilings and checks are unchanged. The complete job log
is retained as `final-form-ci-fe09aad8.log` in the proof-audit evidence directory.
